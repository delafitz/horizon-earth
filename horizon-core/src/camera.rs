//! Orbit (arcball-style) camera, in render-frame units (Earth radius = 1).
//!
//! The camera looks at `target` from a point on a sphere of radius `distance`,
//! parameterised by `yaw` (about +Y) and `pitch` (elevation). All `f64`; the
//! renderer casts the resulting matrices to `f32` at the GPU boundary.

use glam::{DMat4, DVec3};

/// Keep the camera shy of the poles (gimbal degeneracy / flip).
const PITCH_LIMIT: f64 = 1.5533; // ~89 degrees
const DIST_MIN: f64 = 1.35;
const DIST_MAX: f64 = 14.0;

pub struct OrbitCamera {
    pub target: DVec3,
    pub distance: f64,
    pub yaw: f64,
    pub pitch: f64,
    pub fov_y: f64,
    pub near: f64,
    pub far: f64,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: DVec3::ZERO,
            distance: 2.79,
            yaw: 0.0,
            pitch: 0.253,
            fov_y: 45f64.to_radians(),
            near: 0.05,
            far: 200.0,
        }
    }
}

impl OrbitCamera {
    /// Camera position in world space.
    pub fn eye(&self) -> DVec3 {
        let cp = self.pitch.cos();
        self.target
            + self.distance
                * DVec3::new(cp * self.yaw.sin(), self.pitch.sin(), cp * self.yaw.cos())
    }

    pub fn view(&self) -> DMat4 {
        DMat4::look_at_rh(self.eye(), self.target, DVec3::Y)
    }

    pub fn view_proj(&self, aspect: f64) -> DMat4 {
        DMat4::perspective_rh(self.fov_y, aspect, self.near, self.far) * self.view()
    }

    /// Rotate the viewpoint by yaw/pitch deltas (radians).
    pub fn orbit(&mut self, dyaw: f64, dpitch: f64) {
        self.yaw += dyaw;
        self.pitch = (self.pitch + dpitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Dolly in (positive) / out (negative); step is proportional to distance.
    pub fn zoom(&mut self, factor: f64) {
        self.distance = (self.distance * (1.0 - factor)).clamp(DIST_MIN, DIST_MAX);
    }
}
