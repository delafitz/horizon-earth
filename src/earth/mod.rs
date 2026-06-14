//! Geographic projection: latitude/longitude onto a unit sphere, and
//! conversion of GeoJSON polylines into renderable line segments.

use crate::data::PolyLine;
use crate::renderer::mesh::VertexPC;

/// Convert geographic coordinates (degrees) to Cartesian coordinates on a
/// sphere of the given radius.
///
/// Matches the convention in the project spec:
///   x = r * cos(lat) * cos(lon)
///   y = r * sin(lat)
///   z = r * cos(lat) * sin(lon)
///
/// North pole at +Y, longitude 0 at +X. Longitude is negated so the globe
/// reads correctly when viewed from outside (east to the right as a meridian
/// faces the camera); without this the continents come out mirrored.
pub fn latlon_to_xyz(lon_deg: f64, lat_deg: f64, radius: f32) -> [f32; 3] {
    let lat = lat_deg.to_radians();
    let lon = (-lon_deg).to_radians();
    let cl = lat.cos();
    let x = radius as f64 * cl * lon.cos();
    let y = radius as f64 * lat.sin();
    let z = radius as f64 * cl * lon.sin();
    [x as f32, y as f32, z as f32]
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
