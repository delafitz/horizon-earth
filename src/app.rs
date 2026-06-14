//! Window + event loop glue (winit 0.30 `ApplicationHandler`).
//!
//! Runs fullscreen borderless by default (screensaver feel) and exits on any
//! keyboard or mouse activity. Two env vars help during development:
//!   HORIZON_WINDOWED=1  -> run in a 1280x800 window instead of fullscreen
//!   HORIZON_NO_EXIT=1   -> don't quit on input (Escape still quits)

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::renderer::Renderer;

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    start: Instant,
    last_cursor: Option<(f64, f64)>,
    dragging: bool,
    no_exit: bool,
    windowed: bool,
}

impl App {
    pub fn new(windowed: bool, no_exit: bool) -> Self {
        Self {
            window: None,
            renderer: None,
            start: Instant::now(),
            last_cursor: None,
            dragging: false,
            no_exit,
            windowed,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let mut attrs = Window::default_attributes().with_title("Horizon Earth");
        if self.windowed {
            attrs = attrs.with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
        } else {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        // Hide the cursor in screensaver mode.
        window.set_cursor_visible(self.windowed);

        let renderer = pollster::block_on(Renderer::new(window.clone()));

        self.start = Instant::now();
        self.last_cursor = None;
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let escape = matches!(event.logical_key, Key::Named(NamedKey::Escape));
                    if escape || !self.no_exit {
                        event_loop.exit();
                    }
                }
            }

            // Interactive (HORIZON_NO_EXIT): left-drag orbits the camera.
            // Screensaver: any press is "activity" and quits.
            WindowEvent::MouseInput { state, button, .. } => {
                if self.no_exit {
                    if button == MouseButton::Left {
                        self.dragging = state == ElementState::Pressed;
                    }
                } else if state == ElementState::Pressed {
                    event_loop.exit();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.no_exit {
                    let y = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.0,
                    };
                    if let Some(r) = self.renderer.as_mut() {
                        r.zoom_camera(y * 0.1); // scroll up = zoom in
                    }
                } else {
                    event_loop.exit();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let pos = (position.x, position.y);
                let prev = self.last_cursor.replace(pos);
                let Some((x, y)) = prev else {
                    // First report just establishes a baseline.
                    return;
                };
                let (dx, dy) = ((pos.0 - x) as f32, (pos.1 - y) as f32);

                if self.no_exit {
                    if self.dragging {
                        if let Some(r) = self.renderer.as_mut() {
                            // Pixels -> radians; drag grabs and turns the globe.
                            const S: f32 = 0.005;
                            r.orbit_camera(-dx * S, dy * S);
                        }
                    }
                } else if (dx * dx + dy * dy).sqrt() > 6.0 {
                    event_loop.exit();
                }
            }

            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }

            WindowEvent::RedrawRequested => {
                let t = self.start.elapsed().as_secs_f32();
                if let (Some(r), Some(w)) = (self.renderer.as_mut(), self.window.as_ref()) {
                    r.update(t);
                    match r.render() {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            let size = w.inner_size();
                            r.resize(size.width, size.height);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => log::warn!("surface error: {e:?}"),
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
