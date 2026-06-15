//! Solar position: the direction from Earth to the Sun, for day/night lighting.
//!
//! Low-precision model (good to ~0.01°, far more than enough for a terminator):
//! the Sun's apparent ecliptic longitude from the mean longitude/anomaly, then
//! rotated by the obliquity into equatorial ECI (Z-up), as a unit vector.

use glam::DVec3;

use crate::time::{Epoch, J2000_JD};

/// Unit vector from Earth's centre toward the Sun, in the ECI frame (Z-up,
/// equatorial), at `epoch`.
pub fn sun_direction_eci(epoch: Epoch) -> DVec3 {
    let n = epoch.jd - J2000_JD; // days since J2000.0
    let mean_long = 280.460 + 0.985_647_4 * n; // deg
    let mean_anom = (357.528 + 0.985_600_3 * n).to_radians();
    // Apparent ecliptic longitude (equation-of-centre corrected), then radians.
    let lambda =
        (mean_long + 1.915 * mean_anom.sin() + 0.020 * (2.0 * mean_anom).sin()).to_radians();
    let obliquity = (23.439 - 4.0e-7 * n).to_radians();
    DVec3::new(
        lambda.cos(),
        obliquity.cos() * lambda.sin(),
        obliquity.sin() * lambda.sin(),
    )
    .normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_is_unit_and_near_equinox_points_along_x() {
        // Near the J2000 epoch (early Jan) the Sun is past the December
        // solstice: southern declination, so a negative Z component.
        let d = sun_direction_eci(Epoch::j2000());
        assert!((d.length() - 1.0).abs() < 1e-9);
        assert!(d.z < 0.0, "early January Sun should be south of the equator");
    }
}
