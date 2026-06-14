//! Physical constants and units. Length in kilometres, time in seconds.

/// Earth mean equatorial radius (km).
pub const EARTH_RADIUS_KM: f64 = 6371.0;

/// Earth gravitational parameter GM (km^3 / s^2).
pub const EARTH_MU: f64 = 398_600.4418;

/// Earth sidereal rotation rate (rad / s).
pub const EARTH_ANGULAR_VELOCITY: f64 = 7.292_115_9e-5;

pub const SECONDS_PER_DAY: f64 = 86_400.0;

/// Convert a length in kilometres to render units (1 unit = 1 Earth radius).
#[inline]
pub fn km_to_render(km: f64) -> f64 {
    km / EARTH_RADIUS_KM
}
