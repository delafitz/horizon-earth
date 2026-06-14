//! Window + event loop glue (winit 0.30 `ApplicationHandler`).
//!
//! Runs fullscreen borderless by default (screensaver feel) and exits on any
//! keyboard or mouse activity. Two env vars help during development:
//!   HORIZON_WINDOWED=1  -> run in a 1280x800 window instead of fullscreen
//!   HORIZON_NO_EXIT=1   -> don't quit on input (Escape still quits)

use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::renderer::Renderer;

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    start: Instant,
    last_cursor: Option<(f64, f64)>,
    no_exit: bool,
    windowed: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            renderer: None,
            start: Instant::now(),
            last_cursor: None,
            no_exit: std::env::var_os("HORIZON_NO_EXIT").is_some(),
            windowed: std::env::var_os("HORIZON_WINDOWED").is_some(),
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

            WindowEvent::MouseInput { state, .. } => {
                if state == ElementState::Pressed && !self.no_exit {
                    event_loop.exit();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let pos = (position.x, position.y);
                match self.last_cursor {
                    // First report establishes a baseline (avoids quitting on
                    // the cursor's initial placement).
                    None => self.last_cursor = Some(pos),
                    Some((x, y)) => {
                        let moved = ((pos.0 - x).powi(2) + (pos.1 - y).powi(2)).sqrt();
                        if moved > 6.0 && !self.no_exit {
                            event_loop.exit();
                        }
                    }
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
