//! Geographic projection: latitude/longitude onto the sphere, and conversion
//! of GeoJSON polylines into renderable line segments.
//!
//! Projection lives in `horizon_core::frames`: lon/lat -> ECEF -> render frame.
//! The geometry is built in the Earth-fixed orientation (GMST = 0); the
//! renderer applies the GMST spin as a model rotation, which is what keeps
//! coastlines aligned with the inertial-frame satellites.

use glam::DVec3;

use horizon_core::frames::geo_to_render;

use crate::data::PolyLine;
use crate::renderer::mesh::{self, VertexPC};

/// Safety cap on the subdivision recursion (the chord tolerance normally stops
/// it well before this); 4^8 leaves per source triangle worst case.
const MAX_SUBDIV_DEPTH: u32 = 8;

/// Convert geographic coordinates (degrees) to render-frame coordinates at the
/// given render radius (Earth surface = 1.0).
pub fn latlon_to_xyz(lon_deg: f64, lat_deg: f64, radius: f32) -> [f32; 3] {
    geo_to_render(lon_deg, lat_deg, radius as f64).as_vec3().to_array()
}

/// Append every polyline segment as a thick-line instance (one per segment),
/// projected onto the sphere. `layer` (0.0 = coastline, 1.0 = border) tells the
/// shader which width to use. The vertex shader expands each segment into a
/// constant-pixel-width quad, so width stays a live uniform, not baked geometry.
pub fn build_thick_lines(
    lines: &[PolyLine],
    color: [f32; 3],
    layer: f32,
    radius: f32,
    out: &mut Vec<mesh::ThickLineInstance>,
) {
    let col_layer = [color[0], color[1], color[2], layer];
    for line in lines {
        for pair in line.windows(2) {
            out.push(mesh::ThickLineInstance {
                p0: latlon_to_xyz(pair[0][0], pair[0][1], radius),
                p1: latlon_to_xyz(pair[1][0], pair[1][1], radius),
                col_layer,
            });
        }
    }
}

/// Triangulate the closed rings in lon/lat and emit clip-space 2D vertices
/// (x = lon/180, y = lat/90) for baking the equirectangular land mask. No
/// subdivision is needed (it's a flat 2D raster), so the triangulation tiles
/// the polygons exactly — the mask is crack-free regardless of concavity.
pub fn build_land_mask_2d(rings: &[PolyLine], out: &mut Vec<[f32; 2]>) {
    for ring in rings {
        // Unwrap longitude so antimeridian-crossing rings (Antarctica's polar
        // cap, Chukotka, Fiji…) are continuous before triangulating.
        let unwrapped = unwrap_lon(ring);
        for tri in triangulate(&unwrapped).chunks_exact(3) {
            // Emit each triangle at -360/0/+360 so the wrapped parts land in the
            // [-180,180] bake window; off-window copies are simply clipped.
            for off in [-360.0_f64, 0.0, 360.0] {
                for v in tri {
                    out.push([((v[0] + off) / 180.0) as f32, (v[1] / 90.0) as f32]);
                }
            }
        }
    }
}

/// Make a ring's longitudes continuous by removing ±360° jumps, so a ring that
/// crosses the antimeridian (or encircles a pole) triangulates as one piece.
fn unwrap_lon(ring: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(ring.len());
    if let Some(&first) = ring.first() {
        let mut acc = first[0];
        out.push(first);
        for w in ring.windows(2) {
            let mut d = w[1][0] - w[0][0];
            if d > 180.0 {
                d -= 360.0;
            } else if d < -180.0 {
                d += 360.0;
            }
            acc += d;
            out.push([acc, w[1][1]]);
        }
    }
    out
}

/// Triangulate each closed ring and append the triangles to `out` (TriangleList
/// topology: three vertices per triangle), conforming to the sphere. Used to
/// give land a faint translucent body.
///
/// Ear clipping produces flat triangles in lon/lat; a big country yields
/// triangles spanning tens of degrees, and a flat chord that large sags far
/// below the curved surface (visible up close as facets cutting through the
/// globe). So each triangle is recursively subdivided — splitting edges at their
/// geodesic midpoints and snapping them back onto the sphere — until every edge
/// spans at most `tol_rad` radians, making the fill hug the curvature.
pub fn build_fill(
    rings: &[PolyLine],
    color: [f32; 3],
    radius: f32,
    tol_rad: f64,
    out: &mut Vec<VertexPC>,
) {
    // An edge is "small enough" when its chord (on the unit sphere) is within
    // tolerance; chord = 2 sin(theta/2) for an edge subtending angle theta.
    let max_chord = 2.0 * (tol_rad.max(1e-4) * 0.5).sin();
    let max_chord2 = max_chord * max_chord;
    for ring in rings {
        let tris = triangulate(ring);
        for t in tris.chunks_exact(3) {
            // Triangle corners as unit-sphere points in the render frame.
            let a = geo_to_render(t[0][0], t[0][1], 1.0);
            let b = geo_to_render(t[1][0], t[1][1], 1.0);
            let c = geo_to_render(t[2][0], t[2][1], 1.0);
            subdivide_to_sphere(a, b, c, max_chord2, radius as f64, color, 0, out);
        }
    }
}

/// Recursively split triangle `a,b,c` (unit-sphere points) at geodesic midpoints
/// until every edge's chord length is within `max_chord2`, then emit the leaf
/// triangles scaled to `radius`. New vertices are re-normalised onto the sphere
/// so the result conforms to the curvature rather than being a flat chord.
#[allow(clippy::too_many_arguments)]
fn subdivide_to_sphere(
    a: DVec3,
    b: DVec3,
    c: DVec3,
    max_chord2: f64,
    radius: f64,
    color: [f32; 3],
    depth: u32,
    out: &mut Vec<VertexPC>,
) {
    let longest = (a - b)
        .length_squared()
        .max((b - c).length_squared())
        .max((c - a).length_squared());
    if depth >= MAX_SUBDIV_DEPTH || longest <= max_chord2 {
        for p in [a, b, c] {
            out.push(VertexPC { pos: (p * radius).as_vec3().to_array(), col: color });
        }
        return;
    }
    // 1-to-4 split; normalising the midpoints keeps them on the sphere.
    let ab = (a + b).normalize();
    let bc = (b + c).normalize();
    let ca = (c + a).normalize();
    let d = depth + 1;
    subdivide_to_sphere(a, ab, ca, max_chord2, radius, color, d, out);
    subdivide_to_sphere(ab, b, bc, max_chord2, radius, color, d, out);
    subdivide_to_sphere(ca, bc, c, max_chord2, radius, color, d, out);
    subdivide_to_sphere(ab, bc, ca, max_chord2, radius, color, d, out);
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
