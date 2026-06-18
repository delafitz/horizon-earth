//! AISStream collector — a standalone daemon that maintains live tanker
//! positions from aisstream.io (free real-time global AIS) and writes a compact
//! `cache/tankers.json` the Horizon globe can load.
//!
//! AISStream is **live-only**: no history, no snapshot, no "current state of all
//! vessels" on connect. So this process *accumulates* state by listening —
//! latest position per MMSI plus a short rolling track, with ship type arriving
//! separately in periodic `ShipStaticData` messages (~every 6 min). Only tankers
//! (AIS ship type 80–89) are written out. Run it as a long-lived background
//! process so the cache stays warm between app launches (a cold start sees
//! nothing for the first ~10–15 min while vessels transmit).
//!
//! Usage:
//!   AISSTREAM_API_KEY=<key> horizon-ais [out.json]   (default: cache/tankers.json)
//!
//! Get a free key at <https://aisstream.io/>.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tungstenite::Message;

const WS_URL: &str = "wss://stream.aisstream.io/v0/stream";
const TRACK_MAX: usize = 30; // points kept in each vessel's rolling track
const TRACK_MIN_MOVE_DEG: f64 = 0.01; // ~1 km; drop near-duplicate points
const STALE_SECS: u64 = 3 * 3600; // forget vessels unseen for this long
const WRITE_EVERY: Duration = Duration::from_secs(45);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// AIS ship type 80–89 = tanker (all the tanker sub-categories).
fn is_tanker(t: u32) -> bool {
    (80..=89).contains(&t)
}

/// Accumulated per-vessel state. `ship_type` stays `None` until a static-data
/// message classifies the vessel, so position-only vessels aren't yet known to
/// be tankers (and aren't written until they are).
#[derive(Default)]
struct Vessel {
    name: String,
    ship_type: Option<u32>,
    lat: f64,
    lon: f64,
    cog: f64,
    sog: f64,
    seen: u64,
    has_pos: bool,
    track: VecDeque<[f64; 2]>,
}

type State = Arc<Mutex<HashMap<u32, Vessel>>>;

// --- AISStream message envelope (only the fields we use) --------------------

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "MessageType")]
    message_type: String,
    #[serde(rename = "MetaData")]
    meta: MetaData,
    #[serde(rename = "Message")]
    message: Body,
}

#[derive(Deserialize)]
struct MetaData {
    #[serde(rename = "MMSI")]
    mmsi: u32,
    #[serde(rename = "ShipName", default)]
    ship_name: String,
    #[serde(default)]
    latitude: f64,
    #[serde(default)]
    longitude: f64,
}

#[derive(Deserialize, Default)]
struct Body {
    #[serde(rename = "PositionReport")]
    position: Option<Pos>,
    #[serde(rename = "StandardClassBPositionReport")]
    position_b: Option<Pos>,
    #[serde(rename = "ShipStaticData")]
    static_data: Option<Static>,
}

#[derive(Deserialize, Default)]
struct Pos {
    #[serde(rename = "Cog", default)]
    cog: f64,
    #[serde(rename = "Sog", default)]
    sog: f64,
}

#[derive(Deserialize)]
struct Static {
    #[serde(rename = "Type", default)]
    ship_type: u32,
    #[serde(rename = "Name", default)]
    name: String,
}

// --- Output (cache/tankers.json) --------------------------------------------

#[derive(Serialize)]
struct TankerOut {
    mmsi: u32,
    name: String,
    lat: f64,
    lon: f64,
    course: f64,
    sog: f64,
    #[serde(rename = "type")]
    ship_type: u32,
    /// Unix seconds of the last position fix.
    t: u64,
    /// Recent [lat, lon] trail, oldest first.
    track: Vec<[f64; 2]>,
}

/// Fold one decoded message into the shared state.
fn apply(state: &State, env: Envelope) {
    let mmsi = env.meta.mmsi;
    if mmsi == 0 {
        return;
    }
    let mut map = state.lock().unwrap();
    let v = map.entry(mmsi).or_default();
    v.seen = now();
    if !env.meta.ship_name.trim().is_empty() {
        v.name = env.meta.ship_name.trim().to_string();
    }
    match env.message_type.as_str() {
        "PositionReport" | "StandardClassBPositionReport" => {
            let p = env
                .message
                .position
                .or(env.message.position_b)
                .unwrap_or_default();
            v.lat = env.meta.latitude;
            v.lon = env.meta.longitude;
            v.cog = p.cog;
            v.sog = p.sog;
            v.has_pos = true;
            // Append to the rolling track, skipping near-duplicate points.
            let pt = [v.lat, v.lon];
            let moved = v.track.back().map_or(true, |last| {
                (last[0] - pt[0]).abs() > TRACK_MIN_MOVE_DEG
                    || (last[1] - pt[1]).abs() > TRACK_MIN_MOVE_DEG
            });
            if moved {
                v.track.push_back(pt);
                while v.track.len() > TRACK_MAX {
                    v.track.pop_front();
                }
            }
        }
        "ShipStaticData" => {
            if let Some(s) = env.message.static_data {
                v.ship_type = Some(s.ship_type);
                if !s.name.trim().is_empty() {
                    v.name = s.name.trim().to_string();
                }
            }
        }
        _ => {}
    }
}

/// Snapshot the current tankers to `path` atomically (temp file + rename).
fn write_cache(state: &State, path: &PathBuf) {
    let cutoff = now().saturating_sub(STALE_SECS);
    let mut tankers: Vec<TankerOut> = Vec::new();
    {
        let mut map = state.lock().unwrap();
        map.retain(|_, v| v.seen >= cutoff); // prune stale; bound memory
        for (mmsi, v) in map.iter() {
            if let Some(t) = v.ship_type {
                if is_tanker(t) && v.has_pos {
                    tankers.push(TankerOut {
                        mmsi: *mmsi,
                        name: v.name.clone(),
                        lat: v.lat,
                        lon: v.lon,
                        course: v.cog,
                        sog: v.sog,
                        ship_type: t,
                        t: v.seen,
                        track: v.track.iter().copied().collect(),
                    });
                }
            }
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_vec(&tankers) {
        Ok(bytes) => {
            if std::fs::write(&tmp, &bytes)
                .and_then(|_| std::fs::rename(&tmp, path))
                .is_ok()
            {
                eprintln!(
                    "[horizon-ais] wrote {} tankers -> {}",
                    tankers.len(),
                    path.display()
                );
            }
        }
        Err(e) => eprintln!("[horizon-ais] serialize error: {e}"),
    }
}

fn main() {
    // Trim: a stray newline/space from `set -x ...` would corrupt the key and
    // AISStream would silently send nothing.
    let api_key = std::env::var("AISSTREAM_API_KEY")
        .unwrap_or_default()
        .trim()
        .to_string();
    if api_key.is_empty() {
        eprintln!("error: set AISSTREAM_API_KEY (free key at https://aisstream.io/)");
        std::process::exit(2);
    }
    eprintln!("[horizon-ais] API key loaded ({} chars)", api_key.len());
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "cache/tankers.json".to_string()),
    );

    let state: State = Arc::new(Mutex::new(HashMap::new()));
    let msgs = Arc::new(AtomicU64::new(0));

    // Writer thread: snapshot tankers on a fixed cadence, independent of the
    // (bursty, ~300 msg/s) message stream.
    {
        let state = state.clone();
        let path = path.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(WRITE_EVERY);
            write_cache(&state, &path);
        });
    }

    // Heartbeat thread: a steady throughput line so you can see data flowing
    // well before the first cache write.
    {
        let state = state.clone();
        let msgs = msgs.clone();
        std::thread::spawn(move || {
            let mut prev = 0u64;
            loop {
                std::thread::sleep(Duration::from_secs(10));
                let total = msgs.load(Ordering::Relaxed);
                let (vessels, classified, tankers) = {
                    let map = state.lock().unwrap();
                    let classified = map.values().filter(|v| v.ship_type.is_some()).count();
                    let tk = map
                        .values()
                        .filter(|v| v.has_pos && v.ship_type.is_some_and(is_tanker))
                        .count();
                    (map.len(), classified, tk)
                };
                eprintln!(
                    "[horizon-ais] msgs={total} (+{}/10s)  vessels={vessels}  classified={classified}  tankers={tankers}",
                    total - prev
                );
                prev = total;
            }
        });
    }

    // Subscribe to the whole world; ship type isn't a subscription filter, so
    // tankers are selected client-side from ShipStaticData.
    let subscribe = serde_json::json!({
        "APIKey": api_key,
        "BoundingBoxes": [[[-90.0, -180.0], [90.0, 180.0]]],
        "FilterMessageTypes": ["PositionReport", "StandardClassBPositionReport", "ShipStaticData"],
    })
    .to_string();

    // Reconnect loop. The accumulated state map persists across reconnects, so a
    // dropped socket doesn't lose the warm dataset.
    loop {
        eprintln!("[horizon-ais] connecting to {WS_URL} ...");
        match tungstenite::connect(WS_URL) {
            Ok((mut socket, _)) => {
                eprintln!("[horizon-ais] connected; sending subscription");
                if let Err(e) = socket.send(Message::Text(subscribe.clone().into())) {
                    eprintln!("[horizon-ais] subscribe send failed: {e}");
                } else {
                    eprintln!("[horizon-ais] subscribed (world; tankers filtered client-side)");
                    let mut shown = 0u32; // echo the first few raw frames for sanity
                    loop {
                        // AISStream delivers JSON over *binary* frames (some
                        // clients also see text); handle both as UTF-8 JSON.
                        let bytes: Vec<u8> = match socket.read() {
                            Ok(Message::Binary(b)) => b.into(),
                            Ok(Message::Text(t)) => t.as_bytes().to_vec(),
                            Ok(Message::Ping(p)) => {
                                let _ = socket.send(Message::Pong(p));
                                continue;
                            }
                            // AISStream closes with a reason on a bad key / bad
                            // subscription — surface it.
                            Ok(Message::Close(c)) => {
                                eprintln!("[horizon-ais] server closed connection: {c:?}");
                                break;
                            }
                            Err(e) => {
                                eprintln!("[horizon-ais] read error: {e}");
                                break;
                            }
                            Ok(_) => continue,
                        };
                        msgs.fetch_add(1, Ordering::Relaxed);
                        if shown < 3 {
                            let s = String::from_utf8_lossy(&bytes);
                            eprintln!("[horizon-ais] raw[{shown}]: {}", &s[..s.len().min(220)]);
                            shown += 1;
                        }
                        if let Ok(env) = serde_json::from_slice::<Envelope>(&bytes) {
                            apply(&state, env);
                        }
                    }
                }
            }
            Err(e) => eprintln!("[horizon-ais] connect error: {e}"),
        }
        eprintln!("[horizon-ais] disconnected; retrying in 5s");
        std::thread::sleep(Duration::from_secs(5));
    }
}
