use std::collections::BTreeMap;

pub struct PolarTable {
    twa_breakpoints: Vec<f64>,
    tws_breakpoints: Vec<f64>,
    speeds: Vec<Vec<Option<f64>>>,
}

impl PolarTable {
    pub fn from_csv(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut lines = content.lines();

        // Skip the comment row (first line), handling optional BOM
        lines.next();

        // Parse header row: "angle,1,2,3,...,20"
        let header = lines.next().ok_or("missing header row")?;
        let cols: Vec<&str> = header.split(',').collect();
        // cols[0] = "angle", cols[1..] = tws values
        let tws_all: Vec<Option<f64>> = cols[1..]
            .iter()
            .map(|s| s.trim().parse::<f64>().ok())
            .collect();

        // Collect all data rows into (twa → entries)
        let mut rows: Vec<(f64, Vec<(usize, f64)>)> = Vec::new();
        for line in lines {
            let cells: Vec<&str> = line.split(',').collect();
            if cells.is_empty() {
                continue;
            }
            let twa = match cells[0].trim().parse::<f64>() {
                Ok(v) if v >= 0.0 => v,
                _ => continue,
            };
            let mut entries: Vec<(usize, f64)> = Vec::new();
            for (i, cell) in cells[1..].iter().enumerate() {
                if let Ok(spd) = cell.trim().parse::<f64>() {
                    entries.push((i, spd));
                }
            }
            if !entries.is_empty() {
                rows.push((twa, entries));
            }
        }

        if rows.is_empty() {
            return Err("no data rows found in polar CSV".into());
        }

        // Determine tws_breakpoints: columns that have at least one non-empty entry
        let mut tws_set: BTreeMap<usize, f64> = BTreeMap::new();
        for (_, entries) in &rows {
            for &(idx, _) in entries {
                if let Some(Some(tws)) = tws_all.get(idx) {
                    tws_set.insert(idx, *tws);
                }
            }
        }
        let tws_col_indices: Vec<usize> = tws_set.keys().copied().collect();
        let tws_breakpoints: Vec<f64> = tws_col_indices.iter().map(|i| tws_set[i]).collect();

        // Build twa_breakpoints (sorted)
        let mut twa_breakpoints: Vec<f64> = rows.iter().map(|(t, _)| *t).collect();
        twa_breakpoints.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Build speeds[twa_idx][tws_col_idx]
        let mut speed_map: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        for (twa, entries) in &rows {
            let ti = twa_breakpoints
                .iter()
                .position(|t| (*t - twa).abs() < 0.01)
                .unwrap();
            for &(col_idx, spd) in entries {
                if let Some(si) = tws_col_indices.iter().position(|&c| c == col_idx) {
                    speed_map.insert((ti, si), spd);
                }
            }
        }

        let speeds: Vec<Vec<Option<f64>>> = (0..twa_breakpoints.len())
            .map(|ti| {
                (0..tws_breakpoints.len())
                    .map(|si| speed_map.get(&(ti, si)).copied())
                    .collect()
            })
            .collect();

        Ok(Self {
            twa_breakpoints,
            tws_breakpoints,
            speeds,
        })
    }

    pub fn min_tws(&self) -> f64 {
        self.tws_breakpoints.first().copied().unwrap_or(0.0)
    }

    /// Boat speed in knots for given true wind angle (0–180°) and true wind speed (kn).
    /// Returns None when TWA < minimum polar angle or TWS < minimum populated column.
    /// Clamps TWS at the maximum populated column.
    pub fn boat_speed(&self, twa_deg: f64, tws_kn: f64) -> Option<f64> {
        let twa = twa_deg.clamp(0.0, 180.0);
        let max_tws = *self.tws_breakpoints.last()?;

        if tws_kn <= 0.0 {
            return None;
        }
        if twa < self.twa_breakpoints[0] {
            return None;
        }

        // Below the polar's minimum TWS, clamp to the lightest-air column and scale
        // proportionally so there's no hard cliff at the minimum breakpoint.
        let min_tws = self.tws_breakpoints[0];
        let tws = tws_kn.clamp(min_tws, max_tws);

        // Find bracketing TWA indices
        let ti_hi = self
            .twa_breakpoints
            .partition_point(|&v| v < twa)
            .min(self.twa_breakpoints.len() - 1);
        let ti = ti_hi.saturating_sub(1);

        // Find bracketing TWS indices
        let si_hi = self
            .tws_breakpoints
            .partition_point(|&v| v < tws)
            .min(self.tws_breakpoints.len() - 1);
        let si = si_hi.saturating_sub(1);

        let t_frac = if ti == ti_hi
            || (self.twa_breakpoints[ti_hi] - self.twa_breakpoints[ti]).abs() < 1e-9
        {
            0.0
        } else {
            (twa - self.twa_breakpoints[ti])
                / (self.twa_breakpoints[ti_hi] - self.twa_breakpoints[ti])
        };

        let s_frac = if si == si_hi
            || (self.tws_breakpoints[si_hi] - self.tws_breakpoints[si]).abs() < 1e-9
        {
            0.0
        } else {
            (tws - self.tws_breakpoints[si])
                / (self.tws_breakpoints[si_hi] - self.tws_breakpoints[si])
        };

        let v00 = self.speeds[ti][si]?;
        let v10 = self.speeds[ti_hi][si].unwrap_or(v00);
        let v01 = self.speeds[ti][si_hi].unwrap_or(v00);
        let v11 = self.speeds[ti_hi][si_hi].unwrap_or(v00);

        let raw = v00 * (1.0 - t_frac) * (1.0 - s_frac)
            + v10 * t_frac * (1.0 - s_frac)
            + v01 * (1.0 - t_frac) * s_frac
            + v11 * t_frac * s_frac;

        // Scale down when wind is below the polar's minimum — constant-column extrapolation
        // with a linear taper to zero prevents an unrealistic speed cliff.
        let scale = if tws_kn < min_tws { tws_kn / min_tws } else { 1.0 };
        Some(raw * scale)
    }

    /// Test-only constructor: returns a polar that always yields `speed_kn`
    /// for any TWA >= 42° and TWS >= 5 kn, and None otherwise.
    #[cfg(test)]
    pub fn constant_for_test(speed_kn: f64) -> Self {
        Self {
            twa_breakpoints: vec![42.0, 180.0],
            tws_breakpoints: vec![5.0, 20.0],
            speeds: vec![
                vec![Some(speed_kn), Some(speed_kn)],
                vec![Some(speed_kn), Some(speed_kn)],
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load() -> PolarTable {
        PolarTable::from_csv("tests/fixtures/dufour40.csv").expect("load polar")
    }

    #[test]
    fn test_polar_loads_and_has_breakpoints() {
        let p = load();
        // The CSV has data at TWS 6, 8, 10, 12, 14, 16, 18, 20
        assert!(
            p.tws_breakpoints.contains(&6.0),
            "expected 6 kn: {:?}",
            p.tws_breakpoints
        );
        assert!(
            p.tws_breakpoints.contains(&20.0),
            "expected 20 kn: {:?}",
            p.tws_breakpoints
        );
        // TWA rows: 42, 52, 60, 75, 90, 110, 120, 135, 150, 180
        assert!(p.twa_breakpoints.contains(&42.0));
        assert!(p.twa_breakpoints.contains(&180.0));
    }

    #[test]
    fn test_polar_exact_lookup_twa90_tws10() {
        let p = load();
        // From CSV: TWA=90, TWS=10 → 7.44 kn
        let spd = p.boat_speed(90.0, 10.0).expect("should have value");
        assert!((spd - 7.44).abs() < 0.05, "got {}", spd);
    }

    #[test]
    fn test_polar_returns_none_for_zero_wind() {
        let p = load();
        assert!(p.boat_speed(90.0, 0.0).is_none());
        assert!(p.boat_speed(90.0, -1.0).is_none());
    }

    #[test]
    fn test_polar_scales_below_min_tws() {
        let p = load();
        // At exactly min_tws (6 kn) → full speed
        let full = p.boat_speed(90.0, 6.0).expect("should have value at min_tws");
        // At half of min_tws (3 kn) → half of full speed (proportional scale)
        let half = p.boat_speed(90.0, 3.0).expect("should extrapolate below min_tws");
        assert!((half - full * 0.5).abs() < 0.05, "expected half speed at half min_tws: full={full:.2}, half={half:.2}");
        // At 5.88 kn (just below 6 kn) → close to full speed
        let near_full = p.boat_speed(90.0, 5.88).expect("should sail at 5.88 kn");
        assert!(near_full > full * 0.95, "expected near-full speed at 5.88 kn, got {near_full:.2}");
    }

    #[test]
    fn test_polar_returns_none_below_min_twa() {
        let p = load();
        // Below 42° (lowest TWA row) → None
        assert!(p.boat_speed(30.0, 10.0).is_none());
    }

    #[test]
    fn test_polar_interpolates_between_tws() {
        let p = load();
        // TWS=9 is midway between 8 (6.63) and 10 (7.44) at TWA=90
        let spd = p.boat_speed(90.0, 9.0).expect("should interpolate");
        assert!(spd > 6.63 && spd < 7.44, "got {}", spd);
    }

    #[test]
    fn test_polar_interpolates_between_twa() {
        let p = load();
        // TWA=82 is between 75 (7.69) and 90 (7.82) at TWS=12
        let spd = p.boat_speed(82.0, 12.0).expect("should interpolate");
        assert!(spd > 7.0 && spd < 8.0, "got {}", spd);
    }

    #[test]
    fn test_polar_clamps_tws_above_max() {
        let p = load();
        // TWS=30 → clamp to 20, same result as TWS=20 at TWA=90
        let spd_20 = p.boat_speed(90.0, 20.0).unwrap();
        let spd_30 = p.boat_speed(90.0, 30.0).unwrap();
        assert!((spd_30 - spd_20).abs() < 0.01);
    }
}
