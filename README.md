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

### Development env vars

| Variable             | Effect |
|----------------------|--------|
| `HORIZON_WINDOWED=1` | Run in a 1280×800 window instead of fullscreen |
| `HORIZON_NO_EXIT=1`  | Don't quit on input (Escape still quits) |
| `RUST_LOG=info`      | Verbose logging (default is `warn`) |

```sh
HORIZON_WINDOWED=1 HORIZON_NO_EXIT=1 cargo run
```

## Layout

```
src/
  main.rs            entry point + event loop setup
  app.rs             winit ApplicationHandler: window, input, exit-on-activity
  data/              minimal GeoJSON coordinate reader
  earth/             lat/lon -> sphere projection, line-segment building
  renderer/          wgpu surface, pipelines, per-frame draw
    mesh.rs          vertex types + UV sphere generation
assets/
  earth/             Natural Earth GeoJSON (embedded at build time)
  shaders/           WGSL: starfield, globe, lines, atmosphere
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

## Data attribution

Geographic data © [Natural Earth](https://www.naturalearthdata.com/) (public
domain), via the
[nvkelso/natural-earth-vector](https://github.com/nvkelso/natural-earth-vector)
GeoJSON distribution.
