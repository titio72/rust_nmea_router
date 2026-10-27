---
name: add-pgn
description: Use when adding or extending NMEA2000 PGN support in the nmea2k crate - a new pgn<NUMBER>.rs decoder, a new N2kMessage variant, new fields on an existing PGN struct, or checking a decoder against the canboat definition.
---

# Add a PGN decoder to nmea2k

## Overview

The canboat database, read through its `keel` tool, is the source of truth for a PGN: bit
offsets, signedness, resolution, the "not available" sentinels, lookups, and real sample frames.
Do not work from `pgns.json` or memory. Its gaps are how the crate got wrong sentinels
(0xFFFF on signed fields) and wrong time units.

## Step 1 — Get the spec

```bash
python3 .claude/skills/add-pgn/pgn_spec.py <PGN>      # canboat at ~/dev/canboat, or --canboat DIR / $CANBOAT_DIR
```

It prints, per variant, the struct and file name, Single or Fast, the length, every field
(bit offset and length, signedness, resolution, unit, range, raw N/A/OOR/reserved values,
lookup) with a suggested Rust read expression, the lookup values, and the canboat sample frames
with their expected decodes. If it says the file already exists, extend that file. If it lists
several variants (proprietary PGNs), ask the user which device or manufacturer they want.

If keel is not built: `cd ~/dev/canboat && cargo build --release -p keel`.

## Step 2 — Write `nmea2k/src/pgns/pgn<NUMBER>.rs`

Copy the shape of a close existing decoder: `pgn130312.rs` (byte-aligned, single value),
`pgn129026.rs` (several optional values), `pgn129038.rs` (bit-packed, raw fields plus `get_*`).

| Concern | Rule |
|---|---|
| Struct | `#[derive(Debug, Clone)]`; `pub pgn: u32` first; `#[allow(dead_code)]` only on internal fields (sid) |
| Units | SI as canboat gives them: m/s, rad, K, Pa, m. Never knots or degrees in the struct; add `*_knots()`/`*_degrees()` helpers instead |
| Length guard | `if data.len() < <length> { return None; }` |
| Sentinels | Every numeric read goes through `use super::sentinel::Available;` (`.available()`). The script's `rust:` line already does this |
| Main value missing | When the message exists to carry one value (or one inseparable pair, like lat/lon or wind speed+angle) and it is N/A, `from_bytes` returns `None` and keeps a plain `f64` field. Use `?`: `u16::from_le_bytes(..).available()? as f64 * 0.01` |
| Independent values | Several values that are useful on their own become `Option<f64>`. Return `None` only if all of them are missing |
| Lat/lon | Wrap in `latitude(..)` / `longitude(..)` from `sentinel` (range check; AIS 91°/181°) |
| Date + time | `N2kDateTime { date, time }` with `time` in **seconds** (`raw as f64 * 0.0001`); `Option<N2kDateTime>` built with `date.available().zip(time.available())` |
| Lookups | Enum in the same file with a `Display` impl and an `Unknown(u8)` or catch-all variant; match on the raw code |
| Strings | `extract_text_from_bytes(data, byte, len)` from `bit_utils` |
| Bit-packed | `read_bits` / `read_signed_bits` from `bit_utils`; odd widths filter against the lowest sentinel (the script prints it) |
| Repeating sets | Decode into a `Vec` of a small struct; the count field gives the length |
| `Display` | One line with the key fields and units; print `Option`s with `fmt_opt(value, decimals)` |
| `new()` | Only if something outside the decoder constructs it (tests elsewhere, broadcasters) |

## Step 3 — Wire it in

1. `nmea2k/src/pgns/mod.rs`: `pub mod pgn<NUMBER>;` in numeric order, plus `pub use` of the struct and any enums used outside.
2. `nmea2k/src/pgns/message.rs`: import, `N2kMessage` variant, `from_pgn` arm (`.unwrap_or(N2kMessage::Unknown(pgn, data.to_vec()))`), `Display` arm.
3. Type `Fast` or `Mixed`: add the PGN to `is_fast_packet_pgn` in `nmea2k/src/stream_reader.rs`, or every frame is discarded.

Decoding stops here. Making the router use it (monitors, SignalK, UDP, DB) is a separate change. Ask before doing it.

## Step 4 — Tests (inline `#[cfg(test)] mod tests`)

- One test per canboat sample: paste its `data [..]` bytes as the fixture, cite the yaml file in a comment, and assert every decoded field matches `expects` (`~` means `None`, or `from_bytes` returns `None` for a main value).
- A "not available" test for each main value and each `Option` field, if no sample covers it.
- A too-short test: `vec![0u8; length - 1]` gives `None`.
- No samples (common for Fast PGNs): build the bytes from the spec's offsets. For AIS or proprietary payloads, ask the user for a captured frame instead of inventing one.

## Step 5 — Finish

- `cargo build -p nmea2k` with no new warnings; `cargo test -p nmea2k`; `cargo build --all-targets`.
- `AGENTS.md`: add to the numbered NMEA2000 message list. `docs/APPLICATION_SPECS.md`: add a row to the PGN support table (PGN, struct, description, interval from the spec).

## Common mistakes

| Mistake | Consequence |
|---|---|
| Checking `0xFFFF` on a signed 16-bit field | N/A is `0x7FFF`; 0x7FFF decodes as a real value (set humidity read 131 %) |
| Comparing only `== MAX` | Out-of-range (MAX-1) and reserved (MAX-2) pass as readings; use `.available()` |
| `* 100` with an integer resolution | Doesn't compile against `f64`; write `100.0` |
| Storing time as raw 0.0001 s units | ZDA output was garbage; `N2kDateTime.time` is seconds |
| Forgetting `is_fast_packet_pgn` | The PGN silently never decodes on the bus |
| Inventing a test frame for AIS/proprietary data | The test passes against your own misreading; use a canboat sample or a capture |
