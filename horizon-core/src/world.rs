//! The simulated world: a clock, the central body's rotation, and the set of
//! orbiting bodies.
//!
//! The clock is driven from the renderer's real elapsed time scaled by
//! `time_scale` (orbital periods are minutes-to-hours of real time, far too
//! slow to watch at 1×). Everything else is derived from the resulting
//! simulation time, so the model is stateless between frames and deterministic.

use glam::DVec3;

use crate::orbit::KeplerOrbit;
use crate::units::EARTH_ANGULAR_VELOCITY;

/// How many simulated seconds pass per real second.
pub const DEFAULT_TIME_SCALE: f64 = 500.0;

/// An orbiting body (satellite, station, …).
#[derive(Clone)]
pub struct Body {
    pub name: &'static str,
    pub orbit: KeplerOrbit,
    /// Render colour (linear RGB).
    pub color: [f32; 3],
}

pub struct World {
    pub bodies: Vec<Body>,
    pub time_scale: f64,
    /// Simulated seconds since epoch (t = 0 at startup).
    sim_seconds: f64,
}

impl World {
    /// A small demo constellation spanning real altitude regimes (LEO → MEO),
    /// so the scene shows true relative scale and period.
    pub fn demo() -> Self {
        let deg = |x: f64| x.to_radians();
        let bodies = vec![
            Body {
                name: "ISS",
                orbit: KeplerOrbit::circular(420.0, deg(51.6), deg(40.0), 0.0),
                color: [0.749, 0.380, 0.416], // Nord11
            },
            Body {
                name: "Polar LEO",
                orbit: KeplerOrbit::circular(800.0, deg(98.0), deg(120.0), 1.5),
                color: [0.922, 0.796, 0.545], // Nord13
            },
            Body {
                name: "GPS (MEO)",
                orbit: KeplerOrbit::circular(20_180.0, deg(55.0), deg(200.0), 3.0),
                color: [0.639, 0.745, 0.549], // Nord14
            },
        ];
        Self {
            bodies,
            time_scale: DEFAULT_TIME_SCALE,
            sim_seconds: 0.0,
        }
    }

    /// Set the simulation time from the renderer's real elapsed seconds.
    pub fn set_real_elapsed(&mut self, real_seconds: f64) {
        self.sim_seconds = real_seconds * self.time_scale;
    }

    /// Current simulation time (seconds since epoch).
    pub fn sim_seconds(&self) -> f64 {
        self.sim_seconds
    }

    /// Earth's accumulated rotation angle (rad) about the polar axis.
    pub fn earth_rotation(&self) -> f64 {
        EARTH_ANGULAR_VELOCITY * self.sim_seconds
    }

    /// Current ECI position (km) of body `i`.
    pub fn body_position_eci(&self, i: usize) -> DVec3 {
        self.bodies[i].orbit.position_eci(self.sim_seconds)
    }
}
