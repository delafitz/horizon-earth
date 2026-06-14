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
and country borders projected onto the sphere, atmospheric rim glow, starfield
background, Nord palette, vsync-locked render loop, exit-on-activity.

Upcoming: cities (Phase 2), ISS/satellite tracking (Phase 3+), screensaver
integration (Phase 5).

## Tech stack

| Concern    | Choice          | Notes |
|------------|-----------------|-------|
| Language   | Rust            | |
| Windowing  | **winit**       | See decision below |
| Rendering  | wgpu (Metal/Vulkan) | |
| Geo data   | Natural Earth   | `ne_110m` coastline + admin-0 countries, embedded |
| Satellites | sgp4 (planned)  | from CelesTrak TLE |

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

```
src/
  main.rs            entry point, CLI/env options, event loop setup
  app.rs             winit ApplicationHandler: window, input, camera control
  camera.rs          orbit (arcball) camera: target/distance/yaw/pitch -> view
  orbit.rs           orbiting bodies: circular-orbit model + demo satellites
  data/              minimal GeoJSON coordinate reader
  earth/             lat/lon -> sphere projection, line-segment building
  renderer/          wgpu surface, pipelines, per-frame draw
    mesh.rs          vertex types, UV sphere, marker quad/instance
assets/
  earth/             Natural Earth GeoJSON (embedded at build time)
  shaders/           WGSL: starfield, globe, lines, track, markers, atmosphere
cache/               runtime caches (later phases)
```

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
| `demo_bodies()` entries | `src/orbit.rs` | Per body: `radius` (altitude), `inclination`, `raan`, `period` (speed), `phase0`, `color`. |
| `MARKER_SIZE` | `src/renderer/mod.rs` | On-screen marker size (NDC). |
| `body.track(128)` | `src/renderer/mod.rs` | Orbit-track smoothness (segment count). |
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
| `time * 0.12` in `update` | `src/renderer/mod.rs` | Globe axial spin speed. |
| `uv_sphere(64, 96, 1.0)` | `src/renderer/mod.rs` | Sphere tessellation (stacks, sectors). |
| `COLOR_BG` | `src/renderer/mod.rs` | Background colour. |
| `80.0` grid / `0.90` threshold | `assets/shaders/starfield.wgsl` | Star density and sparsity. |

## Data attribution

Geographic data © [Natural Earth](https://www.naturalearthdata.com/) (public
domain), via the
[nvkelso/natural-earth-vector](https://github.com/nvkelso/natural-earth-vector)
GeoJSON distribution.
