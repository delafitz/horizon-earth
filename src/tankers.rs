//! Tanker layer: load the AIS collector's `cache/tankers.json` and turn it into
//! flat surface geometry — a direction-aware marker per ship plus its recent
//! track. Geometry is built in the Earth-fixed frame (like the coastlines) and
//! spun by the renderer's GMST `model` matrix.
//!
//! Markers: a **triangle** pointing along the course-over-ground for vessels
//! that are moving (valid COG, SOG above a small threshold); a small **rect**
//! for stationary / unknown-heading ones.

use glam::Vec3;
use serde::Deserialize;

use crate::earth::latlon_to_xyz;
use crate::renderer::mesh::VertexPC;

/// Render radius for tanker geometry: just above the globe and land fill.
const R: f32 = 1.0016;
/// Marker dimensions in render units (Earth radius = 1).
const LEN: f32 = 0.0085; // triangle length along heading
const HALF: f32 = 0.0036; // half-width / rect half-size
/// Below this speed (knots) a ship's course is meaningless → draw a rect.
const MOVING_SOG_MIN: f64 = 0.5;
/// Wake (stern tail) length in render units, scaled by speed-over-ground.
const WAKE_PER_KNOT: f32 = 0.0009;
const WAKE_MIN: f32 = 0.006;
const WAKE_MAX: f32 = 0.022;

// Soft amber, distinct from the frost-blue cities/coastlines but not harsh.
const COLOR_TANKER: [f32; 3] = [0.82, 0.62, 0.44];
const COLOR_STILL: [f32; 3] = [0.70, 0.56, 0.45];
const COLOR_TRACK: [f32; 3] = [0.50, 0.38, 0.25];

/// One tanker, as written by the `horizon-ais` collector.
#[derive(Deserialize)]
pub struct Tanker {
    #[allow(dead_code)]
    pub mmsi: u32,
    #[allow(dead_code)]
    #[serde(default)]
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub course: f64,
    #[serde(default)]
    pub sog: f64,
    #[allow(dead_code)]
    #[serde(rename = "type", default)]
    pub ship_type: u32,
    /// Recent [lat, lon] trail, oldest first.
    #[serde(default)]
    pub track: Vec<[f64; 2]>,
}

/// Load the tanker cache, returning an empty vec if it's missing or unparseable
/// (the collector may not have produced it yet).
pub fn load(path: &std::path::Path) -> Vec<Tanker> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Build (marker triangles/rects, track line segments) from the tankers. Both
/// are flat surface geometry in the Earth-fixed frame.
pub fn build_geometry(tankers: &[Tanker]) -> (Vec<VertexPC>, Vec<VertexPC>) {
    let mut tris: Vec<VertexPC> = Vec::new();
    let mut tracks: Vec<VertexPC> = Vec::new();

    for t in tankers {
        let p = Vec3::from_array(latlon_to_xyz(t.lon, t.lat, R));
        let up = p.normalize_or_zero();
        // North tangent = +Y (render pole) projected onto the local tangent
        // plane; East completes the right-handed ENU frame (N × U = E).
        let north_raw = Vec3::Y - up * up.dot(Vec3::Y);

        if t.sog > MOVING_SOG_MIN && t.course < 359.5 && north_raw.length() > 1e-4 {
            let north = north_raw.normalize();
            let east = north.cross(up).normalize();
            let cr = (t.course as f32).to_radians();
            let heading = north * cr.cos() + east * cr.sin();
            let side = up.cross(heading).normalize_or_zero();
            // Arrowhead: tip ahead along the course, two corners trailing.
            let stern = p - heading * (LEN * 0.45);
            let tip = p + heading * LEN;
            let bl = stern + side * HALF;
            let br = stern - side * HALF;
            push_tri(&mut tris, tip, bl, br, COLOR_TANKER);
            // Immediate wake: a short tail off the stern, length scaled by
            // speed. Complements the (slowly accumulating) historical track.
            let wake = (t.sog as f32 * WAKE_PER_KNOT).clamp(WAKE_MIN, WAKE_MAX);
            tracks.push(VertexPC { pos: stern.to_array(), col: COLOR_TRACK });
            tracks.push(VertexPC { pos: (stern - heading * wake).to_array(), col: COLOR_TRACK });
        } else {
            // Stationary / no heading: a small axis-aligned rect (two tris).
            let n = if north_raw.length() > 1e-4 { north_raw.normalize() } else { Vec3::X };
            let e = n.cross(up).normalize_or_zero();
            let (a, b, c, d) = (
                p + n * HALF + e * HALF,
                p + n * HALF - e * HALF,
                p - n * HALF - e * HALF,
                p - n * HALF + e * HALF,
            );
            push_tri(&mut tris, a, b, c, COLOR_STILL);
            push_tri(&mut tris, a, c, d, COLOR_STILL);
        }

        // Track: connect consecutive [lat, lon] fixes.
        for w in t.track.windows(2) {
            tracks.push(VertexPC { pos: latlon_to_xyz(w[0][1], w[0][0], R), col: COLOR_TRACK });
            tracks.push(VertexPC { pos: latlon_to_xyz(w[1][1], w[1][0], R), col: COLOR_TRACK });
        }
    }

    (tris, tracks)
}

fn push_tri(out: &mut Vec<VertexPC>, a: Vec3, b: Vec3, c: Vec3, col: [f32; 3]) {
    out.push(VertexPC { pos: a.to_array(), col });
    out.push(VertexPC { pos: b.to_array(), col });
    out.push(VertexPC { pos: c.to_array(), col });
}
