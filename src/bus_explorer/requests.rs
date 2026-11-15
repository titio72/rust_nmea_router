//! Schedules the router's identity requests: two global ISO Requests once the node is claimed,
//! then addressed retries for devices that have not yet told us their NAME or product information.
//!
//! Business logic here never reads the clock: `poll` takes `now` from the caller.

use super::node::N2kNode;
use super::registry::BusRegistry;
use nmea2k::n2k_id::GLOBAL_ADDRESS;
use std::time::{Duration, Instant};

/// Product Information PGN.
const PGN_PRODUCT_INFORMATION: u32 = 126996;
/// ISO Address Claim PGN (carries the NAME).
const PGN_ADDRESS_CLAIM: u32 = 60928;

/// Minimum spacing between two outgoing requests.
const MIN_SPACING: Duration = Duration::from_millis(250);
/// Delay between the global address-claim request and the global product-information request.
const GLOBAL_PRODUCT_DELAY: Duration = Duration::from_secs(3);
/// No addressed request before this long after the global product-information request: devices
/// heard until then are still answering it.
const ADDRESSED_DELAY: Duration = Duration::from_secs(5);
/// Delay between two identity requests to the same device.
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// One initial request plus at most three retries.
const MAX_IDENTITY_REQUESTS: u8 = 4;

#[derive(Debug, Default)]
pub struct RequestScheduler {
    last_sent: Option<Instant>,
    /// When the global 60928 request was queued (`None` until it has been).
    global_claim_sent: Option<Instant>,
    /// When the global 126996 request was queued (`None` until it has been).
    global_product_sent: Option<Instant>,
}

impl RequestScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues at most one request on `node`, and only if at least 250 ms passed since the
    /// previous one. Does nothing until the node has claimed an address.
    pub fn poll(&mut self, reg: &mut BusRegistry, node: &mut N2kNode, now: Instant) {
        let Some(own_address) = node.address() else {
            return;
        };
        if let Some(last) = self.last_sent {
            if now.saturating_duration_since(last) < MIN_SPACING {
                return;
            }
        }

        // The global requests are sent once per scheduler, even if the node later re-claims.
        // Addressed requests wait until the global replies had time to arrive.
        match (self.global_claim_sent, self.global_product_sent) {
            (None, _) => {
                if node.send_request(GLOBAL_ADDRESS, PGN_ADDRESS_CLAIM) {
                    self.global_claim_sent = Some(now);
                    self.last_sent = Some(now);
                }
                return;
            }
            (Some(claim_at), None) => {
                if now.saturating_duration_since(claim_at) >= GLOBAL_PRODUCT_DELAY
                    && node.send_request(GLOBAL_ADDRESS, PGN_PRODUCT_INFORMATION)
                {
                    self.global_product_sent = Some(now);
                    self.last_sent = Some(now);
                }
                return;
            }
            (Some(_), Some(product_at)) => {
                if now.saturating_duration_since(product_at) < ADDRESSED_DELAY {
                    return;
                }
            }
        }

        let Some((source, pgn)) = Self::next_device_request(reg, own_address, now) else {
            return;
        };
        if node.send_request(source, pgn) {
            if let Some(device) = reg.device_mut(source) {
                device.identity_requests += 1;
                device.last_identity_request = Some(now);
            }
            self.last_sent = Some(now);
        }
    }

    /// The lowest-address device that is due an identity request, with the PGN to ask for.
    fn next_device_request(reg: &BusRegistry, own_address: u8, now: Instant) -> Option<(u8, u32)> {
        reg.devices().iter().find_map(|(&source, device)| {
            if source == own_address {
                return None;
            }
            let pgn = if device.name.is_none() {
                PGN_ADDRESS_CLAIM
            } else if device.product.is_none() {
                PGN_PRODUCT_INFORMATION
            } else {
                return None;
            };
            if device.identity_requests >= MAX_IDENTITY_REQUESTS {
                return None;
            }
            let due = device
                .last_identity_request
                .is_none_or(|at| now.saturating_duration_since(at) >= RETRY_INTERVAL);
            // A silent address is a device that is gone.
            if !due || reg.is_address_silent(source, now) {
                return None;
            }
            Some((source, pgn))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus_explorer::node::router_name;
    use crate::bus_explorer::OutFrame;
    use crate::config::NodeConfig;
    use nmea2k::n2k_id::{destination_of, pgn_of, source_of};
    use nmea2k::pgns::{IsoRequest, N2kName, ProductInformation};

    const UNIQUE: u32 = 1234;
    const OWN: u8 = 100;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn cfg() -> NodeConfig {
        NodeConfig { transmit_enabled: true, preferred_address: OWN, unique_number: UNIQUE }
    }

    fn registry(t0: Instant) -> BusRegistry {
        BusRegistry::new(t0, router_name(UNIQUE))
    }

    /// A node claimed at `OWN` by `t0 + 2.3 s`, outbox empty.
    fn claimed_node(reg: &BusRegistry, t0: Instant) -> N2kNode {
        let mut node = N2kNode::new(&cfg(), t0);
        node.drain(t0);
        node.tick(reg, t0 + ms(2000));
        node.drain(t0 + ms(2000));
        node.tick(reg, t0 + ms(2300));
        assert_eq!(node.address(), Some(OWN));
        node.drain(t0 + ms(2300));
        node
    }

    /// Drains the node and decodes each frame as (destination, requested PGN).
    fn drain_requests(node: &mut N2kNode, now: Instant) -> Vec<(u8, u32)> {
        let frames: Vec<OutFrame> = node.drain(now);
        frames
            .iter()
            .map(|f| {
                assert_eq!(pgn_of(f.id), 59904);
                assert_eq!(source_of(f.id), OWN);
                (destination_of(f.id), IsoRequest::from_bytes(&f.data).unwrap().requested_pgn)
            })
            .collect()
    }

    fn product() -> ProductInformation {
        ProductInformation {
            nmea2000_version: Some(2100),
            product_code: Some(1),
            model_id: "Model".to_string(),
            software_version: "1.0".to_string(),
            model_version: "A".to_string(),
            serial_code: "123".to_string(),
            certification_level: Some(0),
            load_equivalency: Some(1),
        }
    }

    #[test]
    fn global_requests_after_claim() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);

        sched.poll(&mut reg, &mut node, t);
        assert_eq!(drain_requests(&mut node, t), vec![(255, 60928)]);

        sched.poll(&mut reg, &mut node, t + ms(300));
        assert!(drain_requests(&mut node, t + ms(300)).is_empty());

        sched.poll(&mut reg, &mut node, t + secs(3));
        assert_eq!(drain_requests(&mut node, t + secs(3)), vec![(255, 126996)]);

        // Never again.
        sched.poll(&mut reg, &mut node, t + secs(4));
        assert!(drain_requests(&mut node, t + secs(4)).is_empty());
    }

    #[test]
    fn new_device_gets_one_addressed_request() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let t = t + secs(8);
        reg.observe_message(5, 130306, true, t);
        sched.poll(&mut reg, &mut node, t);
        assert_eq!(drain_requests(&mut node, t), vec![(5, 60928)]);
        assert_eq!(reg.devices()[&5].identity_requests, 1);
        assert_eq!(reg.devices()[&5].last_identity_request, Some(t));

        sched.poll(&mut reg, &mut node, t + ms(100));
        assert!(drain_requests(&mut node, t + ms(100)).is_empty());
        assert_eq!(reg.devices()[&5].identity_requests, 1);
    }

    #[test]
    fn asks_for_product_when_only_name_is_known() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let t = t + secs(8);
        reg.observe_address_claim(5, N2kName::from_u64(77), t);
        sched.poll(&mut reg, &mut node, t);
        assert_eq!(drain_requests(&mut node, t), vec![(5, 126996)]);
    }

    #[test]
    fn no_addressed_request_until_5s_after_global_product_request() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        assert_eq!(drain_requests(&mut node, t), vec![(255, 60928)]);

        // Heard (NAME known, product not) before the global product request.
        reg.observe_address_claim(5, N2kName::from_u64(77), t + ms(500));
        for at in [ms(1200), ms(2500)] {
            reg.observe_message(5, 130306, true, t + at);
            sched.poll(&mut reg, &mut node, t + at);
            assert!(drain_requests(&mut node, t + at).is_empty(), "before the global request: {at:?}");
        }
        sched.poll(&mut reg, &mut node, t + secs(3));
        assert_eq!(drain_requests(&mut node, t + secs(3)), vec![(255, 126996)]);
        for at in [ms(3300), ms(5000), ms(7999)] {
            reg.observe_message(5, 130306, true, t + at);
            sched.poll(&mut reg, &mut node, t + at);
            assert!(drain_requests(&mut node, t + at).is_empty(), "the global reply is due: {at:?}");
        }
        assert_eq!(reg.devices()[&5].identity_requests, 0);
        // Still no product information 5 s after the global request: ask the device itself.
        sched.poll(&mut reg, &mut node, t + secs(8));
        assert_eq!(drain_requests(&mut node, t + secs(8)), vec![(5, 126996)]);
    }

    #[test]
    fn address_claim_after_name_request_unblocks_product_request() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let t = t + secs(8);
        reg.observe_message(5, 130306, true, t);
        sched.poll(&mut reg, &mut node, t);
        assert_eq!(drain_requests(&mut node, t), vec![(5, 60928)]);
        // The device answers with its NAME: the product request follows on the next poll,
        // not 5 min later, but still 250 ms after the previous request.
        reg.observe_address_claim(5, N2kName::from_u64(77), t + ms(100));
        sched.poll(&mut reg, &mut node, t + ms(200));
        assert!(drain_requests(&mut node, t + ms(200)).is_empty(), "250 ms spacing");
        sched.poll(&mut reg, &mut node, t + ms(250));
        assert_eq!(drain_requests(&mut node, t + ms(250)), vec![(5, 126996)]);
        assert_eq!(reg.devices()[&5].identity_requests, 2);
    }

    #[test]
    fn lowest_address_first_and_one_per_call() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let t = t + secs(8);
        reg.observe_message(9, 130306, true, t);
        reg.observe_message(7, 130306, true, t);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t); // spacing: ignored
        assert_eq!(drain_requests(&mut node, t), vec![(7, 60928)]);
        sched.poll(&mut reg, &mut node, t + ms(250));
        assert_eq!(drain_requests(&mut node, t + ms(250)), vec![(9, 60928)]);
    }

    #[test]
    fn retries_every_5_min_max_3() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let first = t + secs(8);
        let mut sent = Vec::new();
        for minute in [0u64, 5, 10, 15, 20, 25] {
            let now = first + secs(minute * 60);
            // The device keeps chatting (so it is never Gone) but never identifies itself.
            reg.observe_message(5, 130306, true, now);
            sched.poll(&mut reg, &mut node, now);
            sent.push(drain_requests(&mut node, now).len());
            // Between retries: nothing.
            let mid = now + secs(120);
            reg.observe_message(5, 130306, true, mid);
            sched.poll(&mut reg, &mut node, mid);
            assert!(drain_requests(&mut node, mid).is_empty());
        }
        assert_eq!(sent, vec![1, 1, 1, 1, 0, 0]);
        assert_eq!(reg.devices()[&5].identity_requests, 4);
    }

    #[test]
    fn nothing_before_claim() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = N2kNode::new(&cfg(), t0);
        node.drain(t0);
        let mut sched = RequestScheduler::new();
        reg.observe_message(5, 130306, true, t0);

        sched.poll(&mut reg, &mut node, t0 + ms(500));
        assert!(node.drain(t0 + ms(500)).is_empty());
        assert_eq!(reg.devices()[&5].identity_requests, 0);
        assert_eq!(reg.devices()[&5].last_identity_request, None);
    }

    #[test]
    fn known_device_not_asked() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        let t = t + secs(8);
        reg.observe_message(5, 130306, true, t);
        reg.observe_address_claim(5, N2kName::from_u64(77), t);
        reg.observe_product_info(5, product());
        for step in 0..4 {
            let now = t + secs(step * 400);
            reg.observe_message(5, 130306, true, now);
            sched.poll(&mut reg, &mut node, now);
            assert!(drain_requests(&mut node, now).is_empty());
        }
        assert_eq!(reg.devices()[&5].identity_requests, 0);
    }

    #[test]
    fn gone_devices_and_own_address_are_skipped() {
        let t0 = Instant::now();
        let mut reg = registry(t0);
        let mut node = claimed_node(&reg, t0);
        let mut sched = RequestScheduler::new();
        let t = t0 + ms(2300);
        sched.poll(&mut reg, &mut node, t);
        sched.poll(&mut reg, &mut node, t + secs(3));
        node.drain(t + secs(3));

        reg.observe_message(5, 130306, true, t0); // last heard > 60 s ago by now
        reg.observe_message(OWN, 130306, true, t + secs(60));
        let now = t + secs(70);
        sched.poll(&mut reg, &mut node, now);
        assert!(drain_requests(&mut node, now).is_empty());
        assert_eq!(reg.devices()[&5].identity_requests, 0);
        assert_eq!(reg.devices()[&OWN].identity_requests, 0);
    }
}
