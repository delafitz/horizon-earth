//! Horizon Earth — a Nord-themed 3D Earth visualization.
//!
//! Phase 1 (MVP): a fullscreen, rotating globe with coastlines and country
//! borders projected onto a sphere, an atmospheric rim glow, and a starfield
//! background. Exits on keyboard/mouse activity so it can grow into a
//! screensaver.

mod app;
mod data;
mod earth;
mod renderer;

use winit::event_loop::{ControlFlow, EventLoop};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    // Poll continuously so the globe animates every frame.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = app::App::new();
    event_loop.run_app(&mut app).expect("event loop error");
}
