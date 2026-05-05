use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use nmea2k::{pgns::N2kMessage, MessageHandler, N2kFrame};
use serde::Serialize;

const MAX_AGE_MS: u64 = 4 * 3600 * 1000; // 4 hours
const EVICTION_INTERVAL: Duration = Duration::from_secs(5 * 60); // 5 minutes

pub fn new_ais_cache() -> Arc<Mutex<AisTargetCache>> {
    Arc::new(Mutex::new(AisTargetCache::new()))
}

#[derive(Clone, Serialize)]
pub struct AisTargetData {
    pub mmsi: u32,
    pub name: Option<String>,
    pub callsign: Option<String>,
    pub ship_type: Option<u8>,
    pub ais_class: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub sog: Option<f64>,     // m/s
    pub cog: Option<f64>,     // radians
    pub heading: Option<f64>, // radians
    pub nav_status: Option<String>,
    pub last_seen: u64,
}

impl AisTargetData {
    fn new(mmsi: u32, now_ms: u64) -> Self {
        Self {
            mmsi,
            name: None,
            callsign: None,
            ship_type: None,
            ais_class: None,
            latitude: None,
            longitude: None,
            sog: None,
            cog: None,
            heading: None,
            nav_status: None,
            last_seen: now_ms,
        }
    }
}

pub struct AisTargetCache {
    targets: HashMap<u32, AisTargetData>,
    last_eviction: Option<Instant>,
}

impl AisTargetCache {
    fn new() -> Self {
        Self {
            targets: HashMap::new(),
            last_eviction: None,
        }
    }

    pub fn get_recent(&self, now_ms: u64) -> Vec<AisTargetData> {
        self.targets
            .values()
            .filter(|t| now_ms.saturating_sub(t.last_seen) <= MAX_AGE_MS)
            .cloned()
            .collect()
    }

    fn entry(&mut self, mmsi: u32, now_ms: u64) -> &mut AisTargetData {
        self.targets
            .entry(mmsi)
            .or_insert_with(|| AisTargetData::new(mmsi, now_ms))
    }

    fn update_position(&mut self, mmsi: u32, lat: f64, lon: f64, now_ms: u64) {
        let e = self.entry(mmsi, now_ms);
        e.last_seen = now_ms;
        e.latitude = Some(lat);
        e.longitude = Some(lon);
    }

    fn update_motion(&mut self, mmsi: u32, sog: f64, cog: f64, heading: f64, now_ms: u64) {
        let e = self.entry(mmsi, now_ms);
        e.last_seen = now_ms;
        e.sog = Some(sog);
        e.cog = Some(cog);
        e.heading = Some(heading);
    }

    fn evict_old(&mut self, now_ms: u64) {
        self.targets
            .retain(|_, t| now_ms.saturating_sub(t.last_seen) <= MAX_AGE_MS);
    }
}

impl MessageHandler for AisTargetCache {
    fn handle_message(&mut self, frame: &N2kFrame, now: Instant) {
        let now_ms = crate::utilities::instant_to_unix_millis(now) as u64;

        match &frame.message {
            N2kMessage::AisClassAPositionReport(ais) => {
                self.update_position(
                    ais.mmsi,
                    ais.get_latitude_degrees(),
                    ais.get_longitude_degrees(),
                    now_ms,
                );
                self.update_motion(
                    ais.mmsi,
                    ais.get_sog_ms(),
                    ais.get_cog_radians(),
                    ais.get_heading_radians(),
                    now_ms,
                );
                let e = self.entry(ais.mmsi, now_ms);
                e.ais_class = Some("A".to_string());
                e.nav_status = Some(ais.nav_status.to_string());
            }

            N2kMessage::AisClassBPositionReport(ais) => {
                self.update_position(
                    ais.mmsi,
                    ais.get_latitude_degrees(),
                    ais.get_longitude_degrees(),
                    now_ms,
                );
                self.update_motion(
                    ais.mmsi,
                    ais.get_sog_ms(),
                    ais.get_cog_radians(),
                    ais.get_heading_radians(),
                    now_ms,
                );
                let e = self.entry(ais.mmsi, now_ms);
                e.ais_class = Some("B".to_string());
            }

            N2kMessage::AisClassBExtPositionReport(ais) => {
                self.update_position(
                    ais.mmsi,
                    ais.get_latitude_degrees(),
                    ais.get_longitude_degrees(),
                    now_ms,
                );
                self.update_motion(
                    ais.mmsi,
                    ais.get_sog_ms(),
                    ais.get_cog_radians(),
                    ais.get_heading_radians(),
                    now_ms,
                );
                let e = self.entry(ais.mmsi, now_ms);
                e.ais_class = Some("B".to_string());
            }

            N2kMessage::AisAtonReport(ais) => {
                self.update_position(
                    ais.mmsi,
                    ais.get_latitude_degrees(),
                    ais.get_longitude_degrees(),
                    now_ms,
                );
                let e = self.entry(ais.mmsi, now_ms);
                e.ais_class = Some("AtoN".to_string());
                e.name = Some(ais.name.trim().to_string());
            }

            N2kMessage::AisUtcDateReport(ais) => {
                self.update_position(
                    ais.mmsi,
                    ais.get_latitude_degrees(),
                    ais.get_longitude_degrees(),
                    now_ms,
                );
            }

            N2kMessage::AisClassAStaticData(ais) => {
                let e = self.entry(ais.mmsi, now_ms);
                e.last_seen = now_ms;
                e.name = Some(ais.name.trim().to_string());
                e.callsign = Some(ais.callsign.trim().to_string());
                e.ship_type = Some(ais.type_of_ship);
                e.ais_class = Some("A".to_string());
            }

            N2kMessage::AisClassBStaticDataPartA(ais) => {
                let e = self.entry(ais.mmsi, now_ms);
                e.last_seen = now_ms;
                e.name = Some(ais.name.trim().to_string());
                e.ais_class = Some("B".to_string());
            }

            N2kMessage::AisClassBStaticDataPartB(ais) => {
                let e = self.entry(ais.mmsi, now_ms);
                e.last_seen = now_ms;
                e.callsign = Some(ais.callsign.trim().to_string());
                e.ship_type = Some(ais.type_of_ship);
                e.ais_class = Some("B".to_string());
            }

            _ => return,
        }

        let due = self
            .last_eviction
            .is_none_or(|t| now.duration_since(t) >= EVICTION_INTERVAL);
        if due {
            self.evict_old(now_ms);
            self.last_eviction = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::{
        pgns::{
            AisAtonReport, AisAtonType, AisClassAPositionReport, AisClassAStaticData,
            AisClassBExtPositionReport, AisClassBPositionReport, AisClassBStaticDataPartA,
            AisClassBStaticDataPartB, AisNavStatus, AisUtcDateReport,
        },
        Identifier,
    };
    use nmea2k::pgns::nmea2000_date_time::N2kDateTime;
    use socketcan::ExtendedId;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_identifier() -> Identifier {
        Identifier::from_can_id(ExtendedId::new(0).expect("valid CAN id"))
    }

    fn make_frame(message: N2kMessage) -> N2kFrame {
        N2kFrame {
            identifier: make_identifier(),
            message,
            is_fast_packet: false,
            data: vec![],
        }
    }

    fn class_a_position(
        mmsi: u32,
        lat: f64,
        lon: f64,
        sog_ms: f64,
        cog_rad: f64,
        heading_rad: f64,
    ) -> N2kFrame {
        make_frame(N2kMessage::AisClassAPositionReport(
            AisClassAPositionReport {
                pgn: 129038,
                message_id: 1,
                repeat_indicator: 0,
                mmsi,
                latitude_raw: (lat * 1e7) as i32,
                longitude_raw: (lon * 1e7) as i32,
                position_accuracy: false,
                raim: false,
                time_stamp: 0,
                cog_raw: (cog_rad * 10000.0) as u16,
                sog_raw: (sog_ms * 100.0) as u16,
                communication_state: 0,
                ais_transceiver_info: 0,
                heading_raw: (heading_rad * 10000.0) as u16,
                rate_of_turn_raw: 0,
                nav_status: AisNavStatus::UnderWayEngine,
                special_maneuver_indicator: 0,
            },
        ))
    }

    fn class_a_static(mmsi: u32, name: &str, callsign: &str, ship_type: u8) -> N2kFrame {
        make_frame(N2kMessage::AisClassAStaticData(AisClassAStaticData {
            pgn: 129794,
            message_id: 5,
            repeat_indicator: 0,
            mmsi,
            imo_number: 0,
            callsign: callsign.to_string(),
            name: name.to_string(),
            type_of_ship: ship_type,
            length_raw: 0,
            beam_raw: 0,
            position_ref_starboard: 0,
            position_ref_bow: 0,
            eta_date: 0,
            eta_time: 0,
            draft_raw: 0,
            destination: String::new(),
            ais_version: 0,
            gnss_type: 0,
            dte: false,
            eta_date_time: None,
            class: "A".to_string(),
        }))
    }

    #[test]
    fn test_class_a_position_stores_fields() {
        let mut cache = AisTargetCache::new();
        let frame = class_a_position(123456789, 45.0, 9.0, 3.0, 1.0, 2.0);
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);

        let t = &targets[0];
        assert_eq!(t.mmsi, 123456789);
        assert!((t.latitude.unwrap() - 45.0).abs() < 1e-5);
        assert!((t.longitude.unwrap() - 9.0).abs() < 1e-5);
        assert!((t.sog.unwrap() - 3.0).abs() < 0.01);
        assert!((t.cog.unwrap() - 1.0).abs() < 0.001);
        assert!((t.heading.unwrap() - 2.0).abs() < 0.001);
        assert_eq!(t.ais_class.as_deref(), Some("A"));
        assert_eq!(t.nav_status.as_deref(), Some("Under way engine"));
    }

    #[test]
    fn test_class_a_static_stores_name_and_callsign() {
        let mut cache = AisTargetCache::new();
        let frame = class_a_static(987654321, "VESSEL NAME", "CALL1", 70);
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);

        let t = &targets[0];
        assert_eq!(t.mmsi, 987654321);
        assert_eq!(t.name.as_deref(), Some("VESSEL NAME"));
        assert_eq!(t.callsign.as_deref(), Some("CALL1"));
        assert_eq!(t.ship_type, Some(70));
        assert_eq!(t.ais_class.as_deref(), Some("A"));
    }

    #[test]
    fn test_position_then_static_merges_fields() {
        let mut cache = AisTargetCache::new();
        let mmsi = 111222333;
        cache.handle_message(
            &class_a_position(mmsi, 44.0, 8.0, 5.0, 0.5, 0.5),
            Instant::now(),
        );
        cache.handle_message(
            &class_a_static(mmsi, "MERGEDVESSEL", "MRG", 60),
            Instant::now(),
        );

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);

        let t = &targets[0];
        assert!((t.latitude.unwrap() - 44.0).abs() < 1e-5);
        assert_eq!(t.name.as_deref(), Some("MERGEDVESSEL"));
        assert_eq!(t.callsign.as_deref(), Some("MRG"));
    }

    #[test]
    fn test_class_b_static_stores_name_and_callsign() {
        let mut cache = AisTargetCache::new();
        let mmsi = 555666777;

        let part_a = make_frame(N2kMessage::AisClassBStaticDataPartA(
            AisClassBStaticDataPartA {
                pgn: 129809,
                message_id: 24,
                repeat_indicator: 0,
                mmsi,
                name: "CLASSB VESSEL".to_string(),
                sequence_id: 0,
                class: "B".to_string(),
            },
        ));
        let part_b = make_frame(N2kMessage::AisClassBStaticDataPartB(
            AisClassBStaticDataPartB {
                pgn: 129810,
                message_id: 24,
                repeat_indicator: 0,
                mmsi,
                type_of_ship: 37,
                vendor_id: String::new(),
                callsign: "CB1".to_string(),
                length_raw: 0,
                beam_raw: 0,
                position_ref_starboard: 0,
                position_ref_bow: 0,
                mothership_mmsi: 0,
                sequence_id: 1,
                class: "B".to_string(),
            },
        ));

        cache.handle_message(&part_a, Instant::now());
        cache.handle_message(&part_b, Instant::now());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);

        let t = &targets[0];
        assert_eq!(t.name.as_deref(), Some("CLASSB VESSEL"));
        assert_eq!(t.callsign.as_deref(), Some("CB1"));
        assert_eq!(t.ship_type, Some(37));
        assert_eq!(t.ais_class.as_deref(), Some("B"));
    }

    #[test]
    fn test_get_recent_excludes_old_targets() {
        let mut cache = AisTargetCache::new();
        cache.handle_message(
            &class_a_position(100, 45.0, 9.0, 0.0, 0.0, 0.0),
            Instant::now(),
        );

        // Simulate looking up the cache 4h+1ms in the future
        let future_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + MAX_AGE_MS
            + 1;
        let targets = cache.get_recent(future_ms);
        assert!(targets.is_empty());
    }

    #[test]
    fn test_multiple_targets_tracked_independently() {
        let mut cache = AisTargetCache::new();
        cache.handle_message(
            &class_a_position(111, 45.0, 9.0, 1.0, 0.1, 0.1),
            Instant::now(),
        );
        cache.handle_message(
            &class_a_position(222, 46.0, 10.0, 2.0, 0.2, 0.2),
            Instant::now(),
        );

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 2);

        let mmsis: Vec<u32> = targets.iter().map(|t| t.mmsi).collect();
        assert!(mmsis.contains(&111));
        assert!(mmsis.contains(&222));
    }

    #[test]
    fn test_eviction_runs_on_first_message() {
        // With no prior eviction the cache must run evict_old immediately, so a
        // stale entry planted directly into targets is removed on the next message.
        let mut cache = AisTargetCache::new();
        let stale_ms = 0u64; // epoch — guaranteed to be > MAX_AGE_MS ago
        cache.targets.insert(999, AisTargetData::new(999, stale_ms));

        let now = Instant::now();
        cache.handle_message(&class_a_position(1, 45.0, 9.0, 0.0, 0.0, 0.0), now);

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mmsis: Vec<u32> = cache.get_recent(now_ms).iter().map(|t| t.mmsi).collect();
        assert!(
            !mmsis.contains(&999),
            "stale entry should have been evicted"
        );
        assert!(mmsis.contains(&1), "new entry should be present");
    }

    #[test]
    fn test_eviction_suppressed_within_interval() {
        // With a recent eviction timestamp the cache must NOT evict on the next message.
        let mut cache = AisTargetCache::new();
        cache.last_eviction = Some(Instant::now());

        let stale_ms = 0u64;
        cache.targets.insert(999, AisTargetData::new(999, stale_ms));

        cache.handle_message(
            &class_a_position(1, 45.0, 9.0, 0.0, 0.0, 0.0),
            Instant::now(),
        );

        // Stale entry still present because eviction was suppressed
        assert!(
            cache.targets.contains_key(&999),
            "eviction should be suppressed within interval"
        );
    }

    #[test]
    fn test_eviction_runs_after_interval_elapsed() {
        let mut cache = AisTargetCache::new();
        // Backdate last_eviction beyond the interval
        cache.last_eviction = Some(Instant::now() - EVICTION_INTERVAL - Duration::from_secs(1));

        let stale_ms = 0u64;
        cache.targets.insert(999, AisTargetData::new(999, stale_ms));

        cache.handle_message(
            &class_a_position(1, 45.0, 9.0, 0.0, 0.0, 0.0),
            Instant::now(),
        );

        assert!(
            !cache.targets.contains_key(&999),
            "stale entry should have been evicted after interval"
        );
    }

    #[test]
    fn test_class_b_position_stores_fields() {
        let mut cache = AisTargetCache::new();
        let frame = make_frame(N2kMessage::AisClassBPositionReport(AisClassBPositionReport {
            pgn: 129039,
            message_id: 18,
            repeat_indicator: 0,
            mmsi: 338000001,
            latitude_raw: (43_000_000i32),
            longitude_raw: (10_000_000i32),
            position_accuracy: false,
            raim: false,
            time_stamp: 0,
            cog_raw: (1_000u16),
            sog_raw: (250u16),
            communication_state: 0,
            ais_transceiver_info: 0,
            heading_raw: (2_000u16),
            regional_application: 0,
            regional_application_b: 0,
            unit_type: 1,
            integrated_display: false,
            dsc: false,
            band: false,
            can_handle_msg22: false,
            assigned: false,
        }));
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.mmsi, 338000001);
        assert!((t.latitude.unwrap() - 4.3).abs() < 1e-5);
        assert!((t.longitude.unwrap() - 1.0).abs() < 1e-5);
        assert!((t.sog.unwrap() - 2.5).abs() < 0.01);
        assert!((t.cog.unwrap() - 0.1).abs() < 0.001);
        assert!((t.heading.unwrap() - 0.2).abs() < 0.001);
        assert_eq!(t.ais_class.as_deref(), Some("B"));
    }

    #[test]
    fn test_class_b_ext_position_stores_fields() {
        let mut cache = AisTargetCache::new();
        let frame = make_frame(N2kMessage::AisClassBExtPositionReport(
            AisClassBExtPositionReport {
                pgn: 129040,
                message_id: 19,
                repeat_indicator: 0,
                mmsi: 338000002,
                latitude_raw: 440_000_000i32,
                longitude_raw: 80_000_000i32,
                position_accuracy: false,
                raim: false,
                time_stamp: 0,
                cog_raw: 5_000u16,
                sog_raw: 300u16,
                regional_application: 0,
                regional_application_b: 0,
                type_of_ship: 30,
                heading_raw: 6_000u16,
                gnss_type: 0,
                length_raw: 0,
                beam_raw: 0,
                position_ref_starboard_raw: 0,
                position_ref_bow_raw: 0,
            },
        ));
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.mmsi, 338000002);
        assert!((t.latitude.unwrap() - 44.0).abs() < 1e-4);
        assert!((t.longitude.unwrap() - 8.0).abs() < 1e-4);
        assert!((t.sog.unwrap() - 3.0).abs() < 0.01);
        assert!((t.cog.unwrap() - 0.5).abs() < 0.001);
        assert!((t.heading.unwrap() - 0.6).abs() < 0.001);
        assert_eq!(t.ais_class.as_deref(), Some("B"));
    }

    #[test]
    fn test_aton_report_stores_position_and_name() {
        let mut cache = AisTargetCache::new();
        let frame = make_frame(N2kMessage::AisAtonReport(AisAtonReport {
            pgn: 129041,
            message_id: 21,
            repeat_indicator: 0,
            mmsi: 992000001,
            latitude_raw: 430_000_000i32,
            longitude_raw: 90_000_000i32,
            position_accuracy: false,
            raim: false,
            time_stamp: 0,
            length_raw: 0,
            beam_raw: 0,
            position_ref_starboard: 0,
            position_ref_true_north: 0,
            aton_type: AisAtonType::Light,
            off_position: false,
            virtual_aton: false,
            assigned_mode: false,
            name: "EAST BUOY 1".to_string(),
        }));
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.mmsi, 992000001);
        assert!((t.latitude.unwrap() - 43.0).abs() < 1e-4);
        assert!((t.longitude.unwrap() - 9.0).abs() < 1e-4);
        assert_eq!(t.name.as_deref(), Some("EAST BUOY 1"));
        assert_eq!(t.ais_class.as_deref(), Some("AtoN"));
        assert!(t.sog.is_none());
        assert!(t.cog.is_none());
    }

    #[test]
    fn test_utc_date_report_stores_position() {
        let mut cache = AisTargetCache::new();
        let frame = make_frame(N2kMessage::AisUtcDateReport(AisUtcDateReport {
            pgn: 129793,
            message_id: 11,
            repeat_indicator: 0,
            mmsi: 338000003,
            latitude_raw: 450_000_000i32,
            longitude_raw: 120_000_000i32,
            position_accuracy: false,
            raim: false,
            position_time_raw: 0,
            position_date: 19000,
            gnss_type: 0,
            date_time: N2kDateTime::new(19000, 0.0).unwrap(),
        }));
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let targets = cache.get_recent(now_ms);
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.mmsi, 338000003);
        assert!((t.latitude.unwrap() - 45.0).abs() < 1e-4);
        assert!((t.longitude.unwrap() - 12.0).abs() < 1e-4);
        assert!(t.sog.is_none());
        assert!(t.name.is_none());
    }

    #[test]
    fn test_non_ais_frame_is_ignored() {
        let mut cache = AisTargetCache::new();
        let frame = make_frame(N2kMessage::PositionRapidUpdate(
            nmea2k::pgns::PositionRapidUpdate {
                pgn: 129025,
                latitude: 45.0,
                longitude: 9.0,
            },
        ));
        cache.handle_message(&frame, Instant::now());

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(cache.get_recent(now_ms).is_empty());
    }
}
