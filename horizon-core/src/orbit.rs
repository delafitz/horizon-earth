//! Orbital motion in the ECI frame.
//!
//! [`Propagator`] is the abstraction the renderer consumes: given an absolute
//! [`Epoch`] it returns an ECI position (km). Two-body Keplerian motion is one
//! implementation; SGP4 (see [`crate::sgp4`]) is another, and both look
//! identical to the renderer.

use std::f64::consts::{PI, TAU};

use glam::{DMat3, DVec3};

use crate::time::{Epoch, J2000_JD};
use crate::units::{EARTH_MU, EARTH_RADIUS_KM, SECONDS_PER_DAY};

/// Anything that can report where a body is at a given time.
pub trait Propagator {
    /// The reference epoch the motion is defined against.
    fn epoch(&self) -> Epoch;
    /// Position in ECI (km) at the given absolute time.
    fn position_eci(&self, at: Epoch) -> DVec3;
    /// Orbital period (s) — used to sample a full orbit track.
    fn period(&self) -> f64;
}

/// Classical orbital elements (angles in radians, lengths in km), referenced to
/// the ECI frame at [`KeplerOrbit::epoch`].
#[derive(Clone, Copy, Debug)]
pub struct KeplerOrbit {
    pub epoch: Epoch,
    /// Semi-major axis (km).
    pub a: f64,
    /// Eccentricity (0 = circular).
    pub e: f64,
    /// Inclination.
    pub i: f64,
    /// Right ascension of the ascending node.
    pub raan: f64,
    /// Argument of periapsis.
    pub argp: f64,
    /// Mean anomaly at epoch.
    pub m0: f64,
}

impl KeplerOrbit {
    /// A circular orbit at `altitude_km` above the mean surface.
    pub fn circular(epoch: Epoch, altitude_km: f64, inclination: f64, raan: f64, phase: f64) -> Self {
        Self {
            epoch,
            a: EARTH_RADIUS_KM + altitude_km,
            e: 0.0,
            i: inclination,
            raan,
            argp: 0.0,
            m0: phase,
        }
    }

    /// Mean motion (rad / s).
    pub fn mean_motion(&self) -> f64 {
        (EARTH_MU / (self.a * self.a * self.a)).sqrt()
    }

    /// Position in the perifocal plane (periapsis on +x) for an eccentric anomaly.
    fn perifocal_point(&self, eccentric_anomaly: f64) -> DVec3 {
        let (se, ce) = eccentric_anomaly.sin_cos();
        let x = self.a * (ce - self.e);
        let y = self.a * (1.0 - self.e * self.e).sqrt() * se;
        DVec3::new(x, y, 0.0)
    }

    /// Rotation from the perifocal frame to ECI: Rz(raan) · Rx(i) · Rz(argp).
    fn perifocal_to_eci(&self) -> DMat3 {
        DMat3::from_rotation_z(self.raan)
            * DMat3::from_rotation_x(self.i)
            * DMat3::from_rotation_z(self.argp)
    }
}

impl Propagator for KeplerOrbit {
    fn epoch(&self) -> Epoch {
        self.epoch
    }

    fn position_eci(&self, at: Epoch) -> DVec3 {
        let t = at.seconds_since(self.epoch);
        let m = self.m0 + self.mean_motion() * t;
        let ea = solve_kepler(m, self.e);
        self.perifocal_to_eci() * self.perifocal_point(ea)
    }

    fn period(&self) -> f64 {
        TAU / self.mean_motion()
    }
}

/// SGP4 propagation from a set of TLE/OMM elements.
///
/// SGP4 outputs positions in the TEME frame; for a visualisation we treat TEME
/// as our ECI (the difference is the equation of the equinoxes, well under a
/// pixel). Conversion to the rotating Earth is handled by GMST, same as the
/// Keplerian path — the renderer can't tell the two propagators apart.
pub struct Sgp4Orbit {
    constants: sgp4::Constants,
    epoch: Epoch,
    period_s: f64,
}

impl Sgp4Orbit {
    pub fn from_elements(elements: &sgp4::Elements) -> Result<Self, String> {
        let constants = sgp4::Constants::from_elements(elements).map_err(|e| e.to_string())?;
        // `epoch()` is Julian years since J2000; convert to a Julian Date.
        let epoch = Epoch::from_julian_date(J2000_JD + elements.epoch() * 365.25);
        // TLE mean motion is in revolutions per day.
        let period_s = if elements.mean_motion > 0.0 {
            SECONDS_PER_DAY / elements.mean_motion
        } else {
            SECONDS_PER_DAY
        };
        Ok(Self { constants, epoch, period_s })
    }
}

impl Propagator for Sgp4Orbit {
    fn epoch(&self) -> Epoch {
        self.epoch
    }

    fn position_eci(&self, at: Epoch) -> DVec3 {
        let minutes = at.seconds_since(self.epoch) / 60.0;
        match self.constants.propagate(sgp4::MinutesSinceEpoch(minutes)) {
            Ok(p) => DVec3::new(p.position[0], p.position[1], p.position[2]),
            Err(_) => DVec3::ZERO, // decayed / out of range — drop to the origin
        }
    }

    fn period(&self) -> f64 {
        self.period_s
    }
}

/// Sample a propagator's path as `segments + 1` ECI points over one period,
/// starting at `from`. Pass the *current* simulation epoch (not the element-set
/// epoch): SGP4 precesses the RAAN, so a track built at the TLE epoch drifts
/// away from the satellite's actual position as the clock advances.
pub fn sample_track<P: Propagator + ?Sized>(p: &P, from: Epoch, segments: u32) -> Vec<DVec3> {
    let period = p.period();
    (0..=segments)
        .map(|k| from.plus_seconds(period * k as f64 / segments as f64))
        .map(|at| p.position_eci(at))
        .collect()
}

/// Solve Kepler's equation `M = E - e·sin(E)` for the eccentric anomaly `E`.
fn solve_kepler(mean_anomaly: f64, e: f64) -> f64 {
    let m = mean_anomaly.rem_euclid(TAU);
    let mut ea = if e < 0.8 { m } else { PI };
    for _ in 0..30 {
        let dx = (ea - e * ea.sin() - m) / (1.0 - e * ea.cos());
        ea -= dx;
        if dx.abs() < 1e-12 {
            break;
        }
    }
    ea
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orbit() -> KeplerOrbit {
        KeplerOrbit::circular(Epoch::j2000(), 420.0, 51.6_f64.to_radians(), 0.0, 0.0)
    }

    #[test]
    fn iss_like_period_is_about_92_minutes() {
        let minutes = orbit().period() / 60.0;
        assert!((minutes - 92.6).abs() < 1.0, "period was {minutes} min");
    }

    #[test]
    fn geo_period_is_one_sidereal_day() {
        let o = KeplerOrbit::circular(Epoch::j2000(), 35_786.0, 0.0, 0.0, 0.0);
        assert!((o.period() - 86_164.0).abs() < 200.0, "period {}", o.period());
    }

    #[test]
    fn circular_orbit_keeps_constant_radius() {
        let o = KeplerOrbit::circular(Epoch::j2000(), 800.0, 1.2, 0.5, 0.0);
        let r0 = o.position_eci(o.epoch).length();
        let r1 = o.position_eci(o.epoch.plus_seconds(1234.0)).length();
        assert!((r0 - r1).abs() < 1e-6);
        assert!((r0 - (EARTH_RADIUS_KM + 800.0)).abs() < 1e-6);
    }

    #[test]
    fn track_closes_on_itself() {
        let o = orbit();
        let pts = sample_track(&o, o.epoch(), 64);
        assert_eq!(pts.len(), 65);
        assert!((pts[0] - pts[64]).length() < 1.0);
    }
}
