//! Time: a UTC epoch (as a Julian Date) and Greenwich Mean Sidereal Time.
//!
//! For a visualisation we treat UTC ≈ UT1 (ignoring sub-second ΔUT1) and skip
//! precession/nutation/polar motion — all far below one pixel here.

use std::f64::consts::TAU;

use crate::units::SECONDS_PER_DAY;

/// Julian Date of J2000.0 (2000-01-01 12:00 TT).
pub const J2000_JD: f64 = 2_451_545.0;
/// Julian Date of the Unix epoch (1970-01-01 00:00 UTC).
pub const UNIX_EPOCH_JD: f64 = 2_440_587.5;

/// An instant in time, stored as a Julian Date (UTC).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Epoch {
    pub jd: f64,
}

impl Epoch {
    pub fn from_julian_date(jd: f64) -> Self {
        Self { jd }
    }

    /// From seconds since the Unix epoch (what `SystemTime` gives).
    pub fn from_unix_seconds(secs: f64) -> Self {
        Self { jd: UNIX_EPOCH_JD + secs / SECONDS_PER_DAY }
    }

    pub fn j2000() -> Self {
        Self { jd: J2000_JD }
    }

    /// Seconds elapsed from `other` to `self`.
    pub fn seconds_since(&self, other: Epoch) -> f64 {
        (self.jd - other.jd) * SECONDS_PER_DAY
    }

    /// A new epoch `secs` seconds later.
    pub fn plus_seconds(&self, secs: f64) -> Self {
        Self { jd: self.jd + secs / SECONDS_PER_DAY }
    }

    /// Julian centuries since J2000.0.
    pub fn julian_centuries_j2000(&self) -> f64 {
        (self.jd - J2000_JD) / 36_525.0
    }
}

/// Greenwich Mean Sidereal Time at `epoch`, in radians [0, 2π). IAU 1982 model:
/// the angle from the vernal equinox (ECI +X) to the prime meridian (ECEF +X),
/// i.e. the rotation taking ECEF into ECI about the polar axis.
pub fn gmst(epoch: Epoch) -> f64 {
    let t = epoch.julian_centuries_j2000();
    // GMST in seconds of time.
    let secs = 67_310.548_41
        + (876_600.0 * 3_600.0 + 8_640_184.812_866) * t
        + 0.093_104 * t * t
        - 6.2e-6 * t * t * t;
    (secs.rem_euclid(SECONDS_PER_DAY) / SECONDS_PER_DAY) * TAU
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch_round_trips() {
        let e = Epoch::from_unix_seconds(0.0);
        assert!((e.jd - UNIX_EPOCH_JD).abs() < 1e-9);
    }

    #[test]
    fn gmst_is_in_range_and_advances() {
        let e0 = Epoch::j2000();
        let g0 = gmst(e0);
        assert!((0.0..TAU).contains(&g0));
        // ~1.0027 sidereal rotations per solar day.
        let g1 = gmst(e0.plus_seconds(SECONDS_PER_DAY));
        let drift = ((g1 - g0).rem_euclid(TAU)).min((g0 - g1).rem_euclid(TAU));
        assert!(drift < 0.05, "GMST should nearly repeat after a solar day");
    }

    #[test]
    fn gmst_j2000_matches_known_value() {
        // GMST at J2000.0 is ~18.697 374 56 h = 4.894 961 rad.
        let g = gmst(Epoch::j2000());
        assert!((g - 4.894_961).abs() < 1e-3, "gmst {g}");
    }
}
