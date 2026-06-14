//! Fetching and caching of satellite element sets from CelesTrak.
//!
//! This is the *network* side of Horizon, deliberately separate from
//! `horizon-core` so the simulation stays offline and dependency-light. It
//! returns `horizon_core::Elements` (OMM/TLE element sets), which the core
//! turns into SGP4 propagators.
//!
//! Policy: prefer a fresh on-disk cache; otherwise fetch and cache; if the
//! network fails, fall back to a stale cache. The caller decides what to do if
//! everything fails (the app drops to its synthetic demo constellation).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use horizon_core::Elements;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

/// CelesTrak "GP" query for a named group (e.g. "stations", "gps-ops"), JSON.
fn group_url(group: &str) -> String {
    format!("https://celestrak.org/NORAD/elements/gp.php?GROUP={group}&FORMAT=json")
}

/// On-disk cache file for a group.
pub fn cache_path(cache_dir: &Path, group: &str) -> PathBuf {
    cache_dir.join(format!("{group}.json"))
}

fn fetch_raw(group: &str) -> Result<String> {
    let url = group_url(group);
    let body = ureq::get(&url)
        .timeout(Duration::from_secs(15))
        .call()?
        .into_string()?;
    // CelesTrak returns a 200 with a plain-text error for bad queries.
    if !body.trim_start().starts_with('[') {
        return Err(format!("unexpected response for group '{group}': {}", body.trim()).into());
    }
    Ok(body)
}

fn parse(json: &str) -> Result<Vec<Elements>> {
    Ok(serde_json::from_str(json)?)
}

fn is_fresh(path: &Path, max_age: Duration) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|age| age < max_age)
        .unwrap_or(false)
}

/// Load a group's element sets from the on-disk cache only (no network).
pub fn load_cached(group: &str, cache_dir: &Path) -> Result<Vec<Elements>> {
    let path = cache_path(cache_dir, group);
    let els = parse(&fs::read_to_string(&path)?)?;
    log::info!("loaded {} cached objects for '{group}'", els.len());
    Ok(els)
}

/// Load a group's element sets: fresh cache → network (then cache) → stale
/// cache. Errors only if the network fails *and* no cache exists.
pub fn load_group(group: &str, cache_dir: &Path, max_age: Duration) -> Result<Vec<Elements>> {
    let path = cache_path(cache_dir, group);

    if is_fresh(&path, max_age) {
        if let Ok(s) = fs::read_to_string(&path) {
            if let Ok(els) = parse(&s) {
                log::info!("TLEs for '{group}' from fresh cache ({} objects)", els.len());
                return Ok(els);
            }
        }
    }

    match fetch_raw(group) {
        Ok(s) => {
            let els = parse(&s)?;
            let _ = fs::create_dir_all(cache_dir);
            if let Err(e) = fs::write(&path, &s) {
                log::warn!("could not write TLE cache {}: {e}", path.display());
            }
            log::info!("fetched {} objects for '{group}' from CelesTrak", els.len());
            Ok(els)
        }
        Err(e) => {
            log::warn!("TLE fetch for '{group}' failed ({e}); trying stale cache");
            let s = fs::read_to_string(&path)?;
            let els = parse(&s)?;
            log::info!("using stale cached TLEs for '{group}' ({} objects)", els.len());
            Ok(els)
        }
    }
}
