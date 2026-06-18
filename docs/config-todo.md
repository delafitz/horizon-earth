# Config / egui TODO

Knobs we want to expose in `RenderSettings` + the Properties panel
(`src/ui/mod.rs`) but that are currently shared or hardcoded.

## 1. Border vs. coastline intensity (separate)

Today a single brightness uniform drives both line layers:

- `RenderSettings::line_brightness` — `src/ui/mod.rs:38` (default `1.0`,
  slider in the "Land" header, `src/ui/mod.rs:510`). The comment at
  `src/ui/mod.rs:508` notes it's "shared across both line layers."
- Far-side opacity `line_back_alpha` is likewise shared (`src/ui/mod.rs:40`).
- Colours are fixed constants: `COLOR_COAST` / `COLOR_BORDER`
  (`src/renderer/mod.rs:~82`).

**Want:** independent coastline and border *intensity* (brightness
multiplier), so e.g. borders can sit dimmer than coastlines.

Sketch:
- Split `line_brightness` into `coast_brightness` / `border_brightness`
  (keep one if we'd rather not duplicate the far-side alpha too).
- The thick-line instances already carry a `layer` flag (0 = coast,
  1 = border) in their colour/`w` channel — see `src/renderer/mesh.rs:59`
  and the `build_thick_lines` calls at `src/renderer/mod.rs:765-767`. The
  shader (`assets/shaders/thick_lines.wgsl`) can pick the per-layer
  multiplier from a uniform, or we bake brightness into the instance colour
  at build time (simpler, but means a rebuild on slider change rather than a
  uniform push).
- Add the two sliders under the existing "Coastlines" / "Borders"
  collapsing headers (`src/ui/mod.rs:520` / `:530`) next to the width
  controls.

## 2. Cities — daylight and nighttime intensity  ✅ DONE

Implemented:
- `RenderSettings::cities_day_intensity` / `cities_night_intensity`
  (`src/ui/mod.rs`), sliders in the "Cities" header.
- Plumbed via the new `style3` uniform slot (`Uniforms` in
  `src/renderer/mod.rs`); the marker shader mixes them:
  `night_fade = mix(style3.y /*night*/, style3.x /*day*/, day)`
  (`assets/shaders/markers.wgsl`).
- Master `night_mode` toggle ("Day / night shading", top of Properties):
  when off, `sun.w` → 1.0 (no terminator on globe/lines) and the city day
  value collapses to the night value (uniform city glow).

Also done in the same pass: per-type **ground anchor** (nadir line) and
**coverage zone** (footprint ring) split into independent toggles
(`TypeStyle::show_ground` / `show_coverage`, gated build loop in
`Renderer::update`).

### Original notes — cities daylight and nighttime intensity

City brightness is currently two hardcoded pieces:

- **Per-city base brightness** by population: `intensity = 0.7 + t * 0.5`
  at `src/renderer/mod.rs:1312` (rides the colour magnitude; cities have no
  per-dot alpha).
- **Day/night fade** in the shader: `night_fade = mix(1.0, 0.35, day)` at
  `assets/shaders/markers.wgsl:148` — night = full (1.0), day = faded
  (0.35). Final alpha = `marker_alpha * style2.w (cities_alpha) * night_fade`
  (`markers.wgsl:149`).
- The only UI knob is `cities_alpha` (`src/ui/mod.rs:55`, slider at
  `:551`), a single overall opacity.

**Want:** separate **daylight intensity** and **nighttime intensity**
sliders so the day-side fade and night-side glow are tunable independently
(right now the `1.0` / `0.35` endpoints are fixed in the shader).

Sketch:
- Add `cities_day_intensity` / `cities_night_intensity` to
  `RenderSettings`. Defaults to match today: night `1.0`, day `0.35`.
- Plumb both into a uniform the marker shader reads (e.g. pack into a free
  `style*` slot — see the `Style`/uniform layout around
  `src/renderer/mod.rs:157` and `:1254`). Replace the literal
  `mix(1.0, 0.35, day)` with `mix(night_i, day_i, day)`.
- Add the two sliders inside the "Cities" collapsing header
  (`src/ui/mod.rs:542`), enabled with `cities_show`.

### Stretch
- Make the population brightness ramp endpoints (`0.7` / `+0.5` at
  `src/renderer/mod.rs:1312`) configurable too, or fold them into the same
  day/night intensity scheme.
- Terminator softness (`smoothstep(-0.12, 0.12, …)`) is duplicated in
  `globe.wgsl:49` and `markers.wgsl:147`; if we ever expose it, share one
  uniform.
