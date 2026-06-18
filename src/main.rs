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
mod tankers;
mod ui;

use winit::event_loop::{ControlFlow, EventLoop};

const USAGE: &str = "\
Horizon Earth — a Nord-themed 3D Earth visualization.

Usage: horizon [OPTIONS]

Options:
  -w, --windowed     Run in a 1280x800 window instead of fullscreen
  -n, --no-exit      Don't quit on input (enables orbit camera; Escape still quits)
  -d, --demo         Accelerated demo time instead of live wall-clock positions
      --group NAME   CelesTrak group to track (default: active)
      --offline      Skip the network; use cached TLEs or the demo constellation
  -v, --verbose      Verbose logging (equivalent to RUST_LOG=info)
  -h, --help         Print this help

Most flags have an equivalent environment variable:
  HORIZON_WINDOWED=1, HORIZON_NO_EXIT=1, HORIZON_DEMO=1, HORIZON_OFFLINE=1,
  HORIZON_GROUP=<name>, RUST_LOG=info

Real satellites come from CelesTrak (e.g. --group gps-ops, starlink, visual);
they're cached under cache/. In interactive mode ('--no-exit'), press T to
toggle live/demo time.
";

/// Runtime options, resolved from environment variables and command-line
/// flags. A flag and its env var are equivalent; either one enables the option.
struct Options {
    windowed: bool,
    no_exit: bool,
    demo: bool,
    offline: bool,
    group: String,
    verbose: bool,
}

impl Options {
    fn resolve() -> Options {
        let mut o = Options {
            windowed: std::env::var_os("HORIZON_WINDOWED").is_some(),
            no_exit: std::env::var_os("HORIZON_NO_EXIT").is_some(),
            demo: std::env::var_os("HORIZON_DEMO").is_some(),
            offline: std::env::var_os("HORIZON_OFFLINE").is_some(),
            group: std::env::var("HORIZON_GROUP").unwrap_or_else(|_| "active".to_string()),
            verbose: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-w" | "--windowed" => o.windowed = true,
                "-n" | "--no-exit" => o.no_exit = true,
                "-d" | "--demo" => o.demo = true,
                "--live" => o.demo = false,
                "--offline" => o.offline = true,
                "--group" => match args.next() {
                    Some(g) => o.group = g,
                    None => {
                        eprint!("horizon: --group needs a value\n\n{USAGE}");
                        std::process::exit(2);
                    }
                },
                "-v" | "--verbose" => o.verbose = true,
                "-h" | "--help" => {
                    print!("{USAGE}");
                    std::process::exit(0);
                }
                other => {
                    eprint!("horizon: unrecognized argument '{other}'\n\n{USAGE}");
                    std::process::exit(2);
                }
            }
        }
        o
    }
}

fn main() {
    let opts = Options::resolve();

    // `RUST_LOG` still wins if set; otherwise --verbose picks the default level.
    let default_level = if opts.verbose { "info" } else { "warn" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_level)).init();

    let event_loop = EventLoop::new().expect("failed to create event loop");
    // Poll continuously so the globe animates every frame.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = app::App::new(opts.windowed, opts.no_exit, opts.demo, opts.group, opts.offline);
    event_loop.run_app(&mut app).expect("event loop error");
}
