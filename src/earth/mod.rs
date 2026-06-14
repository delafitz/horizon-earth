//! Geographic projection: latitude/longitude onto the sphere, and conversion
//! of GeoJSON polylines into renderable line segments.
//!
//! Projection lives in `horizon_core::frames`: lon/lat -> ECEF -> render frame.
//! The geometry is built in the Earth-fixed orientation (GMST = 0); the
//! renderer applies the GMST spin as a model rotation, which is what keeps
//! coastlines aligned with the inertial-frame satellites.

use horizon_core::frames::geo_to_render;

use crate::data::PolyLine;
use crate::renderer::mesh::VertexPC;

/// Convert geographic coordinates (degrees) to render-frame coordinates at the
/// given render radius (Earth surface = 1.0).
pub fn latlon_to_xyz(lon_deg: f64, lat_deg: f64, radius: f32) -> [f32; 3] {
    geo_to_render(lon_deg, lat_deg, radius as f64).as_vec3().to_array()
}

/// Append every polyline to `out` as a list of GPU line segments (LineList
/// topology: two vertices per segment), projected onto the sphere.
pub fn build_lines(lines: &[PolyLine], color: [f32; 3], radius: f32, out: &mut Vec<VertexPC>) {
    for line in lines {
        for pair in line.windows(2) {
            let a = latlon_to_xyz(pair[0][0], pair[0][1], radius);
            let b = latlon_to_xyz(pair[1][0], pair[1][1], radius);
            out.push(VertexPC { pos: a, col: color });
            out.push(VertexPC { pos: b, col: color });
        }
    }
}

/// Triangulate each closed ring and append the triangles to `out` (TriangleList
/// topology: three vertices per triangle), projected onto the sphere. Used to
/// give land a faint translucent body. Rings are triangulated in lon/lat space
/// by ear clipping; at 110m resolution the per-triangle spherical distortion is
/// negligible.
pub fn build_fill(rings: &[PolyLine], color: [f32; 3], radius: f32, out: &mut Vec<VertexPC>) {
    for ring in rings {
        for v in triangulate(ring) {
            out.push(VertexPC { pos: latlon_to_xyz(v[0], v[1], radius), col: color });
        }
    }
}

/// Ear-clipping triangulation of a simple polygon ring (lon/lat). Returns a flat
/// list of triangle vertices (three per triangle). Holes are not handled.
fn triangulate(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut pts = ring.to_vec();
    // Drop the GeoJSON closing vertex (first == last).
    if pts.len() >= 2 && pts[0] == pts[pts.len() - 1] {
        pts.pop();
    }
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }

    let mut idx: Vec<usize> = (0..n).collect();
    // Ear test below assumes counter-clockwise winding.
    if signed_area(&pts) < 0.0 {
        idx.reverse();
    }

    let mut out = Vec::new();
    // Each successful clip removes one vertex; `guard` backstops degenerate rings.
    let mut guard = n * n;
    while idx.len() > 3 && guard > 0 {
        guard -= 1;
        let m = idx.len();
        let mut clipped = false;
        for i in 0..m {
            let ip = (i + m - 1) % m;
            let inx = (i + 1) % m;
            let a = pts[idx[ip]];
            let b = pts[idx[i]];
            let c = pts[idx[inx]];
            if cross(a, b, c) <= 0.0 {
                continue; // reflex vertex — not an ear tip
            }
            // No other vertex may lie inside the candidate ear triangle.
            let mut ear = true;
            for (k, &v) in idx.iter().enumerate() {
                if k == ip || k == i || k == inx {
                    continue;
                }
                if point_in_tri(pts[v], a, b, c) {
                    ear = false;
                    break;
                }
            }
            if !ear {
                continue;
            }
            out.push(a);
            out.push(b);
            out.push(c);
            idx.remove(i);
            clipped = true;
            break;
        }
        if !clipped {
            break; // self-intersecting / degenerate: stop early
        }
    }
    if idx.len() == 3 {
        out.push(pts[idx[0]]);
        out.push(pts[idx[1]]);
        out.push(pts[idx[2]]);
    }
    out
}

/// Twice the signed area of a polygon (positive = counter-clockwise).
fn signed_area(p: &[[f64; 2]]) -> f64 {
    let n = p.len();
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += p[i][0] * p[j][1] - p[j][0] * p[i][1];
    }
    a
}

/// Z-component of (b-a) × (c-a); > 0 when a→b→c turns left.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// True if `p` lies inside or on triangle `a,b,c` (any winding).
fn point_in_tri(p: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}
