//! Minimal GeoJSON reader.
//!
//! We only need the geometry coordinate rings, so rather than model the full
//! GeoJSON schema we walk the parsed JSON and pull out every line of
//! `[longitude, latitude]` points. Handles LineString, MultiLineString,
//! Polygon and MultiPolygon — enough for Natural Earth coastlines and
//! admin-0 country boundaries.

use serde_json::Value;

/// A single open polyline of `[lon, lat]` points (degrees).
pub type PolyLine = Vec<[f64; 2]>;

/// A populated place: name, position (degrees) and max population.
pub struct City {
    pub name: String,
    pub lon: f64,
    pub lat: f64,
    pub pop: f64,
}

/// Parse a GeoJSON FeatureCollection of `Point` features (Natural Earth
/// populated places) into [`City`] records, reading `name` and `pop_max`.
/// Features missing a name or coordinates are skipped.
pub fn extract_cities(geojson: &str) -> Vec<City> {
    let root: Value = match serde_json::from_str(geojson) {
        Ok(v) => v,
        Err(e) => {
            log::error!("failed to parse cities GeoJSON: {e}");
            return Vec::new();
        }
    };
    let Some(features) = root.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for feature in features {
        let props = feature.get("properties");
        let geom = feature.get("geometry");
        let coords = geom
            .filter(|g| g.get("type").and_then(Value::as_str) == Some("Point"))
            .and_then(|g| g.get("coordinates"))
            .and_then(Value::as_array);
        let (Some(coords), Some(props)) = (coords, props) else {
            continue;
        };
        let (Some(lon), Some(lat)) = (
            coords.first().and_then(Value::as_f64),
            coords.get(1).and_then(Value::as_f64),
        ) else {
            continue;
        };
        let Some(name) = props
            .get("name")
            .or_else(|| props.get("nameascii"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let pop = props.get("pop_max").and_then(Value::as_f64).unwrap_or(0.0);
        out.push(City { name: name.to_string(), lon, lat, pop });
    }
    out
}

/// Parse a GeoJSON FeatureCollection and return every coordinate ring as a
/// flat list of polylines.
pub fn extract_polylines(geojson: &str) -> Vec<PolyLine> {
    let root: Value = match serde_json::from_str(geojson) {
        Ok(v) => v,
        Err(e) => {
            log::error!("failed to parse GeoJSON: {e}");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    let Some(features) = root.get("features").and_then(Value::as_array) else {
        return out;
    };

    for feature in features {
        let geom = match feature.get("geometry") {
            Some(g) if !g.is_null() => g,
            _ => continue,
        };
        let kind = geom.get("type").and_then(Value::as_str).unwrap_or("");
        let coords = match geom.get("coordinates") {
            Some(c) => c,
            None => continue,
        };

        match kind {
            "LineString" => push_line(coords, &mut out),
            "MultiLineString" => push_lines(coords, &mut out),
            "Polygon" => push_lines(coords, &mut out),
            "MultiPolygon" => {
                if let Some(polys) = coords.as_array() {
                    for poly in polys {
                        push_lines(poly, &mut out);
                    }
                }
            }
            _ => {}
        }
    }

    out
}

/// `coords` is an array of `[lon, lat]` positions.
fn push_line(coords: &Value, out: &mut Vec<PolyLine>) {
    if let Some(line) = parse_positions(coords) {
        if line.len() >= 2 {
            out.push(line);
        }
    }
}

/// Parse a GeoJSON FeatureCollection and return the **outer ring** of every
/// Polygon / MultiPolygon — the closed land boundaries, ready to be filled.
/// Holes (inner rings) are skipped so the fill covers the whole landmass.
pub fn extract_polygon_rings(geojson: &str) -> Vec<PolyLine> {
    let root: Value = match serde_json::from_str(geojson) {
        Ok(v) => v,
        Err(e) => {
            log::error!("failed to parse GeoJSON: {e}");
            return Vec::new();
        }
    };

    let mut out = Vec::new();
    let Some(features) = root.get("features").and_then(Value::as_array) else {
        return out;
    };

    for feature in features {
        let geom = match feature.get("geometry") {
            Some(g) if !g.is_null() => g,
            _ => continue,
        };
        let kind = geom.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(coords) = geom.get("coordinates") else {
            continue;
        };
        match kind {
            "Polygon" => push_outer_ring(coords, &mut out),
            "MultiPolygon" => {
                if let Some(polys) = coords.as_array() {
                    for poly in polys {
                        push_outer_ring(poly, &mut out);
                    }
                }
            }
            _ => {}
        }
    }

    out
}

/// `rings` is a polygon's array of rings (outer first, then holes). Push only
/// the outer ring.
fn push_outer_ring(rings: &Value, out: &mut Vec<PolyLine>) {
    if let Some(first) = rings.as_array().and_then(|a| a.first()) {
        push_line(first, out);
    }
}

/// `coords` is an array of lines (each an array of positions).
fn push_lines(coords: &Value, out: &mut Vec<PolyLine>) {
    if let Some(lines) = coords.as_array() {
        for line in lines {
            push_line(line, out);
        }
    }
}

fn parse_positions(coords: &Value) -> Option<PolyLine> {
    let arr = coords.as_array()?;
    let mut line = Vec::with_capacity(arr.len());
    for pt in arr {
        let p = pt.as_array()?;
        let lon = p.first()?.as_f64()?;
        let lat = p.get(1)?.as_f64()?;
        line.push([lon, lat]);
    }
    Some(line)
}
