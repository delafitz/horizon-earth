//! Cameras, in render-frame units (Earth radius = 1).
//!
//! Two rigs share a [`CameraRig`] wrapper that the renderer drives:
//! - [`OrbitCamera`] — arcball: looks at `target` from a point on a sphere of
//!   radius `distance`, parameterised by `yaw` (about +Y) and `pitch`.
//! - [`FlyCamera`] — rides a circular, "TLE-style" orbit (adjustable altitude,
//!   inclination, RAAN and speed) and looks along its velocity, with free
//!   yaw/pitch/roll offsets.
//!
//! All `f64`; the renderer casts the resulting matrices to `f32` at the GPU
//! boundary.

use glam::{DMat4, DQuat, DVec3};

use crate::units::EARTH_RADIUS_KM;

/// Keep the camera shy of the poles (gimbal degeneracy / flip).
const PITCH_LIMIT: f64 = 1.5533; // ~89 degrees
const DIST_MIN: f64 = 1.35;
const DIST_MAX: f64 = 14.0;

/// Map an ECI vector (Z-up) into the render frame (Y-up). Same rotation as
/// [`crate::frames::eci_to_world`] minus the km->radius scale, for geometry
/// already expressed in render units.
#[inline]
fn zup_to_yup(v: DVec3) -> DVec3 {
    DVec3::new(v.x, v.z, -v.y)
}

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

const FLY_ALT_MIN: f64 = 120.0; // km — keep the eye above the surface
const FLY_ALT_MAX: f64 = 40_000.0;
const FLY_SPEED_MIN: f64 = 0.0;
const FLY_SPEED_MAX: f64 = 3.0; // rad/s along the orbit

/// A camera that rides a circular orbit and looks along its velocity vector,
/// with free yaw/pitch/roll offsets. The orbit is parameterised like a TLE:
/// `altitude_km`, `inclination` and `raan` define the ring; `speed` is the
/// angular rate (rad/s) at which `phase` (argument of latitude) advances.
pub struct FlyCamera {
    pub altitude_km: f64,
    pub inclination: f64,
    pub raan: f64,
    pub speed: f64,
    /// Argument of latitude (rad): the camera's current position along the ring.
    pub phase: f64,
    /// Attitude offsets from the default "look along velocity" orientation.
    pub yaw: f64,
    pub pitch: f64,
    pub roll: f64,
    pub fov_y: f64,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            altitude_km: 700.0,
            inclination: 51.6f64.to_radians(), // ISS-ish, a lively tilt
            raan: 0.0,
            speed: 0.25,
            phase: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            fov_y: 60f64.to_radians(),
        }
    }
}

impl FlyCamera {
    /// Orbit radius in render units (Earth surface = 1.0).
    fn radius(&self) -> f64 {
        (EARTH_RADIUS_KM + self.altitude_km) / EARTH_RADIUS_KM
    }

    /// Position and (unit) velocity direction on the orbit, in the render frame.
    fn pos_vel(&self) -> (DVec3, DVec3) {
        let r = self.radius();
        let (su, cu) = self.phase.sin_cos();
        let (si, ci) = self.inclination.sin_cos();
        let (so, co) = self.raan.sin_cos();
        // Standard circular-orbit position (ECI, Z-up), argument of latitude u.
        let pos = DVec3::new(
            co * cu - so * su * ci,
            so * cu + co * su * ci,
            su * si,
        ) * r;
        // d(pos)/du gives the velocity direction.
        let vel = DVec3::new(
            -co * su - so * cu * ci,
            -so * su + co * cu * ci,
            cu * si,
        )
        .normalize();
        (zup_to_yup(pos), zup_to_yup(vel))
    }

    /// Camera position in world space (on the orbit).
    pub fn eye(&self) -> DVec3 {
        self.pos_vel().0
    }

    pub fn view(&self) -> DMat4 {
        let (pos, vel) = self.pos_vel();
        // Default: forward along velocity, up radially outward.
        let mut fwd = vel;
        let mut up = pos.normalize();
        // Apply the attitude offsets in the camera's own frame (yaw about up,
        // then pitch about the new right, then roll about the new forward).
        let q_yaw = DQuat::from_axis_angle(up, self.yaw);
        fwd = q_yaw * fwd;
        let right = fwd.cross(up).normalize();
        let q_pitch = DQuat::from_axis_angle(right, self.pitch);
        fwd = q_pitch * fwd;
        up = q_pitch * up;
        let q_roll = DQuat::from_axis_angle(fwd, self.roll);
        up = q_roll * up;
        DMat4::look_to_rh(pos, fwd, up)
    }

    pub fn view_proj(&self, aspect: f64) -> DMat4 {
        // Adaptive depth range. Up close the globe (r = 1.0) and the shells
        // just above it (land fill, borders, coastlines at 1.001..1.003) are
        // stacked within ~0.003 units; a wide near/far span (e.g. 0.02..200)
        // wrecks depth precision there and they z-fight. Hug the range to the
        // current altitude instead: near is a fraction of the height above the
        // surface (so the planet is never clipped), and far reaches just past
        // synchronous orbit so distant bodies still draw.
        let alt = self.radius() - 1.0;
        let near = (alt * 0.4).clamp(0.01, 1.0);
        let far = self.radius() + 60.0;
        DMat4::perspective_rh(self.fov_y, aspect, near, far) * self.view()
    }

    /// Advance along the orbit by `dt` seconds at the current speed.
    pub fn advance(&mut self, dt: f64) {
        self.phase = (self.phase + self.speed * dt).rem_euclid(std::f64::consts::TAU);
    }

    pub fn adjust_speed(&mut self, delta: f64) {
        self.speed = (self.speed + delta).clamp(FLY_SPEED_MIN, FLY_SPEED_MAX);
    }

    pub fn adjust_altitude(&mut self, delta_km: f64) {
        self.altitude_km = (self.altitude_km + delta_km).clamp(FLY_ALT_MIN, FLY_ALT_MAX);
    }

    pub fn adjust_inclination(&mut self, delta: f64) {
        self.inclination += delta;
    }

    pub fn adjust_raan(&mut self, delta: f64) {
        self.raan = (self.raan + delta).rem_euclid(std::f64::consts::TAU);
    }
}

/// Which rig is currently driving the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    /// Fixed on the Earth's centre (arcball [`OrbitCamera`]).
    Fixed,
    /// Riding an orbit, looking along velocity ([`FlyCamera`]).
    Fly,
}

/// Holds both rigs and the active [`CameraMode`], so toggling preserves each
/// camera's independent state. The renderer talks only to this wrapper.
pub struct CameraRig {
    pub mode: CameraMode,
    pub orbit: OrbitCamera,
    pub fly: FlyCamera,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            mode: CameraMode::Fixed,
            orbit: OrbitCamera::default(),
            fly: FlyCamera::default(),
        }
    }
}

impl CameraRig {
    pub fn toggle(&mut self) {
        self.mode = match self.mode {
            CameraMode::Fixed => CameraMode::Fly,
            CameraMode::Fly => CameraMode::Fixed,
        };
    }

    pub fn eye(&self) -> DVec3 {
        match self.mode {
            CameraMode::Fixed => self.orbit.eye(),
            CameraMode::Fly => self.fly.eye(),
        }
    }

    pub fn view_proj(&self, aspect: f64) -> DMat4 {
        match self.mode {
            CameraMode::Fixed => self.orbit.view_proj(aspect),
            CameraMode::Fly => self.fly.view_proj(aspect),
        }
    }

    /// Per-frame tick: only the fly camera advances along its orbit.
    pub fn advance(&mut self, dt: f64) {
        if self.mode == CameraMode::Fly {
            self.fly.advance(dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fly_eye_sits_on_the_orbit_radius() {
        let cam = FlyCamera::default();
        let expected = (EARTH_RADIUS_KM + cam.altitude_km) / EARTH_RADIUS_KM;
        // The eye must lie on the orbit ring at every phase, for any inclination.
        for k in 0..16 {
            let mut c = FlyCamera {
                phase: k as f64 * std::f64::consts::TAU / 16.0,
                ..FlyCamera::default()
            };
            c.raan = 0.7;
            assert!((c.eye().length() - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn fly_view_proj_is_finite_and_looks_along_velocity() {
        let cam = FlyCamera::default();
        let vp = cam.view_proj(16.0 / 9.0);
        assert!(vp.to_cols_array().iter().all(|x| x.is_finite()));
        // Default attitude: forward should align with the orbit velocity.
        let (pos, vel) = cam.pos_vel();
        let fwd = cam.view().inverse().transform_vector3(DVec3::NEG_Z);
        assert!(fwd.dot(vel) > 0.99, "camera should look along velocity");
        // And the eye is off-planet.
        assert!(pos.length() > 1.0);
    }

    #[test]
    fn advance_moves_along_orbit_and_wraps() {
        let mut cam = FlyCamera { speed: 1.0, phase: 0.0, ..FlyCamera::default() };
        cam.advance(0.5);
        assert!((cam.phase - 0.5).abs() < 1e-9);
        cam.advance(std::f64::consts::TAU); // wraps back into [0, TAU)
        assert!(cam.phase >= 0.0 && cam.phase < std::f64::consts::TAU);
    }

    #[test]
    fn rig_toggles_modes() {
        let mut rig = CameraRig::default();
        assert_eq!(rig.mode, CameraMode::Fixed);
        rig.toggle();
        assert_eq!(rig.mode, CameraMode::Fly);
        rig.toggle();
        assert_eq!(rig.mode, CameraMode::Fixed);
    }
}
