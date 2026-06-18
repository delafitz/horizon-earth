//! Window + event loop glue (winit 0.30 `ApplicationHandler`).
//!
//! Runs fullscreen borderless by default (screensaver feel) and exits on any
//! keyboard or mouse activity. Two env vars help during development:
//!   HORIZON_WINDOWED=1  -> run in a 1280x800 window instead of fullscreen
//!   HORIZON_NO_EXIT=1   -> don't quit on input (Escape still quits)

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use horizon_core::world::{bodies_from_elements, DEFAULT_TIME_SCALE};
use horizon_core::{Category, Epoch, World};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::renderer::{EguiFrame, Renderer};
use crate::ui::{self, UiState};

/// How the simulation clock advances.
#[derive(Clone, Copy, PartialEq)]
pub enum TimeMode {
    /// Real wall-clock time — bodies sit at their true current positions.
    Live,
    /// Accelerated from startup, for lively eye-candy.
    Demo,
}

/// Wall-clock UTC "now" as an [`Epoch`].
fn now_epoch() -> Epoch {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Epoch::from_unix_seconds(secs)
}

/// How long a cached TLE set is considered fresh.
const TLE_MAX_AGE: Duration = Duration::from_secs(6 * 3600);

/// Classify every element once (building a propagator per set, for the orbital
/// period), so the per-type sampler can bucket without rebuilding propagators on
/// every re-sample. Elements that fail to initialise classify to `None`.
fn classify_elements(els: &[horizon_core::Elements]) -> Vec<Option<Category>> {
    use horizon_core::{Propagator, Sgp4Orbit};
    els.iter()
        .map(|el| {
            Sgp4Orbit::from_elements(el).ok().map(|m| {
                let name = el.object_name.as_deref().unwrap_or("");
                Category::classify(name, m.period())
            })
        })
        .collect()
}

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    start: Instant,
    epoch0: Epoch,
    time_mode: TimeMode,
    group: String,
    offline: bool,
    /// All element sets loaded for the group; re-sampled (per type) when a
    /// panel slider changes a type's shown count.
    elements: Vec<horizon_core::Elements>,
    /// Category of each element in `elements` (classified once at load), so the
    /// per-type sampler can bucket without rebuilding every propagator.
    element_cats: Vec<Option<Category>>,
    last_cursor: Option<(f64, f64)>,
    dragging: bool,
    /// Current keyboard modifiers, tracked for trackpad scroll gestures.
    modifiers: ModifiersState,
    no_exit: bool,
    windowed: bool,

    // egui overlay (interactive mode only). The context is cheap to hold even
    // when unused; the winit integration is created once the window exists.
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    ui: UiState,
    /// Demo time-scale in effect at the last frame; a change re-baselines the
    /// clock so adjusting the slider doesn't jump the simulation.
    last_time_scale: f64,
    last_frame: Instant,
    fps: f32,

    // [tankers] Reload cache/tankers.json (from the horizon-ais collector) when
    // it changes; `mtime` gates re-reads, `check_at` throttles the stat call.
    tanker_mtime: Option<SystemTime>,
    tanker_check_at: Instant,
}

impl App {
    pub fn new(windowed: bool, no_exit: bool, demo: bool, group: String, offline: bool) -> Self {
        Self {
            window: None,
            renderer: None,
            start: Instant::now(),
            epoch0: now_epoch(),
            time_mode: if demo { TimeMode::Demo } else { TimeMode::Live },
            group,
            offline,
            elements: Vec::new(),
            element_cats: Vec::new(),
            last_cursor: None,
            dragging: false,
            modifiers: ModifiersState::empty(),
            no_exit,
            windowed,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            ui: UiState::new(!demo, DEFAULT_TIME_SCALE),
            last_time_scale: DEFAULT_TIME_SCALE,
            last_frame: Instant::now(),
            fps: 0.0,
            tanker_mtime: None,
            tanker_check_at: Instant::now(),
        }
    }

    /// The simulation instant to render this frame.
    fn current_epoch(&self) -> Epoch {
        match self.time_mode {
            TimeMode::Live => now_epoch(),
            TimeMode::Demo => self
                .epoch0
                .plus_seconds(self.start.elapsed().as_secs_f64() * self.ui.time_scale),
        }
    }

    /// Load the configured group's element sets (fresh cache → network → stale),
    /// returning an empty vec on failure (the caller drops to the demo world).
    fn load_elements(&self) -> Vec<horizon_core::Elements> {
        let cache = Path::new("cache");
        let loaded = if self.offline {
            horizon_data::load_cached(&self.group, cache)
        } else {
            horizon_data::load_group(&self.group, cache, TLE_MAX_AGE)
        };
        match loaded {
            Ok(els) => els,
            Err(e) => {
                log::warn!("TLE load failed ({e}); using demo constellation");
                Vec::new()
            }
        }
    }

    /// Per-type "max shown" caps, indexed parallel to [`crate::ui::CATEGORIES`].
    fn type_caps(&self) -> Vec<usize> {
        self.ui.settings.types.iter().map(|t| t.max_shown).collect()
    }

    /// Build the world by sampling each category down to its cap: for every
    /// [`crate::ui::CATEGORIES`] entry, take a random `min(available, cap)`
    /// subset of that type's elements. Falls back to the demo constellation when
    /// nothing usable results.
    fn world_from_caps(&self, caps: &[usize]) -> World {
        if self.elements.is_empty() {
            return World::demo(self.epoch0);
        }
        // One xorshift64* RNG for the whole selection (seeded from the clock).
        let mut state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            | 1;
        let mut rng = move || {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545F4914F6CDD1D)
        };

        let mut chosen: Vec<usize> = Vec::new();
        for (ci, cat) in crate::ui::CATEGORIES.iter().enumerate() {
            let cap = caps.get(ci).copied().unwrap_or(0);
            if cap == 0 {
                continue;
            }
            // Indices of elements in this category; partial Fisher–Yates picks
            // the first `n` after shuffling.
            let mut idxs: Vec<usize> = (0..self.elements.len())
                .filter(|&i| self.element_cats.get(i) == Some(&Some(*cat)))
                .collect();
            let n = cap.min(idxs.len());
            for k in 0..n {
                let j = k + (rng() as usize) % (idxs.len() - k);
                idxs.swap(k, j);
            }
            chosen.extend_from_slice(&idxs[..n]);
        }

        let els: Vec<_> = chosen.iter().map(|&i| self.elements[i].clone()).collect();
        let bodies = bodies_from_elements(&els);
        if bodies.is_empty() {
            log::warn!("group '{}' produced no usable bodies; using demo", self.group);
            return World::demo(self.epoch0);
        }
        log::info!("showing {} of {} '{}' objects (per-type caps)",
            bodies.len(), self.elements.len(), self.group);
        World::new(self.epoch0, bodies)
    }

    /// Interactive keyboard shortcuts (not consumed by egui).
    ///   T          toggle live/demo time (reconciled via `ui.live`)
    ///   F          toggle Fixed (Earth-centred) / Fly (orbit-riding) camera
    /// Fly mode only:
    ///   arrows     yaw (left/right), pitch (up/down)
    ///   Q / E      roll
    ///   Z / X      orbit speed  -/+
    ///   G / H      altitude      -/+
    ///   C / V      inclination   -/+
    ///   B / N      RAAN          -/+
    fn handle_shortcut(&mut self, key: &Key) {
        if let Key::Character(s) = key {
            if s.eq_ignore_ascii_case("t") {
                self.ui.live = !self.ui.live;
                return;
            }
            if s.eq_ignore_ascii_case("f") {
                if let Some(r) = self.renderer.as_mut() {
                    r.toggle_camera();
                }
                return;
            }
            if s.eq_ignore_ascii_case("l") {
                if let Some(r) = self.renderer.as_mut() {
                    r.toggle_hires(); // allow/suppress the high-detail (50m) tier
                }
                return;
            }
        }

        let Some(r) = self.renderer.as_mut() else {
            return;
        };
        if !r.is_fly_mode() {
            return; // the remaining controls steer the fly camera
        }

        const ANG: f32 = 0.0349; // ~2 degrees per press
        match key {
            Key::Named(NamedKey::ArrowLeft) => r.fly_look(-ANG, 0.0, 0.0),
            Key::Named(NamedKey::ArrowRight) => r.fly_look(ANG, 0.0, 0.0),
            Key::Named(NamedKey::ArrowUp) => r.fly_look(0.0, ANG, 0.0),
            Key::Named(NamedKey::ArrowDown) => r.fly_look(0.0, -ANG, 0.0),
            Key::Character(s) => match s.to_ascii_lowercase().as_str() {
                "q" => r.fly_look(0.0, 0.0, -ANG),
                "e" => r.fly_look(0.0, 0.0, ANG),
                "z" => r.fly_adjust_speed(-0.05),
                "x" => r.fly_adjust_speed(0.05),
                "g" => r.fly_adjust_altitude(-50.0),
                "h" => r.fly_adjust_altitude(50.0),
                "c" => r.fly_adjust_inclination(-ANG),
                "v" => r.fly_adjust_inclination(ANG),
                "b" => r.fly_adjust_raan(-ANG),
                "n" => r.fly_adjust_raan(ANG),
                _ => {}
            },
            _ => {}
        }
    }

    /// Render one frame: reconcile UI-driven state, advance the world, build the
    /// egui overlay (interactive mode only), and present.
    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        // Reconcile time mode from the UI/keyboard toggle, re-baselining the
        // demo clock on a change so it doesn't jump.
        let want = if self.ui.live { TimeMode::Live } else { TimeMode::Demo };
        if want != self.time_mode {
            self.time_mode = want;
            self.start = Instant::now();
            self.epoch0 = now_epoch();
        }

        // Re-baseline the demo clock when the time-scale slider changes so the
        // simulation continues from its current instant instead of jumping.
        if self.ui.time_scale != self.last_time_scale {
            if self.time_mode == TimeMode::Demo {
                self.epoch0 = self
                    .epoch0
                    .plus_seconds(self.start.elapsed().as_secs_f64() * self.last_time_scale);
                self.start = Instant::now();
            }
            self.last_time_scale = self.ui.time_scale;
        }

        // Re-sample the group when a per-type "max shown" slider committed.
        if self.ui.resample {
            self.ui.resample = false;
            self.ui.selected = None;
            let caps = self.type_caps();
            let world = self.world_from_caps(&caps);
            if let Some(r) = self.renderer.as_mut() {
                r.set_world(world);
            }
        }

        // [tankers] Reload the AIS cache when the collector rewrites it (stat
        // throttled to a few seconds; rebuild only on an mtime change).
        if Instant::now() >= self.tanker_check_at {
            self.tanker_check_at = Instant::now() + Duration::from_secs(3);
            let path = Path::new("cache/tankers.json");
            let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            if mtime != self.tanker_mtime {
                self.tanker_mtime = mtime;
                let tankers = crate::tankers::load(path);
                let (tris, tracks) = crate::tankers::build_geometry(&tankers);
                log::info!("loaded {} tankers", tankers.len());
                if let Some(r) = self.renderer.as_mut() {
                    r.set_tankers(tris, tracks);
                }
            }
        }

        let now = self.current_epoch();

        // Exponential moving-average FPS for the status readout.
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();
        if dt > 0.0 {
            self.fps = if self.fps == 0.0 {
                1.0 / dt
            } else {
                self.fps * 0.9 + 0.1 / dt
            };
        }

        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };

        // Push current parameters in, advance the camera along its orbit (fly
        // mode), then advance the world + GPU buffers.
        renderer.set_settings(self.ui.settings);
        renderer.advance_camera(dt);
        renderer.update(now);

        // Build the egui overlay. The world is borrowed read-only for the UI
        // pass, then released before the mutable render call below.
        let egui_data = if self.no_exit {
            self.egui_state.as_mut().map(|state| {
                let raw = state.take_egui_input(&window);
                let info = ui::FrameInfo {
                    fps: self.fps,
                    gmst_deg: renderer.world().earth_rotation().to_degrees(),
                    zoom: renderer.camera_distance(),
                };
                let ui_state = &mut self.ui;
                let world = renderer.world();
                let out = self
                    .egui_ctx
                    .run(raw, |ctx| ui::draw(ctx, ui_state, world, &info));
                state.handle_platform_output(&window, out.platform_output);
                let prims = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
                (prims, out.textures_delta, out.pixels_per_point)
            })
        } else {
            None
        };

        let frame = egui_data.as_ref().map(|(prims, td, ppp)| EguiFrame {
            primitives: prims,
            textures_delta: td,
            pixels_per_point: *ppp,
        });

        match renderer.render(frame) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let size = window.inner_size();
                renderer.resize(size.width, size.height);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
            Err(e) => log::warn!("surface error: {e:?}"),
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

        // egui winit integration (used only in interactive mode, but cheap to
        // wire up unconditionally).
        ui::install_theme(&self.egui_ctx);
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            Some(2048),
        ));

        self.start = Instant::now();
        self.epoch0 = now_epoch();

        // Load + classify the group once; the per-type sliders re-sample live.
        self.elements = self.load_elements();
        self.element_cats = classify_elements(&self.elements);
        // Each type keeps its default cap; the per-type "max shown" sliders are
        // the sole control over how many of each category are shown.
        let caps = self.type_caps();
        let world = self.world_from_caps(&caps);
        let renderer = pollster::block_on(Renderer::new(window.clone(), world));

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
        // Let egui see the event first (interactive mode). If it consumes the
        // event — e.g. a pointer over a panel or typing in the filter box — we
        // skip the camera/exit handling below so the UI and globe don't fight.
        let mut egui_consumed = false;
        if self.no_exit {
            if let Some(window) = self.window.as_ref() {
                if let Some(state) = self.egui_state.as_mut() {
                    egui_consumed = state.on_window_event(window, &event).consumed;
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let escape = matches!(event.logical_key, Key::Named(NamedKey::Escape));
                    if escape || !self.no_exit {
                        event_loop.exit();
                        return;
                    }
                    // Interactive-only shortcuts (skipped when egui is using the
                    // key, e.g. typing in the filter box).
                    if !egui_consumed {
                        self.handle_shortcut(&event.logical_key);
                    }
                }
            }

            // Interactive (HORIZON_NO_EXIT): left-drag orbits the camera.
            // Screensaver: any press is "activity" and quits.
            WindowEvent::MouseInput { state, button, .. } => {
                if self.no_exit {
                    // A press over a panel belongs to egui, not the camera.
                    if button == MouseButton::Left {
                        self.dragging = state == ElementState::Pressed && !egui_consumed;
                    }
                } else if state == ElementState::Pressed {
                    event_loop.exit();
                }
            }

            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.no_exit {
                    if egui_consumed {
                        return; // scrolling a panel/list, not navigating the globe
                    }
                    // Trackpad/wheel deltas in roughly pixel units.
                    let (dx, dy) = match delta {
                        MouseScrollDelta::LineDelta(x, y) => (x * 8.0, y * 8.0),
                        MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
                    };
                    let mods = self.modifiers;
                    if let Some(r) = self.renderer.as_mut() {
                        // Fixed/realtime mode only; fly mode is keyboard-driven.
                        if !r.is_fly_mode() {
                            const ROT: f32 = 0.005; // rad per pixel
                            if mods.shift_key() {
                                // Shift: orbit the view — the whole scene (Earth
                                // + the satellites attached to it) turns together.
                                r.orbit_camera(-dx * ROT, dy * ROT);
                            } else {
                                r.zoom_camera(dy * 0.002); // plain: scroll up = zoom in
                            }
                        }
                    }
                } else {
                    event_loop.exit();
                }
            }

            // Trackpad two-finger rotate -> roll the camera (tilt the horizon).
            WindowEvent::RotationGesture { delta, .. } => {
                if self.no_exit {
                    if !egui_consumed {
                        if let Some(r) = self.renderer.as_mut() {
                            r.roll_camera(delta.to_radians());
                        }
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

            WindowEvent::RedrawRequested => self.redraw(event_loop),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
