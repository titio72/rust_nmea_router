use std::collections::BTreeMap;

/// Tolerance (degrees) used when comparing / deduplicating TWA breakpoints.
const ANGLE_EPS: f64 = 0.01;

/// Which on-disk polar layout a CSV uses.
enum PolarCsvFormat {
    /// Dense `angle,1,2,...,20` grid with a leading comment row; every row key is a
    /// numeric TWA and unpopulated wind columns are left empty (e.g. `dufour40.csv`).
    AngleGrid,
    /// ORC-style `TWA,6,8,...,20` table whose extreme rows are labelled
    /// (`Beat_Angle`/`Beat_Speed`, `Run_Angle`/`Run_Speed`) and therefore carry a
    /// different optimum angle per wind column (e.g. `dufour_40_ITA_17811.csv`).
    OrcLabelled,
}

pub struct PolarTable {
    twa_breakpoints: Vec<f64>,
    tws_breakpoints: Vec<f64>,
    speeds: Vec<Vec<Option<f64>>>,
    /// Tightest sailable angle per wind column — the beat angle. Constant across columns
    /// for the dense `angle` grid, but wind-speed dependent for ORC-style polars.
    col_min_twa: Vec<f64>,
    /// Deepest angle the polar covers per wind column — the run angle for ORC-style
    /// polars, 180° for the dense `angle` grid.
    col_max_twa: Vec<f64>,
}

impl PolarTable {
    /// Derives the per-column angular limits from the populated cells and returns the
    /// finished table.
    fn new(
        twa_breakpoints: Vec<f64>,
        tws_breakpoints: Vec<f64>,
        speeds: Vec<Vec<Option<f64>>>,
    ) -> Self {
        let col_min_twa: Vec<f64> = (0..tws_breakpoints.len())
            .map(|si| {
                speeds
                    .iter()
                    .zip(&twa_breakpoints)
                    .find(|(row, _)| row[si].is_some())
                    .map_or(f64::INFINITY, |(_, twa)| *twa)
            })
            .collect();
        let col_max_twa: Vec<f64> = (0..tws_breakpoints.len())
            .map(|si| {
                speeds
                    .iter()
                    .zip(&twa_breakpoints)
                    .filter(|(row, _)| row[si].is_some())
                    .next_back()
                    .map_or(f64::NEG_INFINITY, |(_, twa)| *twa)
            })
            .collect();
        Self {
            twa_breakpoints,
            tws_breakpoints,
            speeds,
            col_min_twa,
            col_max_twa,
        }
    }

    pub fn from_csv(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Self::from_csv_str(&content).map_err(|e| format!("{path}: {e}").into())
    }

    /// Parses either supported polar layout, detected from the header row.
    pub fn from_csv_str(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = content.trim_start_matches('\u{feff}');
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();

        let (format, header_idx) = detect_format(&lines);
        let header = *lines.get(header_idx).ok_or("missing header row")?;
        let rows = lines[header_idx + 1..].iter().copied();

        match format {
            PolarCsvFormat::AngleGrid => Self::from_angle_grid(header, rows),
            PolarCsvFormat::OrcLabelled => Self::from_orc_labelled(header, rows),
        }
    }

    /// Dense numeric grid: every data row is keyed by a TWA and shares one TWA axis.
    fn from_angle_grid<'a>(
        header: &str,
        lines: impl Iterator<Item = &'a str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
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

        Ok(Self::new(twa_breakpoints, tws_breakpoints, speeds))
    }

    /// ORC-style table: `Beat_Angle`/`Beat_Speed` and `Run_Angle`/`Run_Speed` give a
    /// per-wind-column optimum angle, so each TWS column is first assembled into its own
    /// speed curve and then resampled onto a shared TWA axis. The labelled rows are
    /// optional — a `TWA`-headed file with only numeric rows parses too.
    fn from_orc_labelled<'a>(
        header: &str,
        lines: impl Iterator<Item = &'a str>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let header_cells: Vec<&str> = header.split(',').skip(1).map(str::trim).collect();
        // Ignore trailing empty cells, but a gap in the middle would silently shift every
        // column, so anything non-numeric before the last value is an error.
        let col_count = header_cells
            .iter()
            .rposition(|c| !c.is_empty())
            .map_or(0, |i| i + 1);
        let mut tws_breakpoints: Vec<f64> = Vec::with_capacity(col_count);
        for cell in &header_cells[..col_count] {
            tws_breakpoints.push(
                cell.parse::<f64>()
                    .map_err(|_| format!("invalid TWS column header {cell:?}"))?,
            );
        }
        if tws_breakpoints.is_empty() {
            return Err("no TWS columns in polar header".into());
        }
        if tws_breakpoints.windows(2).any(|w| w[1] <= w[0]) {
            return Err("TWS column headers must be strictly ascending".into());
        }

        let mut beat_angles: Vec<Option<f64>> = vec![None; col_count];
        let mut beat_speeds: Vec<Option<f64>> = vec![None; col_count];
        let mut run_angles: Vec<Option<f64>> = vec![None; col_count];
        let mut run_speeds: Vec<Option<f64>> = vec![None; col_count];
        let mut fixed_rows: Vec<(f64, Vec<Option<f64>>)> = Vec::new();

        for line in lines {
            let cells: Vec<&str> = line.split(',').collect();
            let label = cells[0].trim();
            if label.is_empty() {
                continue;
            }
            let values: Vec<Option<f64>> = (0..col_count)
                .map(|i| cells.get(i + 1).and_then(|c| c.trim().parse::<f64>().ok()))
                .collect();

            match normalize_label(label).as_str() {
                "beatangle" => beat_angles = values,
                "beatspeed" => beat_speeds = values,
                "runangle" => run_angles = values,
                "runspeed" => run_speeds = values,
                _ => match label.parse::<f64>() {
                    Ok(twa) if (0.0..=180.0).contains(&twa) => fixed_rows.push((twa, values)),
                    _ => continue, // unknown label row — ignore rather than fail the whole file
                },
            }
        }

        // Assemble one (twa, speed) curve per wind column, in ascending angle order.
        let mut curves: Vec<Vec<(f64, f64)>> = vec![Vec::new(); col_count];
        for (ci, curve) in curves.iter_mut().enumerate() {
            if let (Some(a), Some(s)) = (beat_angles[ci], beat_speeds[ci]) {
                curve.push((a, s));
            }
            for (twa, values) in &fixed_rows {
                if let Some(spd) = values[ci] {
                    curve.push((*twa, spd));
                }
            }
            // The certificate carries no data deeper than the optimum running angle, so the
            // curve simply ends there and `boat_speed` reports None beyond it rather than
            // inventing a dead-downwind figure.
            if let (Some(a), Some(s)) = (run_angles[ci], run_speeds[ci]) {
                curve.push((a, s));
            }
            curve.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            curve.dedup_by(|a, b| (a.0 - b.0).abs() < ANGLE_EPS);
        }

        // Drop wind columns that carried no data at all.
        let kept: Vec<usize> = (0..col_count).filter(|&ci| !curves[ci].is_empty()).collect();
        if kept.is_empty() {
            return Err("no data rows found in polar CSV".into());
        }
        let tws_breakpoints: Vec<f64> = kept.iter().map(|&ci| tws_breakpoints[ci]).collect();
        let curves: Vec<Vec<(f64, f64)>> = kept.iter().map(|&ci| curves[ci].clone()).collect();

        // Shared TWA axis: every angle mentioned by any column.
        let mut twa_breakpoints: Vec<f64> = Vec::new();
        for curve in &curves {
            for &(twa, _) in curve {
                if !twa_breakpoints.iter().any(|t| (t - twa).abs() < ANGLE_EPS) {
                    twa_breakpoints.push(twa);
                }
            }
        }
        twa_breakpoints.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Resample each column onto the shared axis. Angles outside a column's own range
        // stay None, which is what keeps the no-go zone wind-speed dependent.
        let speeds: Vec<Vec<Option<f64>>> = twa_breakpoints
            .iter()
            .map(|&twa| {
                curves
                    .iter()
                    .map(|curve| interpolate_curve(curve, twa))
                    .collect()
            })
            .collect();

        Ok(Self::new(twa_breakpoints, tws_breakpoints, speeds))
    }

    pub fn min_tws(&self) -> f64 {
        self.tws_breakpoints.first().copied().unwrap_or(0.0)
    }

    /// Boat speed in knots for given true wind angle (0–180°) and true wind speed (kn).
    /// Returns None outside the angular range the polar covers at that wind speed — tighter
    /// than the beat angle or deeper than the run angle — and for TWS <= 0. Clamps TWS at
    /// the maximum populated column.
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

        // The polar only covers beat angle → run angle, and for ORC-style tables both of
        // those move with the wind speed. Interpolate the two limits across the bracketing
        // columns and report no target speed outside them: closer than the beat angle the
        // boat cannot sail, and deeper than the run angle the certificate holds no data.
        let lerp = |lo: f64, hi: f64| lo + (hi - lo) * s_frac;
        let min_twa = lerp(self.col_min_twa[si], self.col_min_twa[si_hi]);
        let max_twa = lerp(self.col_max_twa[si], self.col_max_twa[si_hi]);
        if twa < min_twa - ANGLE_EPS || twa > max_twa + ANGLE_EPS {
            return None;
        }

        // Inside that window, interpolate within each bracketing wind column. One column may
        // still lack a value at this exact TWA row when the other column's beat or run angle
        // introduced it; `column_speed` holds the populated end in that case.
        let raw = match (
            self.column_speed(si, ti, ti_hi, t_frac),
            self.column_speed(si_hi, ti, ti_hi, t_frac),
        ) {
            (Some(lo), Some(hi)) => lerp(lo, hi),
            (Some(lo), None) => lo,
            (None, Some(hi)) => hi,
            (None, None) => return None,
        };

        // Scale down when wind is below the polar's minimum — constant-column extrapolation
        // with a linear taper to zero prevents an unrealistic speed cliff.
        let scale = if tws_kn < min_tws { tws_kn / min_tws } else { 1.0 };
        Some(raw * scale)
    }

    /// Speed of a single wind column, interpolated between TWA rows `ti` and `ti_hi`.
    /// Returns None when this column has no data at either row.
    fn column_speed(&self, si: usize, ti: usize, ti_hi: usize, t_frac: f64) -> Option<f64> {
        match (self.speeds[ti][si], self.speeds[ti_hi][si]) {
            (Some(lo), Some(hi)) => Some(lo + (hi - lo) * t_frac),
            // One end of the bracket falls outside this column's own beat→run range — hold
            // the populated end rather than extrapolating past it.
            (Some(lo), None) => Some(lo),
            (None, Some(hi)) => Some(hi),
            (None, None) => None,
        }
    }

    /// Test-only constructor: returns a polar that always yields `speed_kn`
    /// for any TWA >= 42° and TWS >= 5 kn, and None otherwise.
    #[cfg(test)]
    pub fn constant_for_test(speed_kn: f64) -> Self {
        Self::new(
            vec![42.0, 180.0],
            vec![5.0, 20.0],
            vec![
                vec![Some(speed_kn), Some(speed_kn)],
                vec![Some(speed_kn), Some(speed_kn)],
            ],
        )
    }
}

/// Lowercases a row label and strips spaces, underscores and hyphens so that
/// `Beat_Angle`, `beat angle` and `BeatAngle` all match.
fn normalize_label(label: &str) -> String {
    label
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '-' | '\t'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Locates the header row and decides which layout the file uses. Falls back to the
/// historical assumption — one comment row followed by the header — when no recognised
/// header key is found.
fn detect_format(lines: &[&str]) -> (PolarCsvFormat, usize) {
    for (idx, line) in lines.iter().enumerate() {
        let first_cell = normalize_label(line.split(',').next().unwrap_or(""));
        match first_cell.as_str() {
            "twa" | "twa/tws" => return (PolarCsvFormat::OrcLabelled, idx),
            "angle" | "angle/wind" | "deg" => return (PolarCsvFormat::AngleGrid, idx),
            _ => {}
        }
    }
    (PolarCsvFormat::AngleGrid, 1)
}

/// Linear interpolation along one wind column's speed curve. Returns None outside the
/// curve's angular range — below its beat angle the boat simply cannot sail that close.
fn interpolate_curve(curve: &[(f64, f64)], twa: f64) -> Option<f64> {
    let &(first_twa, first_spd) = curve.first()?;
    let &(last_twa, last_spd) = curve.last()?;
    if twa < first_twa - ANGLE_EPS || twa > last_twa + ANGLE_EPS {
        return None;
    }
    let hi = curve.partition_point(|&(a, _)| a < twa);
    if hi == 0 {
        return Some(first_spd);
    }
    if hi >= curve.len() {
        return Some(last_spd);
    }
    let (a0, s0) = curve[hi - 1];
    let (a1, s1) = curve[hi];
    if (a1 - a0).abs() < 1e-9 {
        return Some(s1);
    }
    Some(s0 + (s1 - s0) * (twa - a0) / (a1 - a0))
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

    // ---- ORC-style labelled format (Beat_Angle / Run_Angle rows) ----

    fn load_orc() -> PolarTable {
        PolarTable::from_csv("tests/fixtures/dufour_40_ITA_17811.csv").expect("load ORC polar")
    }

    #[test]
    fn test_orc_loads_breakpoints() {
        let p = load_orc();
        assert_eq!(p.tws_breakpoints, vec![6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0]);
        assert!((p.min_tws() - 6.0).abs() < 1e-9);
        // Lowest breakpoint is the tightest beat angle of any column (38.8° @ 16 kn).
        assert!(
            (p.twa_breakpoints[0] - 38.8).abs() < 1e-9,
            "got {:?}",
            p.twa_breakpoints
        );
        // The axis stops at the deepest run angle — the file carries nothing past it.
        assert!((p.twa_breakpoints.last().unwrap() - 169.0).abs() < 1e-9);
        // Fixed rows and every beat/run angle are present.
        for a in [39.6, 44.4, 52.0, 90.0, 135.0, 142.0, 169.0] {
            assert!(
                p.twa_breakpoints.iter().any(|t| (t - a).abs() < ANGLE_EPS),
                "missing {a}° in {:?}",
                p.twa_breakpoints
            );
        }
    }

    #[test]
    fn test_orc_exact_lookup() {
        let p = load_orc();
        // From the CSV: TWA=90, TWS=10 → 7.62 kn
        let spd = p.boat_speed(90.0, 10.0).expect("should have value");
        assert!((spd - 7.62).abs() < 0.01, "got {spd}");
        // TWA=135, TWS=20 → 9.55 kn
        let spd = p.boat_speed(135.0, 20.0).expect("should have value");
        assert!((spd - 9.55).abs() < 0.01, "got {spd}");
    }

    #[test]
    fn test_orc_beat_point_is_per_wind_column() {
        let p = load_orc();
        // Beat_Angle/Beat_Speed at 6 kn is 44.4° / 4.62 kn.
        let spd = p.boat_speed(44.4, 6.0).expect("should sail at its beat angle");
        assert!((spd - 4.62).abs() < 0.02, "got {spd}");
        // 6 kn cannot point tighter than 44.4°, even though 16 kn can reach 38.8°.
        assert!(
            p.boat_speed(40.0, 6.0).is_none(),
            "6 kn must not sail at 40°"
        );
        let spd = p.boat_speed(38.8, 16.0).expect("16 kn beats at 38.8°");
        assert!((spd - 6.83).abs() < 0.02, "got {spd}");
    }

    #[test]
    fn test_orc_beat_angle_interpolates_across_wind_columns() {
        let p = load_orc();
        // At 11 kn the beat angle lies between the 10 kn (41.1°) and 12 kn (40.0°) columns,
        // i.e. ~40.55°.
        let spd = p.boat_speed(41.0, 11.0).expect("11 kn should sail at 41°");
        assert!((6.2..6.6).contains(&spd), "got {spd}");
        assert!(
            p.boat_speed(40.2, 11.0).is_none(),
            "40.2° is tighter than the 11 kn beat angle"
        );
        // 40° is tighter than both the 6 kn (44.4°) and 8 kn (42.3°) beat angles.
        assert!(p.boat_speed(40.0, 7.0).is_none(), "7 kn must not sail at 40°");
    }

    #[test]
    fn test_orc_returns_none_below_min_twa() {
        let p = load_orc();
        assert!(p.boat_speed(30.0, 10.0).is_none());
    }

    #[test]
    fn test_orc_returns_none_past_the_run_angle() {
        let p = load_orc();
        // Run_Angle/Run_Speed at 20 kn is 169° / 8.75 kn — the deepest covered angle.
        let run = p.boat_speed(169.0, 20.0).expect("should sail at its run angle");
        assert!((run - 8.75).abs() < 0.02, "got {run}");
        assert!(p.boat_speed(175.0, 20.0).is_none(), "no data past 169° @ 20 kn");
        assert!(p.boat_speed(180.0, 20.0).is_none(), "no dead-downwind target");
        // At 6 kn the run angle is only 142°, so much less of the range is covered.
        let run_6 = p.boat_speed(142.0, 6.0).expect("6 kn runs at 142°");
        assert!((run_6 - 3.85).abs() < 0.02, "got {run_6}");
        assert!(p.boat_speed(150.0, 6.0).is_none(), "no data past 142° @ 6 kn");
        // The run angle interpolates across columns: at 11 kn it lies between 147° and 151°.
        assert!(p.boat_speed(148.0, 11.0).is_some(), "148° is inside 11 kn range");
        assert!(p.boat_speed(152.0, 11.0).is_none(), "152° is past the 11 kn run angle");
    }

    #[test]
    fn test_orc_interpolates_between_tws_and_twa() {
        let p = load_orc();
        // TWS=9 lies between 8 (7.03) and 10 (7.62) at TWA=90.
        let spd = p.boat_speed(90.0, 9.0).expect("should interpolate TWS");
        assert!(spd > 7.03 && spd < 7.62, "got {spd}");
        // TWA=100 lies between 90 (7.62) and 110 (7.60) at TWS=10.
        let spd = p.boat_speed(100.0, 10.0).expect("should interpolate TWA");
        assert!(spd > 7.5 && spd < 7.7, "got {spd}");
    }

    #[test]
    fn test_orc_labelled_rows_are_optional() {
        // A TWA-headed file with only numeric rows must still parse.
        let csv = "TWA,6,10\n52,5.0,6.8\n90,6.0,7.6\n";
        let p = PolarTable::from_csv_str(csv).expect("plain TWA header should parse");
        assert_eq!(p.tws_breakpoints, vec![6.0, 10.0]);
        assert_eq!(p.twa_breakpoints, vec![52.0, 90.0]);
        let spd = p.boat_speed(90.0, 10.0).unwrap();
        assert!((spd - 7.6).abs() < 0.01, "got {spd}");
    }

    #[test]
    fn test_orc_rejects_non_numeric_tws_header() {
        let csv = "TWA,6,ten\n90,6.0,7.6\n";
        assert!(PolarTable::from_csv_str(csv).is_err());
    }

    #[test]
    fn test_angle_grid_format_still_detected() {
        // The legacy file keeps parsing through the shared entry point.
        let p = PolarTable::from_csv("tests/fixtures/dufour40.csv").expect("load legacy polar");
        assert!(p.twa_breakpoints.contains(&42.0));
        assert!((p.boat_speed(90.0, 10.0).unwrap() - 7.44).abs() < 0.05);
    }
}
