//! Two-body (Keplerian) orbital motion in the ECI frame.
//!
//! A deliberately classical model: six orbital elements propagated by solving
//! Kepler's equation. Good enough to place satellites realistically at correct
//! altitudes and periods; a future SGP4 propagator can implement the same
//! `position_eci` / `sample_track` interface without touching the renderer.

use std::f64::consts::{PI, TAU};

use glam::{DMat3, DVec3};

use crate::units::{EARTH_MU, EARTH_RADIUS_KM};

/// Classical orbital elements (angles in radians, lengths in km), referenced to
/// the ECI frame at the simulation epoch (t = 0).
#[derive(Clone, Copy, Debug)]
pub struct KeplerOrbit {
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
    /// A circular orbit at `altitude_km` above the mean surface, with the given
    /// inclination, node, and starting phase (all radians).
    pub fn circular(altitude_km: f64, inclination: f64, raan: f64, phase: f64) -> Self {
        Self {
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

    /// Orbital period (s).
    pub fn period(&self) -> f64 {
        TAU / self.mean_motion()
    }

    /// Position in ECI (km) at time `t` seconds after epoch.
    pub fn position_eci(&self, t: f64) -> DVec3 {
        let m = self.m0 + self.mean_motion() * t;
        let ea = solve_kepler(m, self.e);
        self.perifocal_to_eci() * self.perifocal_point(ea)
    }

    /// Sample the orbit path as `segments + 1` ECI points (a closed loop),
    /// independent of time — the geometry of a two-body orbit is fixed.
    pub fn sample_track(&self, segments: u32) -> Vec<DVec3> {
        let rot = self.perifocal_to_eci();
        (0..=segments)
            .map(|k| {
                let ea = TAU * k as f64 / segments as f64;
                rot * self.perifocal_point(ea)
            })
            .collect()
    }

    /// Position in the perifocal plane (orbit plane, periapsis on +x) for a
    /// given eccentric anomaly.
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

/// Solve Kepler's equation `M = E - e·sin(E)` for the eccentric anomaly `E`
/// using Newton–Raphson.
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

    #[test]
    fn iss_like_period_is_about_92_minutes() {
        let o = KeplerOrbit::circular(420.0, 51.6_f64.to_radians(), 0.0, 0.0);
        let minutes = o.period() / 60.0;
        assert!((minutes - 92.6).abs() < 1.0, "period was {minutes} min");
    }

    #[test]
    fn geo_period_is_one_sidereal_day() {
        // a for a 24h-ish orbit; period should land near a sidereal day.
        let o = KeplerOrbit::circular(35_786.0, 0.0, 0.0, 0.0);
        assert!((o.period() - 86_164.0).abs() < 200.0, "period {}", o.period());
    }

    #[test]
    fn circular_orbit_keeps_constant_radius() {
        let o = KeplerOrbit::circular(800.0, 1.2, 0.5, 0.0);
        let r0 = o.position_eci(0.0).length();
        let r1 = o.position_eci(1234.0).length();
        assert!((r0 - r1).abs() < 1e-6);
        assert!((r0 - (EARTH_RADIUS_KM + 800.0)).abs() < 1e-6);
    }
}
