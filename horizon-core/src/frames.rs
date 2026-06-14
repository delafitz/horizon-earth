//! Coordinate frames and the bridge from physics space to render space.
//!
//! Physics is done in ECI (Earth-centred inertial), Z-up, kilometres. The
//! renderer wants Y-up with the Earth radius normalised to 1.0. These two
//! conventions differ by a single fixed rotation (Z-up -> Y-up) plus a scale.

use glam::DVec3;

use crate::units::EARTH_RADIUS_KM;

/// Map an ECI position (Z-up, km) into the render world frame (Y-up, Earth
/// radius = 1.0).
///
/// The rotation is Rx(-90°): ECI +Z (north pole) -> render +Y, ECI +X -> render
/// +X, ECI +Y -> render -Z. Scale divides by the Earth radius.
#[inline]
pub fn eci_to_world(eci_km: DVec3) -> DVec3 {
    let r = eci_km / EARTH_RADIUS_KM;
    DVec3::new(r.x, r.z, -r.y)
}

/// Geodetic longitude/latitude (degrees) on a sphere of `radius_km` to an
/// Earth-fixed (ECEF, Z-up) Cartesian position in km.
///
/// Longitude is measured eastward from +X; +Z is the north pole.
#[inline]
pub fn latlon_to_ecef(lon_deg: f64, lat_deg: f64, radius_km: f64) -> DVec3 {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    let cl = lat.cos();
    DVec3::new(radius_km * cl * lon.cos(), radius_km * cl * lon.sin(), radius_km * lat.sin())
}
