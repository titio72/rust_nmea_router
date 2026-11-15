//! NMEA2000 bus explorer: a passive registry of every device on the bus, plus the router acting
//! as a polite NMEA2000 node (address claim, identity requests, heartbeat).
//!
//! [`BusExplorer`] is the facade the router loop drives: it sees every received frame (before
//! the source filter acts), every decoded message, and writes the router's own frames through a
//! caller-supplied writer. It owns the node state machine; the [`BusRegistry`] is shared with the
//! web layer behind a mutex. On the router side the mutex is locked only inside the facade's
//! methods, never across a CAN write; the web handler locks it briefly to build a snapshot.
//!
//! Frames already handed to the kernel's transmit queue (up to txqueuelen) cannot be recalled:
//! after the node yields its address, those still go out from the old address.

pub mod names;
pub mod node;
pub mod registry;
pub mod requests;
pub mod snapshot;
pub mod warnings;

use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use nmea2k::n2k_id::{destination_of, pgn_of, source_of, GLOBAL_ADDRESS, NULL_ADDRESS};
use nmea2k::pgns::{AddressClaim, IsoRequest};
use nmea2k::{is_fast_packet_pgn, ExtendedId, N2kFrame, N2kMessage, N2kStreamReader};

use crate::config::NodeConfig;
use node::{router_name, N2kNode};
use registry::BusRegistry;
use requests::RequestScheduler;

/// One CAN frame to transmit (`data` is at most 8 bytes).
#[derive(Debug, Clone)]
pub struct OutFrame {
    pub id: ExtendedId,
    pub data: Vec<u8>,
}

/// The registry shared between the router loop (writer) and the web layer (reader).
pub type SharedBusRegistry = Arc<Mutex<BusRegistry>>;

const PGN_ISO_REQUEST: u32 = 59904;
const PGN_ADDRESS_CLAIM: u32 = 60928;
const PGN_PRODUCT_INFO: u32 = 126996;

/// A gap longer than this between two `transmit` calls means the router loop stalled.
const STALL_GAP: Duration = Duration::from_secs(1);
/// After a stall, transmit nothing for this long so backlogged frames (claims) are read first.
const STALL_HOLD: Duration = Duration::from_secs(1);

/// Linux `ENOBUFS` ("no buffer space"): the CAN device's transmit queue (txqueuelen) is full.
/// `libc` is not a dependency, so the value is spelled out; 105 is the asm-generic value used by
/// x86, ARM and aarch64 Linux.
const ENOBUFS: i32 = 105;

/// What one [`BusExplorer::transmit`] call did, for the caller's metrics and logging.
#[derive(Debug, Default)]
pub struct TxReport {
    /// Frames written.
    pub written: u64,
    /// The bus pushed back (transmit queue full): the unsent frames stay queued for the next call.
    pub backpressure: bool,
    /// A write failed: the error, and how many queued frames (the failed one included) were dropped.
    pub failure: Option<(io::Error, usize)>,
}

/// A full transmit queue: worth retrying later rather than giving up on the frame.
fn is_backpressure(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::WouldBlock || e.raw_os_error() == Some(ENOBUFS)
}

/// Creates the shared registry; the router's own NAME comes from `cfg.unique_number`.
pub fn new_shared_registry(started: Instant, cfg: &NodeConfig) -> SharedBusRegistry {
    Arc::new(Mutex::new(BusRegistry::new(started, router_name(cfg.unique_number))))
}

/// Locks the registry, recovering the value if another thread panicked while holding it.
fn lock(registry: &SharedBusRegistry) -> MutexGuard<'_, BusRegistry> {
    registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct BusExplorer {
    registry: SharedBusRegistry,
    node: N2kNode,
    scheduler: RequestScheduler,
    /// Assembles 126996 only, independently of the pipeline's reader and of the source filter.
    identity_reader: N2kStreamReader,
    /// Frames drained from the node but not yet written (the bus pushed back). The node is
    /// advanced and drained again only once this is empty.
    pending: VecDeque<OutFrame>,
    /// Time of the previous `transmit` call, to detect loop stalls.
    last_poll: Option<Instant>,
    /// While set and in the future, transmission is held after a stall.
    hold_until: Option<Instant>,
}

impl BusExplorer {
    /// Create it once the CAN socket is open: `now` is when the router starts listening, which
    /// starts the node's listen window and the warnings' startup grace period.
    pub fn new(registry: SharedBusRegistry, cfg: &NodeConfig, now: Instant) -> Self {
        let node = N2kNode::new(cfg, now);
        {
            let mut reg = lock(&registry);
            reg.set_listening_since(now);
            reg.set_node_state(node.address(), node.state_label());
        }
        Self {
            registry,
            node,
            scheduler: RequestScheduler::new(),
            identity_reader: N2kStreamReader::new(),
            pending: VecDeque::new(),
            last_poll: None,
            hold_until: None,
        }
    }

    /// Observes one received CAN frame. Call for **every** frame, before the source filter's
    /// decision is acted on; `accepted` is that decision. Identity frames (60928, 59904, 126996)
    /// are handled whatever `accepted` says.
    ///
    /// The socket does not receive the router's own frames (`CAN_RAW_RECV_OWN_MSGS` is off), so
    /// any frame from the router's address comes from another node. A claim is handled by the
    /// node's yield logic; anything else is an address conflict and makes the node claim again.
    pub fn on_frame(&mut self, id: ExtendedId, data: &[u8], accepted: bool, now: Instant) {
        let pgn = pgn_of(id);
        let source = source_of(id);
        // Count messages, not frames: a fast-packet message is counted on its first frame only.
        let message_start = !is_fast_packet_pgn(pgn) || data.first().is_none_or(|b| b & 0x1F == 0);

        if pgn != PGN_ADDRESS_CLAIM && Some(source) == self.node.bus_address() {
            self.node.on_address_conflict(now);
        }
        let mut reg = lock(&self.registry);
        if message_start {
            reg.observe_message(source, pgn, accepted, now);
        }
        match pgn {
            PGN_ADDRESS_CLAIM if source != NULL_ADDRESS && source != GLOBAL_ADDRESS => {
                // `from_bytes` rejects anything shorter than 8 bytes.
                if let Some(claim) = AddressClaim::from_bytes(data) {
                    reg.observe_address_claim(source, claim.name, now);
                    let before = (self.node.address(), self.node.state_label());
                    self.node.on_address_claim(source, claim.name, &reg, now);
                    // A claim for the address our queued frames come from, or any node state
                    // change (yield, name conflict): those frames must never reach the bus.
                    let state_changed = before != (self.node.address(), self.node.state_label());
                    if state_changed || self.pending.iter().any(|f| source_of(f.id) == source) {
                        self.pending.clear();
                    }
                    if state_changed {
                        // Publish a yield at once, so a snapshot never shows the claimer at an
                        // address the router has already left as a conflict.
                        reg.set_node_state(self.node.address(), self.node.state_label());
                    }
                }
            }
            PGN_ISO_REQUEST => {
                // `from_bytes` rejects anything shorter than 3 bytes.
                if let Some(request) = IsoRequest::from_bytes(data) {
                    self.node.on_iso_request(source, destination_of(id), request.requested_pgn);
                }
            }
            PGN_PRODUCT_INFO => {
                if let Some(frame) = self.identity_reader.process_frame(id, data) {
                    if let N2kMessage::ProductInformation(info) = frame.message {
                        reg.observe_product_info(source, info);
                    }
                }
            }
            _ => {}
        }
    }

    /// Observes a message the pipeline's reader assembled (only frames the filter accepted).
    pub fn on_decoded(&mut self, frame: &N2kFrame, now: Instant) {
        let id = frame.identifier.as_can_id();
        let known = !matches!(frame.message, N2kMessage::Unknown(..));
        lock(&self.registry).observe_decoded(source_of(id), pgn_of(id), known, now);
    }

    /// Advances the node and writes its frames with `write`, in order. Call on every loop
    /// iteration; `write` performs the CAN write (the registry is not locked while it runs).
    ///
    /// - Backpressure (`WouldBlock` / `ENOBUFS`): the unsent frames stay queued and are written,
    ///   in order, by the next calls; the node is not advanced until they are all out, so a claim's
    ///   250 ms settle wait never starts for a frame still waiting. An Address Claim that hits
    ///   backpressure is handled as a failure instead, so the wait restarts from its real send.
    /// - Any other error: the queued frames are dropped and the node re-arms its claim if needed.
    /// - After a loop stall (> 1 s since the previous call) everything queued is discarded and
    ///   nothing is sent for 1 s, so backlogged claims are processed first.
    pub fn transmit<F>(&mut self, now: Instant, mut write: F) -> TxReport
    where
        F: FnMut(&OutFrame) -> io::Result<()>,
    {
        let mut report = TxReport::default();
        if !self.advance(now) {
            return report;
        }
        while let Some(frame) = self.pending.front() {
            match write(frame) {
                Ok(()) => {
                    self.pending.pop_front();
                    report.written += 1;
                }
                Err(e) if is_backpressure(&e) && pgn_of(frame.id) != PGN_ADDRESS_CLAIM => {
                    report.backpressure = true;
                    break;
                }
                Err(e) => {
                    report.failure = Some((e, self.pending.len()));
                    self.pending.clear();
                    self.node.on_transmit_failed(now);
                    break;
                }
            }
        }
        report
    }

    /// Stall handling, then (only when nothing is still queued) `tick`, scheduler, published node
    /// state and `drain` into the queue. Returns `false` when nothing may be written now.
    fn advance(&mut self, now: Instant) -> bool {
        let stalled = self
            .last_poll
            .is_some_and(|last| now.saturating_duration_since(last) > STALL_GAP);
        self.last_poll = Some(now);
        if stalled {
            self.pending.clear();
            self.node.discard_outbox();
            self.hold_until = Some(now + STALL_HOLD);
            return false;
        }
        if let Some(until) = self.hold_until {
            if now < until {
                return false;
            }
            self.hold_until = None;
        }
        if self.pending.is_empty() {
            {
                let mut reg = lock(&self.registry);
                self.node.tick(&reg, now);
                self.scheduler.poll(&mut reg, &mut self.node, now);
                reg.set_node_state(self.node.address(), self.node.state_label());
            }
            self.pending.extend(self.node.drain(now));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_explorer::node::router_product_info;
    use crate::config::NodeConfig;
    use nmea2k::encode_fast_packet;
    use nmea2k::n2k_id::{build_id, destination_of, pgn_of, source_of};
    use nmea2k::pgns::{AddressClaim, IsoRequest, N2kName};
    use std::time::{Duration, Instant};

    const UNIQUE: u32 = 1234;
    const OWN: u8 = 100;
    /// Linux ENETDOWN: an error that is not backpressure.
    const ENETDOWN: i32 = 100;

    fn cfg() -> NodeConfig {
        NodeConfig { transmit_enabled: true, preferred_address: OWN, unique_number: UNIQUE }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn enobufs() -> io::Error {
        io::Error::from_raw_os_error(ENOBUFS)
    }

    fn explorer(t0: Instant) -> (BusExplorer, SharedBusRegistry) {
        let reg = new_shared_registry(t0, &cfg());
        (BusExplorer::new(reg.clone(), &cfg(), t0), reg)
    }

    /// One `transmit` call with a writer that always succeeds; returns the frames written.
    fn poll(ex: &mut BusExplorer, now: Instant) -> Vec<OutFrame> {
        let mut out = Vec::new();
        let report = ex.transmit(now, |f| {
            out.push(f.clone());
            Ok(())
        });
        assert_eq!(report.written as usize, out.len());
        out
    }

    /// One `transmit` call whose writer accepts `ok` frames, then fails every write with `err()`.
    /// Returns the report and every frame offered to the writer (written or not).
    fn poll_failing(
        ex: &mut BusExplorer,
        now: Instant,
        ok: usize,
        err: fn() -> io::Error,
    ) -> (TxReport, Vec<OutFrame>) {
        let mut offered = Vec::new();
        let report = ex.transmit(now, |f| {
            offered.push(f.clone());
            if offered.len() > ok { Err(err()) } else { Ok(()) }
        });
        (report, offered)
    }

    /// An explorer claimed at `OWN`, polled every 500 ms from `t0`; returns it with the time
    /// of the poll at which the claim settled (queue empty after that poll).
    fn claimed_explorer(t0: Instant) -> (BusExplorer, SharedBusRegistry, Instant) {
        let (mut ex, reg) = explorer(t0);
        let mut t = t0;
        for _ in 0..=4 {
            poll(&mut ex, t);
            t += ms(500);
        }
        // Claim was handed off at t0 + 2 s; it settles 250 ms later.
        let settled = t0 + ms(2300);
        poll(&mut ex, settled);
        assert_eq!(lock(&reg).node_state(), (Some(OWN), "claimed"));
        (ex, reg, settled)
    }

    /// Makes the claimed explorer queue a 20-frame product-information answer.
    fn request_product_info(ex: &mut BusExplorer, now: Instant) {
        let request = IsoRequest { requested_pgn: 126996 }.to_bytes();
        ex.on_frame(build_id(6, 59904, OWN, 9), &request, true, now);
    }

    /// Fast-packet frame counters (`data[0] & 0x1F`) of the 126996 frames in `frames`.
    fn product_frame_counters(frames: &[OutFrame]) -> Vec<u8> {
        frames.iter().filter(|f| pgn_of(f.id) == 126996).map(|f| f.data[0] & 0x1F).collect()
    }

    fn feed_fast_packet(ex: &mut BusExplorer, id: ExtendedId, payload: &[u8], seq: u8, accepted: bool, now: Instant) {
        for frame in encode_fast_packet(payload, seq) {
            ex.on_frame(id, &frame, accepted, now);
        }
    }

    #[test]
    fn listening_since_set_when_explorer_created() {
        let t0 = Instant::now();
        let reg = new_shared_registry(t0, &cfg());
        assert_eq!(lock(&reg).listening_since(), None);
        let opened = t0 + Duration::from_secs(40);
        let _ex = BusExplorer::new(reg.clone(), &cfg(), opened);
        assert_eq!(lock(&reg).listening_since(), Some(opened));
        assert_eq!(lock(&reg).started(), t0);
    }

    #[test]
    fn fast_packet_counts_messages_not_frames() {
        let t0 = Instant::now();
        let (mut ex, reg) = explorer(t0);
        let id = build_id(3, 129029, 255, 0);
        let payload = [0x11u8; 43];
        assert_eq!(encode_fast_packet(&payload, 0).len(), 7);
        for i in 0..3u64 {
            feed_fast_packet(&mut ex, id, &payload, i as u8, true, t0 + Duration::from_secs(i));
        }
        let reg = lock(&reg);
        let stats = &reg.devices()[&0].pgns[&129029];
        assert_eq!(stats.count, 3);
        let interval = stats.interval_ewma.expect("interval after 3 messages");
        assert!((interval.as_secs_f64() - 1.0).abs() < 0.01, "interval {:?}", interval);
    }

    #[test]
    fn identity_learned_even_if_filtered() {
        let t0 = Instant::now();
        let (mut ex, reg) = explorer(t0);
        let id = build_id(6, 126996, 255, 3);
        let payload = router_product_info(77).to_bytes();
        assert_eq!(encode_fast_packet(&payload, 0).len(), 20);
        feed_fast_packet(&mut ex, id, &payload, 2, false, t0);
        let reg = lock(&reg);
        let device = &reg.devices()[&3];
        let product = device.product.as_ref().expect("product info learned");
        assert_eq!(product.model_id, "NMEA Router");
        assert_eq!(product.serial_code, "77");
        assert_eq!(device.pgns[&126996].count, 1);
        assert!(!device.pgns[&126996].last_accepted);
    }

    #[test]
    fn address_claim_learned_even_if_filtered() {
        let t0 = Instant::now();
        let (mut ex, reg) = explorer(t0);
        let name = N2kName::from_u64(0x00A1_B2C3_D4E5_F607);
        let claim = AddressClaim { name }.to_bytes();
        ex.on_frame(build_id(6, 60928, 255, 9), &claim, false, t0);
        assert_eq!(lock(&reg).devices()[&9].name, Some(name));
    }

    #[test]
    fn malformed_identity_ignored() {
        let t0 = Instant::now();
        let (mut ex, reg, t) = claimed_explorer(t0);
        let short_claim = [0x01u8, 0x02, 0x03, 0x04, 0x05, 0x06];
        ex.on_frame(build_id(6, 60928, 255, 9), &short_claim, true, t);
        // A 2-byte ISO request to us must not be answered.
        ex.on_frame(build_id(6, 59904, OWN, 9), &[0x14, 0xF0], true, t);
        // Empty frames of a single-frame and a fast-packet PGN.
        ex.on_frame(build_id(6, 60928, 255, 10), &[], true, t);
        ex.on_frame(build_id(6, 126996, 255, 10), &[], true, t);
        // Claims from 254 / 255 are not devices.
        let claim = AddressClaim { name: N2kName::from_u64(42) }.to_bytes();
        ex.on_frame(build_id(6, 60928, 255, 254), &claim, true, t);
        ex.on_frame(build_id(6, 60928, 255, 255), &claim, true, t);

        let reg_guard = lock(&reg);
        assert_eq!(reg_guard.devices()[&9].name, None);
        assert!(!reg_guard.devices().contains_key(&254));
        assert!(!reg_guard.devices().contains_key(&255));
        drop(reg_guard);
        let out = poll(&mut ex, t + ms(10));
        assert!(
            out.iter().all(|f| destination_of(f.id) != 9 || pgn_of(f.id) == 59904),
            "nothing but our own requests may go to 9: {:?}",
            out
        );
    }

    #[test]
    fn iso_request_reaches_node() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        let request = IsoRequest { requested_pgn: 60928 }.to_bytes();
        ex.on_frame(build_id(6, 59904, OWN, 9), &request, false, t);
        let out = poll(&mut ex, t + ms(10));
        assert!(out.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN), "{:?}", out);
    }

    #[test]
    fn address_claim_reaches_node() {
        let t0 = Instant::now();
        let (mut ex, reg, t) = claimed_explorer(t0);
        let other = AddressClaim { name: N2kName::from_u64(42) }.to_bytes();
        ex.on_frame(build_id(6, 60928, 255, OWN), &other, true, t);
        assert_eq!(lock(&reg).node_state(), (None, "claiming"), "the yield is published at once");
        let out = poll(&mut ex, t + ms(10));
        assert!(out.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN + 1), "{:?}", out);
    }

    #[test]
    fn frame_from_own_address_reclaims() {
        let t0 = Instant::now();
        let (mut ex, reg, t) = claimed_explorer(t0);
        // Another node transmits (not a claim) from our address.
        ex.on_frame(build_id(2, 127250, 255, OWN), &[0u8; 8], true, t + ms(10));
        let out = poll(&mut ex, t + ms(20));
        let claims = out.iter().filter(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN).count();
        assert_eq!(claims, 1, "{:?}", out);
        assert_eq!(lock(&reg).node_state(), (Some(OWN), "claimed"));
        assert!(lock(&reg).devices().contains_key(&OWN), "the other node is recorded");

        // A second conflict within 5 s: rate limited.
        ex.on_frame(build_id(2, 127250, 255, OWN), &[0u8; 8], true, t + ms(1000));
        let out = poll(&mut ex, t + ms(1010));
        assert!(out.iter().all(|f| pgn_of(f.id) != 60928), "{:?}", out);
        // Frames from other addresses are never a conflict.
        ex.on_frame(build_id(2, 127250, 255, OWN + 1), &[0u8; 8], true, t + ms(6000));
        let out = poll(&mut ex, t + ms(6010));
        assert!(out.iter().all(|f| pgn_of(f.id) != 60928), "{:?}", out);
    }

    #[test]
    fn stall_discards_and_holds() {
        let t0 = Instant::now();
        let (mut ex, reg, t) = claimed_explorer(t0);
        // Queued just before the loop stalls: an answer that would be product information.
        request_product_info(&mut ex, t + ms(100));

        assert!(poll(&mut ex, t + Duration::from_secs(5)).is_empty(), "stall detected: nothing sent");
        assert!(poll(&mut ex, t + ms(5500)).is_empty(), "still holding");
        let out = poll(&mut ex, t + ms(6100));
        assert!(!out.is_empty(), "resumes after the hold");
        assert!(out.iter().all(|f| pgn_of(f.id) != 126996), "stale answer discarded: {:?}", out);
        // The scheduler resumed: the global product-information request.
        assert!(out.iter().any(|f| {
            pgn_of(f.id) == 59904
                && destination_of(f.id) == 255
                && IsoRequest::from_bytes(&f.data).map(|r| r.requested_pgn) == Some(126996)
        }));
        assert_eq!(lock(&reg).node_state(), (Some(OWN), "claimed"));
    }

    #[test]
    fn stall_clears_backpressured_frames() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        request_product_info(&mut ex, t + ms(10));
        let (report, _) = poll_failing(&mut ex, t + ms(10), 5, enobufs);
        assert!(report.backpressure);

        let (report, offered) = poll_failing(&mut ex, t + Duration::from_secs(5), 0, enobufs);
        assert!(offered.is_empty(), "nothing written on the stall");
        assert!(!report.backpressure && report.failure.is_none());
        assert!(poll(&mut ex, t + ms(5500)).is_empty(), "still holding");
        let out = poll(&mut ex, t + ms(6100));
        assert!(product_frame_counters(&out).is_empty(), "stale remainder discarded: {:?}", out);
    }

    #[test]
    fn backpressure_keeps_remainder_and_resumes_in_order() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        request_product_info(&mut ex, t + ms(10));

        let (report, offered) = poll_failing(&mut ex, t + ms(10), 5, enobufs);
        assert_eq!(report.written, 5);
        assert!(report.backpressure);
        assert!(report.failure.is_none(), "backpressure is not a failure");
        assert_eq!(product_frame_counters(&offered[..5]), (0..5).collect::<Vec<u8>>());

        // WouldBlock is backpressure too.
        let (report, _) = poll_failing(&mut ex, t + ms(20), 3, || io::Error::from(io::ErrorKind::WouldBlock));
        assert_eq!(report.written, 3);
        assert!(report.backpressure && report.failure.is_none());

        let out = poll(&mut ex, t + ms(30));
        assert_eq!(product_frame_counters(&out), (8..20).collect::<Vec<u8>>(), "rest, in order");
    }

    #[test]
    fn no_drain_while_frames_are_pending() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        request_product_info(&mut ex, t + ms(10));
        poll_failing(&mut ex, t + ms(10), 0, enobufs);
        // The global product-information request falls due 3 s after the claim; the bus is
        // still pushing back, so only the front pending frame is ever offered.
        for step in 1..=8 {
            let (report, offered) = poll_failing(&mut ex, t + ms(10 + step * 500), 0, enobufs);
            assert!(report.backpressure);
            assert_eq!(offered.len(), 1);
            assert_eq!(product_frame_counters(&offered), vec![0], "front frame retried");
        }
        let later = t + ms(4510);
        let out = poll(&mut ex, later);
        assert_eq!(out.len(), 20, "only the pending answer, nothing newly drained: {:?}", out);
        assert_eq!(product_frame_counters(&out), (0..20).collect::<Vec<u8>>());
        let next = poll(&mut ex, later + ms(10));
        assert!(
            next.iter().any(|f| pgn_of(f.id) == 59904 && destination_of(f.id) == 255),
            "node advanced once the queue emptied: {:?}",
            next
        );
    }

    #[test]
    fn backpressured_claim_is_a_failure() {
        let t0 = Instant::now();
        let (mut ex, reg) = explorer(t0);
        let mut t = t0;
        for _ in 0..4 {
            poll(&mut ex, t);
            t += ms(500);
        }
        let (report, offered) = poll_failing(&mut ex, t0 + ms(2000), 0, enobufs);
        assert!(offered.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN));
        assert!(!report.backpressure);
        assert!(report.failure.is_some(), "a claim that did not go out is a failure");
        // The settle wait restarts from the real send: still claiming, claim sent again.
        let out = poll(&mut ex, t0 + ms(2300));
        assert!(out.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN), "claim re-sent: {:?}", out);
        assert_eq!(lock(&reg).node_state(), (None, "claiming"));
        poll(&mut ex, t0 + ms(2600));
        assert_eq!(lock(&reg).node_state(), (Some(OWN), "claimed"));
    }

    #[test]
    fn other_error_drops_the_rest() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        request_product_info(&mut ex, t + ms(10));
        let (report, offered) =
            poll_failing(&mut ex, t + ms(10), 5, || io::Error::from_raw_os_error(ENETDOWN));
        assert_eq!(report.written, 5);
        assert_eq!(offered.len(), 6, "nothing attempted after the failure");
        assert!(!report.backpressure);
        let (_, dropped) = report.failure.expect("failure reported");
        assert_eq!(dropped, 15);
        let out = poll(&mut ex, t + ms(20));
        assert!(product_frame_counters(&out).is_empty(), "rest dropped: {:?}", out);
    }

    #[test]
    fn transmit_failure_while_claiming_resends_claim() {
        let t0 = Instant::now();
        let (mut ex, _reg) = explorer(t0);
        let mut t = t0;
        for _ in 0..4 {
            poll(&mut ex, t);
            t += ms(500);
        }
        let (report, offered) =
            poll_failing(&mut ex, t0 + ms(2000), 0, || io::Error::from_raw_os_error(ENETDOWN));
        assert!(offered.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN));
        assert!(report.failure.is_some());
        let out = poll(&mut ex, t0 + ms(2300));
        assert!(out.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN), "claim re-sent: {:?}", out);
    }

    #[test]
    fn yield_clears_pending_frames_from_the_old_address() {
        let t0 = Instant::now();
        let (mut ex, _reg, t) = claimed_explorer(t0);
        request_product_info(&mut ex, t + ms(10));
        poll_failing(&mut ex, t + ms(10), 5, enobufs);
        // Another node claims our address: we yield, and the half-sent answer from OWN must go.
        let other = AddressClaim { name: N2kName::from_u64(42) }.to_bytes();
        ex.on_frame(build_id(6, 60928, 255, OWN), &other, true, t + ms(20));
        let out = poll(&mut ex, t + ms(30));
        assert!(out.iter().all(|f| source_of(f.id) != OWN), "nothing from the old address: {:?}", out);
        assert!(out.iter().any(|f| pgn_of(f.id) == 60928 && source_of(f.id) == OWN + 1), "{:?}", out);
    }

    #[test]
    fn decoded_marks_pgn_used() {
        let t0 = Instant::now();
        let (mut ex, reg) = explorer(t0);
        let id = build_id(2, 129025, 255, 22);
        let data = [0u8; 8];
        ex.on_frame(id, &data, true, t0);
        let mut reader = nmea2k::N2kStreamReader::new();
        let frame = reader.process_frame(id, &data).expect("single frame");
        ex.on_decoded(&frame, t0);
        assert_eq!(lock(&reg).devices()[&22].pgns[&129025].last_used, Some(t0));
    }
}
