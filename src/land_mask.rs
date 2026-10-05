use std::error::Error;

const LAT_MIN: f64 = 28.0;
const LON_MIN: f64 = -8.0;
const LAT_RANGE: f64 = 20.0; // 28..48
const LON_RANGE: f64 = 50.0; // -8..42

pub struct LandMask {
    lat_min: f64,
    lat_step: f64,
    rows: usize,
    lon_min: f64,
    lon_step: f64,
    cols: usize,
    grid: Vec<u8>,
}

impl LandMask {
    pub fn from_geojson(path: &str, resolution_deg: f64) -> Result<Self, Box<dyn Error>> {
        let contents = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&contents)?;
        from_geojson_value(&json, resolution_deg)
    }

    pub fn is_land(&self, lat: f64, lon: f64) -> bool {
        let lat_max = self.lat_min + self.lat_step * self.rows as f64;
        let lon_max = self.lon_min + self.lon_step * self.cols as f64;
        if lat < self.lat_min || lat >= lat_max { return false; }
        if lon < self.lon_min || lon >= lon_max { return false; }
        let row = ((lat - self.lat_min) / self.lat_step) as usize;
        let col = ((lon - self.lon_min) / self.lon_step) as usize;
        if row >= self.rows || col >= self.cols { return false; }
        let idx = row * self.cols + col;
        (self.grid[idx / 8] >> (idx % 8)) & 1 == 1
    }

    #[cfg(test)]
    pub fn from_geojson_value_for_test(
        json: &serde_json::Value,
        resolution_deg: f64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        from_geojson_value(json, resolution_deg)
    }
}

fn from_geojson_value(json: &serde_json::Value, resolution_deg: f64) -> Result<LandMask, Box<dyn Error>> {
    let lat_step = resolution_deg;
    let lon_step = resolution_deg;
    let rows = (LAT_RANGE / lat_step).ceil() as usize;
    let cols = (LON_RANGE / lon_step).ceil() as usize;
    let mut grid = vec![0u8; (rows * cols + 7) / 8];

    let features = json["features"]
        .as_array()
        .ok_or("GeoJSON missing 'features' array")?;

    for feature in features {
        let geom = &feature["geometry"];
        for ring in extract_rings(geom) {
            rasterize_ring(&ring, &mut grid, LAT_MIN, lat_step, rows, LON_MIN, lon_step, cols);
        }
    }

    Ok(LandMask { lat_min: LAT_MIN, lat_step, rows, lon_min: LON_MIN, lon_step, cols, grid })
}

fn extract_rings(geom: &serde_json::Value) -> Vec<Vec<[f64; 2]>> {
    let mut rings = Vec::new();
    match geom["type"].as_str() {
        Some("Polygon") => {
            if let Some(poly_rings) = geom["coordinates"].as_array() {
                for ring_arr in poly_rings {
                    if let Some(ring) = parse_ring(ring_arr) {
                        rings.push(ring);
                    }
                }
            }
        }
        Some("MultiPolygon") => {
            if let Some(polys) = geom["coordinates"].as_array() {
                for poly in polys {
                    if let Some(poly_rings) = poly.as_array() {
                        for ring_arr in poly_rings {
                            if let Some(ring) = parse_ring(ring_arr) {
                                rings.push(ring);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    rings
}

fn parse_ring(arr: &serde_json::Value) -> Option<Vec<[f64; 2]>> {
    arr.as_array()?.iter().map(|pt| {
        let coords = pt.as_array()?;
        let lon = coords.get(0)?.as_f64()?;
        let lat = coords.get(1)?.as_f64()?;
        Some([lon, lat])
    }).collect()
}

fn rasterize_ring(
    ring: &[[f64; 2]],
    grid: &mut Vec<u8>,
    lat_min: f64, lat_step: f64, rows: usize,
    lon_min: f64, lon_step: f64, cols: usize,
) {
    if ring.len() < 3 { return; }

    let lat_min_ring = ring.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
    let lat_max_ring = ring.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
    let lat_bound_max = lat_min + lat_step * rows as f64;

    if lat_max_ring < lat_min || lat_min_ring >= lat_bound_max { return; }

    let row_start = ((lat_min_ring - lat_min) / lat_step).floor().max(0.0) as usize;
    let row_end = (((lat_max_ring - lat_min) / lat_step).ceil() as usize).min(rows);
    let n = ring.len();

    for row in row_start..row_end {
        let lat_c = lat_min + (row as f64 + 0.5) * lat_step;
        let mut crossings: Vec<f64> = Vec::new();

        for i in 0..n {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let (lat_a, lat_b) = (a[1], b[1]);
            if (lat_a <= lat_c && lat_b > lat_c) || (lat_b <= lat_c && lat_a > lat_c) {
                let t = (lat_c - lat_a) / (lat_b - lat_a);
                crossings.push(a[0] + t * (b[0] - a[0]));
            }
        }

        if crossings.is_empty() { continue; }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut i = 0;
        while i + 1 < crossings.len() {
            // Mark a column land only if its center falls within the crossing interval —
            // matching the row's own center-sampling — instead of marking every column the
            // interval merely overlaps. The overlap approach flags an entire coastal cell as
            // land from a sliver at its edge, which false-positives on harbors and river
            // mouths sitting in the water portion of that same cell.
            let col_start = (((crossings[i] - lon_min) / lon_step) - 0.5).ceil().max(0.0) as usize;
            let col_end = ((((crossings[i + 1] - lon_min) / lon_step) - 0.5).ceil() as usize).min(cols);
            for col in col_start..col_end {
                let idx = row * cols + col;
                grid[idx / 8] |= 1 << (idx % 8);
            }
            i += 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_mask() -> LandMask {
        // A 1°×1° land square: lon 10–11, lat 40–41 (inside Med bbox)
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]]
                },
                "properties": {}
            }]
        });
        from_geojson_value(&json, 0.05).unwrap()
    }

    #[test]
    fn test_is_land_inside_polygon() {
        let mask = synthetic_mask();
        assert!(mask.is_land(40.5, 10.5), "center of polygon should be land");
    }

    #[test]
    fn test_is_sea_outside_polygon() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(39.5, 10.5), "south of polygon should be sea");
        assert!(!mask.is_land(40.5, 9.5),  "west of polygon should be sea");
    }

    #[test]
    fn test_is_sea_outside_bbox() {
        let mask = synthetic_mask();
        assert!(!mask.is_land(60.0, 10.5), "above lat 48 is outside bbox");
        assert!(!mask.is_land(40.5, 50.0), "east of lon 42 is outside bbox");
        assert!(!mask.is_land(27.0, 10.5), "below lat 28 is outside bbox");
    }

    #[test]
    fn test_multipolygon_geometry() {
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "MultiPolygon",
                    "coordinates": [
                        [[[10.0, 40.0], [11.0, 40.0], [11.0, 41.0], [10.0, 41.0], [10.0, 40.0]]],
                        [[[15.0, 35.0], [16.0, 35.0], [16.0, 36.0], [15.0, 36.0], [15.0, 35.0]]]
                    ]
                },
                "properties": {}
            }]
        });
        let mask = from_geojson_value(&json, 0.05).unwrap();
        assert!(mask.is_land(40.5, 10.5));
        assert!(mask.is_land(35.5, 15.5));
        assert!(!mask.is_land(38.0, 13.0)); // gap between polygons
    }

    #[test]
    fn test_harbor_notch_not_marked_land() {
        // A coastline with a bay: land fills lon 10-20 / lat 30-40 except a notch open to the
        // north between lon 14.1-16.4, lat 36-40 (a harbor). At 1° resolution the notch's left
        // wall sits just inside grid cell [14,15), leaving that cell 90% water with its center
        // at lon 14.5 squarely in the harbor — it must not be flagged as land.
        let json = serde_json::json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "geometry": {
                    "type": "Polygon",
                    "coordinates": [[
                        [10.0, 30.0], [20.0, 30.0], [20.0, 40.0],
                        [16.4, 40.0], [16.4, 36.0], [14.1, 36.0], [14.1, 40.0],
                        [10.0, 40.0], [10.0, 30.0]
                    ]]
                },
                "properties": {}
            }]
        });
        let mask = from_geojson_value(&json, 1.0).unwrap();

        assert!(!mask.is_land(36.5, 14.5), "harbor cell (mostly water) should not be land");
        assert!(mask.is_land(36.5, 16.5), "cell with land-covered center should still be land");
        assert!(mask.is_land(32.0, 15.0), "well inside the landmass should be land");
    }

    #[test]
    fn test_from_geojson_missing_file() {
        let result = LandMask::from_geojson("/nonexistent/path.geojson", 0.05);
        assert!(result.is_err());
    }

}
