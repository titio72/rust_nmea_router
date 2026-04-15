/// Backup log file parser for gap filling.
///
/// Log files are named `{log_dir}/YYYY-MM-DDTHHZ.log`, one per 2-hour UTC block.
/// Supported line types:
///   GNSS  <lon>  <lat>  <timestamp> ...
///   RPMe  <engine_num>  <rpm>
///   Roll  <val1>  <val2>          (degrees)
///   Wind  <speed_kn>  <angle_deg> (apparent, bow-relative)
///   Pres  <sensor_id>  <mbar>
///   CabT  <sensor_id>  <celsius>
///   HumI  <sensor_id>  <percent>
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use chrono::{DateTime, Datelike, Utc};

#[derive(Debug, Clone)]
pub struct GnssEntry {
    pub timestamp: SystemTime,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone)]
pub struct WindEntry {
    pub timestamp: SystemTime,
    /// Apparent wind speed in knots
    pub speed_kn: f64,
    /// Apparent wind angle in degrees (bow-relative, 0–360)
    pub angle_deg: f64,
}

#[derive(Debug, Clone)]
pub struct RollEntry {
    pub timestamp: SystemTime,
    pub val1: f64,
    pub val2: f64,
}

#[derive(Debug, Clone)]
pub struct PresEntry {
    pub timestamp: SystemTime,
    /// Pressure in millibar
    pub pressure_mbar: f64,
}

#[derive(Debug, Clone)]
pub struct CabTEntry {
    pub timestamp: SystemTime,
    pub temp_c: f64,
}

#[derive(Debug, Clone)]
pub struct HumIEntry {
    pub timestamp: SystemTime,
    /// Humidity percentage (0–100)
    pub humidity_pct: f64,
}

#[derive(Debug, Clone)]
pub struct RpmeEntry {
    pub timestamp: SystemTime,
    pub rpm: f64,
}

#[derive(Debug, Clone)]
pub enum LogEntry {
    Gnss(GnssEntry),
    Wind(WindEntry),
    Roll(RollEntry),
    Pres(PresEntry),
    CabT(CabTEntry),
    HumI(HumIEntry),
    Rpme(RpmeEntry),
}

impl LogEntry {
    pub fn timestamp(&self) -> SystemTime {
        match self {
            LogEntry::Gnss(e) => e.timestamp,
            LogEntry::Wind(e) => e.timestamp,
            LogEntry::Roll(e) => e.timestamp,
            LogEntry::Pres(e) => e.timestamp,
            LogEntry::CabT(e) => e.timestamp,
            LogEntry::HumI(e) => e.timestamp,
            LogEntry::Rpme(e) => e.timestamp,
        }
    }
}

/// Parse a single log line. Returns `None` for unrecognised or malformed lines.
pub fn parse_line(line: &str) -> Option<LogEntry> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.len() < 2 {
        return None;
    }

    // Token 0 is the line timestamp, token 1 is the message type.
    let ts = parse_timestamp(tokens[0])?;
    let kind = tokens[1];

    match kind {
        "GNSS" => {
            // GNSS <lon> <lat> <gnss_timestamp> ...
            if tokens.len() < 4 {
                return None;
            }
            let lon: f64 = tokens[2].parse().ok()?;
            let lat: f64 = tokens[3].parse().ok()?;
            // Validate coordinate ranges
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return None;
            }
            Some(LogEntry::Gnss(GnssEntry { timestamp: ts, latitude: lat, longitude: lon }))
        }
        "RPMe" => {
            // RPMe <engine_num> <rpm>
            if tokens.len() < 4 {
                return None;
            }
            let rpm: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::Rpme(RpmeEntry { timestamp: ts, rpm }))
        }
        "Roll" => {
            // Roll <val1> <val2>
            if tokens.len() < 4 {
                return None;
            }
            let val1: f64 = tokens[2].parse().ok()?;
            let val2: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::Roll(RollEntry { timestamp: ts, val1, val2 }))
        }
        "Wind" => {
            // Wind <speed_kn> <angle_deg>
            if tokens.len() < 4 {
                return None;
            }
            let speed_kn: f64 = tokens[2].parse().ok()?;
            let angle_deg: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::Wind(WindEntry { timestamp: ts, speed_kn, angle_deg }))
        }
        "Pres" => {
            // Pres <sensor_id> <mbar>
            if tokens.len() < 4 {
                return None;
            }
            let pressure_mbar: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::Pres(PresEntry { timestamp: ts, pressure_mbar }))
        }
        "CabT" => {
            // CabT <sensor_id> <celsius>
            if tokens.len() < 4 {
                return None;
            }
            let temp_c: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::CabT(CabTEntry { timestamp: ts, temp_c }))
        }
        "HumI" => {
            // HumI <sensor_id> <percent>
            if tokens.len() < 4 {
                return None;
            }
            let humidity_pct: f64 = tokens[3].parse().ok()?;
            Some(LogEntry::HumI(HumIEntry { timestamp: ts, humidity_pct }))
        }
        _ => None,
    }
}

/// Compute the log file paths that may contain data between `start` and `end` (both inclusive).
/// Files are written every 2 hours on even-hour boundaries: 00Z, 02Z, 04Z, …, 22Z.
pub fn find_log_files(log_dir: &Path, start: SystemTime, end: SystemTime) -> Vec<PathBuf> {
    let start_dt: DateTime<Utc> = start.into();
    let end_dt: DateTime<Utc> = end.into();

    let mut paths = Vec::new();

    // Iterate day by day from start_date to end_date to handle midnight crossings.
    let mut current_day = start_dt.date_naive();
    let end_day = end_dt.date_naive();

    while current_day <= end_day {
        // For each even-hour slot in the day, add the file if it might overlap [start, end].
        for slot_hour in (0u32..24).step_by(2) {
            let slot_start = chrono::NaiveDateTime::new(
                current_day,
                chrono::NaiveTime::from_hms_opt(slot_hour, 0, 0).unwrap(),
            );
            let slot_end = slot_start + chrono::Duration::hours(2);

            let slot_start_sys: SystemTime =
                DateTime::<Utc>::from_naive_utc_and_offset(slot_start, Utc).into();
            let slot_end_sys: SystemTime =
                DateTime::<Utc>::from_naive_utc_and_offset(slot_end, Utc).into();

            // Include this slot if it overlaps [start, end]
            if slot_end_sys > start && slot_start_sys < end {
                let filename = format!(
                    "{}-{:02}-{:02}T{:02}Z.log",
                    current_day.year(),
                    current_day.month(),
                    current_day.day(),
                    slot_hour
                );
                paths.push(log_dir.join(filename));
            }
        }
        current_day = current_day.succ_opt().unwrap_or(current_day);
        if current_day > end_day {
            break;
        }
    }

    paths
}

/// Read and return all log entries from `log_dir` that fall within [`start`, `end`].
/// Lines that fail to parse or fall outside the time window are silently skipped.
pub fn read_entries_in_range(
    log_dir: &Path,
    start: SystemTime,
    end: SystemTime,
) -> Vec<LogEntry> {
    let files = find_log_files(log_dir, start, end);
    let mut entries: Vec<LogEntry> = Vec::new();

    for path in &files {
        if !path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            if let Some(entry) = parse_line(line) {
                let ts = entry.timestamp();
                if ts >= start && ts <= end {
                    entries.push(entry);
                }
            }
        }
    }

    // Ensure entries are sorted by timestamp (files may overlap slightly near boundaries)
    entries.sort_by(|a, b| a.timestamp().cmp(&b.timestamp()));
    entries
}

fn parse_timestamp(s: &str) -> Option<SystemTime> {
    // Expected format: 2026-04-11T06:00:00.677Z
    let dt = DateTime::parse_from_rfc3339(s).ok()?;
    Some(SystemTime::from(dt))
}

// ---- tests ----------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn test_parse_gnss_line() {
        let line = "2026-04-11T06:00:00.677Z GNSS    9.4553728   42.9589615 2026-04-11T06:00:00.499Z -178 0";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::Gnss(g) = entry {
            assert!((g.longitude - 9.4553728).abs() < 1e-6);
            assert!((g.latitude - 42.9589615).abs() < 1e-6);
        } else {
            panic!("expected Gnss entry");
        }
    }

    #[test]
    fn test_parse_rpme_line() {
        let line = "2026-04-11T06:00:00.678Z RPMe 0 0";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::Rpme(r) = entry {
            assert_eq!(r.rpm, 0.0);
        } else {
            panic!("expected Rpme entry");
        }
    }

    #[test]
    fn test_parse_roll_line() {
        let line = "2026-04-11T06:00:00.804Z Roll   0.6   1.2";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::Roll(r) = entry {
            assert!((r.val1 - 0.6).abs() < 1e-9);
            assert!((r.val2 - 1.2).abs() < 1e-9);
        } else {
            panic!("expected Roll entry");
        }
    }

    #[test]
    fn test_parse_wind_line() {
        let line = "2026-04-11T06:00:00.804Z Wind  3.89 328.0";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::Wind(w) = entry {
            assert!((w.speed_kn - 3.89).abs() < 1e-9);
            assert!((w.angle_deg - 328.0).abs() < 1e-9);
        } else {
            panic!("expected Wind entry");
        }
    }

    #[test]
    fn test_parse_pres_line() {
        let line = "2026-04-11T06:02:02.089Z Pres 0 1017.0";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::Pres(p) = entry {
            assert!((p.pressure_mbar - 1017.0).abs() < 1e-9);
        } else {
            panic!("expected Pres entry");
        }
    }

    #[test]
    fn test_parse_cabt_line() {
        let line = "2026-04-11T06:02:02.089Z CabT 0 23.31";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::CabT(c) = entry {
            assert!((c.temp_c - 23.31).abs() < 1e-9);
        } else {
            panic!("expected CabT entry");
        }
    }

    #[test]
    fn test_parse_humi_line() {
        let line = "2026-04-11T06:02:02.089Z HumI 0 48.5";
        let entry = parse_line(line).expect("should parse");
        if let LogEntry::HumI(h) = entry {
            assert!((h.humidity_pct - 48.5).abs() < 1e-9);
        } else {
            panic!("expected HumI entry");
        }
    }

    #[test]
    fn test_parse_unknown_line_returns_none() {
        let line = "2026-04-11T06:00:00.678Z UNKN foo bar";
        assert!(parse_line(line).is_none());
    }

    #[test]
    fn test_find_log_files_single_day_one_slot() {
        use std::time::UNIX_EPOCH;
        use chrono::{TimeZone};
        let start = SystemTime::from(Utc.with_ymd_and_hms(2026, 4, 11, 6, 0, 0).unwrap());
        let end   = SystemTime::from(Utc.with_ymd_and_hms(2026, 4, 11, 7, 18, 0).unwrap());
        let _ = UNIX_EPOCH; // suppress unused import
        let files = find_log_files(Path::new("/tmp"), start, end);
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        // The gap spans 06:00–07:18 → only the 06Z slot overlaps
        assert!(names.contains(&"2026-04-11T06Z.log".to_string()), "expected 06Z file, got {:?}", names);
        // The 04Z slot ends at 06:00 exactly (exclusive end) — boundary: slot_end > start must hold.
        // 04Z slot_end = 06:00 == start, so 06:00 > 06:00 is false → should NOT be included.
        assert!(!names.contains(&"2026-04-11T04Z.log".to_string()), "04Z should not be included");
    }
}
