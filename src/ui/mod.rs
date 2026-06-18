//! egui overlay: a right-side property editor (render/time params) and a
//! left-side display panel (status readout + tracked-object list).
//!
//! This module is deliberately self-contained: it owns the UI *state* and the
//! immediate-mode `draw` pass, reads the simulated [`World`] read-only, and
//! writes user edits back into [`UiState`]. The app loop reconciles that state
//! into the renderer/clock each frame, so nothing here touches wgpu directly.

use egui::{Color32, Context, RichText};

use horizon_core::category::Category;
use horizon_core::time::UNIX_EPOCH_JD;
use horizon_core::units::EARTH_RADIUS_KM;
use horizon_core::{Epoch, World};

/// Renderer knobs the property editor drives. Copied into the renderer each
/// frame (see `Renderer::set_settings`).
#[derive(Clone, Copy)]
pub struct RenderSettings {
    // --- Satellites ---
    // Size, labels, orbit-track and ground toggles are per-type (see TypeStyle);
    // these are shared appearance params driven by single uniforms.
    /// Near-side orbit-track opacity.
    pub track_alpha: f32,
    /// Far-side opacity multiplier for satellite layers (markers/tracks/ground),
    /// seen "through the glass".
    pub sat_back_alpha: f32,
    /// Ground-anchor (nadir line + footprint ring) stroke width in pixels.
    pub ground_width: f32,
    /// Ground-anchor opacity.
    pub ground_alpha: f32,
    /// Base on-screen height of satellite name labels, in pixels (at full,
    /// close-up size). The distance at which labels fade out scales off this.
    pub label_size: f32,
    /// Brightness multiplier on satellite label text.
    pub label_intensity: f32,
    /// Shared on-screen marker size multiplier across all satellite types.
    pub marker_size: f32,
    /// Shared brightness multiplier on satellite marker symbols.
    pub marker_intensity: f32,
    /// Per-type attributes (visibility, symbol, size, labels, track, ground),
    /// indexed parallel to [`CATEGORIES`].
    pub types: [TypeStyle; CATEGORIES.len()],

    // --- Land (coastlines / borders / fill) ---
    /// Multiplier on coastline & border line colour.
    pub line_brightness: f32,
    /// Opacity of the far-side ("through the glass") coastlines & borders.
    pub line_back_alpha: f32,
    /// Opacity of the translucent land fill.
    pub fill_alpha: f32,
    pub coast_visible: bool,
    /// Coastline stroke width in pixels.
    pub coast_width: f32,
    pub border_visible: bool,
    /// Country-border stroke width in pixels.
    pub border_width: f32,

    // --- Cities ---
    pub cities_show: bool,
    /// Minimum population for a city to be drawn.
    pub cities_min_pop: f32,
    /// City marker + label opacity.
    pub cities_alpha: f32,
    /// Brightness multiplier for cities on the daylit side of the terminator.
    pub cities_day_intensity: f32,
    /// Brightness multiplier for cities on the night side of the terminator.
    pub cities_night_intensity: f32,
    /// Whether city name labels are shown.
    pub cities_labels: bool,

    // --- Lighting ---
    /// Master toggle for the day/night terminator. When off, the globe, land
    /// lines and cities are lit uniformly (no night dimming, cities at their
    /// night intensity everywhere).
    pub night_mode: bool,

    // --- Tankers (live AIS, from horizon-ais collector) ---
    pub tankers_show: bool,

    // --- Atmosphere ---
    pub show_atmosphere: bool,
    /// Glow strength.
    pub atmo_intensity: f32,
    /// Shell reach above the surface (outer radius = 1.0 + this).
    pub atmo_thickness: f32,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            track_alpha: 0.35,
            sat_back_alpha: 0.15,
            ground_width: 1.5,
            ground_alpha: 0.5,
            label_size: 24.0,
            label_intensity: 1.0,
            marker_size: 1.0,
            marker_intensity: 1.0,
            types: default_types(),
            line_brightness: 1.0,
            line_back_alpha: 0.1,
            fill_alpha: 0.35,
            coast_visible: true,
            coast_width: 2.0,
            border_visible: true,
            border_width: 1.4,
            cities_show: true,
            cities_min_pop: 100_000.0,
            cities_alpha: 0.85,
            cities_day_intensity: 0.35,
            cities_night_intensity: 1.0,
            cities_labels: false,
            night_mode: true,
            tankers_show: true,
            show_atmosphere: true,
            atmo_intensity: 0.45,
            atmo_thickness: 0.06,
        }
    }
}

/// Index of `cat` within [`CATEGORIES`].
fn type_index(cat: Category) -> usize {
    CATEGORIES.iter().position(|&c| c == cat).unwrap_or(0)
}

impl RenderSettings {
    fn ty(&self, cat: Category) -> &TypeStyle {
        &self.types[type_index(cat)]
    }

    /// Marker `kind` for a body of category `cat`, after any per-type override.
    pub fn symbol_kind(&self, cat: Category) -> f32 {
        self.ty(cat).symbol.kind(cat)
    }

    /// Shared marker size multiplier — `0.0` when the type is hidden, which
    /// collapses its billboards to nothing.
    pub fn marker_scale(&self, cat: Category) -> f32 {
        if self.ty(cat).visible {
            self.marker_size
        } else {
            0.0
        }
    }

    /// Whether HUD labels for category `cat` should be drawn (per-type `labels`,
    /// gated by the type's master `visible`).
    pub fn label_visible(&self, cat: Category) -> bool {
        let t = self.ty(cat);
        t.visible && t.show_labels
    }

    /// Whether the ground anchor (nadir drop-line) for category `cat` should be
    /// drawn (per-type `ground`, gated by the type's master `visible`).
    pub fn ground_visible(&self, cat: Category) -> bool {
        let t = self.ty(cat);
        t.visible && t.show_ground
    }

    /// Whether the coverage zone (footprint ring) for category `cat` should be
    /// drawn (per-type `coverage`, gated by the type's master `visible`).
    pub fn coverage_visible(&self, cat: Category) -> bool {
        let t = self.ty(cat);
        t.visible && t.show_coverage
    }

    /// Whether the sub-satellite crosshatch (graticule-aligned "+") for category
    /// `cat` should be drawn (per-type `crosshatch`, gated by `visible`).
    pub fn crosshatch_visible(&self, cat: Category) -> bool {
        let t = self.ty(cat);
        t.visible && t.show_crosshatch
    }

    /// Whether orbit tracks for category `cat` should be drawn (hidden types
    /// suppress tracks regardless of the per-type track toggle).
    pub fn track_visible(&self, cat: Category) -> bool {
        let t = self.ty(cat);
        t.visible && t.show_track
    }

    /// Bitmask (one bit per [`CATEGORIES`] slot) of which types show orbit
    /// tracks — the renderer rebuilds the track buffer when this changes.
    pub fn track_mask(&self) -> u32 {
        let mut m = 0;
        for (i, t) in self.types.iter().enumerate() {
            if t.visible && t.show_track {
                m |= 1 << i;
            }
        }
        m
    }

    /// Coastline / border stroke widths in px, `0.0` when hidden (the thick-line
    /// shader collapses a zero-width segment to nothing).
    pub fn coast_width_px(&self) -> f32 {
        if self.coast_visible {
            self.coast_width
        } else {
            0.0
        }
    }
    pub fn border_width_px(&self) -> f32 {
        if self.border_visible {
            self.border_width
        } else {
            0.0
        }
    }

    /// Ground-anchor stroke width in px (shared appearance; per-type on/off is
    /// handled in the renderer's ground build loop).
    pub fn ground_width_px(&self) -> f32 {
        self.ground_width
    }
}

/// Per-satellite-type render attributes.
#[derive(Clone, Copy)]
pub struct TypeStyle {
    pub visible: bool,
    pub symbol: Symbol,
    /// Whether HUD labels are drawn for this type.
    pub show_labels: bool,
    pub show_track: bool,
    /// Whether the ground anchor (nadir drop-line to the sub-satellite point)
    /// is drawn.
    pub show_ground: bool,
    /// Whether the coverage zone (footprint / horizon ring) is drawn.
    pub show_coverage: bool,
    /// Whether the sub-satellite crosshatch (graticule-aligned "+") is drawn.
    pub show_crosshatch: bool,
    /// Max objects of this type to show (random subset). Capped at [`MAX_SAMPLE`].
    pub max_shown: usize,
}

impl Default for TypeStyle {
    fn default() -> Self {
        Self {
            visible: true,
            symbol: Symbol::Auto,
            show_labels: true,
            show_track: true,
            show_ground: true,
            show_coverage: true,
            show_crosshatch: false, // off by default — an extra mark, opt-in
            max_shown: DEFAULT_SHOWN,
        }
    }
}

/// Default per-type "max shown" cap (out-of-box load).
pub const DEFAULT_SHOWN: usize = 100;
/// Upper bound of the per-type "max shown" slider.
pub const MAX_SAMPLE: usize = 300;

/// Per-type upper bound for the "max shown" slider — Starlink can show its whole
/// constellation; the rest stay modest.
pub fn max_shown_cap(cat: Category) -> usize {
    match cat {
        Category::Starlink => 10_000,
        _ => MAX_SAMPLE,
    }
}

/// Default per-type styles: crewed stations and the (rare, notable) AST sats
/// start visible; the rest are toggled on from the panel as wanted (keeps the
/// out-of-box view uncluttered).
fn default_types() -> [TypeStyle; CATEGORIES.len()] {
    let mut types = [TypeStyle::default(); CATEGORIES.len()];
    for (i, t) in types.iter_mut().enumerate() {
        t.visible = matches!(CATEGORIES[i], Category::Station | Category::Ast);
    }
    types
}

/// The categories shown (in order) in the per-type symbol editor.
pub const CATEGORIES: [Category; 7] = [
    Category::Station,
    Category::Ast,
    Category::Leo,
    Category::Starlink,
    Category::Gnss,
    Category::Geo,
    Category::Other,
];

/// Per-type marker symbol choice. `Auto` defers to the category's own default.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    Auto,
    Box,
    Filled,
    Triangle,
}

impl Symbol {
    const ALL: [Symbol; 4] = [Symbol::Auto, Symbol::Box, Symbol::Filled, Symbol::Triangle];

    /// Marker-shader `kind` value (0 = outline box, 1 = filled, 2 = wire triangle).
    fn kind(self, cat: Category) -> f32 {
        match self {
            Symbol::Auto => cat.marker_kind(),
            Symbol::Box => 0.0,
            Symbol::Filled => 1.0,
            Symbol::Triangle => 2.0,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Symbol::Auto => "Auto",
            Symbol::Box => "Box",
            Symbol::Filled => "Filled",
            Symbol::Triangle => "Triangle",
        }
    }
}

/// All UI-owned state. The app reads `live`/`time_scale`/`settings` back out
/// after each `draw` and applies them to the clock and renderer.
pub struct UiState {
    pub settings: RenderSettings,
    /// `true` = live wall-clock time, `false` = accelerated demo time.
    pub live: bool,
    /// Demo speed: simulated seconds per real second.
    pub time_scale: f64,
    /// Index into `World::bodies` of the inspected object, if any.
    pub selected: Option<usize>,
    /// Case-insensitive substring filter for the body list.
    pub filter: String,
    /// Set when a per-type "max shown" slider changes; the app re-samples the
    /// group (per type) and clears it.
    pub resample: bool,
}

impl UiState {
    pub fn new(live: bool, time_scale: f64) -> Self {
        Self {
            settings: RenderSettings::default(),
            live,
            time_scale,
            selected: None,
            filter: String::new(),
            resample: false,
        }
    }
}

/// Per-frame scalars the panels show that aren't derivable from `World` alone.
pub struct FrameInfo {
    pub fps: f32,
    pub gmst_deg: f64,
    /// Camera distance from Earth-center, in Earth radii (the zoom level).
    pub zoom: f64,
}

/// Install the lightweight, translucent "Nord wireframe" theme: panels tinted
/// dark but see-through over the globe, widgets drawn as thin frost outlines
/// rather than filled blocks, and no drop shadows.
pub fn install_theme(ctx: &Context) {
    use egui::style::{Selection, WidgetVisuals, Widgets};
    use egui::{FontData, FontDefinitions, FontFamily, Rounding, Stroke, Style, Visuals};

    // Geo (embedded) as the default proportional + monospace face. Falls in
    // front of egui's built-ins so all panel text uses it, with the originals
    // kept as glyph fallbacks.
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "geo".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Geo-Regular.ttf")),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().insert(0, "geo".to_owned());
    }
    ctx.set_fonts(fonts);

    // Nord palette (opaque, for strokes and text).
    let nord3 = Color32::from_rgb(0x4C, 0x56, 0x6A); // dim outline
    let nord4 = Color32::from_rgb(0xD8, 0xDE, 0xE9); // body text
    let nord6 = Color32::from_rgb(0xEC, 0xEF, 0xF4); // bright text
    let nord8 = Color32::from_rgb(0x88, 0xC0, 0xD0); // frost accent

    // Translucent fills so the 3D scene shows through the chrome (lower alpha
    // = glassier).
    let panel = Color32::from_rgba_unmultiplied(0x2E, 0x34, 0x40, 105);
    let window = Color32::from_rgba_unmultiplied(0x3B, 0x42, 0x52, 120);
    let field = Color32::from_rgba_unmultiplied(0x2E, 0x34, 0x40, 140);
    let hover = Color32::from_rgba_unmultiplied(0x43, 0x4C, 0x5E, 90);
    let press = Color32::from_rgba_unmultiplied(0x4C, 0x56, 0x6A, 120);
    let select = Color32::from_rgba_unmultiplied(0x5E, 0x81, 0xAC, 110);

    let rounding = Rounding::same(2.0);
    let line = |c| Stroke::new(1.0, c);
    let widget = |fill: Color32, stroke: Color32, text: Color32, expansion: f32| WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: line(stroke),
        rounding,
        fg_stroke: line(text),
        expansion,
    };

    let widgets = Widgets {
        // Labels / separators / panel chrome: no fill, faint outline.
        noninteractive: widget(Color32::TRANSPARENT, nord3, nord4, 0.0),
        // Idle controls: pure wireframe — outline only, no fill.
        inactive: widget(Color32::TRANSPARENT, nord3, nord4, 0.0),
        hovered: widget(hover, nord8, nord6, 1.0),
        active: widget(press, nord8, nord6, 1.0),
        open: widget(window, nord8, nord6, 0.0),
    };

    let visuals = Visuals {
        widgets,
        selection: Selection { bg_fill: select, stroke: line(nord8) },
        hyperlink_color: nord8,
        faint_bg_color: Color32::from_rgba_unmultiplied(0x43, 0x4C, 0x5E, 40),
        extreme_bg_color: field,
        panel_fill: panel,
        window_fill: window,
        window_stroke: line(nord3),
        window_rounding: rounding,
        window_shadow: egui::epaint::Shadow::NONE,
        popup_shadow: egui::epaint::Shadow::NONE,
        ..Visuals::dark()
    };

    let mut style = Style::default();
    style.visuals = visuals;
    // A little extra spacing reads as "lightweight".
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}

/// Build both panels for this frame.
pub fn draw(ctx: &Context, ui: &mut UiState, world: &World, info: &FrameInfo) {
    properties_panel(ctx, ui, world);
    display_panel(ctx, ui, world, info);
}

/// The floating translucent "card" frame shared by both side panels: inset from
/// the window edges (so the globe wraps around them) with a thin frost border.
fn panel_frame(ctx: &Context) -> egui::Frame {
    let style = ctx.style();
    egui::Frame {
        fill: style.visuals.panel_fill,
        stroke: style.visuals.window_stroke,
        rounding: egui::Rounding::same(3.0),
        inner_margin: egui::Margin::symmetric(10.0, 8.0),
        outer_margin: egui::Margin::same(8.0),
        ..egui::Frame::none()
    }
}

/// Right side: render + time parameters, plus an inspector for the selected
/// body.
fn properties_panel(ctx: &Context, ui: &mut UiState, world: &World) {
    egui::SidePanel::right("properties")
        .resizable(true)
        .default_width(240.0)
        .frame(panel_frame(ctx))
        .show(ctx, |p| {
            p.heading("Properties");
            p.separator();

            p.label(RichText::new("TIME").weak());
            p.horizontal(|h| {
                h.selectable_value(&mut ui.live, true, "Live");
                h.selectable_value(&mut ui.live, false, "Demo");
            });
            p.add_enabled(
                !ui.live,
                egui::Slider::new(&mut ui.time_scale, 100.0..=1000.0)
                    .text("sim ×"),
            );

            let selected = ui.selected;
            let mut request_resample = false;
            let s = &mut ui.settings;
            p.add_space(6.0);

            p.checkbox(&mut s.night_mode, "Day / night shading");
            p.add_space(6.0);

            egui::CollapsingHeader::new("Satellites")
                .default_open(true)
                .show(p, |c| {
                    // Shared appearance (single uniforms); per-type size/labels/
                    // tracks/ground toggles live in the type nodes below.
                    c.add(egui::Slider::new(&mut s.track_alpha, 0.0..=1.0).text("track opacity"));
                    c.add(
                        egui::Slider::new(&mut s.sat_back_alpha, 0.0..=1.0)
                            .text("far-side opacity"),
                    );
                    c.add(
                        egui::Slider::new(&mut s.marker_size, 0.25..=4.0).text("symbol size ×"),
                    );
                    c.add(
                        egui::Slider::new(&mut s.marker_intensity, 0.0..=2.0)
                            .text("symbol intensity"),
                    );
                    c.add(
                        egui::Slider::new(&mut s.label_size, 8.0..=48.0).text("label size px"),
                    );
                    c.add(
                        egui::Slider::new(&mut s.label_intensity, 0.0..=2.0)
                            .text("label intensity"),
                    );

                    egui::CollapsingHeader::new("Ground anchors")
                        .id_salt("ground")
                        .default_open(false)
                        .show(c, |g| {
                            g.add(egui::Slider::new(&mut s.ground_width, 0.5..=6.0).text("width px"));
                            g.add(egui::Slider::new(&mut s.ground_alpha, 0.0..=1.0).text("opacity"));
                        });

                    c.add_space(4.0);
                    c.label(RichText::new("BY TYPE").weak());
                    for (i, &cat) in CATEGORIES.iter().enumerate() {
                        let t = &mut s.types[i];
                        // Custom collapsing node so the visibility checkbox lives
                        // on the header row (toggle a whole type without expanding).
                        let id = c.make_persistent_id(("type", i));
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            c.ctx(),
                            id,
                            false,
                        )
                        .show_header(c, |h| {
                            h.checkbox(&mut t.visible, "");
                            h.label(RichText::new(category_label(cat)).color(nord(cat.color())));
                        })
                        .body(|b| {
                            // Max objects of this type to show (random subset).
                            let resp = b.add(
                                egui::Slider::new(&mut t.max_shown, 0..=max_shown_cap(cat))
                                    .logarithmic(true)
                                    .text("max shown"),
                            );
                            if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                                request_resample = true;
                            }
                            b.horizontal(|h| {
                                h.label("symbol");
                                egui::ComboBox::from_id_salt(("symbol", i))
                                    .selected_text(t.symbol.label())
                                    .width(104.0)
                                    .show_ui(h, |cb| {
                                        for opt in Symbol::ALL {
                                            cb.selectable_value(&mut t.symbol, opt, opt.label());
                                        }
                                    });
                            });
                            b.checkbox(&mut t.show_labels, "labels");
                            b.checkbox(&mut t.show_track, "orbit track");
                            b.checkbox(&mut t.show_ground, "ground anchor");
                            b.checkbox(&mut t.show_coverage, "coverage zone");
                            b.checkbox(&mut t.show_crosshatch, "crosshatch");
                        });
                    }
                });

            egui::CollapsingHeader::new("Land")
                .default_open(false)
                .show(p, |c| {
                    // Shared across both line layers (one brightness / far-side
                    // alpha uniform drives coastlines and borders alike).
                    c.add(
                        egui::Slider::new(&mut s.line_brightness, 0.2..=2.0)
                            .text("line brightness"),
                    );
                    c.add(
                        egui::Slider::new(&mut s.line_back_alpha, 0.0..=1.0)
                            .text("far-side lines"),
                    );
                    c.add(egui::Slider::new(&mut s.fill_alpha, 0.0..=1.0).text("land fill"));

                    egui::CollapsingHeader::new("Coastlines")
                        .id_salt("coastlines")
                        .default_open(true)
                        .show(c, |g| {
                            g.checkbox(&mut s.coast_visible, "visible");
                            g.add_enabled(
                                s.coast_visible,
                                egui::Slider::new(&mut s.coast_width, 0.5..=6.0).text("width px"),
                            );
                        });
                    egui::CollapsingHeader::new("Borders")
                        .id_salt("borders")
                        .default_open(true)
                        .show(c, |g| {
                            g.checkbox(&mut s.border_visible, "visible");
                            g.add_enabled(
                                s.border_visible,
                                egui::Slider::new(&mut s.border_width, 0.5..=6.0).text("width px"),
                            );
                        });
                });

            egui::CollapsingHeader::new("Cities")
                .default_open(false)
                .show(p, |c| {
                    c.checkbox(&mut s.cities_show, "visible");
                    c.add_enabled_ui(s.cities_show, |c| {
                        c.add(
                            egui::Slider::new(&mut s.cities_min_pop, 100_000.0..=50_000_000.0)
                                .text("min population"),
                        );
                        c.add(egui::Slider::new(&mut s.cities_alpha, 0.0..=1.0).text("opacity"));
                        c.add(
                            egui::Slider::new(&mut s.cities_night_intensity, 0.0..=2.0)
                                .text("night intensity"),
                        );
                        c.add(
                            egui::Slider::new(&mut s.cities_day_intensity, 0.0..=2.0)
                                .text("day intensity"),
                        );
                        c.checkbox(&mut s.cities_labels, "labels");
                    });
                });

            egui::CollapsingHeader::new("Tankers")
                .default_open(false)
                .show(p, |c| {
                    c.checkbox(&mut s.tankers_show, "visible");
                    c.weak("live AIS — run the horizon-ais collector");
                });

            egui::CollapsingHeader::new("Atmosphere")
                .default_open(false)
                .show(p, |c| {
                    c.checkbox(&mut s.show_atmosphere, "enabled");
                    c.add_enabled_ui(s.show_atmosphere, |c| {
                        c.add(
                            egui::Slider::new(&mut s.atmo_intensity, 0.0..=1.5).text("intensity"),
                        );
                        c.add(
                            egui::Slider::new(&mut s.atmo_thickness, 0.0..=0.25).text("depth"),
                        );
                    });
                });

            p.add_space(6.0);
            p.separator();
            match selected {
                Some(i) if i < world.bodies.len() => inspector(p, world, i),
                _ => {
                    p.weak("No body selected — pick one from the list.");
                }
            }

            // Flag a re-sample now that the settings borrow is done.
            if request_resample {
                ui.resample = true;
            }
        });
}

/// The selected-body detail block.
fn inspector(p: &mut egui::Ui, world: &World, i: usize) {
    let b = &world.bodies[i];
    let col = nord(b.color);
    p.heading(RichText::new(&b.name).color(col));
    let pos = world.body_position_eci(i);
    let alt = (pos.length() - EARTH_RADIUS_KM).max(0.0);
    let (lat, lon) = world.body_latlon(i);
    egui::Grid::new("inspector").num_columns(2).show(p, |g| {
        g.label("category");
        g.label(category_label(b.category));
        g.end_row();
        g.label("altitude");
        g.label(format!("{alt:.0} km"));
        g.end_row();
        g.label("latitude");
        g.label(format!("{lat:.2}°"));
        g.end_row();
        g.label("longitude");
        g.label(format!("{lon:.2}°"));
        g.end_row();
    });
}

/// Left side: clock/status readout and the scrollable, filterable body list.
fn display_panel(ctx: &Context, ui: &mut UiState, world: &World, info: &FrameInfo) {
    egui::SidePanel::left("display")
        .resizable(true)
        .default_width(260.0)
        .frame(panel_frame(ctx))
        .show(ctx, |p| {
            p.heading("Horizon");
            p.separator();

            egui::Grid::new("status").num_columns(2).show(p, |g| {
                g.label("UTC");
                g.label(format_utc(world.current()));
                g.end_row();
                g.label("GMST");
                g.label(format!("{:.2}°", info.gmst_deg));
                g.end_row();
                g.label("mode");
                g.label(if ui.live { "live" } else { "demo" });
                g.end_row();
                g.label("objects");
                g.label(format!("{}", world.bodies.len()));
                g.end_row();
                g.label("zoom");
                g.label(format!("{:.1} R⊕", info.zoom));
                g.end_row();
                g.label("fps");
                g.label(format!("{:.0}", info.fps));
                g.end_row();
            });

            p.add_space(8.0);
            p.separator();
            p.horizontal(|h| {
                h.label("Filter");
                h.text_edit_singleline(&mut ui.filter);
                if h.button("✕").clicked() {
                    ui.filter.clear();
                }
            });

            // Indices passing the name filter, kept so the scroll area can cull
            // to only the visible rows (cheap even for large constellations).
            let needle = ui.filter.to_ascii_uppercase();
            let rows: Vec<usize> = world
                .bodies
                .iter()
                .enumerate()
                .filter(|(_, b)| needle.is_empty() || b.name.to_ascii_uppercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
            p.weak(format!("{} shown", rows.len()));

            let row_h = p.text_style_height(&egui::TextStyle::Body) + 4.0;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(p, row_h, rows.len(), |p, range| {
                    for r in range {
                        let i = rows[r];
                        let b = &world.bodies[i];
                        let label = RichText::new(&b.name).color(nord(b.color));
                        if p.selectable_label(ui.selected == Some(i), label).clicked() {
                            ui.selected = Some(i);
                        }
                    }
                });
        });
}

/// Linear-RGB body colour → an egui sRGB colour. The renderer writes to a
/// non-sRGB target, so these floats are already display values.
fn nord(c: [f32; 3]) -> Color32 {
    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgb(to(c[0]), to(c[1]), to(c[2]))
}

fn category_label(c: Category) -> &'static str {
    match c {
        Category::Station => "Station",
        Category::Ast => "AST",
        Category::Leo => "LEO",
        Category::Starlink => "Starlink",
        Category::Gnss => "GNSS",
        Category::Geo => "GEO",
        Category::Other => "Other",
    }
}

/// `Epoch` (Julian Date, UTC) → `YYYY-MM-DD HH:MM:SSZ`, no external date crate.
fn format_utc(e: Epoch) -> String {
    let unix = (e.jd - UNIX_EPOCH_JD) * 86_400.0;
    let days = (unix / 86_400.0).floor() as i64;
    let sod = (unix - days as f64 * 86_400.0) as i64;
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}Z")
}

/// Days since 1970-01-01 → civil (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}
