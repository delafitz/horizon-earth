//! Orbiting bodies (satellites etc.).
//!
//! A first, deliberately simple model: circular orbits described by a handful
//! of orbital elements, evaluated in an Earth-centred *inertial* frame. The
//! globe spins independently (its model matrix), so bodies sweep across the
//! surface as they should. Later phases can swap `position` for a real SGP4
//! propagator without changing the renderer.

use glam::{Mat4, Vec3};

/// Earth radius in world units (the globe mesh has radius 1.0).
pub const EARTH_RADIUS: f32 = 1.0;

#[derive(Clone, Copy)]
pub struct Orbit {
    /// Orbit radius from Earth's centre, in world units (1.0 == surface).
    pub radius: f32,
    /// Inclination of the orbital plane (radians).
    pub inclination: f32,
    /// Right ascension of the ascending node — orientation of the plane
    /// about the polar axis (radians).
    pub raan: f32,
    /// Seconds for one full revolution (visualisation time, not real time).
    pub period: f32,
    /// Phase offset along the orbit at t=0 (radians).
    pub phase0: f32,
    pub color: [f32; 3],
}

impl Orbit {
    /// World-space position of the body at time `t` (seconds).
    pub fn position(&self, t: f32) -> Vec3 {
        let theta = self.phase0 + std::f32::consts::TAU * t / self.period;
        let in_plane = Vec3::new(self.radius * theta.cos(), 0.0, self.radius * theta.sin());
        self.orientation().transform_point3(in_plane)
    }

    /// Rotation taking the orbital plane (initially the XZ plane) into world
    /// space: tilt by inclination, then swing the node around the polar axis.
    fn orientation(&self) -> Mat4 {
        Mat4::from_rotation_y(self.raan) * Mat4::from_rotation_x(self.inclination)
    }

    /// Sample the orbit path as `segments` world-space points (one loop).
    pub fn track(&self, segments: u32) -> Vec<Vec3> {
        let rot = self.orientation();
        (0..=segments)
            .map(|i| {
                let a = std::f32::consts::TAU * (i as f32) / (segments as f32);
                rot.transform_point3(Vec3::new(self.radius * a.cos(), 0.0, self.radius * a.sin()))
            })
            .collect()
    }
}

/// A small demo constellation so there is something to look at. Colours are
/// from the Nord aurora palette so they pop against the cool globe.
pub fn demo_bodies() -> Vec<Orbit> {
    let d = |x: f32| x.to_radians();
    vec![
        // ISS-like low orbit, ~51.6 deg inclination.
        Orbit {
            radius: EARTH_RADIUS + 0.08,
            inclination: d(51.6),
            raan: d(0.0),
            period: 24.0,
            phase0: 0.0,
            color: [0.749, 0.380, 0.416], // Nord11 #BF616A
        },
        // A polar orbit.
        Orbit {
            radius: EARTH_RADIUS + 0.18,
            inclination: d(90.0),
            raan: d(60.0),
            period: 38.0,
            phase0: 1.7,
            color: [0.922, 0.796, 0.545], // Nord13 #EBCB8B
        },
        // A higher, gentler-inclination orbit.
        Orbit {
            radius: EARTH_RADIUS + 0.45,
            inclination: d(28.0),
            raan: d(200.0),
            period: 70.0,
            phase0: 3.4,
            color: [0.639, 0.745, 0.549], // Nord14 #A3BE8C
        },
    ]
}
