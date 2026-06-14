//! Horizon simulation core.
//!
//! Engine-agnostic, double-precision model of the world: physical units and
//! constants, coordinate frames, orbital motion, a simulation clock, and the
//! viewing camera. This crate has **no rendering or windowing dependencies**
//! (only `glam` for vector/matrix math) so the same model can drive the
//! current hand-rolled wgpu renderer today, or a different backend later.
//!
//! Conventions:
//! - Physical quantities are SI-ish: **kilometres** for length, **seconds** for
//!   time, radians for angles, all `f64`.
//! - The physics frame is **ECI (Earth-centred inertial), Z-up** — standard
//!   astrodynamics, equatorial plane in XY, north pole +Z.
//! - The *render* frame is **Y-up with the Earth radius normalised to 1.0**
//!   (what the GPU sees). [`frames::eci_to_world`] is the single bridge between
//!   the two; everything physical stays in ECI/km until then.

pub mod camera;
pub mod category;
pub mod frames;
pub mod orbit;
pub mod time;
pub mod units;
pub mod world;

pub use camera::OrbitCamera;
pub use category::Category;
pub use orbit::{KeplerOrbit, Propagator, Sgp4Orbit};
pub use time::{gmst, Epoch};
pub use world::{Body, World};

/// Re-exported so downstream crates (e.g. the data fetcher) can parse TLE/OMM
/// element sets without depending on the `sgp4` crate directly.
pub use sgp4::Elements;
