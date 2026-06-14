//! Orbit (arcball-style) camera.
//!
//! The camera always looks at `target` (the globe centre) from a point on a
//! sphere of radius `distance`, parameterised by `yaw` (around +Y) and `pitch`
//! (elevation above the equatorial plane). Input nudges those angles; the
//! globe's own axial spin is a separate model transform, so dragging moves the
//! viewpoint rather than the planet.

use glam::{Mat4, Vec3};

/// Don't let the camera reach the poles exactly (gimbal degeneracy / flip).
const PITCH_LIMIT: f32 = 1.5533; // ~89 degrees
const DIST_MIN: f32 = 1.35;
const DIST_MAX: f32 = 14.0;

pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        // Matches the original hardcoded eye (0, 0.7, 2.7) looking at origin:
        // distance = |eye|, pitch = asin(eye.y / distance), yaw = 0 (on +Z).
        Self {
            target: Vec3::ZERO,
            distance: 2.79,
            yaw: 0.0,
            pitch: 0.253,
            fov_y: 45f32.to_radians(),
            near: 0.1,
            far: 100.0,
        }
    }
}

impl OrbitCamera {
    /// Camera position in world space.
    pub fn eye(&self) -> Vec3 {
        let cp = self.pitch.cos();
        self.target
            + self.distance
                * Vec3::new(cp * self.yaw.sin(), self.pitch.sin(), cp * self.yaw.cos())
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far) * self.view()
    }

    /// Rotate the viewpoint by the given yaw/pitch deltas (radians).
    pub fn orbit(&mut self, dyaw: f32, dpitch: f32) {
        self.yaw += dyaw;
        self.pitch = (self.pitch + dpitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Dolly in/out. Positive `factor` zooms in; the step is proportional so it
    /// feels even across the whole range.
    pub fn zoom(&mut self, factor: f32) {
        self.distance = (self.distance * (1.0 - factor)).clamp(DIST_MIN, DIST_MAX);
    }
}
