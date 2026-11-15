//! The router as an NMEA2000 node: address claim, answers to ISO requests, heartbeat.
//!
//! `N2kNode` is a pure state machine: callers feed it what the bus said and the current time,
//! and collect the frames it wants to send with [`N2kNode::drain`]. It never reads the clock.
//!
//! Politeness rules (spec "The router becomes a proper N2K node"):
//! - listen 2 s before claiming, and only ever claim an address that has been silent for 60 s;
//! - always yield: any other claim for our address, whatever its NAME, moves us elsewhere;
//! - another node sending anything else from our address (a claim we missed) gets our claim
//!   again, at most once every 5 s, so normal address contention sorts it out;
//! - transmit nothing but the claim itself until the claim has stood for 250 ms;
//! - if no address is free, send Cannot Claim (source 254) and stay silent;
//! - if another node shows our exact NAME, stay silent for good.

use super::registry::BusRegistry;
use super::OutFrame;
use crate::config::{NodeConfig, MAX_PREFERRED_ADDRESS};
use nmea2k::encode_fast_packet;
use nmea2k::n2k_id::{build_id, pgn_of, source_of, GLOBAL_ADDRESS, NULL_ADDRESS};
use nmea2k::pgns::{
    AddressClaim, Heartbeat, IsoAck, IsoRequest, N2kName, PgnList, PgnListFunction, ProductInformation,
};
use std::time::{Duration, Instant};
use tracing::error;

pub const TRANSMIT_PGNS: [u32; 6] = [59392, 59904, 60928, 126464, 126993, 126996];
pub const RECEIVE_PGNS: [u32; 3] = [59904, 60928, 126996];

const PGN_ISO_ACK: u32 = 59392;
const PGN_ISO_REQUEST: u32 = 59904;
const PGN_ADDRESS_CLAIM: u32 = 60928;
const PGN_PGN_LIST: u32 = 126464;
const PGN_HEARTBEAT: u32 = 126993;
const PGN_PRODUCT_INFO: u32 = 126996;

const PRIORITY_DEFAULT: u8 = 6;
const PRIORITY_HEARTBEAT: u8 = 7;

/// Listen this long after start before claiming.
const LISTEN_DURATION: Duration = Duration::from_millis(2000);
/// A claim must stand this long before anything else is sent from the address.
const CLAIM_SETTLE: Duration = Duration::from_millis(250);
const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(60_000);
/// Shortest time between two claims re-sent because another node uses our address.
const CONFLICT_RECLAIM_INTERVAL: Duration = Duration::from_millis(5_000);
/// The heartbeat sequence counter runs 0..=252 (253-255 are reserved values of a u8 field).
const HEARTBEAT_SEQUENCE_MODULO: u16 = 253;

/// The router NAME. Device function 130 / class 25 (PC gateway), system instance 15 and the
/// arbitrary-address-capable bit make it compare higher than normal marine devices, so it loses
/// every address contest.
pub fn router_name(unique_number: u32) -> N2kName {
    N2kName {
        unique_number,
        manufacturer_code: 2046,
        device_instance_lower: 0,
        device_instance_upper: 0,
        device_function: 130,
        device_class: 25,
        system_instance: 15,
        industry_group: 4,
        arbitrary_address_capable: true,
    }
}

pub fn router_product_info(unique_number: u32) -> ProductInformation {
    ProductInformation {
        nmea2000_version: Some(2100),
        product_code: Some(1),
        model_id: "NMEA Router".to_string(),
        software_version: env!("CARGO_PKG_VERSION").to_string(),
        model_version: String::new(),
        serial_code: unique_number.to_string(),
        certification_level: Some(0),
        load_equivalency: Some(1),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeState {
    /// `transmit_enabled` is false: never sends anything.
    Disabled,
    /// Request for Address Claim sent from 254; building the registry until `until`.
    Listening { until: Instant },
    /// Claiming `address`; only the claim itself may be sent until it settles. `since` is when
    /// `drain` handed the claim frame to the caller (`None` while the claim is still queued), so
    /// the 250 ms wait always runs from the real send, never from when the claim was queued.
    Claiming { address: u8, since: Option<Instant> },
    /// `address` is ours; may answer requests and send.
    Claimed { address: u8, next_heartbeat: Instant },
    /// No free address: Cannot Claim sent, silent from now on.
    CannotClaim,
    /// Another node uses our exact NAME: silent from now on.
    NameConflict,
}

pub struct N2kNode {
    name: N2kName,
    unique_number: u32,
    preferred_address: u8,
    state: NodeState,
    outbox: Vec<OutFrame>,
    /// Fast-packet sequence counter, one per node, 0..=7.
    fast_packet_sequence: u8,
    heartbeat_sequence: u8,
    /// When the claim was last re-sent because of an address conflict.
    last_conflict_claim: Option<Instant>,
}

impl N2kNode {
    pub fn new(cfg: &NodeConfig, now: Instant) -> Self {
        let mut node = Self {
            name: router_name(cfg.unique_number),
            unique_number: cfg.unique_number,
            // Config validation already enforces this; clamp anyway so 252..=255 can never be claimed.
            preferred_address: cfg.preferred_address.min(MAX_PREFERRED_ADDRESS),
            state: NodeState::Disabled,
            outbox: Vec::new(),
            fast_packet_sequence: 0,
            heartbeat_sequence: 0,
            last_conflict_claim: None,
        };
        if cfg.transmit_enabled {
            node.state = NodeState::Listening { until: now + LISTEN_DURATION };
            node.queue_single(
                PRIORITY_DEFAULT,
                PGN_ISO_REQUEST,
                GLOBAL_ADDRESS,
                NULL_ADDRESS,
                &IsoRequest { requested_pgn: PGN_ADDRESS_CLAIM }.to_bytes(),
            );
        }
        node
    }

    pub fn tick(&mut self, reg: &BusRegistry, now: Instant) {
        match self.state {
            NodeState::Listening { until } if now >= until => {
                let address = self.find_silent_address(reg, now, self.preferred_address, None);
                self.claim(address);
            }
            NodeState::Claiming { address, since: Some(since) }
                if now.saturating_duration_since(since) >= CLAIM_SETTLE =>
            {
                self.state = NodeState::Claimed { address, next_heartbeat: now };
                self.send_heartbeat_if_due(now);
            }
            NodeState::Claimed { .. } => self.send_heartbeat_if_due(now),
            _ => {}
        }
    }

    /// A PGN 60928 claim from `source` was received.
    pub fn on_address_claim(&mut self, source: u8, name: N2kName, reg: &BusRegistry, now: Instant) {
        if matches!(self.state, NodeState::Disabled | NodeState::NameConflict) {
            return;
        }
        // Our socket does not receive our own frames, so this NAME is another node.
        if name == self.name {
            error!(
                "NMEA2000 node: another device (source {}) uses the router's NAME {}; the router stops \
                 transmitting. Change can.node.unique_number (now {}) to a value no other node uses.",
                source, self.name, self.unique_number
            );
            self.state = NodeState::NameConflict;
            self.outbox.clear();
            return;
        }
        let current = match self.state {
            NodeState::Claiming { address, .. } | NodeState::Claimed { address, .. } => address,
            _ => return,
        };
        if source != current {
            return;
        }
        // Always yield, whatever the NAME: drop anything still queued from this address.
        self.outbox.clear();
        let start = if current >= MAX_PREFERRED_ADDRESS { 0 } else { current + 1 };
        let address = self.find_silent_address(reg, now, start, Some(current));
        self.claim(address);
    }

    /// Another node sent a frame other than an Address Claim from [`Self::bus_address`]: it uses
    /// our address, perhaps because its claim was missed. Re-sends our claim (J1939-81 / ISO
    /// 11783-5), at most once every 5 s: if the other node's NAME is lower it answers with its own
    /// claim and we yield through [`Self::on_address_claim`]; otherwise it must move. The address
    /// is still ours meanwhile, so the 250 ms settle wait is not restarted and normal traffic goes on.
    pub fn on_address_conflict(&mut self, now: Instant) {
        let Some(address) = self.bus_address() else {
            return;
        };
        if self
            .last_conflict_claim
            .is_some_and(|at| now.saturating_duration_since(at) < CONFLICT_RECLAIM_INTERVAL)
        {
            return;
        }
        self.last_conflict_claim = Some(now);
        self.queue_claim(address);
    }

    /// An ISO Request (59904) from `source` to `destination` was received.
    pub fn on_iso_request(&mut self, source: u8, destination: u8, requested_pgn: u32) {
        let NodeState::Claimed { address, .. } = self.state else {
            return;
        };
        let to_us = destination == address;
        if !to_us && destination != GLOBAL_ADDRESS {
            return;
        }
        // A requester at 254 (no address) or 255 (invalid) cannot be replied to: only a Request for
        // Address Claim is answered, and that answer goes to global anyway.
        if source == NULL_ADDRESS || source == GLOBAL_ADDRESS {
            if requested_pgn == PGN_ADDRESS_CLAIM {
                self.queue_claim(address);
            }
            return;
        }
        match requested_pgn {
            PGN_ADDRESS_CLAIM => self.queue_claim(address),
            PGN_PRODUCT_INFO => {
                let payload = router_product_info(self.unique_number).to_bytes();
                self.queue_fast_packet(PGN_PRODUCT_INFO, GLOBAL_ADDRESS, address, &payload);
            }
            PGN_PGN_LIST => {
                let reply_to = if to_us { source } else { GLOBAL_ADDRESS };
                for (function, pgns) in [
                    (PgnListFunction::Transmit, TRANSMIT_PGNS.to_vec()),
                    (PgnListFunction::Receive, RECEIVE_PGNS.to_vec()),
                ] {
                    let payload = PgnList { function, pgns }.to_bytes();
                    self.queue_fast_packet(PGN_PGN_LIST, reply_to, address, &payload);
                }
            }
            other if to_us => {
                self.queue_single(PRIORITY_DEFAULT, PGN_ISO_ACK, source, address, &IsoAck::nak(other).to_bytes());
            }
            _ => {}
        }
    }

    /// Queues an ISO Request for `pgn` to `destination`. Returns `false` (nothing queued) unless
    /// the address is claimed.
    pub fn send_request(&mut self, destination: u8, pgn: u32) -> bool {
        let NodeState::Claimed { address, .. } = self.state else {
            return false;
        };
        self.queue_single(
            PRIORITY_DEFAULT,
            PGN_ISO_REQUEST,
            destination,
            address,
            &IsoRequest { requested_pgn: pgn }.to_bytes(),
        );
        true
    }

    /// The claimed address, once the claim has settled.
    pub fn address(&self) -> Option<u8> {
        match self.state {
            NodeState::Claimed { address, .. } => Some(address),
            _ => None,
        }
    }

    /// The address our frames come from on the bus: the claimed one, or the one being claimed
    /// once its claim has been handed off for sending.
    pub fn bus_address(&self) -> Option<u8> {
        match self.state {
            NodeState::Claimed { address, .. } | NodeState::Claiming { address, since: Some(_) } => {
                Some(address)
            }
            _ => None,
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.state {
            NodeState::Disabled => "disabled",
            NodeState::Listening { .. } => "listening",
            NodeState::Claiming { .. } => "claiming",
            NodeState::Claimed { .. } => "claimed",
            NodeState::CannotClaim => "cannot_claim",
            NodeState::NameConflict => "name_conflict",
        }
    }

    /// Hands the queued frames to the caller, which writes them at `now`. A pending claim starts
    /// its 250 ms settle wait here.
    pub fn drain(&mut self, now: Instant) -> Vec<OutFrame> {
        let frames = std::mem::take(&mut self.outbox);
        if let NodeState::Claiming { address, since: None } = self.state {
            if frames.iter().any(|f| is_claim_from(f, address)) {
                self.state = NodeState::Claiming { address, since: Some(now) };
            }
        }
        frames
    }

    /// The caller failed to write a frame. While claiming, the claim may not have reached the bus,
    /// so it is queued again and the 250 ms wait restarts from the next hand-off. In any other
    /// state this does nothing. `now` is unused: the restart is timed by the next `drain`.
    pub fn on_transmit_failed(&mut self, _now: Instant) {
        self.rearm_claim();
    }

    /// Drops every queued frame. A claim that was dropped before it was sent is queued again and
    /// its 250 ms wait restarts from the next hand-off.
    pub fn discard_outbox(&mut self) {
        self.outbox.clear();
        self.rearm_claim();
    }

    /// While claiming: make sure exactly one claim is queued and clear the settle timer.
    fn rearm_claim(&mut self) {
        let NodeState::Claiming { address, .. } = self.state else {
            return;
        };
        self.state = NodeState::Claiming { address, since: None };
        if !self.outbox.iter().any(|f| is_claim_from(f, address)) {
            self.queue_claim(address);
        }
    }

    /// First silent address from `start` upward, wrapping within 0..=251, skipping `exclude`.
    fn find_silent_address(&self, reg: &BusRegistry, now: Instant, start: u8, exclude: Option<u8>) -> Option<u8> {
        let range = MAX_PREFERRED_ADDRESS as u16 + 1;
        (0..range)
            .map(|i| ((start as u16 + i) % range) as u8)
            .find(|&a| Some(a) != exclude && reg.is_address_silent(a, now))
    }

    /// Claims `address`, or sends Cannot Claim if there is none.
    fn claim(&mut self, address: Option<u8>) {
        match address {
            Some(address) => {
                self.queue_claim(address);
                self.state = NodeState::Claiming { address, since: None };
            }
            None => {
                self.queue_claim(NULL_ADDRESS);
                self.state = NodeState::CannotClaim;
            }
        }
    }

    fn send_heartbeat_if_due(&mut self, now: Instant) {
        let NodeState::Claimed { address, next_heartbeat } = self.state else {
            return;
        };
        if now < next_heartbeat {
            return;
        }
        let heartbeat = Heartbeat {
            interval_ms: HEARTBEAT_INTERVAL.as_millis() as u32,
            sequence: self.heartbeat_sequence,
        };
        self.heartbeat_sequence = ((self.heartbeat_sequence as u16 + 1) % HEARTBEAT_SEQUENCE_MODULO) as u8;
        self.queue_single(PRIORITY_HEARTBEAT, PGN_HEARTBEAT, GLOBAL_ADDRESS, address, &heartbeat.to_bytes());
        self.state = NodeState::Claimed { address, next_heartbeat: now + HEARTBEAT_INTERVAL };
    }

    fn queue_claim(&mut self, source: u8) {
        let claim = AddressClaim { name: self.name }.to_bytes();
        self.queue_single(PRIORITY_DEFAULT, PGN_ADDRESS_CLAIM, GLOBAL_ADDRESS, source, &claim);
    }

    fn queue_single(&mut self, priority: u8, pgn: u32, destination: u8, source: u8, data: &[u8]) {
        debug_assert!(data.len() <= 8);
        self.outbox.push(OutFrame { id: build_id(priority, pgn, destination, source), data: data.to_vec() });
    }

    fn queue_fast_packet(&mut self, pgn: u32, destination: u8, source: u8, payload: &[u8]) {
        let id = build_id(PRIORITY_DEFAULT, pgn, destination, source);
        for frame in encode_fast_packet(payload, self.fast_packet_sequence) {
            self.outbox.push(OutFrame { id, data: frame.to_vec() });
        }
        self.fast_packet_sequence = (self.fast_packet_sequence + 1) % 8;
    }
}

fn is_claim_from(frame: &OutFrame, address: u8) -> bool {
    pgn_of(frame.id) == PGN_ADDRESS_CLAIM && source_of(frame.id) == address
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmea2k::n2k_id::{destination_of, pgn_of, source_of};
    use nmea2k::pgns::{IsoAck, IsoRequest};
    use std::time::Duration;

    const UNIQUE: u32 = 1234;

    fn cfg(preferred_address: u8) -> NodeConfig {
        NodeConfig { transmit_enabled: true, preferred_address, unique_number: UNIQUE }
    }

    fn registry(t0: Instant) -> BusRegistry {
        BusRegistry::new(t0, router_name(UNIQUE))
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn priority_of(f: &OutFrame) -> u8 {
        ((f.id.as_raw() >> 26) & 0x7) as u8
    }

    /// (pgn, source, destination) of a frame.
    fn decode(f: &OutFrame) -> (u32, u8, u8) {
        (pgn_of(f.id), source_of(f.id), destination_of(f.id))
    }

    /// Reassembles fast-packet frames (all of one PGN, in order) into their payloads.
    fn reassemble(frames: &[OutFrame]) -> Vec<(u8, Vec<u8>)> {
        let mut out: Vec<(u8, usize, Vec<u8>)> = Vec::new();
        for f in frames {
            let seq = f.data[0] >> 5;
            if f.data[0] & 0x1F == 0 {
                out.push((seq, f.data[1] as usize, f.data[2..].to_vec()));
            } else {
                let last = out.last_mut().expect("continuation without first frame");
                assert_eq!(last.0, seq, "sequence changed inside a message");
                last.2.extend_from_slice(&f.data[1..]);
            }
        }
        out.into_iter()
            .map(|(seq, len, mut data)| {
                data.truncate(len);
                (seq, data)
            })
            .collect()
    }

    /// A node claimed at 100 at `t0 + 2.3 s`, outbox empty.
    fn claimed_node(reg: &BusRegistry, t0: Instant) -> N2kNode {
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        node.tick(reg, t0 + ms(2300));
        assert_eq!(node.address(), Some(100));
        node.drain(t0 + ms(2300));
        node
    }

    #[test]
    fn router_name_uses_global_constants() {
        let n = router_name(UNIQUE);
        assert_eq!(n.unique_number, UNIQUE);
        assert_eq!(n.manufacturer_code, 2046);
        assert_eq!(n.device_instance_lower, 0);
        assert_eq!(n.device_instance_upper, 0);
        assert_eq!(n.device_function, 130);
        assert_eq!(n.device_class, 25);
        assert_eq!(n.system_instance, 15);
        assert_eq!(n.industry_group, 4);
        assert!(n.arbitrary_address_capable);
    }

    #[test]
    fn router_product_info_values() {
        let p = router_product_info(UNIQUE);
        assert_eq!(p.model_id, "NMEA Router");
        assert_eq!(p.software_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(p.serial_code, "1234");
        assert_eq!(p.nmea2000_version, Some(2100));
        assert_eq!(p.product_code, Some(1));
        assert_eq!(p.certification_level, Some(0));
        assert_eq!(p.load_equivalency, Some(1));
        assert_eq!(p.to_bytes().len(), 134);
    }

    #[test]
    fn listens_before_claiming() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        assert_eq!(node.state_label(), "listening");
        let out = node.drain(t0);
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (59904, 254, 255));
        assert_eq!(priority_of(&out[0]), 6);
        assert_eq!(IsoRequest::from_bytes(&out[0].data).unwrap().requested_pgn, 60928);
        node.tick(&reg, t0 + ms(1000));
        assert!(node.drain(t0 + ms(1000)).is_empty());
        assert_eq!(node.address(), None);
    }

    #[test]
    fn claims_preferred_when_silent() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        let out = node.drain(t0 + ms(2000));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 100, 255));
        assert_eq!(priority_of(&out[0]), 6);
        assert_eq!(out[0].data, nmea2k::pgns::AddressClaim { name: router_name(UNIQUE) }.to_bytes());
        assert_eq!(node.state_label(), "claiming");
        node.tick(&reg, t0 + ms(2100));
        assert_eq!(node.address(), None);
        node.tick(&reg, t0 + ms(2300));
        assert_eq!(node.address(), Some(100));
        assert_eq!(node.state_label(), "claimed");
    }

    #[test]
    fn skips_busy_preferred_address() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        reg.observe_message(100, 130306, true, t0 + ms(1000));
        reg.observe_message(101, 130306, true, t0 + ms(1000));
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        let out = node.drain(t0 + ms(2000));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 102, 255));
        node.tick(&reg, t0 + ms(2300));
        assert_eq!(node.address(), Some(102));
    }

    #[test]
    fn wraps_and_skips_claim_only_device() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        reg.observe_address_claim(251, N2kName::from_u64(42), t0 + ms(500));
        reg.observe_message(0, 130306, true, t0 + ms(500));
        let mut node = N2kNode::new(&cfg(251), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        let out = node.drain(t0 + ms(2000));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 1, 255));
    }

    #[test]
    fn never_claims_reserved_addresses() {
        // Everything 0..=251 busy except nothing: 252 and 253 must never be picked.
        let t0 = Instant::now();
        let mut reg = registry(t0);
        for a in 0..=250u8 {
            reg.observe_message(a, 130306, true, t0);
        }
        let mut node = N2kNode::new(&cfg(251), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        let out = node.drain(t0 + ms(2000));
        assert_eq!(decode(&out[0]), (60928, 251, 255));
        // 251 then gets taken: no address left, never 252/253.
        let mut reg2 = registry(t0);
        for a in 0..=251u8 {
            reg2.observe_message(a, 130306, true, t0 + ms(2100));
        }
        node.on_address_claim(251, N2kName::from_u64(42), &reg2, t0 + ms(2100));
        let out = node.drain(t0 + ms(2100));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 254, 255));
        assert_eq!(node.state_label(), "cannot_claim");
    }

    #[test]
    fn yields_on_any_conflicting_claim() {
        let t0 = Instant::now();
        let own = router_name(UNIQUE);
        let lower_name = N2kName::from_u64(1);
        let higher_name = N2kName::from_u64(u64::MAX);
        assert!(lower_name < own && higher_name > own);

        for (name, record_in_registry) in [(lower_name, true), (higher_name, false)] {
            let mut reg = registry(t0);
            let mut node = claimed_node(&reg, t0);
            let t = t0 + ms(5000);
            if record_in_registry {
                reg.observe_address_claim(100, name, t);
            }
            node.on_address_claim(100, name, &reg, t);
            assert_eq!(node.address(), None, "must stop using 100 at once");
            assert_eq!(node.state_label(), "claiming");
            let out = node.drain(t);
            assert_eq!(out.len(), 1);
            assert_eq!(decode(&out[0]), (60928, 101, 255));
            node.tick(&reg, t + ms(250));
            assert_eq!(node.address(), Some(101));
        }
    }

    #[test]
    fn yields_while_still_claiming() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.tick(&reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        node.on_address_claim(100, N2kName::from_u64(7), &reg, t0 + ms(2100));
        let out = node.drain(t0 + ms(2100));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 101, 255));
        // The 250 ms wait restarts from the new claim.
        node.tick(&reg, t0 + ms(2300));
        assert_eq!(node.address(), None);
        node.tick(&reg, t0 + ms(2350));
        assert_eq!(node.address(), Some(101));
    }

    #[test]
    fn claim_for_another_address_is_ignored() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        node.on_address_claim(5, N2kName::from_u64(7), &reg, t0 + ms(3000));
        assert!(node.drain(t0 + ms(3000)).is_empty());
        assert_eq!(node.address(), Some(100));
    }

    #[test]
    fn cannot_claim_when_full() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        for a in 0..=251u8 {
            reg.observe_message(a, 130306, true, t0 + ms(1000));
        }
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        let out = node.drain(t0 + ms(2000));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 254, 255));
        assert_eq!(out[0].data, nmea2k::pgns::AddressClaim { name: router_name(UNIQUE) }.to_bytes());
        assert_eq!(node.state_label(), "cannot_claim");
        assert_eq!(node.address(), None);
        assert!(!node.send_request(5, 126996));
        node.on_iso_request(5, 255, 60928);
        node.tick(&reg, t0 + ms(120_000));
        assert!(node.drain(t0 + ms(120_000)).is_empty());
    }

    #[test]
    fn identical_name_silences() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        node.send_request(5, 126996); // pending in the outbox, must be dropped
        node.on_address_claim(37, router_name(UNIQUE), &reg, t0 + ms(3000));
        assert_eq!(node.state_label(), "name_conflict");
        assert_eq!(node.address(), None);
        assert!(node.drain(t0 + ms(3000)).is_empty());
        node.tick(&reg, t0 + ms(70_000));
        node.on_iso_request(5, 255, 60928);
        node.on_address_claim(100, N2kName::from_u64(7), &reg, t0 + ms(71_000));
        assert!(!node.send_request(5, 126996));
        assert!(node.drain(t0 + ms(71_000)).is_empty());
        assert_eq!(node.state_label(), "name_conflict");
    }

    #[test]
    fn identical_name_while_listening_drops_pending_request() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.on_address_claim(37, router_name(UNIQUE), &reg, t0 + ms(500));
        assert!(node.drain(t0 + ms(500)).is_empty());
        node.tick(&reg, t0 + ms(2000));
        assert!(node.drain(t0 + ms(2000)).is_empty());
        assert_eq!(node.state_label(), "name_conflict");
    }

    #[test]
    fn answers_requests() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);

        node.on_iso_request(5, 255, 60928);
        let out = node.drain(t0 + ms(2300));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 100, 255));

        node.on_iso_request(5, 100, 126996);
        let out = node.drain(t0 + ms(2300));
        assert_eq!(out.len(), 20);
        for f in &out {
            assert_eq!(decode(f), (126996, 100, 255));
            assert_eq!(priority_of(f), 6);
            assert_eq!(f.data.len(), 8);
        }
        let msgs = reassemble(&out);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, router_product_info(UNIQUE).to_bytes());

        node.on_iso_request(5, 100, 126464);
        let out = node.drain(t0 + ms(2300));
        for f in &out {
            assert_eq!(decode(f), (126464, 100, 5));
            assert_eq!(priority_of(f), 6);
        }
        let msgs = reassemble(&out);
        assert_eq!(msgs.len(), 2);
        let mut transmit = vec![0u8];
        TRANSMIT_PGNS.iter().for_each(|p| transmit.extend_from_slice(&p.to_le_bytes()[..3]));
        let mut receive = vec![1u8];
        RECEIVE_PGNS.iter().for_each(|p| receive.extend_from_slice(&p.to_le_bytes()[..3]));
        assert_eq!(msgs[0].1, transmit);
        assert_eq!(msgs[1].1, receive);
        assert_ne!(msgs[0].0, msgs[1].0, "each message has its own sequence number");

        node.on_iso_request(7, 255, 126464);
        let out = node.drain(t0 + ms(2300));
        assert!(!out.is_empty());
        assert!(out.iter().all(|f| decode(f) == (126464, 100, 255)));

        node.on_iso_request(5, 100, 130306);
        let out = node.drain(t0 + ms(2300));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (59392, 100, 5));
        assert_eq!(priority_of(&out[0]), 6);
        assert_eq!(out[0].data, IsoAck::nak(130306).to_bytes());

        node.on_iso_request(5, 255, 130306);
        assert!(node.drain(t0 + ms(2300)).is_empty());

        node.on_iso_request(5, 55, 60928);
        node.on_iso_request(5, 55, 126996);
        node.on_iso_request(5, 55, 130306);
        assert!(node.drain(t0 + ms(2300)).is_empty());
    }

    #[test]
    fn fast_packet_sequence_wraps_mod_8() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut seqs = Vec::new();
        for _ in 0..10 {
            node.on_iso_request(5, 100, 126996);
            seqs.push(reassemble(&node.drain(t0 + ms(2300)))[0].0);
        }
        for w in seqs.windows(2) {
            assert_eq!(w[1], (w[0] + 1) % 8);
        }
    }

    #[test]
    fn send_request_only_when_claimed() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        assert!(!node.send_request(5, 126996));
        node.tick(&reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        assert!(!node.send_request(5, 126996), "not during the 250 ms claim wait");
        assert!(node.drain(t0 + ms(2000)).is_empty());
        node.tick(&reg, t0 + ms(2300));
        node.drain(t0 + ms(2300));
        assert!(node.send_request(5, 126996));
        let out = node.drain(t0 + ms(2300));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (59904, 100, 5));
        assert_eq!(priority_of(&out[0]), 6);
        assert_eq!(IsoRequest::from_bytes(&out[0].data).unwrap().requested_pgn, 126996);
        assert!(node.send_request(255, 60928));
        assert_eq!(decode(&node.drain(t0 + ms(2300))[0]), (59904, 100, 255));
    }

    #[test]
    fn silent_until_claimed() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        for pgn in [60928, 126996, 126464, 130306] {
            node.on_iso_request(5, 255, pgn);
            node.on_iso_request(5, 100, pgn);
        }
        assert!(node.drain(t0).is_empty());
        node.tick(&reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        for pgn in [60928, 126996, 126464, 130306] {
            node.on_iso_request(5, 255, pgn);
            node.on_iso_request(5, 100, pgn);
        }
        assert!(node.drain(t0 + ms(2000)).is_empty(), "nothing during the 250 ms claim wait");
    }

    #[test]
    fn heartbeat_every_60s() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.tick(&reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        let t = t0 + ms(2300);
        node.tick(&reg, t);
        let out = node.drain(t);
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (126993, 100, 255));
        assert_eq!(priority_of(&out[0]), 7);
        assert_eq!(u16::from_le_bytes([out[0].data[0], out[0].data[1]]), 60_000);
        let first_seq = out[0].data[2];

        node.tick(&reg, t + ms(30_000));
        assert!(node.drain(t + ms(30_000)).is_empty());
        node.tick(&reg, t + ms(59_999));
        assert!(node.drain(t + ms(59_999)).is_empty());

        node.tick(&reg, t + ms(60_000));
        let out = node.drain(t + ms(60_000));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (126993, 100, 255));
        assert_eq!(priority_of(&out[0]), 7);
        assert_eq!(out[0].data[2], first_seq.wrapping_add(1));

        node.tick(&reg, t + ms(60_500));
        assert!(node.drain(t + ms(60_500)).is_empty());
    }

    #[test]
    fn discard_outbox_drops_pending_frames() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        node.on_iso_request(5, 100, 126996);
        node.discard_outbox();
        assert!(node.drain(t0 + ms(2300)).is_empty());
        assert_eq!(node.address(), Some(100));
    }

    #[test]
    fn disabled_never_transmits() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let config = NodeConfig { transmit_enabled: false, preferred_address: 100, unique_number: UNIQUE };
        let mut node = N2kNode::new(&config, t0);
        assert_eq!(node.state_label(), "disabled");
        assert!(node.drain(t0).is_empty());
        for s in [1000, 2000, 2300, 62_300, 200_000] {
            node.tick(&reg, t0 + ms(s));
            node.on_iso_request(5, 255, 60928);
            node.on_iso_request(5, 100, 126996);
            node.on_iso_request(5, 100, 130306);
            node.on_address_claim(100, N2kName::from_u64(7), &reg, t0 + ms(s));
            assert!(!node.send_request(5, 126996));
            node.on_transmit_failed(t0 + ms(s));
            assert!(node.drain(t0 + ms(s)).is_empty());
        }
        node.on_address_claim(37, router_name(UNIQUE), &reg, t0 + ms(300_000));
        assert!(node.drain(t0 + ms(300_000)).is_empty());
        assert_eq!(node.state_label(), "disabled");
        assert_eq!(node.address(), None);
    }

    /// Frames drained at `now` that are a claim from `address`.
    fn claims_from(frames: &[OutFrame], address: u8) -> usize {
        frames.iter().filter(|f| decode(f) == (60928, address, 255)).count()
    }

    #[test]
    fn discarded_claim_is_resent_and_settle_restarts() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000)); // claim queued, never handed out
        node.discard_outbox();
        node.tick(&reg, t0 + ms(2400));
        assert_eq!(node.address(), None, "a claim that was never sent must not settle");
        assert_eq!(node.state_label(), "claiming");
        let out = node.drain(t0 + ms(2400));
        assert_eq!(out.len(), 1);
        assert_eq!(claims_from(&out, 100), 1);
        node.tick(&reg, t0 + ms(2649));
        assert_eq!(node.address(), None);
        node.tick(&reg, t0 + ms(2650));
        assert_eq!(node.address(), Some(100));
    }

    #[test]
    fn held_claim_settles_from_hand_off() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000)); // claim queued; transmission held for 1 s
        node.tick(&reg, t0 + ms(3000));
        assert_eq!(node.address(), None);
        let out = node.drain(t0 + ms(3000));
        assert_eq!(out.len(), 1, "only the claim, no heartbeat right behind it");
        assert_eq!(claims_from(&out, 100), 1);
        node.tick(&reg, t0 + ms(3200));
        assert_eq!(node.address(), None);
        assert!(node.drain(t0 + ms(3200)).is_empty());
        node.tick(&reg, t0 + ms(3250));
        assert_eq!(node.address(), Some(100));
        let out = node.drain(t0 + ms(3250));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (126993, 100, 255));
    }

    #[test]
    fn transmit_failure_while_claiming_resends_claim() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        node.tick(&reg, t0 + ms(2000));
        assert_eq!(claims_from(&node.drain(t0 + ms(2000)), 100), 1);
        node.on_transmit_failed(t0 + ms(2010));
        node.tick(&reg, t0 + ms(2300));
        assert_eq!(node.address(), None, "the failed claim must not settle");
        let out = node.drain(t0 + ms(2300));
        assert_eq!(out.len(), 1);
        assert_eq!(claims_from(&out, 100), 1);
        node.tick(&reg, t0 + ms(2549));
        assert_eq!(node.address(), None);
        node.tick(&reg, t0 + ms(2550));
        assert_eq!(node.address(), Some(100));
    }

    #[test]
    fn transmit_failure_outside_claiming_is_a_no_op() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut listening = N2kNode::new(&cfg(100), t0);
        listening.drain(t0);
        listening.on_transmit_failed(t0 + ms(10));
        assert!(listening.drain(t0 + ms(10)).is_empty());
        assert_eq!(listening.state_label(), "listening");

        let mut node = claimed_node(&reg, t0);
        node.on_transmit_failed(t0 + ms(2400));
        assert!(node.drain(t0 + ms(2400)).is_empty());
        assert_eq!(node.address(), Some(100));
    }

    #[test]
    fn address_conflict_resends_claim_at_most_every_5s() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let t = t0 + ms(5000);

        node.on_address_conflict(t);
        let out = node.drain(t);
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 100, 255));
        assert_eq!(out[0].data, nmea2k::pgns::AddressClaim { name: router_name(UNIQUE) }.to_bytes());
        assert_eq!(node.address(), Some(100), "the address is still ours");
        assert_eq!(node.state_label(), "claimed");
        // Normal traffic goes on: requests can still be sent.
        assert!(node.send_request(5, 126996));
        node.drain(t);

        for later in [ms(1), ms(2500), ms(4999)] {
            node.on_address_conflict(t + later);
            assert!(node.drain(t + later).is_empty(), "rate limited at {later:?}");
        }
        node.on_address_conflict(t + ms(5000));
        assert_eq!(claims_from(&node.drain(t + ms(5000)), 100), 1);

        // The other node has the lower NAME and answers with its claim: we yield as usual.
        let lower_name = N2kName::from_u64(1);
        assert!(lower_name < router_name(UNIQUE));
        node.on_address_claim(100, lower_name, &reg, t + ms(5010));
        assert_eq!(node.address(), None);
        let out = node.drain(t + ms(5010));
        assert_eq!(out.len(), 1);
        assert_eq!(decode(&out[0]), (60928, 101, 255));
    }

    #[test]
    fn address_conflict_while_claiming_resends_claim_without_restarting_settle() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = N2kNode::new(&cfg(100), t0);
        node.drain(t0);
        // Claim queued but not yet handed off: our frames are not on the bus yet.
        node.tick(&reg, t0 + ms(2000));
        assert_eq!(node.bus_address(), None);
        node.on_address_conflict(t0 + ms(2000));
        assert_eq!(claims_from(&node.drain(t0 + ms(2000)), 100), 1, "only the normal claim");
        assert_eq!(node.bus_address(), Some(100));

        node.on_address_conflict(t0 + ms(2100));
        let out = node.drain(t0 + ms(2100));
        assert_eq!(out.len(), 1);
        assert_eq!(claims_from(&out, 100), 1);
        node.tick(&reg, t0 + ms(2250));
        assert_eq!(node.address(), Some(100), "settles 250 ms after the first claim");
    }

    #[test]
    fn address_conflict_ignored_when_not_on_the_bus() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut listening = N2kNode::new(&cfg(100), t0);
        listening.drain(t0);
        listening.on_address_conflict(t0 + ms(500));
        assert!(listening.drain(t0 + ms(500)).is_empty());

        let config = NodeConfig { transmit_enabled: false, preferred_address: 100, unique_number: UNIQUE };
        let mut disabled = N2kNode::new(&config, t0);
        disabled.on_address_conflict(t0 + ms(500));
        assert!(disabled.drain(t0 + ms(500)).is_empty());

        let mut silenced = claimed_node(&reg, t0);
        silenced.on_address_claim(37, router_name(UNIQUE), &reg, t0 + ms(3000));
        silenced.on_address_conflict(t0 + ms(3000));
        assert!(silenced.drain(t0 + ms(3000)).is_empty());
    }

    #[test]
    fn requests_from_null_or_global_source_get_only_a_claim() {
        let t0 = Instant::now();
        let reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let now = t0 + ms(2400);
        for source in [254u8, 255] {
            for destination in [100u8, 255] {
                for pgn in [126996, 126464, 130306, 59904] {
                    node.on_iso_request(source, destination, pgn);
                }
                assert!(node.drain(now).is_empty(), "source {source} dest {destination}");
                node.on_iso_request(source, destination, 60928);
                let out = node.drain(now);
                assert_eq!(out.len(), 1);
                assert_eq!(decode(&out[0]), (60928, 100, 255));
            }
        }
    }
}
