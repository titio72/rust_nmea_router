# NMEA2000 Bus Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A live web page listing every device on the NMEA2000 bus (identity, liveness, PGNs sent, whether the source filter uses or drops them), backed by the router becoming a polite, address-claimed N2K node that asks devices for their identity.

**Architecture:** The `nmea2k` crate gains PDU-aware identifier helpers, network-management PGN codecs (59904, 60928, 126996, 126993, 126464, 59392), a fast-packet encoder and a CAN write function in both backends. A new router module `src/bus_explorer/` holds pure, `now`-driven logic: `BusRegistry` (what is on the bus), `N2kNode` (address claim + responses + heartbeat), `RequestScheduler` (identity requests), and a `BusExplorer` facade the router loop calls before the source filter and after message assembly. The registry is shared with the web layer as `Arc<Mutex<BusRegistry>>`; `GET /api/bus/devices` returns a snapshot rendered by `static/bus-explorer.html`.

**Tech Stack:** Rust (workspace: `nmea_router` + internal `nmea2k` crate), socketcan 3 (Linux) / mock backend, axum, serde, vanilla JS page with `shared-theme.js` / `shared.css`.

**Spec:** `docs/superpowers/specs/2026-11-14-bus-explorer-design.md` — read it before starting any task.

## Global Constraints

- **Git: never run `git add`, `git commit` or `git push`.** The owner reviews and commits. Each task ends at "tests pass" — there is no commit step.
- Read `AGENTS.md` §Rules, §Code Style & Conventions, §Common Patterns, §Code Hygiene, §UI structure before writing code.
- Business logic never calls `Instant::now()`; callers pass `now: Instant` (AGENTS.md).
- `router_loop.rs` and `main.rs` contain no `#[cfg(target_os)]`; both CAN backends export identical signatures.
- The existing pipeline must behave byte-for-byte as today: `should_process_frame_by_id` receives the same `Identifier`, the same frames reach `N2kStreamReader` and `process_n2k_message`.
- Transmit failures never reopen the socket, never exit, never block the receive loop.
- Nothing is persisted to MariaDB or disk.
- Thresholds from the spec: gone = **60 s** without a frame; warnings suppressed for the first **60 s** after start; listen **2 s** before claiming; wait **250 ms** after claim before other traffic; identity retry every **5 min**, **max 3 retries**; outgoing requests spaced **≥ 250 ms**; heartbeat every **60 s**; page polls every **2 s**.
- Router NAME: unique number from config, manufacturer code **2046**, device instance 0, device function **130**, device class **25**, system instance **15**, industry group **4**, arbitrary-address-capable **1**.
- Config changes update `config.rs` defaults + validation, `config.example.json`, and docs (owner's standing checklist).
- New PGN decoders go through the `add-pgn` skill and are checked against canboat.
- New endpoint uses the `add-api-endpoint` skill; new page uses the `add-ui-page` skill.

## Review Focus

1. **Source 255 in `pgn_source_map`** (the documented "always reject" value) → no red warning for that PGN; the owner rejects it on purpose. Test in Task 8.
2. **Frames from source 254 (cannot-claim nodes) or 255** → never become device rows, never count as "heard" for address selection, never panic. Test in Task 7.
3. **Fast-packet PGNs** (126996 is 20 frames, 129029 is 7) → rate and counts reflect messages, not CAN frames. Test in Task 11.
4. **Malformed identity frames** — 60928 shorter than 8 bytes, 59904 shorter than 3, 126996 shorter than 134, strings padded with `0xFF`/`@`/`0x00` or non-UTF-8 → ignored or cleaned, no panic. Tests in Tasks 2 and 3.
5. **Preferred address already in use** (heard within 60 s, including a device that only sent an address claim) → the router never sends a claim from that address; it takes the next silent one upward, wrapping, within 0–251. Test in Task 9.

---

## File Structure

```
nmea2k/src/
  n2k_id.rs                 NEW  PDU-aware pgn/destination/source extraction, identifier builder
  fast_packet_writer.rs     NEW  payload -> Vec<[u8; 8]> fast-packet segmentation
  pgns/pgn59904.rs          NEW  ISO Request
  pgns/pgn60928.rs          NEW  ISO Address Claim + N2kName
  pgns/pgn126996.rs         NEW  Product Information
  pgns/pgn126993.rs         NEW  Heartbeat (encode only)
  pgns/pgn126464.rs         NEW  PGN List (encode only)
  pgns/pgn59392.rs          NEW  ISO Acknowledgement (encode only)
  pgns/mod.rs, pgns/message.rs, lib.rs, stream_reader.rs   MODIFY
  canbus.rs, canbus_mock.rs MODIFY  write_nmea2k_frame
src/
  bus_explorer/mod.rs       NEW  BusExplorer facade, SharedBusRegistry, OutFrame
  bus_explorer/registry.rs  NEW  BusRegistry, DeviceEntry, DeviceStatus
  bus_explorer/warnings.rs  NEW  compute_warnings
  bus_explorer/names.rs     NEW  PGN / manufacturer / class / function names
  bus_explorer/snapshot.rs  NEW  serde views + build_snapshot
  bus_explorer/node.rs      NEW  N2kNode (address claim, responses, heartbeat)
  bus_explorer/requests.rs  NEW  RequestScheduler
  config.rs, router_loop.rs, main.rs, app_metrics.rs, web/api.rs, web/server.rs   MODIFY
static/bus-explorer.html    NEW
static/js/shared-theme.js   MODIFY  nav entry
config.example.json, docs/REST_API.md, docs/SOURCE_FILTER.md, docs/ARCHITECTURE.md, docs/APPLICATION_SPECS.md   MODIFY
```

---

### Task 1: PDU-aware identifier helpers (nmea2k)

The external `nmea2000::Identifier::pgn()` returns `(id >> 8) & 0x3FFFF`, so PDU1 frames keep the destination in the PGN (global Address Claim reads 61183). These helpers are used by everything new; existing code keeps using `Identifier`.

**Files:**
- Create: `nmea2k/src/n2k_id.rs`
- Modify: `nmea2k/src/lib.rs` (add `pub mod n2k_id;`)

**Interfaces:**
- Produces:
  - `pub const GLOBAL_ADDRESS: u8 = 255;` `pub const NULL_ADDRESS: u8 = 254;`
  - `pub fn pgn_of(id: ExtendedId) -> u32` — PDU1 (PF < 240): `(DP<<16)|(PF<<8)`, low byte zeroed; PDU2: PF and PS both kept.
  - `pub fn destination_of(id: ExtendedId) -> u8` — PS for PDU1, `GLOBAL_ADDRESS` for PDU2.
  - `pub fn source_of(id: ExtendedId) -> u8`
  - `pub fn build_id(priority: u8, pgn: u32, destination: u8, source: u8) -> ExtendedId` — for PDU1 PGNs puts `destination` in PS; for PDU2 ignores it.

- [ ] **Step 1: Write failing tests** in `n2k_id.rs` `#[cfg(test)] mod tests`:

```rust
#[test] fn global_address_claim_is_60928() {
    let id = ExtendedId::new(0x18EEFF05).unwrap();       // prio 6, PGN 60928 -> 255, src 5
    assert_eq!(pgn_of(id), 60928);
    assert_eq!(destination_of(id), 255);
    assert_eq!(source_of(id), 5);
}
#[test] fn addressed_request_keeps_destination() {
    let id = ExtendedId::new(0x18EA0564).unwrap();       // 59904 to 5 from 100
    assert_eq!(pgn_of(id), 59904);
    assert_eq!(destination_of(id), 5);
    assert_eq!(source_of(id), 100);
}
#[test] fn pdu2_pgn_unchanged() {
    let id = ExtendedId::new(0x09F80116).unwrap();       // 129025 from 22
    assert_eq!(pgn_of(id), 129025);
    assert_eq!(destination_of(id), GLOBAL_ADDRESS);
}
#[test] fn build_id_round_trips() {
    for (p, pgn, d, s) in [(6, 59904, 5, 100), (6, 60928, 255, 100), (7, 126993, 255, 100), (6, 126996, 255, 100)] {
        let id = build_id(p, pgn, d, s);
        assert_eq!(pgn_of(id), pgn);
        assert_eq!(source_of(id), s);
        let is_pdu1 = ((pgn >> 8) & 0xFF) < 240;
        assert_eq!(destination_of(id), if is_pdu1 { d } else { GLOBAL_ADDRESS });
        assert_eq!(id.as_raw() >> 26, p as u32);
    }
}
```

- [ ] **Step 2:** `cargo test -p nmea2k n2k_id` → FAIL (module missing).
- [ ] **Step 3:** Implement the four functions and constants.
- [ ] **Step 4:** `cargo test -p nmea2k n2k_id` → PASS.

---

### Task 2: ISO Request (59904) and Address Claim (60928) codecs (nmea2k)

Use the `add-pgn` skill for both; verify layouts against canboat.

**Files:**
- Create: `nmea2k/src/pgns/pgn59904.rs`, `nmea2k/src/pgns/pgn60928.rs`
- Modify: `nmea2k/src/pgns/mod.rs` (modules + re-exports), `nmea2k/src/pgns/message.rs` (variants `IsoRequest`, `AddressClaim` in `N2kMessage` and `from_pgn` arms for 59904 and 60928; `Display` arms)

**Interfaces:**
- Produces:
  - `pub struct IsoRequest { pub requested_pgn: u32 }` — `from_bytes(&[u8]) -> Option<Self>` (needs ≥ 3 bytes, 24-bit LE); `to_bytes(&self) -> [u8; 3]`.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub struct N2kName { pub unique_number: u32, pub manufacturer_code: u16, pub device_instance_lower: u8, pub device_instance_upper: u8, pub device_function: u8, pub device_class: u8, pub system_instance: u8, pub industry_group: u8, pub arbitrary_address_capable: bool }`
    - `from_u64(raw: u64) -> Self`, `to_u64(&self) -> u64`; `impl Ord/PartialOrd` by `to_u64()` (lower value = higher priority).
    - Bit layout (LSB first): unique number 0–20, manufacturer 21–31, instance lower 32–34, instance upper 35–39, function 40–47, reserved 48 (encode as 1), class 49–55, system instance 56–59, industry group 60–62, AAC 63.
  - `pub struct AddressClaim { pub name: N2kName }` — `from_bytes(&[u8]) -> Option<Self>` (needs ≥ 8 bytes, u64 LE); `to_bytes(&self) -> [u8; 8]`.

- [ ] **Step 1: Write failing tests**:

```rust
// pgn60928.rs
#[test] fn name_round_trips_router_name() {
    let n = N2kName { unique_number: 271828, manufacturer_code: 2046, device_instance_lower: 0,
        device_instance_upper: 0, device_function: 130, device_class: 25, system_instance: 15,
        industry_group: 4, arbitrary_address_capable: true };
    assert_eq!(N2kName::from_u64(n.to_u64()), n);
    let claim = AddressClaim { name: n };
    assert_eq!(AddressClaim::from_bytes(&claim.to_bytes()).unwrap().name, n);
}
#[test] fn name_field_positions() {
    let raw: u64 = (1u64 << 63) | (4u64 << 60) | (15u64 << 56) | (25u64 << 49) | (130u64 << 40) | (2046u64 << 21) | 271828;
    let n = N2kName::from_u64(raw);
    assert_eq!((n.industry_group, n.system_instance, n.device_class, n.device_function, n.manufacturer_code, n.unique_number),
               (4, 15, 25, 130, 2046, 271828));
}
#[test] fn router_name_loses_to_typical_device() {
    // EV-1-like: industry 4, system instance 0, class 60, AAC 1 -> lower NAME wins
    let ev1 = N2kName { unique_number: 1, manufacturer_code: 1851, device_instance_lower: 0, device_instance_upper: 0,
        device_function: 140, device_class: 60, system_instance: 0, industry_group: 4, arbitrary_address_capable: true };
    let router = N2kName { system_instance: 15, device_class: 25, device_function: 130, manufacturer_code: 2046, unique_number: 271828, ..ev1 };
    assert!(ev1 < router);
}
#[test] fn short_claim_rejected() { assert!(AddressClaim::from_bytes(&[0u8; 7]).is_none()); }
// pgn59904.rs
#[test] fn request_round_trip() {
    assert_eq!(IsoRequest { requested_pgn: 126996 }.to_bytes(), [0x14, 0xF0, 0x01]);
    assert_eq!(IsoRequest::from_bytes(&[0x00, 0xEE, 0x00]).unwrap().requested_pgn, 60928);
    assert!(IsoRequest::from_bytes(&[0x00, 0xEE]).is_none());
}
```

- [ ] **Step 2:** `cargo test -p nmea2k pgn60928 pgn59904` → FAIL.
- [ ] **Step 3:** Implement per the interfaces above.
- [ ] **Step 4:** `cargo test -p nmea2k` → all PASS (existing tests unaffected).

---

### Task 3: Product Information (126996) codec and encode-only Heartbeat / PGN List / ISO Ack (nmea2k)

**Files:**
- Create: `nmea2k/src/pgns/pgn126996.rs`, `pgn126993.rs`, `pgn126464.rs`, `pgn59392.rs`
- Modify: `pgns/mod.rs`, `pgns/message.rs` (variant `ProductInformation` + `from_pgn` arm for 126996 only; the three encode-only types get no variant)

**Interfaces:**
- Produces:
  - `pub struct ProductInformation { pub nmea2000_version: Option<u16>, pub product_code: Option<u16>, pub model_id: String, pub software_version: String, pub model_version: String, pub serial_code: String, pub certification_level: Option<u8>, pub load_equivalency: Option<u8> }` — layout: version u16 (×0.001), product code u16, four 32-byte strings, cert level u8, LEN u8 = 134 bytes. `from_bytes(&[u8]) -> Option<Self>` (needs ≥ 134; strings cleaned with the existing text helper in `bit_utils`, which strips `0xFF`/`@`/space/`0x00` padding; invalid UTF-8 replaced lossily); `to_bytes(&self) -> Vec<u8>` (exactly 134 bytes, strings truncated to 32 and padded with `0xFF`; `None` numerics encoded as all-ones).
  - `pub struct Heartbeat { pub interval_ms: u32, pub sequence: u8 }` — `to_bytes(&self) -> [u8; 8]` per canboat 126993 (transmit interval, sequence counter, controller states "error active", equipment status "operational", remaining bits 1).
  - `pub enum PgnListFunction { Transmit = 0, Receive = 1 }`; `pub struct PgnList { pub function: PgnListFunction, pub pgns: Vec<u32> }` — `to_bytes(&self) -> Vec<u8>` (function byte, then 3-byte LE PGNs).
  - `pub struct IsoAck { pub control: u8, pub group_function: u8, pub pgn: u32 }` with `IsoAck::nak(pgn: u32) -> Self` (control 1, group function 0xFF) — `to_bytes(&self) -> [u8; 8]` (control, group fn, three `0xFF`, 24-bit PGN).

- [ ] **Step 1: Write failing tests**:

```rust
#[test] fn product_info_round_trip() {
    let p = ProductInformation { nmea2000_version: Some(2100), product_code: Some(1), model_id: "NMEA Router".into(),
        software_version: "0.1.0".into(), model_version: "".into(), serial_code: "271828".into(),
        certification_level: Some(0), load_equivalency: Some(1) };
    let bytes = p.to_bytes();
    assert_eq!(bytes.len(), 134);
    let d = ProductInformation::from_bytes(&bytes).unwrap();
    assert_eq!((d.model_id.as_str(), d.software_version.as_str(), d.serial_code.as_str()), ("NMEA Router", "0.1.0", "271828"));
}
#[test] fn product_info_cleans_padding() {
    let mut b = vec![0xFFu8; 134];
    b[4..4 + 5].copy_from_slice(b"i70s@");                 // '@' then 0xFF padding
    b[36..36 + 4].copy_from_slice(&[b'3', b'.', 0xC3, 0x28]); // invalid UTF-8 tail
    let d = ProductInformation::from_bytes(&b).unwrap();
    assert_eq!(d.model_id, "i70s");
    assert!(d.software_version.starts_with("3."));
    assert_eq!(d.nmea2000_version, None);
}
#[test] fn product_info_too_short() { assert!(ProductInformation::from_bytes(&[0u8; 133]).is_none()); }
#[test] fn pgn_list_layout() {
    let l = PgnList { function: PgnListFunction::Transmit, pgns: vec![60928, 126996] };
    assert_eq!(l.to_bytes(), vec![0, 0x00, 0xEE, 0x00, 0x14, 0xF0, 0x01]);
}
#[test] fn nak_layout() { assert_eq!(IsoAck::nak(130306).to_bytes(), [1, 0xFF, 0xFF, 0xFF, 0xFF, 0x02, 0xFD, 0x01]); }
```
Heartbeat: one test asserting the sequence byte position and interval field per canboat (the implementer writes the expected bytes from the canboat definition).

- [ ] **Step 2:** `cargo test -p nmea2k pgn126996 pgn126464 pgn59392 pgn126993` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p nmea2k` → PASS.

---

### Task 4: Fast-packet encoder and public fast-packet PGN check (nmea2k)

**Files:**
- Create: `nmea2k/src/fast_packet_writer.rs`
- Modify: `nmea2k/src/lib.rs` (module + re-exports), `nmea2k/src/stream_reader.rs` (move the `matches!` list into `pub fn is_fast_packet_pgn(pgn: u32) -> bool` at module level, add 126464 to it; the method delegates to it)

**Interfaces:**
- Produces:
  - `pub fn encode_fast_packet(payload: &[u8], sequence: u8) -> Vec<[u8; 8]>` — frame 0: `[(seq & 7) << 5 | 0, len, 6 bytes]`; frame n: `[(seq & 7) << 5 | n, 7 bytes]`; last frame padded with `0xFF`. Panics in debug only if `payload.len() > 223` (callers never exceed it).
  - `pub fn is_fast_packet_pgn(pgn: u32) -> bool` re-exported from `nmea2k`.

- [ ] **Step 1: Write failing tests**:

```rust
#[test] fn product_info_reassembles() {
    let payload: Vec<u8> = (0..134u8).collect();
    let frames = encode_fast_packet(&payload, 3);
    assert_eq!(frames.len(), 20);
    assert_eq!(frames[0][0], 3 << 5);
    assert_eq!(frames[0][1], 134);
    let mut reader = N2kStreamReader::new();
    let id = crate::n2k_id::build_id(6, 126996, 255, 100);
    let out: Vec<_> = frames.iter().filter_map(|f| reader.process_frame(id, f)).collect();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].data, payload);
}
#[test] fn last_frame_padded() {
    let frames = encode_fast_packet(&[1, 2, 3, 4, 5, 6, 7], 0);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1], [1, 7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
}
#[test] fn fast_packet_list_includes_identity_pgns() {
    assert!(is_fast_packet_pgn(126996) && is_fast_packet_pgn(126464) && !is_fast_packet_pgn(60928));
}
```

- [ ] **Step 2:** `cargo test -p nmea2k fast_packet` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test -p nmea2k` → PASS (existing captured-frame tests in `stream_reader.rs` still pass).

---

### Task 5: CAN transmit in both backends (nmea2k)

**Files:**
- Modify: `nmea2k/src/canbus.rs`, `nmea2k/src/canbus_mock.rs`

**Interfaces:**
- Produces: `pub fn write_nmea2k_frame(socket: &CanSocket, id: ExtendedId, data: &[u8]) -> Result<(), std::io::Error>` in both. Linux: build a `socketcan::CanFrame` from the extended id and data (`InvalidInput` error if `data.len() > 8`), `socket.write_frame(&frame)`. Mock: `Ok(())` for `data.len() <= 8`, `InvalidInput` otherwise.

- [ ] **Step 1: Write failing tests** in `canbus_mock.rs`: `write_accepts_up_to_8_bytes` (`Ok` for 8 bytes) and `write_rejects_9_bytes` (`ErrorKind::InvalidInput`). Linux backend: no unit test (needs vcan), consistent with the existing file.
- [ ] **Step 2:** The mock is compiled only off Linux, so on this Linux machine verify with `cargo check -p nmea2k` after Step 3 and run the mock tests on macOS if available; otherwise note it in the hand-off.
- [ ] **Step 3:** Implement both.
- [ ] **Step 4:** `cargo build --workspace` and `cargo test -p nmea2k` → PASS.

---

### Task 6: `can.node` configuration

**Files:**
- Modify: `src/config.rs` (new `NodeConfig`, field `node` on `CanConfig` with `#[serde(default)]`, validation in `validate_and_fix`, the `CanConfig { .. }` literal in tests ~line 819), `config.example.json`, `docs/APPLICATION_SPECS.md` (config reference near the `source_filter` block), `docs/ARCHITECTURE.md` (config table ~line 343)

**Interfaces:**
- Produces: `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct NodeConfig { pub transmit_enabled: bool, pub preferred_address: u8, pub unique_number: u32 }` with defaults `true`, `100`, `271828`; `pub const MAX_UNIQUE_NUMBER: u32 = 0x1F_FFFF;`. Accessed as `config.can.node`.

- [ ] **Step 1: Write failing tests** in `config::tests`:
  - `node_defaults_when_absent`: parsing `{"can": {"interface": "can0", "enabled": true}, ...minimal}` yields `transmit_enabled == true`, `preferred_address == 100`, `unique_number == 271828`.
  - `node_invalid_values_reverted`: `preferred_address: 252` → 100; `unique_number: 0x200000` → 271828 after `validate_and_fix` (warn and fix, like the other validators).
  - `node_explicit_values_kept`: `{"transmit_enabled": false, "preferred_address": 120, "unique_number": 42}` round-trips.
- [ ] **Step 2:** `cargo test config::tests::node` → FAIL.
- [ ] **Step 3:** Implement; add the `node` block to `config.example.json` under `can`; document the three fields and that `transmit_enabled: false` makes the explorer listen-only.
- [ ] **Step 4:** `cargo test config::tests` → PASS; `./target/debug/nmea_router --validate-config` with `config.example.json` copied to a scratch dir → accepted.

---

### Task 7: `BusRegistry` — device observation and status

**Files:**
- Create: `src/bus_explorer/mod.rs` (module declarations only for now), `src/bus_explorer/registry.rs`
- Modify: `src/main.rs` (`mod bus_explorer;`)

**Interfaces:**
- Consumes: `nmea2k::pgns::{N2kName, ProductInformation}`, `nmea2k::n2k_id::{NULL_ADDRESS, GLOBAL_ADDRESS}`.
- Produces (`registry.rs`):
  - `pub const GONE_AFTER: Duration = Duration::from_secs(60);`
  - `pub const NETWORK_MANAGEMENT_PGNS: [u32; 6] = [59392, 59904, 60928, 126464, 126993, 126996];`
  - `pub enum DeviceStatus { InUse, NotUsed, Dropped, Gone }` (precedence Gone > Dropped > InUse > NotUsed)
  - `pub struct PgnStats { pub first_seen: Instant, pub last_seen: Instant, pub count: u64, pub interval_ewma: Option<Duration>, pub last_accepted: bool, pub last_dropped: Option<Instant>, pub last_used: Option<Instant> }` — EWMA α = 0.2 over inter-arrival times.
  - `pub struct DeviceEntry { pub first_heard: Instant, pub last_heard: Instant, pub pgns: BTreeMap<u32, PgnStats>, pub name: Option<N2kName>, pub product: Option<ProductInformation>, pub moved_to: Option<u8>, pub identity_requests: u8, pub last_identity_request: Option<Instant> }`
  - `pub struct BusRegistry` with:
    - `pub fn new(started: Instant, own_name: N2kName) -> Self`
    - `pub fn started(&self) -> Instant`, `pub fn own_name(&self) -> N2kName`
    - `pub fn observe_message(&mut self, source: u8, pgn: u32, accepted: bool, now: Instant)` — one call per **message** (facade guarantees); sources 254/255 ignored.
    - `pub fn observe_decoded(&mut self, source: u8, pgn: u32, known: bool, now: Instant)` — sets `last_used = now` when `known` and pgn ∉ `NETWORK_MANAGEMENT_PGNS`.
    - `pub fn observe_address_claim(&mut self, source: u8, name: N2kName, now: Instant)` — if another address holds the same NAME, set its `moved_to = Some(source)` and move `product` to the new entry.
    - `pub fn observe_product_info(&mut self, source: u8, info: ProductInformation)`
    - `pub fn status(&self, source: u8, now: Instant) -> Option<DeviceStatus>` — Dropped if any `last_dropped` within 60 s; InUse if any `last_used` within 60 s.
    - `pub fn is_address_silent(&self, address: u8, now: Instant) -> bool` — true if never heard or last heard > 60 s ago.
    - `pub fn devices(&self) -> &BTreeMap<u8, DeviceEntry>`, `pub fn device_mut(&mut self, source: u8) -> Option<&mut DeviceEntry>`
    - `pub fn last_sent(&self, source: u8, pgn: u32) -> Option<Instant>`
    - `pub fn set_node_state(&mut self, own_address: Option<u8>, label: &'static str)`, `pub fn node_state(&self) -> (Option<u8>, &'static str)`

- [ ] **Step 1: Write failing tests** (`registry.rs` tests, all with a fixed `t0 = Instant::now()` and offsets):
  - `new_source_is_not_used_until_decoded`: `observe_message(5, 130306, true, t0)` → `status(5, t0) == Some(NotUsed)`; then `observe_decoded(5, 130306, true, t0)` → `InUse`.
  - `network_management_never_counts_as_use`: decoded known 126996 → still `NotUsed`.
  - `rejected_message_is_dropped`: `observe_message(41, 129025, false, t0)` → `Dropped`, even if 41 also has an `InUse` PGN.
  - `gone_after_60s`: last heard `t0`, `status(5, t0 + 61 s) == Some(Gone)`; at `t0 + 59 s` not gone.
  - `ignores_null_and_global_sources`: messages from 254 and 255 → `devices()` empty and `is_address_silent(254, t0)`.
  - `rate_reflects_message_interval`: messages at `t0`, `+1 s`, `+2 s` → `interval_ewma` ≈ 1 s (±50 ms).
  - `address_move_keeps_identity`: claim at 22 with NAME N + product P, later claim at 41 with N → entry 22 `moved_to == Some(41)`, entry 41 has `product` P.
  - `claim_only_device_is_not_silent`: `observe_address_claim(100, n, t0)` → `!is_address_silent(100, t0 + 10 s)`.
- [ ] **Step 2:** `cargo test bus_explorer::registry` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test bus_explorer::registry` → PASS.

---

### Task 8: Warnings, names, and API snapshot

**Files:**
- Create: `src/bus_explorer/warnings.rs`, `src/bus_explorer/names.rs`, `src/bus_explorer/snapshot.rs`

**Interfaces:**
- Consumes: `BusRegistry` (Task 7), `crate::config::SourceFilterConfig`.
- Produces:
  - `names.rs`: `pub fn pgn_name(pgn: u32) -> Option<&'static str>` (every PGN decoded by nmea2k plus the network-management set, names taken from `pgns.json` at dev time — not loaded at runtime); `pub fn manufacturer_name(code: u16) -> Option<&'static str>` (at least Raymarine, Garmin, Navico, B&G, Simrad, Actisense, Yacht Devices, Maretron, Furuno — codes verified against canboat's `MANUFACTURER_CODE` lookup); `pub fn device_class_name(class: u8) -> Option<&'static str>`; `pub fn device_function_name(class: u8, function: u8) -> Option<&'static str>` (canboat `DEVICE_FUNCTION`, at least the classes present on the owner's bus: 25, 30, 40, 60, 75, 85, 120).
  - `warnings.rs`: `#[derive(Serialize)] pub enum WarningLevel { Red, Amber }` (serialized lowercase); `#[derive(Serialize)] pub struct BusWarning { pub level: WarningLevel, pub pgn: u32, pub configured_source: Option<u8>, pub actual_sources: Vec<u8>, pub message: String }`; `pub const REQUIRED_PGNS: [u32; 3] = [129025, 129026, 126992];` `pub fn compute_warnings(reg: &BusRegistry, filter: &SourceFilterConfig, now: Instant) -> Vec<BusWarning>`.
    - Red: for each `(pgn, cfg)` in `pgn_source_map` with `cfg != 255`: `last_sent(cfg, pgn)` is none or > 60 s old **and** ≥ 1 other source sent it within 60 s. Message: `"{Name} ({pgn}) arrives only from src {a, b}, but the filter expects src {cfg} — it is being dropped."` (`{Name}` = `pgn_name` or `"PGN {pgn}"`).
    - Amber: for each pgn in `REQUIRED_PGNS`: no source sent it within 60 s. Message: `"No device is sending {Name} ({pgn})."`
    - Nothing at all while `now - reg.started() < 60 s`.
  - `snapshot.rs`: serde views and `pub fn build_snapshot(reg: &BusRegistry, filter: &SourceFilterConfig, can_enabled: bool, now: Instant) -> BusSnapshot` where
    - `BusSnapshot { can_enabled: bool, uptime_s: f64, node: NodeView { transmit_enabled: bool, own_address: Option<u8>, state: &'static str }, warnings: Vec<BusWarning>, devices: Vec<DeviceView> }`
    - `DeviceView { source: u8, is_self: bool, status: &'static str /* "in_use"|"not_used"|"dropped"|"gone"|"self" */, last_heard_s: Option<f64>, manufacturer: Option<String>, model: Option<String>, device_class: Option<String>, device_function: Option<String>, software_version: Option<String>, identity: &'static str /* "known"|"asking"|"no_reply" */, moved_to: Option<u8>, name: Option<NameView>, product: Option<ProductView>, pgns: Vec<PgnView> }`
    - `PgnView { pgn: u32, name: Option<&'static str>, interval_s: Option<f64>, last_seen_s: f64, accepted: bool, used: bool, also_accepted_from: Vec<u8> }` — `also_accepted_from` lists other sources whose same PGN was accepted and used within 60 s (the "router is mixing sources" note).
    - `NameView` / `ProductView` mirror every field of `N2kName` / `ProductInformation`; unknown manufacturer renders as `"code {n}"`.
    - `identity`: `"known"` when both name and product are present; `"no_reply"` when `identity_requests >= 4` (initial + 3 retries) and still missing; else `"asking"`.
    - The router's own row (`is_self: true`, `status: "self"`, `model: "this router"`) appears when `node_state().0` is `Some`.
    - Devices sorted by source address.
  - `NodeView.transmit_enabled` comes from the node state label (`"disabled"` ⇒ false).

- [ ] **Step 1: Write failing tests**:
  - `warnings::red_when_gps_moves` — filter `{129025: 22}`, registry: src 41 sends 129025 at `t0 + 70 s`, src 22 never → one Red warning, `actual_sources == [41]`, `configured_source == Some(22)`, message contains `"src 41"` and `"expects src 22"`.
  - `warnings::no_red_for_reject_all_255` — filter `{129025: 255}`, src 41 sending → no Red warning.
  - `warnings::no_red_when_configured_source_alive` — 22 and 41 both send within 60 s → no Red.
  - `warnings::amber_for_missing_required_pgn` — at `t0 + 70 s` nobody sent 126992 → Amber for 126992; optional 127250 missing → no warning.
  - `warnings::suppressed_during_first_minute` — same as red case at `t0 + 30 s` → empty.
  - `names::known_manufacturers` — `manufacturer_name(1851) == Some("Raymarine")`, `manufacturer_name(229) == Some("Garmin")`, `manufacturer_name(9999) == None`.
  - `snapshot::mixing_sources_noted` — 0 and 12 both accepted + used 127250 → each `PgnView` for 127250 lists the other in `also_accepted_from`.
  - `snapshot::identity_states` — 0 requests → `"asking"`; 4 requests no reply → `"no_reply"`; name + product → `"known"`.
  - `snapshot::serializes` — `serde_json::to_value(build_snapshot(..))` has keys `can_enabled, uptime_s, node, warnings, devices`.
- [ ] **Step 2:** `cargo test bus_explorer::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test bus_explorer::` → PASS.

---

### Task 9: `N2kNode` — address claim, responses, heartbeat

**Files:**
- Create: `src/bus_explorer/node.rs`
- Modify: `src/bus_explorer/mod.rs` (add `pub struct OutFrame { pub id: ExtendedId, pub data: Vec<u8> }` — one CAN frame, ≤ 8 bytes)

**Interfaces:**
- Consumes: Tasks 1–4 codecs, `NodeConfig` (Task 6), `BusRegistry::is_address_silent` (Task 7).
- Produces:
  - `pub fn router_name(unique_number: u32) -> N2kName` (values from Global Constraints).
  - `pub fn router_product_info(unique_number: u32) -> ProductInformation` — model id `"NMEA Router"`, software version `env!("CARGO_PKG_VERSION")`, serial = unique number as decimal, NMEA2000 version 2100, product code 1, cert level 0, LEN 1.
  - `pub const TRANSMIT_PGNS: [u32; 6] = [59392, 59904, 60928, 126464, 126993, 126996];` `pub const RECEIVE_PGNS: [u32; 3] = [59904, 60928, 126996];`
  - `pub struct N2kNode` with:
    - `pub fn new(cfg: &NodeConfig, now: Instant) -> Self` — if `transmit_enabled`, state `Listening { until: now + 2 s }` and queues a global ISO Request for 60928 from source 254; else `Disabled` and never queues anything.
    - `pub fn tick(&mut self, reg: &BusRegistry, now: Instant)` — Listening expired → pick `preferred_address` if `reg.is_address_silent`, else next silent upward wrapping within 0..=251; queue Address Claim from it, state `Claiming { address, since: now }`; none free → queue claim from 254 (Cannot Claim), state `CannotClaim`. Claiming for ≥ 250 ms → `Claimed`. Claimed → heartbeat every 60 s (first one when entering Claimed).
    - `pub fn on_address_claim(&mut self, source: u8, name: N2kName, reg: &BusRegistry, now: Instant)` — `name == own` from any source → `NameConflict` (log `error!` naming `can.node.unique_number`), outbox cleared, silent forever. Claim at own address with a different NAME while Claiming/Claimed → move to next silent address (excluding the current one) and re-claim, or Cannot Claim.
    - `pub fn on_iso_request(&mut self, source: u8, destination: u8, requested_pgn: u32)` — only when `Claimed` and `destination` is own address or 255: 60928 → claim; 126996 → product info (fast packet, addressed by our source, global destination); 126464 → transmit list then receive list (fast packet); other PGN and `destination == own` → `IsoAck::nak` to `source`; other PGN global → nothing.
    - `pub fn send_request(&mut self, destination: u8, pgn: u32) -> bool` — queues an ISO Request from own address; `false` (nothing queued) unless `Claimed`.
    - `pub fn address(&self) -> Option<u8>` — `Some` only when `Claimed`.
    - `pub fn state_label(&self) -> &'static str` — `"disabled" | "listening" | "claiming" | "claimed" | "cannot_claim" | "name_conflict"`.
    - `pub fn drain(&mut self) -> Vec<OutFrame>`, `pub fn discard_outbox(&mut self)`
  - Priorities: 6 for claim, request, product info, PGN list, NAK; 7 for heartbeat. Fast-packet sequence counter increments per message (mod 8).

- [ ] **Step 1: Write failing tests** (decode the `OutFrame`s with Task 1 helpers to assert PGN/source/destination):
  - `listens_before_claiming` — `new` at t0 → first drained frame is 59904 from 254 to 255 requesting 60928; `tick(t0 + 1 s)` drains nothing.
  - `claims_preferred_when_silent` — `tick(t0 + 2 s)` → one 60928 frame from 100; `address() == None` at `+2.1 s`, `Some(100)` at `+2.3 s`.
  - `skips_busy_preferred_address` — registry heard 100 and 101 at `t0 + 1 s` → claim from 102. (Review Focus 5)
  - `wraps_and_skips_claim_only_device` — preferred 251, registry has claim-only device at 251 and traffic at 0 → claim from 1.
  - `yields_on_any_conflicting_claim` — claimed 100; `on_address_claim(100, lower_name)` and separately `(100, higher_name)` → both move to next silent address with a new 60928 frame.
  - `cannot_claim_when_full` — all 0..=251 heard → claim frame from 254, `state_label() == "cannot_claim"`, `send_request` returns false.
  - `identical_name_silences` — `on_address_claim(37, own_name)` → `"name_conflict"`, `drain()` empty afterwards even after `tick`.
  - `answers_requests` — claimed: global request 60928 → one claim frame; request 126996 to us → 20 frames from 100; request 126464 → frames for both lists; request 130306 to us → NAK to requester; request 130306 global → nothing; request to 55 → nothing.
  - `silent_until_claimed` — any `on_iso_request` while Listening → nothing.
  - `heartbeat_every_60s` — claimed at t; heartbeats at t and t + 60 s, none at t + 30 s; priority 7.
  - `disabled_never_transmits` — `transmit_enabled: false` → `drain()` empty across ticks and requests; `state_label() == "disabled"`.
- [ ] **Step 2:** `cargo test bus_explorer::node` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test bus_explorer::node` → PASS.

---

### Task 10: `RequestScheduler` — identity requests

**Files:**
- Create: `src/bus_explorer/requests.rs`

**Interfaces:**
- Consumes: `BusRegistry` (`devices`, `device_mut`), `N2kNode::send_request`, `N2kNode::address`.
- Produces: `pub struct RequestScheduler` with `pub fn new() -> Self` and `pub fn poll(&mut self, reg: &mut BusRegistry, node: &mut N2kNode, now: Instant)` — queues **at most one** request per call and only if ≥ 250 ms since the last one:
  1. Once, as soon as the node is claimed: global 60928; then global 126996 no earlier than 3 s after it.
  2. Otherwise, the lowest-address device missing name or product whose `last_identity_request` is none, or ≥ 5 min ago, and whose `identity_requests < 4`: addressed request for the missing PGN (60928 first if name missing, else 126996); increments `identity_requests` and sets `last_identity_request` only when `send_request` returned true.
  - Devices with status Gone and the router's own address are skipped.

- [ ] **Step 1: Write failing tests**:
  - `global_requests_after_claim` — claimed node; polls at t, t + 0.3 s, t + 3 s → global 60928 first, global 126996 not before t + 3 s.
  - `new_device_gets_one_addressed_request` — after globals, device 5 with no identity → one 59904 to 5 for 60928; next poll 0.1 s later → nothing (spacing).
  - `retries_every_5_min_max_3` — device never replies → requests at T, T + 5 min, T + 10 min, T + 15 min, none at T + 20 min; `identity_requests == 4`.
  - `nothing_before_claim` — node Listening → no requests, counters untouched.
  - `known_device_not_asked` — device with name and product → never requested.
- [ ] **Step 2:** `cargo test bus_explorer::requests` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test bus_explorer::requests` → PASS.

---

### Task 11: `BusExplorer` facade and router-loop wiring

**Files:**
- Modify: `src/bus_explorer/mod.rs`, `src/router_loop.rs` (`RouterLoop` field + `new` param, `run()`, `run_can_pipeline` signature), `src/main.rs` (create registry, pass to pipeline and web), `src/app_metrics.rs` (`can_tx_frames`, `can_tx_errors` counters, included in `log()`/`reset()`)

**Interfaces:**
- Consumes: everything from Tasks 1–10.
- Produces (`mod.rs`):
  - `pub type SharedBusRegistry = Arc<Mutex<BusRegistry>>;`
  - `pub fn new_shared_registry(started: Instant, cfg: &NodeConfig) -> SharedBusRegistry` (own name = `router_name(cfg.unique_number)`).
  - `pub struct BusExplorer` with `pub fn new(registry: SharedBusRegistry, cfg: &NodeConfig, now: Instant) -> Self` (owns `N2kNode`, `RequestScheduler`, a private `N2kStreamReader` used only for 126996), and:
    - `pub fn on_frame(&mut self, id: ExtendedId, data: &[u8], accepted: bool, now: Instant)` — call for **every** frame **before** the filter decision is acted on. Uses `pgn_of`/`source_of`/`destination_of`. Calls `observe_message` only on message starts (non-fast-packet PGN, or fast-packet frame with `data[0] & 0x1F == 0`). 60928 (≥ 8 bytes, source ≠ 254) → registry + node; 59904 (≥ 3 bytes) → node; 126996 → private reader → registry. Independent of `accepted`.
    - `pub fn on_decoded(&mut self, frame: &N2kFrame, now: Instant)` — `observe_decoded(source, pgn, !matches!(message, N2kMessage::Unknown(..)), now)`.
    - `pub fn poll_outgoing(&mut self, now: Instant) -> Vec<OutFrame>` — if the gap since the previous call is > 1 s (loop stalled), `discard_outbox()` and hold all transmission for 1 s while backlogged frames are read; otherwise `node.tick`, `scheduler.poll`, `set_node_state`, then `node.drain()`.
  - Lock the registry only inside these methods, never across a CAN write.
- Router loop changes in `run()`:
  - compute `accepted = should_process_frame_by_id(&self.config, id)` once, call `self.bus_explorer.on_frame(extended_id, &data, accepted, Instant::now())`, then `if !accepted { continue; }` (same frames continue as before).
  - right after `self.reader.process_frame` yields a frame: `self.bus_explorer.on_decoded(&n2k_frame, now)`.
  - in the periodic section: write each `OutFrame` with `CanBus::write_nmea2k_frame`; count `can_tx_frames` / `can_tx_errors`; on error `warn!` at most once per 60 s (field `last_tx_warn: Option<Instant>`).
- `main.rs`: `let bus_registry = bus_explorer::new_shared_registry(Instant::now(), &config.can.node);` before the web thread; pass a clone into `start_web_server` (Task 12) and into `run_can_pipeline(.., bus_registry)`.

- [ ] **Step 1: Write failing tests** in `bus_explorer/mod.rs` (feed raw frames, inspect the shared registry):
  - `fast_packet_counts_messages_not_frames` — 3 complete 129029 messages (7 frames each) from src 0 at 1 s spacing → `pgns[129029].count == 3`, interval ≈ 1 s. (Review Focus 3)
  - `identity_learned_even_if_filtered` — feed 126996 frames from src 3 with `accepted = false` → `product` set for 3.
  - `malformed_identity_ignored` — 60928 with 6 bytes, 59904 with 2 bytes → no panic, no name recorded. (Review Focus 4)
  - `stall_discards_and_holds` — claimed explorer; `poll_outgoing(t)`, then `poll_outgoing(t + 5 s)` → empty; `poll_outgoing(t + 5.5 s)` → empty; `poll_outgoing(t + 6.1 s)` → resumes.
- [ ] **Step 2:** `cargo test bus_explorer::tests` → FAIL.
- [ ] **Step 3:** Implement facade, then wire `router_loop.rs`, `main.rs`, `app_metrics.rs`.
- [ ] **Step 4:** `cargo test` → all non-DB tests PASS; `cargo clippy --workspace --all-targets` → no new warnings.

---

### Task 12: `GET /api/bus/devices`

Use the `add-api-endpoint` skill.

**Files:**
- Modify: `src/web/api.rs` (`AppState.bus_registry: SharedBusRegistry`, handler, route in `create_api_router` for both read-only and normal mode, the four test `AppState { .. }` literals ~lines 2367, 3437, 3462, 4302), `src/web/server.rs` (`start_web_server` gains `bus_registry: SharedBusRegistry`), `src/main.rs` (pass it), `docs/REST_API.md`

**Interfaces:**
- Consumes: `build_snapshot` (Task 8), `SharedBusRegistry` (Task 11).
- Produces: `pub async fn get_bus_devices(State(state): State<AppState>) -> Json<ApiResponse<BusSnapshot>>` at `.route("/bus/devices", get(get_bus_devices))` — locks the registry (poison-tolerant like `AppState::db`), calls `build_snapshot(&reg, &state.config.can.source_filter, state.config.can.enabled, Instant::now())`.

- [ ] **Step 1: Write failing tests** with the existing `create_test_app` / `create_test_app_read_only` helpers:
  - `test_get_bus_devices_empty` — both apps: status 200, `data.devices` is an empty array, `data.warnings` empty (inside first minute).
  - `test_get_bus_devices_lists_device` — app whose registry has src 5 observed → `data.devices[0].source == 5`, `status == "not_used"`.
- [ ] **Step 2:** `cargo test web::api::tests::test_get_bus_devices` → FAIL.
- [ ] **Step 3:** Implement; document the endpoint and response shape in `docs/REST_API.md`.
- [ ] **Step 4:** `cargo test` → PASS.

---

### Task 13: Bus Explorer page

Use the `add-ui-page` skill. Layout = spec option A (mockup: `.superpowers/brainstorm/4061922-1794641205/content/layout.html`, option A).

**Files:**
- Create: `static/bus-explorer.html`
- Modify: `static/js/shared-theme.js` (nav entry `{ href: '/bus-explorer.html', label: 'Bus Explorer', page: 'bus-explorer', roHidden: true }` after SignalK Browser), `docs/SOURCE_FILTER.md` (short "Checking the filter" section pointing to the page and its red warning)

**Interfaces:**
- Consumes: `GET /api/bus/devices` JSON (Task 12).

- [ ] **Step 1:** Build the page: header line (`N devices · updated N s ago`, node state + own address), warning banners (red before amber, one line each, `message` verbatim), device table (Src, Manufacturer / Model, Class, SW, Last heard, Router), click a row to expand PGNs (name, rate as `"N Hz"` below 1 s interval else `"every N s"`, used/dropped marker, "also accepted from src N" note) plus NAME and product details. Gone rows greyed; moved rows show `moved to src N`; unknown identity shows `unknown (asking…)` / `unknown (asked, no reply)`; `can_enabled: false` shows "CAN disabled" instead of the table. Poll every 2 s; keep expanded rows expanded across refreshes.
- [ ] **Step 2:** Verify in a browser against the running router (`cargo run` with a vcan0 config if no boat bus; with `cangen vcan0 -e` or replayed frames if available): page loads in light and dark themes, nav entry present, no console errors, layout fits at phone width.
- [ ] **Step 3:** `cargo test` → PASS (no Rust regressions).

---

### Task 14: Final verification

- [ ] **Step 1:** `cargo test` → all non-DB tests PASS.
- [ ] **Step 2:** `cargo clippy --workspace --all-targets` → no new warnings.
- [ ] **Step 3:** `cargo build --release` → OK; `./target/release/nmea_router --validate-config` with `config.example.json` → OK.
- [ ] **Step 4:** If a cross toolchain is available: `cargo check --workspace --all-targets --target aarch64-unknown-linux-gnu` → OK (Pi target). Report whether the macOS mock backend was compiled (Task 5).
- [ ] **Step 5:** On vcan0 with `candump vcan0` running: router start shows 59904 from 254, then after 2 s a 60928 from 100 (or the next free address), a heartbeat, a global 60928 request and a global 126996 request ≥ 3 s later. Report the captured lines to the owner.
- [ ] **Step 6:** Stop. Summarise changes for the owner; do not stage or commit.
