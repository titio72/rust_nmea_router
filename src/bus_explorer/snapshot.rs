//! Serializable view of the bus registry served by the bus-explorer API.
//!
//! Business logic here never reads the clock: `now` is passed in.

use super::names::{device_class_name, device_function_name, manufacturer_name, pgn_name};
use super::registry::{BusRegistry, DeviceEntry, DeviceStatus, PgnStats, GONE_AFTER};
use super::warnings::{compute_warnings, conflicting_address, BusWarning};
use crate::config::SourceFilterConfig;
use nmea2k::pgns::{N2kName, ProductInformation};
use serde::Serialize;
use std::time::Instant;

/// Initial identity request plus three retries.
const IDENTITY_ATTEMPTS: u8 = 4;

/// Node state label meaning the router does not transmit on the bus.
const DISABLED_LABEL: &str = "disabled";

/// Node states in which the router never sends identity requests.
const SILENT_NODE_STATES: [&str; 3] = [DISABLED_LABEL, "cannot_claim", "name_conflict"];

/// Status of a device heard transmitting from the router's own address.
const ADDRESS_CONFLICT_STATUS: &str = "address_conflict";

#[derive(Debug, Clone, Serialize)]
pub struct BusSnapshot {
    pub can_enabled: bool,
    pub uptime_s: f64,
    pub node: NodeView,
    pub warnings: Vec<BusWarning>,
    pub devices: Vec<DeviceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeView {
    pub transmit_enabled: bool,
    pub own_address: Option<u8>,
    pub state: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceView {
    pub source: u8,
    pub is_self: bool,
    /// "in_use" | "not_used" | "dropped" | "gone" | "self" | "address_conflict"
    pub status: &'static str,
    pub last_heard_s: Option<f64>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub device_class: Option<String>,
    pub device_function: Option<String>,
    pub software_version: Option<String>,
    /// "known" | "asking" | "no_reply" | "not_asked"
    pub identity: &'static str,
    pub moved_to: Option<u8>,
    pub name: Option<NameView>,
    pub product: Option<ProductView>,
    pub pgns: Vec<PgnView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PgnView {
    pub pgn: u32,
    pub name: Option<&'static str>,
    pub interval_s: Option<f64>,
    pub last_seen_s: f64,
    pub accepted: bool,
    /// Rejected because the filter blocks this PGN from every source (configured source 255).
    pub blocked: bool,
    pub used: bool,
    /// Other sources whose same PGN was accepted and used within 60 s, listed only when this
    /// source's PGN is itself accepted and used: the router is mixing sources.
    pub also_accepted_from: Vec<u8>,
}

/// Every field of the NAME, with the raw code plus its name where one is known.
#[derive(Debug, Clone, Serialize)]
pub struct NameView {
    pub unique_number: u32,
    pub manufacturer_code: u16,
    pub manufacturer: String,
    pub device_instance_lower: u8,
    pub device_instance_upper: u8,
    pub device_function: u8,
    pub device_function_name: String,
    pub device_class: u8,
    pub device_class_name: String,
    pub system_instance: u8,
    pub industry_group: u8,
    pub arbitrary_address_capable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductView {
    pub nmea2000_version: Option<u16>,
    pub product_code: Option<u16>,
    pub model_id: String,
    pub software_version: String,
    pub model_version: String,
    pub serial_code: String,
    pub certification_level: Option<u8>,
    pub load_equivalency: Option<u8>,
}

fn manufacturer_label(code: u16) -> String {
    manufacturer_name(code).map_or_else(|| format!("code {code}"), str::to_string)
}

fn class_label(class: u8) -> String {
    device_class_name(class).map_or_else(|| format!("class {class}"), str::to_string)
}

fn function_label(class: u8, function: u8) -> String {
    device_function_name(class, function)
        .map_or_else(|| format!("function {function}"), str::to_string)
}

impl NameView {
    fn new(n: &N2kName) -> Self {
        Self {
            unique_number: n.unique_number,
            manufacturer_code: n.manufacturer_code,
            manufacturer: manufacturer_label(n.manufacturer_code),
            device_instance_lower: n.device_instance_lower,
            device_instance_upper: n.device_instance_upper,
            device_function: n.device_function,
            device_function_name: function_label(n.device_class, n.device_function),
            device_class: n.device_class,
            device_class_name: class_label(n.device_class),
            system_instance: n.system_instance,
            industry_group: n.industry_group,
            arbitrary_address_capable: n.arbitrary_address_capable,
        }
    }
}

impl ProductView {
    fn new(p: &ProductInformation) -> Self {
        Self {
            nmea2000_version: p.nmea2000_version,
            product_code: p.product_code,
            model_id: p.model_id.clone(),
            software_version: p.software_version.clone(),
            model_version: p.model_version.clone(),
            serial_code: p.serial_code.clone(),
            certification_level: p.certification_level,
            load_equivalency: p.load_equivalency,
        }
    }
}

fn status_label(status: DeviceStatus) -> &'static str {
    match status {
        DeviceStatus::InUse => "in_use",
        DeviceStatus::NotUsed => "not_used",
        DeviceStatus::Dropped => "dropped",
        DeviceStatus::Gone => "gone",
    }
}

fn is_recent(t: Option<Instant>, now: Instant) -> bool {
    t.is_some_and(|t| now.saturating_duration_since(t) <= GONE_AFTER)
}

/// Accepted by the filter and decoded for use within the last 60 s.
fn is_in_use(stats: &PgnStats, now: Instant) -> bool {
    stats.last_accepted && is_recent(stats.last_used, now)
}

/// `asking` is whether the router's node sends identity requests in its current state.
fn identity_label(device: &DeviceEntry, asking: bool) -> &'static str {
    if device.name.is_some() && device.product.is_some() {
        "known"
    } else if !asking {
        "not_asked"
    } else if device.identity_requests >= IDENTITY_ATTEMPTS {
        "no_reply"
    } else {
        "asking"
    }
}

fn pgn_views(
    reg: &BusRegistry,
    filter: &SourceFilterConfig,
    source: u8,
    device: &DeviceEntry,
    now: Instant,
) -> Vec<PgnView> {
    device
        .pgns
        .iter()
        .map(|(&pgn, stats)| {
            let also_accepted_from = if is_in_use(stats, now) {
                reg.devices()
                    .iter()
                    .filter(|&(&other, _)| other != source)
                    .filter(|(_, d)| d.pgns.get(&pgn).is_some_and(|s| is_in_use(s, now)))
                    .map(|(&other, _)| other)
                    .collect()
            } else {
                Vec::new()
            };
            PgnView {
                pgn,
                name: pgn_name(pgn),
                interval_s: stats.interval_ewma.map(|d| d.as_secs_f64()),
                last_seen_s: now.saturating_duration_since(stats.last_seen).as_secs_f64(),
                accepted: stats.last_accepted,
                blocked: !stats.last_accepted && filter.blocks_every_source(pgn),
                used: is_recent(stats.last_used, now),
                also_accepted_from,
            }
        })
        .collect()
}

/// The row of one registry entry. `status` overrides the derived status label when set.
fn device_view(
    reg: &BusRegistry,
    filter: &SourceFilterConfig,
    source: u8,
    device: &DeviceEntry,
    status: Option<&'static str>,
    now: Instant,
) -> DeviceView {
    let derived = || {
        let status = reg
            .status(source, now, |pgn| filter.blocks_every_source(pgn))
            .unwrap_or(DeviceStatus::NotUsed);
        status_label(status)
    };
    // The router never sends requests to its own address, so a conflicting device is not asked.
    let asking = status != Some(ADDRESS_CONFLICT_STATUS) && !SILENT_NODE_STATES.contains(&reg.node_state().1);
    DeviceView {
        source,
        is_self: false,
        status: status.unwrap_or_else(derived),
        last_heard_s: Some(
            now.saturating_duration_since(device.last_heard)
                .as_secs_f64(),
        ),
        manufacturer: device
            .name
            .as_ref()
            .map(|n| manufacturer_label(n.manufacturer_code)),
        model: device.product.as_ref().map(|p| p.model_id.clone()),
        device_class: device.name.as_ref().map(|n| class_label(n.device_class)),
        device_function: device
            .name
            .as_ref()
            .map(|n| function_label(n.device_class, n.device_function)),
        software_version: device.product.as_ref().map(|p| p.software_version.clone()),
        identity: identity_label(device, asking),
        moved_to: device.moved_to,
        name: device.name.as_ref().map(NameView::new),
        product: device.product.as_ref().map(ProductView::new),
        pgns: pgn_views(reg, filter, source, device, now),
    }
}

/// The router's own row: it knows its NAME and never has to ask for it.
fn self_view(source: u8, name: &N2kName) -> DeviceView {
    DeviceView {
        source,
        is_self: true,
        status: "self",
        last_heard_s: None,
        manufacturer: Some(manufacturer_label(name.manufacturer_code)),
        model: Some("this router".to_string()),
        device_class: Some(class_label(name.device_class)),
        device_function: Some(function_label(name.device_class, name.device_function)),
        software_version: None,
        identity: "known",
        moved_to: None,
        name: Some(NameView::new(name)),
        product: None,
        pgns: Vec::new(),
    }
}

pub fn build_snapshot(
    reg: &BusRegistry,
    filter: &SourceFilterConfig,
    can_enabled: bool,
    now: Instant,
) -> BusSnapshot {
    let (own_address, state) = reg.node_state();
    let conflict = conflicting_address(reg, now);
    let mut devices: Vec<DeviceView> = reg
        .devices()
        .iter()
        .filter_map(|(&source, device)| {
            if Some(source) != own_address {
                Some(device_view(reg, filter, source, device, None, now))
            } else if Some(source) == conflict {
                // Another device transmits from the router's address: shown next to the own row.
                Some(device_view(reg, filter, source, device, Some(ADDRESS_CONFLICT_STATUS), now))
            } else {
                // Gone: heard there before the router claimed the address.
                None
            }
        })
        .collect();
    if let Some(address) = own_address {
        devices.push(self_view(address, &reg.own_name()));
    }
    // By address; at the router's own address its row comes before a conflicting device.
    devices.sort_by_key(|d| (d.source, !d.is_self));
    BusSnapshot {
        can_enabled,
        uptime_s: now.saturating_duration_since(reg.started()).as_secs_f64(),
        node: NodeView {
            transmit_enabled: state != DISABLED_LABEL,
            own_address,
            state,
        },
        warnings: compute_warnings(reg, filter, now),
        devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::pgns::{N2kName, ProductInformation};
    use std::time::Duration;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn registry(t0: Instant) -> BusRegistry {
        BusRegistry::new(t0, N2kName::from_u64(999))
    }

    fn no_filter() -> SourceFilterConfig {
        SourceFilterConfig::default()
    }

    fn product() -> ProductInformation {
        ProductInformation {
            nmea2000_version: Some(2100),
            product_code: Some(1),
            model_id: "EV-1".to_string(),
            software_version: "1.2".to_string(),
            model_version: "A".to_string(),
            serial_code: "123".to_string(),
            certification_level: Some(0),
            load_equivalency: Some(1),
        }
    }

    fn name(mfr: u16) -> N2kName {
        N2kName {
            unique_number: 5,
            manufacturer_code: mfr,
            device_instance_lower: 0,
            device_instance_upper: 0,
            device_function: 140,
            device_class: 60,
            system_instance: 0,
            industry_group: 4,
            arbitrary_address_capable: true,
        }
    }

    fn used(r: &mut BusRegistry, src: u8, pgn: u32, now: Instant) {
        r.observe_message(src, pgn, true, now);
        r.observe_decoded(src, pgn, true, now);
    }

    fn json(r: &BusRegistry, can: bool, now: Instant) -> serde_json::Value {
        serde_json::to_value(build_snapshot(r, &no_filter(), can, now)).unwrap()
    }

    #[test]
    fn mixing_sources_noted() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        used(&mut r, 0, 127250, t0);
        used(&mut r, 12, 127250, t0);
        used(&mut r, 12, 130306, t0);
        let v = json(&r, true, t0 + secs(1));
        let devices = v["devices"].as_array().unwrap();
        let pgn = |src: usize, pgn: u64| {
            devices[src]["pgns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["pgn"] == pgn)
                .unwrap()
                .clone()
        };
        assert_eq!(
            pgn(0, 127250)["also_accepted_from"],
            serde_json::json!([12])
        );
        assert_eq!(pgn(1, 127250)["also_accepted_from"], serde_json::json!([0]));
        assert_eq!(pgn(1, 130306)["also_accepted_from"], serde_json::json!([]));
    }

    #[test]
    fn rejected_or_stale_source_is_not_mixing() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        used(&mut r, 0, 127250, t0 + secs(100));
        r.observe_message(12, 127250, false, t0 + secs(100));
        used(&mut r, 13, 127250, t0);
        let v = json(&r, true, t0 + secs(101));
        let devices = v["devices"].as_array().unwrap();
        for d in devices {
            for p in d["pgns"].as_array().unwrap() {
                assert_eq!(p["also_accepted_from"], serde_json::json!([]), "{d}");
            }
        }
    }

    #[test]
    fn identity_states() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(1, 130306, true, t0);
        r.observe_message(2, 130306, true, t0);
        r.observe_message(3, 130306, true, t0);
        r.device_mut(2).unwrap().identity_requests = 4;
        r.observe_address_claim(3, name(1851), t0);
        r.observe_product_info(3, product());
        let v = json(&r, true, t0);
        let d = v["devices"].as_array().unwrap();
        assert_eq!(d[0]["identity"], "asking");
        assert_eq!(d[1]["identity"], "no_reply");
        assert_eq!(d[2]["identity"], "known");
    }

    #[test]
    fn identity_partial_and_no_reply_threshold() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_address_claim(1, name(1851), t0);
        r.device_mut(1).unwrap().identity_requests = 3;
        assert_eq!(json(&r, true, t0)["devices"][0]["identity"], "asking");
        r.device_mut(1).unwrap().identity_requests = 4;
        assert_eq!(json(&r, true, t0)["devices"][0]["identity"], "no_reply");
    }

    #[test]
    fn serializes() {
        let t0 = Instant::now();
        let r = registry(t0);
        let v = json(&r, true, t0 + secs(5));
        let obj = v.as_object().unwrap();
        let mut keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["can_enabled", "devices", "node", "uptime_s", "warnings"]
        );
        assert_eq!(v["can_enabled"], true);
        assert_eq!(v["uptime_s"], 5.0);
        assert_eq!(v["node"]["state"], "starting");
        assert_eq!(v["node"]["transmit_enabled"], true);
        assert!(v["node"]["own_address"].is_null());
    }

    #[test]
    fn transmit_disabled_label() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.set_node_state(None, "disabled");
        let v = json(&r, true, t0);
        assert_eq!(v["node"]["transmit_enabled"], false);
        assert_eq!(v["node"]["state"], "disabled");
    }

    #[test]
    fn device_row_fields_and_sorting() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        used(&mut r, 41, 130306, t0);
        r.observe_address_claim(22, name(1851), t0);
        r.observe_product_info(22, product());
        r.observe_message(22, 129025, false, t0 + secs(2));
        let v = json(&r, true, t0 + secs(4));
        let d = v["devices"].as_array().unwrap();
        assert_eq!(d[0]["source"], 22);
        assert_eq!(d[1]["source"], 41);
        assert_eq!(d[0]["is_self"], false);
        assert_eq!(d[0]["status"], "dropped");
        assert_eq!(d[1]["status"], "in_use");
        assert_eq!(d[0]["manufacturer"], "Raymarine");
        assert_eq!(d[0]["model"], "EV-1");
        assert_eq!(d[0]["software_version"], "1.2");
        assert_eq!(d[0]["device_class"], "Navigation");
        assert_eq!(d[0]["device_function"], "Ownship Attitude");
        assert_eq!(d[0]["last_heard_s"], 2.0);
        assert!(d[1]["manufacturer"].is_null());
        assert_eq!(d[0]["name"]["manufacturer_code"], 1851);
        assert_eq!(d[0]["product"]["model_id"], "EV-1");
        let p = &d[0]["pgns"][0];
        assert_eq!(p["pgn"], 129025);
        assert_eq!(p["name"], "Position, Rapid Update");
        assert_eq!(p["accepted"], false);
        assert_eq!(p["used"], false);
        assert_eq!(p["last_seen_s"], 2.0);
    }

    #[test]
    fn unknown_manufacturer_renders_code() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_address_claim(9, name(9999), t0);
        let v = json(&r, true, t0);
        assert_eq!(v["devices"][0]["manufacturer"], "code 9999");
        assert_eq!(v["devices"][0]["name"]["manufacturer"], "code 9999");
    }

    #[test]
    fn own_row_present_when_address_claimed() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        used(&mut r, 10, 130306, t0);
        r.set_node_state(Some(7), "active");
        let v = json(&r, true, t0 + secs(1));
        let d = v["devices"].as_array().unwrap();
        assert_eq!(d[0]["source"], 7);
        assert_eq!(d[0]["is_self"], true);
        assert_eq!(d[0]["status"], "self");
        assert_eq!(d[0]["model"], "this router");
        assert_eq!(d[0]["identity"], "known");
        assert_eq!(d[0]["name"]["unique_number"], 999);
        assert_eq!(d[1]["source"], 10);
        assert_eq!(v["node"]["own_address"], 7);
    }

    #[test]
    fn address_conflict_row_next_to_own_row() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.set_listening_since(t0);
        r.set_node_state(Some(7), "claimed");
        // Heard at 7 before the router claimed it, and gone since: hidden behind the own row.
        r.observe_message(7, 130306, true, t0);
        let v = json(&r, true, t0 + secs(70));
        let d = v["devices"].as_array().unwrap();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0]["is_self"], true);

        // Another device transmits from 7 now.
        r.observe_address_claim(7, name(1851), t0 + secs(71));
        used(&mut r, 7, 127250, t0 + secs(71));
        let v = json(&r, true, t0 + secs(72));
        let d = v["devices"].as_array().unwrap();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0]["source"].clone(), d[0]["status"].clone()), (7.into(), "self".into()));
        assert_eq!(d[1]["source"], 7);
        assert_eq!(d[1]["is_self"], false);
        assert_eq!(d[1]["status"], "address_conflict");
        assert_eq!(d[1]["manufacturer"], "Raymarine");
        assert_eq!(d[1]["identity"], "not_asked", "the router never asks its own address");
        assert!(d[1]["pgns"].as_array().unwrap().iter().any(|p| p["pgn"] == 127250));
        let red: Vec<_> = v["warnings"].as_array().unwrap().iter().filter(|w| w["level"] == "red").collect();
        assert_eq!(red.len(), 1);
        assert_eq!(red[0]["message"], "Another device is transmitting from the router's address 7.");
    }

    #[test]
    fn blocked_pgn_is_not_dropped() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, t0);
        r.observe_message(41, 130306, true, t0);
        r.observe_message(42, 129025, false, t0);
        r.observe_message(42, 129026, false, t0);
        let mut filter = no_filter();
        filter.pgn_source_map.insert(129025, 255);
        filter.pgn_source_map.insert(129026, 22);
        let v = serde_json::to_value(build_snapshot(&r, &filter, true, t0)).unwrap();
        let d = v["devices"].as_array().unwrap();
        let pgn = |dev: usize, pgn: u64| {
            d[dev]["pgns"].as_array().unwrap().iter().find(|p| p["pgn"] == pgn).unwrap().clone()
        };
        assert_eq!(d[0]["status"], "not_used", "only blocked PGNs rejected");
        assert_eq!(pgn(0, 129025)["accepted"], false);
        assert_eq!(pgn(0, 129025)["blocked"], true);
        assert_eq!(pgn(0, 130306)["blocked"], false);
        assert_eq!(d[1]["status"], "dropped", "129026 is wanted from src 22");
        assert_eq!(pgn(1, 129025)["blocked"], true);
        assert_eq!(pgn(1, 129026)["blocked"], false);
        assert_eq!(pgn(1, 129026)["accepted"], false);
    }

    #[test]
    fn identity_not_asked_when_node_is_silent() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(1, 130306, true, t0);
        r.observe_address_claim(2, name(1851), t0);
        r.observe_address_claim(3, name(1851), t0);
        r.observe_product_info(3, product());
        r.device_mut(1).unwrap().identity_requests = 4;
        for state in ["disabled", "cannot_claim", "name_conflict"] {
            r.set_node_state(None, state);
            let v = json(&r, true, t0);
            let d = v["devices"].as_array().unwrap();
            assert_eq!(d[0]["identity"], "not_asked", "{state}");
            assert_eq!(d[1]["identity"], "not_asked", "{state}");
            assert_eq!(d[2]["identity"], "known", "{state}");
        }
        for state in ["listening", "claiming", "claimed"] {
            r.set_node_state(None, state);
            let v = json(&r, true, t0);
            assert_eq!(v["devices"][0]["identity"], "no_reply", "{state}");
            assert_eq!(v["devices"][1]["identity"], "asking", "{state}");
        }
    }

    #[test]
    fn warnings_included() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.set_listening_since(t0);
        let v = json(&r, true, t0 + secs(70));
        assert_eq!(v["warnings"].as_array().unwrap().len(), 3);
        assert_eq!(v["warnings"][0]["level"], "amber");
    }
}
