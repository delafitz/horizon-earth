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

**In progress — world model & interaction.** Glass globe (everything shows
through: far-side coastlines, orbit tracks, satellites, stars); orbit camera
(mouse drag + zoom); an engine-agnostic `f64` simulation core (`horizon-core`)
with real UTC epochs, GMST-driven Earth orientation, ECI/render frames, and a
`Propagator` abstraction with both Keplerian two-body and **SGP4** motion. Real
tracked objects come from CelesTrak via `horizon-data` (cached, offline-capable),
with **live** (true current positions) and **demo** (accelerated) time modes.
Bodies are classified (station/LEO/Starlink/GNSS/GEO) into Nord colours and box
vs filled-square markers, with HUD labels (name + altitude) in a rectilinear
vector stroke font, occlusion-culled and decluttered.

Upcoming: cities; selection/info panels; screensaver integration
(Phase 5, `wlr-layer-shell`).

## Tech stack

| Concern    | Choice          | Notes |
|------------|-----------------|-------|
| Language   | Rust            | |
| Windowing  | **winit**       | See decision below |
| Rendering  | wgpu (Metal/Vulkan) | |
| Geo data   | Natural Earth   | `ne_110m` coastline + admin-0 countries, embedded |
| Sim core   | `horizon-core`  | f64, units in km/s, ECI frame + GMST; render-agnostic |
| Orbits     | Keplerian + SGP4 (`sgp4`) | via a `Propagator` trait |
| Sat data   | `horizon-data`  | CelesTrak TLE/OMM fetch + cache (`ureq`) |

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
| `-d, --demo`      | `HORIZON_DEMO=1`      | Accelerated demo time instead of live wall-clock positions |
| `--group NAME`    | `HORIZON_GROUP=NAME`  | CelesTrak group to track (default `stations`; e.g. `gps-ops`, `starlink`, `visual`) |
| `--offline`       | `HORIZON_OFFLINE=1`   | Skip the network; use cached TLEs (or the demo constellation) |
| `-v, --verbose`   | `RUST_LOG=info`       | Verbose logging (default is `warn`) |

In interactive mode (`--no-exit`), **T** toggles live/demo time, **F** switches
between the fixed (Earth-centred) and fly (orbit-riding) cameras, and **@**
toggles the HUD overlay (satellite labels, fly banner, and the egui panels) for a
clean view.

The trackpad drives the camera in both modes:

| Gesture | Fixed camera | Fly camera |
|---|---|---|
| Two-finger scroll | Orbit the globe | Look around (yaw/pitch) |
| Shift + scroll | Zoom | Orbit altitude |
| Ctrl + scroll | Roll the horizon | Zoom the look (FOV) |
| Two-finger rotate | Roll | Roll |
| Pinch | Zoom | Orbit altitude |

In fly mode the keyboard adds speed (Z/X), inclination (C/V) and RAAN (B/N); the
in-app banner lists the full set.

Real satellites are fetched from CelesTrak and cached under `cache/`; if the
fetch fails the app falls back to a synthetic constellation.

```sh
cargo run -- --windowed --no-exit
# or, equivalently:
HORIZON_WINDOWED=1 HORIZON_NO_EXIT=1 cargo run
```

## Layout

A Cargo workspace splits the engine-agnostic model from the renderer:

```
horizon-core/        simulation core — f64, no rendering/windowing/network deps
  src/
    units.rs         physical constants (km, s, GM, Earth rotation rate)
    time.rs          UTC epoch (Julian Date) + GMST
    frames.rs        ECI <-> render-frame bridge; lat/lon -> ECEF/render
    orbit.rs         Propagator trait; Keplerian two-body + SGP4 propagators
    category.rs      classify a body (name + orbit) -> Nord colour + symbol
    camera.rs        orbit (arcball) camera: target/distance/yaw/pitch -> view
    world.rs         current epoch + GMST rotation + bodies (demo + from TLEs)
horizon-data/        CelesTrak TLE/OMM fetch + on-disk cache (the network side)
src/                 the app (depends on horizon-core + horizon-data)
  main.rs            entry point, CLI/env options, event loop setup
  app.rs             winit ApplicationHandler: window, input, camera control
  data/              minimal GeoJSON coordinate reader
  earth/             lat/lon -> sphere projection, line-segment building
  renderer/          wgpu surface, pipelines, per-frame draw
    mesh.rs          vertex types, UV sphere, marker quad/instance
    glyphs.rs        rectilinear vector stroke font (HUD labels)
assets/
  earth/             Natural Earth GeoJSON (embedded at build time)
  shaders/           WGSL: starfield, globe, lines, track, markers, atmosphere, label
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
| alpha `0.35` in the returned colour | Surface transparency (glass globe). |
| `0.30 + 0.70 * d` | Ambient / diffuse lighting balance. |
| `base` colour | Globe fill (Nord1). |

### Coastlines & borders

| Knob | Where | Effect |
|------|-------|--------|
| `COLOR_COAST`, `COLOR_BORDER` | `src/renderer/mod.rs` | Line colours. |
| `1.0020` / `1.0030` in `build_lines(...)` | `src/renderer/mod.rs` | Border / coastline height above the surface (coastlines win overlaps). |
| alpha `0.28` in `fs_back` | `assets/shaders/lines.wgsl` | Faintness of far-side (behind-globe) lines. |
| alpha `0.14` in `fs_back` | `assets/shaders/track.wgsl` | Faintness of far-side orbit tracks. |
| `×0.4` in `fs_back` | `assets/shaders/markers.wgsl` | Faintness of satellites behind the globe. |

### Orbiting bodies & tracks

| Knob | Where | Effect |
|------|-------|--------|
| `--group` / `HORIZON_GROUP` | CLI / env | Which CelesTrak group of real objects to track. |
| `TLE_MAX_AGE` | `src/app.rs` | How long a cached TLE set stays fresh before re-fetch. |
| `World::demo()` bodies | `horizon-core/src/world.rs` | Synthetic-fallback bodies: name, `KeplerOrbit`, colour. |
| `PALETTE` | `horizon-core/src/world.rs` | Colours cycled across tracked bodies. |
| `DEFAULT_TIME_SCALE` | `horizon-core/src/world.rs` | Demo-mode simulated seconds per real second. |
| orbital elements / `circular(...)` | `horizon-core/src/orbit.rs` | Semi-major axis, eccentricity, inclination, RAAN, arg. periapsis, mean anomaly. |
| `Category` color/filled/size | `horizon-core/src/category.rs` | Per-category Nord colour, box vs filled square, bold size. |
| `MARKER_SIZE` | `src/renderer/mod.rs` | Base on-screen marker size (NDC); scaled per category. |
| `sample_track(.., 128)` | `src/renderer/mod.rs` | Orbit-track smoothness (segment count). |
| `LABEL_PX` | `src/renderer/mod.rs` | HUD label glyph height (px); declutter/spacing derive from it. |
| `strokes(..)` | `src/renderer/glyphs.rs` | Vector font letterforms (3x5 stroke grid). |
| alpha `0.35` in `fs_main` | `assets/shaders/track.wgsl` | Orbit-track faintness. |
| `square_alpha` | `assets/shaders/markers.wgsl` | Box/filled-square shape, edge softness, outline thickness. |

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
