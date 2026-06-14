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
