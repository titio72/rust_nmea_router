//! Passive registry of the devices heard on the NMEA2000 bus: per-source and per-PGN
//! observation counters, message rates, identity (NAME, product information) and a status
//! derived from what the router's frame filter and decoder did with the traffic.
//!
//! Business logic here never reads the clock: every method that needs the time takes `now`.

use nmea2k::n2k_id::{GLOBAL_ADDRESS, NULL_ADDRESS};
use nmea2k::pgns::{N2kName, ProductInformation};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// A device silent for longer than this is "gone"; a drop/use observation older than this no
/// longer counts towards the status.
pub const GONE_AFTER: Duration = Duration::from_secs(60);

/// ISO network-management PGNs: traffic every node sends regardless of whether anyone uses
/// it, so decoding them never makes a device "in use".
pub const NETWORK_MANAGEMENT_PGNS: [u32; 6] = [59392, 59904, 60928, 126464, 126993, 126996];

/// Weight of the newest inter-arrival time in the per-PGN interval average.
const EWMA_ALPHA: f64 = 0.2;

/// ISO Address Claim PGN (carries the NAME).
const PGN_ADDRESS_CLAIM: u32 = 60928;

/// Device status, in decreasing precedence: Gone > Dropped > InUse > NotUsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStatus {
    /// At least one of its PGNs was decoded and used by the router in the last 60 s.
    InUse,
    /// Heard, but nothing it sends is used by the router.
    NotUsed,
    /// At least one of its messages was rejected by the frame filter in the last 60 s because the
    /// filter expects that PGN from another source. A PGN the filter blocks from every source
    /// (configured source 255) does not count: that rejection is deliberate.
    Dropped,
    /// Nothing heard from it for more than 60 s.
    Gone,
}

/// Per-(source, PGN) observation statistics. Counts are messages, not CAN frames.
#[derive(Debug, Clone)]
pub struct PgnStats {
    #[allow(dead_code)] // recorded for diagnostics; the snapshot does not expose it yet
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub count: u64,
    /// Exponentially weighted average (alpha 0.2) of the inter-arrival time; `None` until
    /// the second message.
    pub interval_ewma: Option<Duration>,
    /// Whether the most recent message was accepted by the frame filter.
    pub last_accepted: bool,
    pub last_dropped: Option<Instant>,
    pub last_used: Option<Instant>,
}

impl PgnStats {
    /// Statistics of a PGN whose first message arrives at `now`.
    fn first(now: Instant, accepted: bool) -> Self {
        Self {
            first_seen: now,
            last_seen: now,
            count: 1,
            interval_ewma: None,
            last_accepted: accepted,
            last_dropped: if accepted { None } else { Some(now) },
            last_used: None,
        }
    }
}

/// Everything known about one source address.
#[derive(Debug, Clone)]
pub struct DeviceEntry {
    #[allow(dead_code)] // recorded for diagnostics; the snapshot does not expose it yet
    pub first_heard: Instant,
    pub last_heard: Instant,
    pub pgns: BTreeMap<u32, PgnStats>,
    pub name: Option<N2kName>,
    pub product: Option<ProductInformation>,
    /// Set when the same NAME later claimed another address.
    pub moved_to: Option<u8>,
    pub identity_requests: u8,
    pub last_identity_request: Option<Instant>,
}

impl DeviceEntry {
    fn new(now: Instant) -> Self {
        Self {
            first_heard: now,
            last_heard: now,
            pgns: BTreeMap::new(),
            name: None,
            product: None,
            moved_to: None,
            identity_requests: 0,
            last_identity_request: None,
        }
    }
}

pub struct BusRegistry {
    started: Instant,
    /// When the router started listening on the bus (the CAN socket is open and the bus explorer
    /// exists); `None` until then.
    listening_since: Option<Instant>,
    own_name: N2kName,
    devices: BTreeMap<u8, DeviceEntry>,
    node_address: Option<u8>,
    node_label: &'static str,
}

/// Sources 254 (node that cannot claim an address) and 255 (global) are not devices.
fn is_device_address(source: u8) -> bool {
    source != NULL_ADDRESS && source != GLOBAL_ADDRESS
}

fn is_within(t: Option<Instant>, now: Instant) -> bool {
    t.is_some_and(|t| now.saturating_duration_since(t) <= GONE_AFTER)
}

impl BusRegistry {
    pub fn new(started: Instant, own_name: N2kName) -> Self {
        Self {
            started,
            listening_since: None,
            own_name,
            devices: BTreeMap::new(),
            node_address: None,
            node_label: "starting",
        }
    }

    pub fn started(&self) -> Instant {
        self.started
    }

    pub fn listening_since(&self) -> Option<Instant> {
        self.listening_since
    }

    /// Records that the router started listening on the bus at `now`.
    pub fn set_listening_since(&mut self, now: Instant) {
        self.listening_since = Some(now);
    }

    pub fn own_name(&self) -> N2kName {
        self.own_name
    }

    /// Records one received **message** (not one CAN frame) and whether the frame filter
    /// accepted it. Sources 254 and 255 are ignored.
    pub fn observe_message(&mut self, source: u8, pgn: u32, accepted: bool, now: Instant) {
        if !is_device_address(source) {
            return;
        }
        let device = self
            .devices
            .entry(source)
            .or_insert_with(|| DeviceEntry::new(now));
        device.last_heard = now;
        match device.pgns.get_mut(&pgn) {
            Some(stats) => {
                let interval = now.saturating_duration_since(stats.last_seen);
                stats.interval_ewma = Some(match stats.interval_ewma {
                    None => interval,
                    Some(prev) => interval.mul_f64(EWMA_ALPHA) + prev.mul_f64(1.0 - EWMA_ALPHA),
                });
                stats.last_seen = now;
                stats.count += 1;
                stats.last_accepted = accepted;
                if !accepted {
                    stats.last_dropped = Some(now);
                }
            }
            None => {
                device.pgns.insert(pgn, PgnStats::first(now, accepted));
            }
        }
    }

    /// Records that the message just observed was decoded; `known` is whether the decoder
    /// understood the PGN. Marks the PGN as used unless it is a network-management PGN, and
    /// only when the filter accepted that message (a rejected message is never "used").
    pub fn observe_decoded(&mut self, source: u8, pgn: u32, known: bool, now: Instant) {
        if !known || NETWORK_MANAGEMENT_PGNS.contains(&pgn) {
            return;
        }
        if let Some(stats) = self
            .devices
            .get_mut(&source)
            .and_then(|d| d.pgns.get_mut(&pgn))
        {
            if stats.last_accepted {
                stats.last_used = Some(now);
            }
        }
    }

    /// Records a PGN 60928 address claim. A device that only ever claims an address is still
    /// "heard". If another address held the same NAME, that entry is marked as moved and its
    /// product information follows the NAME to the new address.
    ///
    /// A different NAME claiming an address is a different device: everything learned about the
    /// previous one (product information, PGNs, identity requests) is forgotten and only the claim
    /// just observed is kept. Any claim makes the device due an identity request again, so its
    /// product information can be asked for at once instead of after the retry interval.
    pub fn observe_address_claim(&mut self, source: u8, name: N2kName, now: Instant) {
        if !is_device_address(source) {
            return;
        }
        let mut carried_product = None;
        for (&address, entry) in self.devices.iter_mut() {
            if address != source && entry.name == Some(name) {
                entry.moved_to = Some(source);
                if carried_product.is_none() {
                    carried_product = entry.product.clone();
                }
            }
        }
        let device = self
            .devices
            .entry(source)
            .or_insert_with(|| DeviceEntry::new(now));
        if device.name.is_some_and(|old| old != name) {
            // The claim was counted (by `observe_message`) just before this call: keep only it.
            let claim_accepted = device
                .pgns
                .get(&PGN_ADDRESS_CLAIM)
                .is_none_or(|s| s.last_accepted);
            device.pgns.clear();
            device
                .pgns
                .insert(PGN_ADDRESS_CLAIM, PgnStats::first(now, claim_accepted));
            device.product = None;
            device.identity_requests = 0;
            device.first_heard = now;
        }
        device.last_heard = now;
        device.name = Some(name);
        device.moved_to = None;
        device.last_identity_request = None;
        if device.product.is_none() {
            device.product = carried_product;
        }
    }

    /// Stores the PGN 126996 product information of a device already heard.
    pub fn observe_product_info(&mut self, source: u8, info: ProductInformation) {
        if let Some(device) = self.devices.get_mut(&source) {
            device.product = Some(info);
        }
    }

    /// The status of `source`; `is_blocked(pgn)` says whether the filter rejects `pgn` from every
    /// source on purpose, so that its rejections do not make the device "dropped".
    pub fn status(&self, source: u8, now: Instant, is_blocked: impl Fn(u32) -> bool) -> Option<DeviceStatus> {
        let device = self.devices.get(&source)?;
        if now.saturating_duration_since(device.last_heard) > GONE_AFTER {
            return Some(DeviceStatus::Gone);
        }
        if device
            .pgns
            .iter()
            .any(|(&pgn, s)| is_within(s.last_dropped, now) && !is_blocked(pgn))
        {
            return Some(DeviceStatus::Dropped);
        }
        if device.pgns.values().any(|s| is_within(s.last_used, now)) {
            return Some(DeviceStatus::InUse);
        }
        Some(DeviceStatus::NotUsed)
    }

    /// True if nothing was heard from `address` in the last 60 s (or ever).
    pub fn is_address_silent(&self, address: u8, now: Instant) -> bool {
        self.devices
            .get(&address)
            .is_none_or(|d| now.saturating_duration_since(d.last_heard) > GONE_AFTER)
    }

    pub fn devices(&self) -> &BTreeMap<u8, DeviceEntry> {
        &self.devices
    }

    pub fn device_mut(&mut self, source: u8) -> Option<&mut DeviceEntry> {
        self.devices.get_mut(&source)
    }

    /// When `pgn` was last seen from `source`.
    pub fn last_sent(&self, source: u8, pgn: u32) -> Option<Instant> {
        self.devices.get(&source)?.pgns.get(&pgn).map(|s| s.last_seen)
    }

    pub fn set_node_state(&mut self, own_address: Option<u8>, label: &'static str) {
        self.node_address = own_address;
        self.node_label = label;
    }

    pub fn node_state(&self) -> (Option<u8>, &'static str) {
        (self.node_address, self.node_label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(unique: u32) -> N2kName {
        N2kName::from_u64(unique as u64)
    }

    fn product(model: &str) -> ProductInformation {
        ProductInformation {
            nmea2000_version: Some(2100),
            product_code: Some(1),
            model_id: model.to_string(),
            software_version: "1.0".to_string(),
            model_version: "A".to_string(),
            serial_code: "123".to_string(),
            certification_level: Some(0),
            load_equivalency: Some(1),
        }
    }

    fn registry(t0: Instant) -> BusRegistry {
        BusRegistry::new(t0, name(999))
    }

    /// Status with no PGN blocked by the filter.
    fn status(r: &BusRegistry, source: u8, now: Instant) -> Option<DeviceStatus> {
        r.status(source, now, |_| false)
    }

    #[test]
    fn new_source_is_not_used_until_decoded() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        assert_eq!(status(&r,5, t0), Some(DeviceStatus::NotUsed));
        r.observe_decoded(5, 130306, true, t0);
        assert_eq!(status(&r,5, t0), Some(DeviceStatus::InUse));
    }

    #[test]
    fn unknown_decoded_pgn_is_not_use() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        r.observe_decoded(5, 130306, false, t0);
        assert_eq!(status(&r,5, t0), Some(DeviceStatus::NotUsed));
    }

    #[test]
    fn decoded_of_a_rejected_message_is_not_use() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, false, t0);
        r.observe_decoded(5, 130306, true, t0);
        assert!(r.devices()[&5].pgns[&130306].last_used.is_none());
    }

    #[test]
    fn network_management_never_counts_as_use() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 126996, true, t0);
        r.observe_decoded(5, 126996, true, t0);
        assert_eq!(status(&r,5, t0), Some(DeviceStatus::NotUsed));
    }

    #[test]
    fn rejected_message_is_dropped() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(41, 130306, true, t0);
        r.observe_decoded(41, 130306, true, t0);
        r.observe_message(41, 129025, false, t0);
        assert_eq!(status(&r,41, t0), Some(DeviceStatus::Dropped));
        // The drop ages out after 60 s of silence on that PGN, and the device is InUse again.
        r.observe_message(41, 130306, true, t0 + Duration::from_secs(30));
        r.observe_decoded(41, 130306, true, t0 + Duration::from_secs(30));
        assert_eq!(
            status(&r,41, t0 + Duration::from_secs(61)),
            Some(DeviceStatus::InUse)
        );
    }

    #[test]
    fn gone_after_60s() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        r.observe_decoded(5, 130306, true, t0);
        assert_eq!(
            status(&r,5, t0 + Duration::from_secs(59)),
            Some(DeviceStatus::InUse)
        );
        assert_eq!(
            status(&r,5, t0 + Duration::from_secs(61)),
            Some(DeviceStatus::Gone)
        );
    }

    #[test]
    fn in_use_expires_when_last_used_is_stale() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        r.observe_decoded(5, 130306, true, t0);
        // Still heard (via another PGN) but the used PGN has not been seen for > 60 s.
        r.observe_message(5, 127250, true, t0 + Duration::from_secs(50));
        r.observe_message(5, 127250, true, t0 + Duration::from_secs(100));
        assert_eq!(
            status(&r,5, t0 + Duration::from_secs(100)),
            Some(DeviceStatus::NotUsed)
        );
    }

    #[test]
    fn unknown_source_has_no_status() {
        let t0 = Instant::now();
        let r = registry(t0);
        assert_eq!(status(&r,5, t0), None);
    }

    #[test]
    fn ignores_null_and_global_sources() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(254, 130306, true, t0);
        r.observe_message(255, 130306, false, t0);
        r.observe_decoded(254, 130306, true, t0);
        r.observe_address_claim(254, name(1), t0);
        r.observe_address_claim(255, name(2), t0);
        r.observe_product_info(254, product("x"));
        assert!(r.devices().is_empty());
        assert!(r.is_address_silent(254, t0));
        assert!(r.is_address_silent(255, t0));
    }

    #[test]
    fn rate_reflects_message_interval() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        for s in 0..3 {
            r.observe_message(5, 130306, true, t0 + Duration::from_secs(s));
        }
        let stats = &r.devices()[&5].pgns[&130306];
        assert_eq!(stats.count, 3);
        let ewma = stats.interval_ewma.expect("ewma after 3 messages");
        let diff = ewma.abs_diff(Duration::from_secs(1));
        assert!(diff <= Duration::from_millis(50), "ewma {ewma:?}");
    }

    #[test]
    fn single_message_has_no_rate() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        assert!(r.devices()[&5].pgns[&130306].interval_ewma.is_none());
    }

    #[test]
    fn address_move_keeps_identity() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        let n = name(42);
        r.observe_address_claim(22, n, t0);
        r.observe_product_info(22, product("P"));
        r.observe_address_claim(41, n, t0 + Duration::from_secs(5));
        assert_eq!(r.devices()[&22].moved_to, Some(41));
        assert_eq!(r.devices()[&41].product, Some(product("P")));
        assert_eq!(r.devices()[&41].name, Some(n));
        assert_eq!(r.devices()[&41].moved_to, None);
    }

    #[test]
    fn address_move_back_clears_moved_to() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        let n = name(42);
        r.observe_address_claim(22, n, t0);
        r.observe_address_claim(41, n, t0 + Duration::from_secs(5));
        r.observe_address_claim(22, n, t0 + Duration::from_secs(10));
        assert_eq!(r.devices()[&22].moved_to, None);
        assert_eq!(r.devices()[&41].moved_to, Some(22));
    }

    #[test]
    fn repeated_claim_at_same_address_is_not_a_move() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        let n = name(42);
        r.observe_address_claim(22, n, t0);
        r.observe_address_claim(22, n, t0 + Duration::from_secs(5));
        assert_eq!(r.devices()[&22].moved_to, None);
    }

    #[test]
    fn different_name_at_an_address_starts_a_new_identity() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(22, PGN_ADDRESS_CLAIM, true, t0);
        r.observe_address_claim(22, name(42), t0);
        r.observe_product_info(22, product("Old"));
        r.observe_message(22, 130306, false, t0);
        {
            let d = r.device_mut(22).unwrap();
            d.identity_requests = 3;
            d.last_identity_request = Some(t0);
            d.moved_to = Some(9);
        }

        let t = t0 + Duration::from_secs(10);
        r.observe_message(22, PGN_ADDRESS_CLAIM, true, t);
        r.observe_address_claim(22, name(43), t);
        let d = &r.devices()[&22];
        assert_eq!(d.name, Some(name(43)));
        assert_eq!(d.product, None, "the old device's product information is forgotten");
        assert_eq!(d.pgns.keys().copied().collect::<Vec<_>>(), vec![PGN_ADDRESS_CLAIM]);
        let claim = &d.pgns[&PGN_ADDRESS_CLAIM];
        assert_eq!((claim.count, claim.first_seen, claim.last_seen), (1, t, t));
        assert!(claim.last_accepted);
        assert_eq!(d.identity_requests, 0);
        assert_eq!(d.last_identity_request, None);
        assert_eq!(d.first_heard, t);
        assert_eq!(d.last_heard, t);
        assert_eq!(d.moved_to, None);
        assert_eq!(status(&r, 22, t), Some(DeviceStatus::NotUsed), "the old drop went with the old device");
    }

    #[test]
    fn same_name_reclaim_keeps_identity() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_address_claim(22, name(42), t0);
        r.observe_product_info(22, product("P"));
        r.observe_message(22, 130306, true, t0);
        r.device_mut(22).unwrap().identity_requests = 2;
        r.observe_address_claim(22, name(42), t0 + Duration::from_secs(10));
        let d = &r.devices()[&22];
        assert_eq!(d.product, Some(product("P")));
        assert!(d.pgns.contains_key(&130306));
        assert_eq!(d.identity_requests, 2);
        assert_eq!(d.first_heard, t0);
    }

    #[test]
    fn address_claim_makes_identity_request_due() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(22, 130306, true, t0);
        r.device_mut(22).unwrap().identity_requests = 1;
        r.device_mut(22).unwrap().last_identity_request = Some(t0);
        r.observe_address_claim(22, name(42), t0 + Duration::from_secs(1));
        assert_eq!(r.devices()[&22].last_identity_request, None);
        assert_eq!(r.devices()[&22].identity_requests, 1, "a first NAME keeps the request count");
    }

    #[test]
    fn blocked_pgn_rejection_is_not_dropped() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(41, 129025, false, t0);
        let blocked = |pgn: u32| pgn == 129025;
        assert_eq!(r.status(41, t0, blocked), Some(DeviceStatus::NotUsed));
        assert_eq!(status(&r, 41, t0), Some(DeviceStatus::Dropped));
        // A PGN wanted from another source still makes it dropped.
        r.observe_message(41, 129026, false, t0);
        assert_eq!(r.status(41, t0, blocked), Some(DeviceStatus::Dropped));
    }

    #[test]
    fn listening_since_is_unset_until_recorded() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        assert_eq!(r.listening_since(), None);
        r.set_listening_since(t0 + Duration::from_secs(30));
        assert_eq!(r.listening_since(), Some(t0 + Duration::from_secs(30)));
        assert_eq!(r.started(), t0);
    }

    #[test]
    fn claim_only_device_is_not_silent() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_address_claim(100, name(7), t0);
        assert!(!r.is_address_silent(100, t0 + Duration::from_secs(10)));
        assert!(r.is_address_silent(100, t0 + Duration::from_secs(61)));
        assert!(r.is_address_silent(101, t0));
    }

    #[test]
    fn last_sent_is_last_seen_of_the_pgn() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.observe_message(5, 130306, true, t0);
        r.observe_message(5, 130306, true, t0 + Duration::from_secs(2));
        assert_eq!(r.last_sent(5, 130306), Some(t0 + Duration::from_secs(2)));
        assert_eq!(r.last_sent(5, 1), None);
        assert_eq!(r.last_sent(6, 130306), None);
    }

    #[test]
    fn node_state_round_trips() {
        let t0 = Instant::now();
        let mut r = registry(t0);
        r.set_node_state(Some(7), "active");
        assert_eq!(r.node_state(), (Some(7), "active"));
        assert_eq!(r.started(), t0);
        assert_eq!(r.own_name(), name(999));
    }
}
