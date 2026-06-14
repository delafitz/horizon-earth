# Horizon Earth

A Nord-themed 3D Earth visualization written in Rust, targeting Wayland
desktops (primary target: [niri](https://github.com/YaLTeR/niri)).

A real-time rotating globe with coastlines, country borders, and — in later
phases — cities and live satellite positions. Designed to work both as a
standalone desktop visualization and as an idle-triggered screensaver. The
aesthetic blends NASA mission control, a Bloomberg terminal, and modern
Wayland desktops: clean vector graphics over photorealism.

## Status

**Phase 1 (MVP) — done.** Fullscreen rotating globe, Natural Earth coastlines
and country borders projected onto the sphere, atmospheric glow, starfield
background, Nord palette, vsync-locked render loop, exit-on-activity.

**In progress — world model & interaction.** Translucent globe with darker
far-side lines; orbit camera (mouse drag + zoom); an engine-agnostic `f64`
simulation core (`horizon-core`) with a sim clock, ECI/render frames, and
Keplerian two-body satellite motion at real altitudes/periods, rendered as
markers + orbit tracks.

Upcoming: real epochs + GMST so ground tracks align to geography; SGP4 from
CelesTrak TLEs; cities/labels (HUD); screensaver integration (Phase 5).

## Tech stack

| Concern    | Choice          | Notes |
|------------|-----------------|-------|
| Language   | Rust            | |
| Windowing  | **winit**       | See decision below |
| Rendering  | wgpu (Metal/Vulkan) | |
| Geo data   | Natural Earth   | `ne_110m` coastline + admin-0 countries, embedded |
| Sim core   | `horizon-core`  | f64, units in km/s, ECI frame; render-agnostic |
| Orbits     | Keplerian two-body | SGP4 + CelesTrak TLE planned |

### Windowing: winit, not GTK4

The original spec named GTK4, but the foundation uses **winit**. wgpu + GTK4 is
an awkward integration (render-to-texture composited into a `GtkGLArea`),
whereas winit is the standard wgpu pairing and runs natively on **both** macOS
(Metal, for development) and Wayland (the deploy target) from one codebase. The
screensaver concerns GTK4 was meant to cover — idle launch, DPMS, lock-screen —
are handled by external Wayland tooling (e.g. `swayidle`) regardless of toolkit.
When the screensaver phase needs a true overlay surface, that will use
`wlr-layer-shell`; the wgpu render core stays identical.

## Build & run

```sh
cargo run --release
```

Runs fullscreen borderless and exits on any keyboard or mouse activity.

### Options

Each flag has an equivalent environment variable; either one enables it
(`horizon --help` lists them).

| Flag              | Env var               | Effect |
|-------------------|-----------------------|--------|
| `-w, --windowed`  | `HORIZON_WINDOWED=1`  | Run in a 1280×800 window instead of fullscreen |
| `-n, --no-exit`   | `HORIZON_NO_EXIT=1`   | Don't quit on input — enables the orbit camera (Escape still quits) |
| `-v, --verbose`   | `RUST_LOG=info`       | Verbose logging (default is `warn`) |

In interactive mode (`--no-exit`), left-drag orbits the camera and scroll zooms.

```sh
cargo run -- --windowed --no-exit
# or, equivalently:
HORIZON_WINDOWED=1 HORIZON_NO_EXIT=1 cargo run
```

## Layout

A Cargo workspace splits the engine-agnostic model from the renderer:

```
horizon-core/        simulation core — pure f64, no rendering/windowing deps
  src/
    units.rs         physical constants (km, s, GM, Earth rotation rate)
    frames.rs        ECI <-> render-frame bridge; lat/lon -> ECEF
    orbit.rs         Keplerian two-body propagation + orbit-track sampling
    camera.rs        orbit (arcball) camera: target/distance/yaw/pitch -> view
    world.rs         sim clock + central-body rotation + orbiting bodies
src/                 the app (depends on horizon-core)
  main.rs            entry point, CLI/env options, event loop setup
  app.rs             winit ApplicationHandler: window, input, camera control
  data/              minimal GeoJSON coordinate reader
  earth/             lat/lon -> sphere projection, line-segment building
  renderer/          wgpu surface, pipelines, per-frame draw
    mesh.rs          vertex types, UV sphere, marker quad/instance
assets/
  earth/             Natural Earth GeoJSON (embedded at build time)
  shaders/           WGSL: starfield, globe, lines, track, markers, atmosphere
cache/               runtime caches (later phases)
```

`horizon-core` is the portability seam: it holds the physics (orbits, frames,
time) in double precision and SI-ish units (km, s), with positions only mapped
into the render frame (Y-up, Earth radius = 1) at the GPU boundary. Swapping the
renderer later (or adding a backend) doesn't touch the model.

## Visual style (Nord)

| Element      | Colour              |
|--------------|---------------------|
| Background   | `#2E3440` (Nord0)   |
| Globe fill   | `#3B4252` (Nord1)   |
| Country borders | `#4C566A` (Nord3) |
| Coastlines   | `#88C0D0` (Nord8)   |
| Atmosphere   | `#81A1C1` (Nord9)   |

## Tuning knobs

Most of the look and feel is controlled by a handful of named values. Shader
values (`assets/shaders/*.wgsl`) are embedded via `include_str!`, so editing a
shader just needs a rebuild. Knobs are referenced by file + constant/expression
rather than line number (line numbers drift).

### Atmosphere — `assets/shaders/atmosphere.wgsl`

The glow is driven by the view ray's closest approach to the globe centre (the
"impact parameter" `b`), so it stays anchored as the camera orbits/zooms.

| Knob | Effect |
|------|--------|
| `OUTER` (const) | Atmosphere outer reach; also the shell scale. Larger = taller halo. |
| `0.985` in `inner = smoothstep(0.985, SURFACE, b)` | How far the glow bleeds under the surface. Closer to `1.0` = less under-surface glow. |
| `fade = 1.0 - smoothstep(SURFACE, OUTER, b)` | Outward falloff to transparent at the outer edge. |
| `* 0.45` | Overall glow intensity. |

### Globe — `assets/shaders/globe.wgsl`

| Knob | Effect |
|------|--------|
| alpha `0.45` in the returned colour | Surface transparency. |
| `0.30 + 0.70 * d` | Ambient / diffuse lighting balance. |
| `base` colour | Globe fill (Nord1). |

### Coastlines & borders

| Knob | Where | Effect |
|------|-------|--------|
| `COLOR_COAST`, `COLOR_BORDER` | `src/renderer/mod.rs` | Line colours. |
| `1.0020` / `1.0030` in `build_lines(...)` | `src/renderer/mod.rs` | Border / coastline height above the surface (coastlines win overlaps). |
| alpha `0.28` in `fs_back` | `assets/shaders/lines.wgsl` | Faintness of far-side (behind-globe) lines. |

### Orbiting bodies & tracks

| Knob | Where | Effect |
|------|-------|--------|
| `World::demo()` bodies | `horizon-core/src/world.rs` | Per body: name, `KeplerOrbit` (altitude/inclination/node/phase), colour. |
| `DEFAULT_TIME_SCALE` | `horizon-core/src/world.rs` | Simulated seconds per real second (orbital speed). |
| orbital elements / `circular(...)` | `horizon-core/src/orbit.rs` | Semi-major axis, eccentricity, inclination, RAAN, arg. periapsis, mean anomaly. |
| `MARKER_SIZE` | `src/renderer/mod.rs` | On-screen marker size (NDC). |
| `body.orbit.sample_track(128)` | `src/renderer/mod.rs` | Orbit-track smoothness (segment count). |
| alpha `0.35` in `fs_main` | `assets/shaders/track.wgsl` | Orbit-track faintness. |
| `smoothstep` / `core` | `assets/shaders/markers.wgsl` | Marker dot softness and core brightness. |

### Camera & input

| Knob | Where | Effect |
|------|-------|--------|
| `DIST_MIN` / `DIST_MAX` | `src/camera.rs` | Zoom limits. |
| `PITCH_LIMIT` | `src/camera.rs` | How close to the poles you can tilt. |
| `fov_y`, default `distance` / `pitch` | `src/camera.rs` | Field of view and starting view. |
| `const S` in `CursorMoved` | `src/app.rs` | Drag orbit sensitivity. |
| `y * 0.1` in `MouseWheel` | `src/app.rs` | Zoom step per scroll. |
| `> 6.0` in `CursorMoved` | `src/app.rs` | Screensaver "activity" threshold (px). |

### Scene

| Knob | Where | Effect |
|------|-------|--------|
| `DEFAULT_TIME_SCALE` | `horizon-core/src/world.rs` | Globe spin is now physical (sidereal rate); this scales sim time, hence apparent spin + orbit speed together. |
| `uv_sphere(64, 96, 1.0)` | `src/renderer/mod.rs` | Sphere tessellation (stacks, sectors). |
| `COLOR_BG` | `src/renderer/mod.rs` | Background colour. |
| `80.0` grid / `0.90` threshold | `assets/shaders/starfield.wgsl` | Star density and sparsity. |

## Data attribution

Geographic data © [Natural Earth](https://www.naturalearthdata.com/) (public
domain), via the
[nvkelso/natural-earth-vector](https://github.com/nvkelso/natural-earth-vector)
GeoJSON distribution.
