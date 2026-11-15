//! Bus warnings: configuration problems visible in the observed traffic.
//!
//! Business logic here never reads the clock: `now` is passed in.

use super::names::pgn_name;
use super::registry::BusRegistry;
use crate::config::SourceFilterConfig;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WarningLevel {
    Red,
    Amber,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BusWarning {
    pub level: WarningLevel,
    pub pgn: u32,
    pub configured_source: Option<u8>,
    pub actual_sources: Vec<u8>,
    pub message: String,
}

pub const REQUIRED_PGNS: [u32; 3] = [129025, 129026, 126992];

/// Warnings are withheld for this long after the router starts listening, and "sent recently"
/// means within it.
const WINDOW: Duration = Duration::from_secs(60);

/// The PGN an address-conflict warning refers to: the conflict is about address claiming.
const PGN_ADDRESS_CLAIM: u32 = 60928;

fn is_recent(t: Option<Instant>, now: Instant) -> bool {
    t.is_some_and(|t| now.saturating_duration_since(t) <= WINDOW)
}

fn display_name(pgn: u32) -> String {
    pgn_name(pgn).map_or_else(|| format!("PGN {pgn}"), str::to_string)
}

/// Sources (ascending) that sent `pgn` within the last 60 s, optionally excluding one.
fn recent_sources(reg: &BusRegistry, pgn: u32, except: Option<u8>, now: Instant) -> Vec<u8> {
    reg.devices()
        .keys()
        .copied()
        .filter(|&src| Some(src) != except && is_recent(reg.last_sent(src, pgn), now))
        .collect()
}

/// The router's claimed address, if another device was heard transmitting from it within the
/// last 60 s.
pub fn conflicting_address(reg: &BusRegistry, now: Instant) -> Option<u8> {
    let (own_address, _) = reg.node_state();
    own_address.filter(|&a| !reg.is_address_silent(a, now))
}

/// Red: another device transmits from the router's own address (reported at once). Red: the
/// filter expects a PGN from a source that is silent while another source sends it (the data is
/// being dropped). Amber: nobody sends a PGN the router needs. The last two are withheld until
/// the router has been listening for 60 s, while devices are still being discovered. A
/// configured source of 255 ("reject every source") is deliberate and never warned about.
pub fn compute_warnings(
    reg: &BusRegistry,
    filter: &SourceFilterConfig,
    now: Instant,
) -> Vec<BusWarning> {
    let mut warnings = Vec::new();
    if let Some(address) = conflicting_address(reg, now) {
        warnings.push(BusWarning {
            level: WarningLevel::Red,
            pgn: PGN_ADDRESS_CLAIM,
            configured_source: None,
            actual_sources: vec![address],
            message: format!("Another device is transmitting from the router's address {address}."),
        });
    }
    let listened_long_enough = reg
        .listening_since()
        .is_some_and(|since| now.saturating_duration_since(since) >= WINDOW);
    if !listened_long_enough {
        return warnings;
    }

    let mut configured: Vec<(u32, u8)> = filter
        .pgn_source_map
        .iter()
        .map(|(&pgn, &src)| (pgn, src))
        .filter(|&(pgn, _)| !filter.blocks_every_source(pgn))
        .collect();
    configured.sort_unstable();
    for (pgn, cfg) in configured {
        if is_recent(reg.last_sent(cfg, pgn), now) {
            continue;
        }
        let actual = recent_sources(reg, pgn, Some(cfg), now);
        if actual.is_empty() {
            continue;
        }
        let list = actual
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        warnings.push(BusWarning {
            level: WarningLevel::Red,
            pgn,
            configured_source: Some(cfg),
            message: format!(
                "{} ({pgn}) arrives only from src {list}, but the filter expects src {cfg} \u{2014} it is being dropped.",
                display_name(pgn)
            ),
            actual_sources: actual,
        });
    }

    for pgn in REQUIRED_PGNS {
        if recent_sources(reg, pgn, None, now).is_empty() {
            warnings.push(BusWarning {
                level: WarningLevel::Amber,
                pgn,
                configured_source: None,
                actual_sources: Vec::new(),
                message: format!("No device is sending {} ({pgn}).", display_name(pgn)),
            });
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::pgns::N2kName;
    use std::time::Duration;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// A registry listening on the bus since `t0`.
    fn registry(t0: Instant) -> BusRegistry {
        let mut r = BusRegistry::new(t0, N2kName::from_u64(999));
        r.set_listening_since(t0);
        r
    }

    fn filter(pairs: &[(u32, u8)]) -> SourceFilterConfig {
        SourceFilterConfig {
            pgn_source_map: pairs.iter().copied().collect(),
        }
    }

    fn reds(w: &[BusWarning]) -> Vec<&BusWarning> {
        w.iter().filter(|w| w.level == WarningLevel::Red).collect()
    }

    #[test]
    fn red_when_gps_moves() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        let red = reds(&w);
        assert_eq!(red.len(), 1);
        assert_eq!(red[0].pgn, 129025);
        assert_eq!(red[0].actual_sources, vec![41]);
        assert_eq!(red[0].configured_source, Some(22));
        assert!(red[0].message.contains("src 41"), "{}", red[0].message);
        assert!(
            red[0].message.contains("expects src 22"),
            "{}",
            red[0].message
        );
        assert_eq!(
            red[0].message,
            "Position, Rapid Update (129025) arrives only from src 41, but the filter expects src 22 \u{2014} it is being dropped."
        );
    }

    #[test]
    fn red_lists_every_actual_source() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, now);
        r.observe_message(7, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        let red = reds(&w);
        assert_eq!(red[0].actual_sources, vec![7, 41]);
        assert!(red[0].message.contains("src 7, 41"), "{}", red[0].message);
    }

    #[test]
    fn red_when_configured_source_went_quiet() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(22, 129025, true, t0);
        let now = t0 + secs(100);
        r.observe_message(41, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        assert_eq!(reds(&w).len(), 1);
    }

    #[test]
    fn no_red_for_reject_all_255() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 255)]), now);
        assert!(reds(&w).is_empty());
    }

    #[test]
    fn no_red_when_configured_source_alive() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let mut r = registry(t0);
        r.observe_message(22, 129025, true, now - secs(5));
        r.observe_message(41, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        assert!(reds(&w).is_empty());
    }

    #[test]
    fn no_red_when_nobody_sends_the_pgn() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let r = registry(t0);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        assert!(reds(&w).is_empty());
    }

    #[test]
    fn amber_for_missing_required_pgn() {
        let t0 = Instant::now();
        let now = t0 + secs(70);
        let mut r = registry(t0);
        r.observe_message(22, 129025, true, now);
        r.observe_message(22, 129026, true, now);
        let w = compute_warnings(&r, &filter(&[]), now);
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].level, WarningLevel::Amber);
        assert_eq!(w[0].pgn, 126992);
        assert_eq!(w[0].message, "No device is sending System Time (126992).");
        assert_eq!(w[0].configured_source, None);
        assert!(w[0].actual_sources.is_empty());
        // Optional PGN 127250 missing: no warning for it.
        assert!(w.iter().all(|w| w.pgn != 127250));
    }

    #[test]
    fn amber_when_required_pgn_went_quiet() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(22, 126992, true, t0);
        let w = compute_warnings(&r, &filter(&[]), t0 + secs(70));
        assert!(w
            .iter()
            .any(|w| w.pgn == 126992 && w.level == WarningLevel::Amber));
    }

    #[test]
    fn suppressed_during_first_minute() {
        let t0 = Instant::now();
        let now = t0 + secs(30);
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, now);
        let w = compute_warnings(&r, &filter(&[(129025, 22)]), now);
        assert!(w.is_empty());
    }

    #[test]
    fn suppression_runs_from_listening_start() {
        let t0 = Instant::now();
        let mut r = BusRegistry::new(t0, N2kName::from_u64(999));
        let f = filter(&[(129025, 22)]);
        let late = t0 + secs(300);
        r.observe_message(41, 129025, false, late);
        assert!(compute_warnings(&r, &f, late).is_empty(), "not listening yet: everything withheld");

        // The socket opened long after the registry was created.
        let listening = t0 + secs(280);
        r.set_listening_since(listening);
        assert!(compute_warnings(&r, &f, late).is_empty(), "20 s after listening started");
        let now = listening + secs(60);
        r.observe_message(41, 129025, false, now);
        assert_eq!(reds(&compute_warnings(&r, &f, now)).len(), 1);
    }

    #[test]
    fn address_conflict_is_red_and_not_suppressed() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.set_node_state(Some(100), "claimed");
        assert!(compute_warnings(&r, &filter(&[]), t0 + secs(5)).is_empty());

        r.observe_message(100, 127250, true, t0 + secs(5));
        let w = compute_warnings(&r, &filter(&[]), t0 + secs(5));
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].level, WarningLevel::Red);
        assert_eq!(w[0].pgn, 60928);
        assert_eq!(w[0].actual_sources, vec![100]);
        assert_eq!(w[0].configured_source, None);
        assert_eq!(w[0].message, "Another device is transmitting from the router's address 100.");

        // Gone after 60 s of silence.
        let w = compute_warnings(&r, &filter(&[]), t0 + secs(70));
        assert!(w.iter().all(|w| w.pgn != 60928), "{w:?}");
        // No address of our own: no conflict.
        r.observe_message(100, 127250, true, t0 + secs(70));
        r.set_node_state(None, "claiming");
        let w = compute_warnings(&r, &filter(&[]), t0 + secs(70));
        assert!(w.iter().all(|w| w.pgn != 60928), "{w:?}");
    }

    #[test]
    fn serializes_level_lowercase() {
        let w = BusWarning {
            level: WarningLevel::Red,
            pgn: 1,
            configured_source: None,
            actual_sources: vec![],
            message: String::new(),
        };
        assert_eq!(serde_json::to_value(&w).unwrap()["level"], "red");
        assert_eq!(serde_json::to_value(WarningLevel::Amber).unwrap(), "amber");
    }
}
