//! The simulated world: a clock instant, the central body's orientation, and
//! the set of orbiting bodies.
//!
//! The world is told the current [`Epoch`] each frame (the app decides whether
//! that is wall-clock "now" or accelerated demo time) and everything else is
//! derived from it — stateless between frames and deterministic.

use glam::DVec3;

use crate::orbit::{KeplerOrbit, Propagator, Sgp4Orbit};
use crate::time::{gmst, Epoch};

/// Default simulated-seconds-per-real-second for demo mode.
pub const DEFAULT_TIME_SCALE: f64 = 500.0;

/// Nord aurora palette, cycled across tracked bodies.
const PALETTE: [[f32; 3]; 5] = [
    [0.749, 0.380, 0.416], // Nord11
    [0.816, 0.529, 0.439], // Nord12
    [0.922, 0.796, 0.545], // Nord13
    [0.639, 0.745, 0.549], // Nord14
    [0.706, 0.557, 0.678], // Nord15
];

/// An orbiting body (satellite, station, …).
pub struct Body {
    pub name: String,
    /// Render colour (linear RGB).
    pub color: [f32; 3],
    pub motion: Box<dyn Propagator + Send + Sync>,
}

pub struct World {
    pub bodies: Vec<Body>,
    current: Epoch,
}

impl World {
    pub fn new(epoch0: Epoch, bodies: Vec<Body>) -> Self {
        Self { bodies, current: epoch0 }
    }

    /// A small synthetic constellation spanning real altitude regimes
    /// (LEO → MEO), referenced to `epoch0`.
    pub fn demo(epoch0: Epoch) -> Self {
        let deg = |x: f64| x.to_radians();
        let body = |name: &str, color, orbit| Body {
            name: name.to_string(),
            color,
            motion: Box::new(orbit) as Box<dyn Propagator + Send + Sync>,
        };
        let bodies = vec![
            body(
                "ISS",
                [0.749, 0.380, 0.416], // Nord11
                KeplerOrbit::circular(epoch0, 420.0, deg(51.6), deg(40.0), 0.0),
            ),
            body(
                "Polar LEO",
                [0.922, 0.796, 0.545], // Nord13
                KeplerOrbit::circular(epoch0, 800.0, deg(98.0), deg(120.0), 1.5),
            ),
            body(
                "GPS (MEO)",
                [0.639, 0.745, 0.549], // Nord14
                KeplerOrbit::circular(epoch0, 20_180.0, deg(55.0), deg(200.0), 3.0),
            ),
        ];
        Self::new(epoch0, bodies)
    }

    /// Set the current simulation instant.
    pub fn set_time(&mut self, at: Epoch) {
        self.current = at;
    }

    pub fn current(&self) -> Epoch {
        self.current
    }

    /// Earth's rotation angle (rad) about the polar axis = GMST(now). This is
    /// the rotation that carries the Earth-fixed frame into the inertial frame.
    pub fn earth_rotation(&self) -> f64 {
        gmst(self.current)
    }

    /// Current ECI position (km) of body `i`.
    pub fn body_position_eci(&self, i: usize) -> DVec3 {
        self.bodies[i].motion.position_eci(self.current)
    }
}

/// Build bodies from real TLE/OMM element sets, propagated with SGP4. Element
/// sets that fail to initialise are skipped; colours cycle the Nord palette.
pub fn bodies_from_elements(elements: &[crate::Elements]) -> Vec<Body> {
    elements
        .iter()
        .enumerate()
        .filter_map(|(i, el)| {
            let motion = Sgp4Orbit::from_elements(el).ok()?;
            let name = el
                .object_name
                .clone()
                .unwrap_or_else(|| format!("NORAD {}", el.norad_id));
            Some(Body {
                name,
                color: PALETTE[i % PALETTE.len()],
                motion: Box::new(motion),
            })
        })
        .collect()
}
