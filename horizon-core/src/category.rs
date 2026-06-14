//! Body categories: classify a satellite from its name and orbit, and map that
//! to a Nord colour, a HUD marker symbol, and a relative size.

use std::f64::consts::TAU;

use crate::units::{EARTH_MU, EARTH_RADIUS_KM};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Crewed stations and their docked vehicles (ISS, CSS, Soyuz, Dragon, …).
    Station,
    /// Generic low Earth orbit.
    Leo,
    /// Starlink constellation.
    Starlink,
    /// Navigation / MEO constellations (GPS, Galileo, …).
    Gnss,
    /// Geostationary / geosynchronous.
    Geo,
    /// Anything that doesn't fit the above (incl. higher / eccentric orbits).
    Other,
}

const STATION_KEYS: [&str; 17] = [
    "ISS", "ZARYA", "NAUKA", "POISK", "RASSVET", "ZVEZDA", "CSS", "TIANHE",
    "WENTIAN", "MENGTIAN", "SOYUZ", "PROGRESS", "DRAGON", "CREW", "CYGNUS",
    "SHENZHOU", "TIANZHOU",
];
const GNSS_KEYS: [&str; 6] = ["GPS", "NAVSTAR", "GALILEO", "GLONASS", "BEIDOU", "QZS"];

impl Category {
    /// Classify from the object name and orbital period (seconds). Altitude is
    /// derived from the period (circular-equivalent semi-major axis).
    pub fn classify(name: &str, period_s: f64) -> Category {
        let n = name.to_ascii_uppercase();
        let a = (EARTH_MU * (period_s / TAU).powi(2)).cbrt();
        let alt = a - EARTH_RADIUS_KM;

        if STATION_KEYS.iter().any(|k| n.contains(k)) {
            Category::Station
        } else if n.contains("STARLINK") {
            Category::Starlink
        } else if GNSS_KEYS.iter().any(|k| n.contains(k)) || (18_000.0..30_000.0).contains(&alt) {
            Category::Gnss
        } else if (period_s / 60.0 - 1436.0).abs() < 60.0 || (33_000.0..38_000.0).contains(&alt) {
            Category::Geo
        } else if alt < 2_000.0 {
            Category::Leo
        } else {
            Category::Other
        }
    }

    /// Render colour (linear-ish RGB, Nord palette).
    pub fn color(self) -> [f32; 3] {
        match self {
            Category::Station => [0.925, 0.937, 0.957],  // Nord6  snow
            Category::Leo => [0.506, 0.631, 0.757],      // Nord9  frost
            Category::Starlink => [0.561, 0.737, 0.733], // Nord7  frost
            Category::Gnss => [0.639, 0.745, 0.549],     // Nord14 green
            Category::Geo => [0.922, 0.796, 0.545],      // Nord13 yellow
            Category::Other => [0.816, 0.529, 0.439],    // Nord12 orange
        }
    }

    /// Filled square (true) vs outline box (false).
    pub fn filled(self) -> bool {
        matches!(self, Category::Station | Category::Geo)
    }

    /// On-screen size multiplier — crewed stations are rendered bold.
    pub fn size_scale(self) -> f32 {
        if matches!(self, Category::Station) {
            1.7
        } else {
            1.0
        }
    }
}
