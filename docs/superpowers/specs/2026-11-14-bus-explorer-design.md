# NMEA2000 Bus Explorer — Design

Date: 2026-11-14
Status: reviewed, awaiting owner approval

## Intent

A live page showing every device on the NMEA2000 bus, from a vantage point that is **not** a Raymarine
device. It answers two questions the owner has been burned by:

1. **"Is it really there?"** — The i70s periodically fails to find the ITC-5 although it is on the bus
   (a Raymarine software bug). The explorer gives independent ground truth: if the ITC-5 shows up here,
   the wiring and the ITC-5 are fine and the i70s is the problem.
2. **"Is the router actually using it?"** — The router once silently ignored the GPS because
   `can.source_filter.pgn_source_map` pinned position PGNs to a source address the GPS no longer used
   (N2K devices can claim a different address at power-up). It took a while to diagnose. The explorer
   must make that situation obvious within seconds.

Success: opening the page on the boat, the owner can tell at a glance which devices are alive, what
they are, and whether the router is consuming or dropping their data — and gets a loud warning in the
"wrong source" case.

## Goals

- List every source address heard on the bus, with identity and liveness.
- Show, per device, which PGNs it sends and whether the router's source filter accepts or drops each.
- Warn loudly when the source filter is rejecting data the router needs.
- Actively query devices for their identity (router transmits on the bus).
- Lay the groundwork (a proper, address-claimed N2K node) for future features that configure
  Raymarine devices. **Those features are not part of this work.**

## Non-goals

- No persistence: nothing is written to MariaDB. State is in memory and resets on router restart.
- No editing of the source filter from the page; `config.json` remains the only way to change it.
- No Raymarine device configuration, no proprietary PGNs.
- No raw-frame sniffer / packet log view.
- No detection of a wrong device sitting at a configured source address (deferred, see *Warnings*).

## Anti-goals (would be a failure even if it "works")

- Anything that can disturb the bus: request storms, an address-claim fight with a real device, or
  transmitting from an address the router hasn't properly claimed. Being a polite, standards-compliant
  node matters more than identity completeness.
- Changing which frames the existing pipeline processes. The explorer observes; it must not alter
  filtering, recording, or broadcasting behavior.
- Transmit failures (bus off, buffer full) affecting the receive loop or crashing the router.

## Layout (owner chose option A, device-first)

Mockup reference: `.superpowers/brainstorm/*/content/layout.html`, option A.

New page `static/bus-explorer.html`, following AGENTS.md §UI structure (`header-bar`,
`level-1-container`, `shared-theme.js`, `shared.css`). Added to the nav in `static/js/shared-theme.js`
with `roHidden: true` (like Monitor / SignalK Browser — meaningless when CAN is disabled).

- **Header line:** device count, "updated N s ago".
- **Warning banner** (only when applicable), above the table — see *Warnings*.
- **Device table**, one row per source address heard:

  | Column | Content |
  |---|---|
  | Src | source address (0–253) |
  | Manufacturer / Model | from address claim (manufacturer code → name) and product info (model id); "unknown (asked, no reply)" or "unknown (asking…)" when not yet identified |
  | Class | device class / function from address claim, as names |
  | SW | software version from product info |
  | Last heard | age of the most recent frame from that source |
  | Router | status, see below |

- **Router status** per device:
  - **in use** — at least one of its PGNs passes the source filter **and** decodes to a known
    `N2kMessage` variant (not `Unknown`), excluding network-management PGNs (59392, 59904, 60928,
    126464, 126993, 126996). Whether the time-sync gate later skips the frame doesn't matter. When two
    sources of the same PGN are both accepted (e.g. heading from EV-1 and ACU), both are "in use" and
    the expanded rows show an info note "also accepted from src N" — the router is mixing sources. (my call)
  - **not used** — alive, but nothing it sends meets the "in use" rule (e.g. i70s)
  - **dropped** — it sends a PGN the source filter rejects *and that PGN is in `pgn_source_map`*
    (i.e. the router wanted that data, from someone else)
  - **gone** — no frame for 60 s. Gone devices stay in the list (greyed) until router restart.

  Precedence when several apply: gone > dropped > in use > not used.

- **Expanding a row** shows:
  - the PGNs that source sends: number + name, approximate rate (Hz or "every N s"), and
    used / dropped marking;
  - the full NAME from address claim: unique number, manufacturer code, device instance (lower/upper),
    device function, device class, system instance, industry group, arbitrary-address-capable;
  - product info: NMEA2000 version, product code, model id, software version, model version, serial
    code, certification level, load equivalency.

- The router's own node appears as a row labelled "this router".
- Page refreshes by polling every 2 s (my call; SignalK WebSocket is not used for this).

## Warnings

- **Red (the GPS case):** for each PGN in `pgn_source_map`: the configured source has not sent that PGN
  in the last 60 s **and** at least one other source has. Text names the PGN, the source(s) actually
  sending it, and the configured source, e.g.
  "Position (129025) arrives only from src 41, but the filter expects src 22 — position is being dropped."
- **Amber:** for each *required* health stream (`Stream::is_required()`: Position 129025,
  COG/SOG 129026, System Time 126992): no source at all has sent that PGN in the last 60 s.
  (Optional streams are excluded so an absent-by-design sensor never nags.)
- **Known gap, deferred (owner: "a real case", later):** a *different* device taking over the configured
  address and sending the same PGN (e.g. the plotter's internal GPS landing on src 22) is **not**
  detected — the router silently uses the wrong sensor. Fixing it needs an expected device identity
  (NAME) per filtered PGN, i.e. an extension of `source_filter` in config. Keep the registry's NAME
  data structured so this can be added without reworking it.
- Neither warning is shown during the first 60 s after router start (my call — avoids false alarms
  before devices have been heard).

## How the router gets device identity

Identity comes from PGN 60928 (ISO Address Claim → NAME) and PGN 126996 (Product Information,
fast-packet). Devices send these at power-up or when asked via PGN 59904 (ISO Request). The owner
approved the router transmitting.

### The router becomes a proper N2K node

To transmit, the router must own a claimed source address (ISO 11783-5 address claim):

- **The router always yields — it must never take an address away from a real device.** (my call,
  from the anti-goal; nothing is persisted, so it re-discovers on every start.)
  - **Listen first:** on CAN start, send a global Request for Address Claim (59904 for 60928) from the
    null address 254, then listen for 2 s while building the registry.
  - **Claim only a free address:** `preferred_address` if no one answered from it and no traffic has
    been seen from it; otherwise the next address upward (wrapping, within 0–251) that is silent.
    Then send Address Claim and wait the standard 250 ms before sending anything else.
  - **Deliberately high NAME:** set system instance to 15 and the arbitrary-address-capable bit, so the
    router's NAME compares higher than any normal marine device and loses every contest.
  - **On a conflicting claim** for its address (from anyone, whatever the NAME): move to the next silent
    address and re-claim. If none, send Cannot Claim (source 254) and stop transmitting — the explorer
    keeps working passively.
  - **Identical NAME guard:** if a claim arrives with a NAME identical to the router's own, stop
    transmitting and log an error telling the owner to change `can.node.unique_number`.
- After a stall of the loop (see *Threading* below), process any pending address claims before
  transmitting anything.
- Respond to ISO Requests addressed to it or to global (255) for 60928 (Address Claim — mandatory) and
  126996 (Product Information, so the i70s/plotter list it by name).
- Respond to unknown requests addressed to it with ISO Acknowledgement NAK (PGN 59392) — builder may
  skip if non-trivial; note it in code if skipped.
- Send a Heartbeat (126993) every 60 s and answer PGN List requests (126464, transmit/receive lists)
  — standard node behaviour, so Raymarine displays don't show the router as flickering. (my call)

### Requesting identity

- At startup, after the claim settles: one global ISO Request for 60928, then one global request for
  126996 a few seconds later.
- When a new source address is first heard: a request for 126996 (and 60928 if not yet known)
  addressed to that source.
- For any device still unidentified: retry every 5 minutes, addressed to that device, **max 3 retries**,
  then mark "asked, no reply" and stop (my call, to keep the router polite).
- Global rate limit: at most one outgoing request every 250 ms (my call).
- Address changes: when an address claim arrives on a new address with a NAME the registry already
  knows, treat it as the same physical device that moved — keep identity, update the address, mark the
  old address entry as moved (show "moved to src N" in the row of the old address until it is gone).

## Architecture (fits existing code)

### nmea2k crate

- New decoders, via the `add-pgn` skill (validate against canboat):
  - `pgn60928.rs` — ISO Address Claim (8 bytes, single frame; decode the 64-bit NAME into its fields)
  - `pgn126996.rs` — Product Information (134 bytes, fast-packet; already in `is_fast_packet_pgn`)
  - `pgn59904.rs` — ISO Request (3 bytes; needed to recognise requests addressed to the router)
- Encoders for 59904, 60928, 126996 (and 59392 if implemented). 126996 needs fast-packet *transmit*
  (frame segmentation with sequence counter) — new code.
- **Transmit in both CAN backends**, same signature in each:
  `write_nmea2k_frame(socket: &CanSocket, id: ExtendedId, data: &[u8]) -> Result<(), std::io::Error>`.
  Linux: SocketCAN write. Mock: no-op returning `Ok(())` (the mock bus is silent, so nobody answers).
  Keep `router_loop`/`main` free of platform conditionals (CLAUDE.md §Platform support).
  Destination address goes into the PDU1 identifier for addressed PGNs (59904, 59392); add a helper
  for building identifiers with priority/PGN/destination/source.
- **PDU1 PGN extraction:** the external `nmea2000` crate's `Identifier::pgn()` returns
  `(id >> 8) & 0x3FFFF`, so for PDU1 frames (PF < 240) the destination byte stays in the PGN — a global
  Address Claim reads as 61183, a request to src 5 as 59909. The explorer needs its own PDU-aware
  `pgn()` / `destination()` extraction. **Do not change** what the existing `should_process_frame_by_id`
  call receives — the pipeline's filtering must stay byte-for-byte as it is.

### Router side

- New module `src/bus_explorer/` (name is builder's call), containing:
  - `BusRegistry` — per source address: first/last heard, per-PGN counters + last-seen for rate,
    accepted/dropped counts, NAME, product info, request state. Pure logic, takes `now: Instant`
    from the caller (AGENTS.md rule: never call `Instant::now()` inside business logic).
  - `N2kNode` — address-claim state machine and request scheduler; produces outgoing frames, does
    not own the socket. Also pure, unit-testable with synthetic frames and times.
- **Observation point:** in `RouterLoop::run()`, record every frame's `(source, pgn)` **before**
  `should_process_frame_by_id`, together with whether the filter accepted it. This is the only way to
  see dropped data.
- **Identity messages must reach the explorer regardless of the source filter.** Feed 60928 / 126996 /
  59904 frames to the explorer through its own path (it may need its own fast-packet assembler for
  126996, or the pipeline's reader can be used if the filter never applies to these PGNs — builder's
  call, but a user putting 126996 in `pgn_source_map` must not blind the explorer).
- Outgoing frames from `N2kNode` are written in the loop's periodic section (runs on every iteration,
  including read timeouts, so at ≥2 Hz). Write errors: count in `AppMetrics`, log rate-limited at
  `warn`, never reopen the socket or exit because of a write failure.
- Ignore frames whose source is the router's own address in the registry's "other devices" logic, but
  do show the router row.
- **Threading:** the node runs inside the existing router loop thread (no second socket). That loop
  does synchronous DB work and can stall for tens of seconds; during a stall the router neither
  transmits nor defends its address. Accepted: because the router always yields and transmits nothing
  while stalled, a stall can't cause a conflict. On resume, handle backlogged claims first (above).
  (my call)
- **Sharing with the web layer:** `Arc<Mutex<BusRegistry>>` created in `main.rs` and passed to both
  `run_can_pipeline` and `start_web_server` → `AppState`, the same pattern as `ais_cache`. Hold the
  lock briefly; build the API snapshot by cloning out.

### API

- `GET /api/bus/devices` (use the `add-api-endpoint` skill). Returns a JSON snapshot: generation time,
  router uptime, warnings (already computed server-side), and the device list with per-PGN details.
  Ages as seconds (f64) computed server-side from `Instant`s.
- PGN names and manufacturer names: resolved server-side. PGN names from the nmea2k crate where it
  knows them; otherwise from a small static table or "PGN NNNNN". Manufacturer codes from a static
  table generated from canboat (builder's call how complete — Raymarine, Garmin, Navico/B&G/Simrad,
  Actisense, Yacht Devices, Maretron, Furuno at minimum, else "code NNNN"). `pgns.json` is 1.3 MB and
  should not be loaded at runtime just for names.
- With CAN disabled: return 200 with an empty device list and a flag `can_enabled: false`; the page
  shows "CAN disabled".

### Configuration

New fields under `can` (my call on names; update `config.example.json`, `config.rs` defaults and
validation, and docs per the config-change checklist):

```json
"can": {
  "node": {
    "transmit_enabled": true,
    "preferred_address": 100,
    "unique_number": 1
  }
}
```

- `transmit_enabled: false` → fully passive explorer (no claim, no requests, no responses).
- NAME for the router: device class 25 (Inter/Intranetwork Device), function 130 (PC Gateway),
  industry group 4 (marine), system instance 15, arbitrary-address-capable = 1. Manufacturer code 2046
  (the usual DIY code). Default `unique_number` must **not** be 1 or any other default of the common
  ttlappalainen NMEA2000 library (owner's ESP32/Android-adjacent gear may use it) — pick a distinctive
  default and validate it fits the 21-bit field. (my call)
- Product info: model id "NMEA Router", software version = crate version, serial = unique number.

## Testing

- Decoder/encoder round-trip tests for 59904, 60928, 126996 with real captured frames if available
  (follow the existing captured-frame test style in `stream_reader.rs`).
- Fast-packet transmit: segment then reassemble with `N2kStreamReader` → identical payload.
- `N2kNode`: listen-first then claim only a silent address; any conflicting claim → move to next silent
  address; no free address → cannot-claim and silence; identical-NAME guard; respond to global and
  addressed requests, ignore requests addressed elsewhere; heartbeat cadence.
- PDU1 extraction: 60928 to global, 59904 to a specific address, PDU2 PGNs unaffected.
- Request scheduler: new device → one addressed request; retry cadence; max retries; global rate limit.
- `BusRegistry`: status precedence, gone after 60 s, address-move detection, both warning rules
  (including the 60 s startup suppression and required-streams-only amber rule).
- Linux cross-check: `cargo check --workspace --all-targets --target aarch64-unknown-linux-gnu`.

## Left to the builder

- Module and type names; whether identity frames use a dedicated fast-packet assembler.
- Exact rate display format; table styling within `shared.css` conventions.
- Completeness of manufacturer and PGN name tables beyond the minimum listed.
- Whether to implement the NAK response (document if skipped).

## Calls made by Claude (not explicitly discussed — owner to check)

- 60 s "gone" threshold; gone devices kept until restart.
- 2 s polling rather than WebSocket.
- Retry every 5 min, max 3 retries; 250 ms global request spacing.
- Startup warning suppression for 60 s.
- Amber warning limited to required health streams.
- Config shape under `can.node`, preferred address 100, PC Gateway NAME with system instance 15.
- Address-move tracking by NAME.
- The router also answers Product Information requests (so it shows up by name on the i70s/plotter).
- Router always yields its address: listen first, claim only silent addresses, deliberately high NAME.
- "In use" = accepted by the filter and decoded to a known message; mixing sources is shown as a note.
- Heartbeat every 60 s and PGN-list responses.
- Node stays in the router loop thread; DB stalls make it briefly deaf, which is safe because it yields.
