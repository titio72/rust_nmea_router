Add support for a new NMEA2000 PGN to the `nmea2k` crate. Given a PGN number, look up its definition in `pgns.json`, implement the Rust struct, wire it into the module, and write parse tests.

## Arguments
$ARGUMENTS is the PGN number to implement, e.g. `127257`

## Step 1 — Look up the PGN definition

Read `pgns.json` and find the entry where `"PGN": <number>`. Extract:

- `Description` — becomes the doc comment and struct name source
- `Type` — `"Single"` or `"Fast"` (Fast Packet messages are multi-frame)
- `Length` — minimum byte length for the `from_bytes` guard
- `Fields[]` — each field has: `Id`, `Name`, `BitOffset`, `BitLength`, `Type`, `Resolution`, `Signed`, `Units`, optional `EnumValues`

Derive the **struct name** from `Description` in PascalCase (e.g. "Vessel Heading" → `VesselHeading`).

## Step 2 — Create `nmea2k/src/pgns/pgn<NUMBER>.rs`

Follow these conventions exactly — study existing files before writing:

### Struct layout
```rust
use std::fmt;
// Add `use super::nmea2000_date_time::N2kDateTime;` if any Date/Time fields exist
// Add `use super::bit_utils::{read_bits, read_signed_bits};` if fields are NOT byte-aligned

#[derive(Debug, Clone)]
pub struct <StructName> {
    #[allow(dead_code)]
    pub pgn: u32,
    // Fields that are only used internally (sid, instance fields not consumed elsewhere):
    #[allow(dead_code)]
    <field>: <type>,
    // Publicly useful fields — no #[allow(dead_code)]:
    pub <field>: <type>,
}
```

### Field type mapping (from pgns.json `Type` + `Signed`)
| pgns.json Type | Rust field type | Notes |
|---|---|---|
| Integer, unsigned | `u8`/`u16`/`u32` | pick smallest that fits `BitLength` |
| Integer, signed | `i8`/`i16`/`i32` | pick smallest |
| Number, unsigned | `f64` | raw integer × Resolution |
| Number, signed | `f64` | raw signed integer × Resolution |
| Latitude / Longitude | `f64` | degrees, usually i64 × Resolution |
| Date | `u16` | days since 1970-01-01 |
| Time | `f64` | seconds in raw units (see N2kDateTime) |
| Lookup / EnumValues | `enum <Name>` + `u8` backing | see below |
| ASCII text | `String` | use `extract_text_from_bytes` |

### Enum fields
When a field has `EnumValues`, define a `#[derive(Debug, Clone)]` enum in the same file. Each variant is the value description in PascalCase. Always add a catch-all fallback.

### `from_bytes` method
- First line: `if data.len() < <Length> { return None; }`
- For **byte-aligned** fields (BitOffset divisible by 8, BitLength divisible by 8): use `u16::from_le_bytes([...])` / `i32::from_le_bytes([...])` etc.
- For **bit-packed** fields (any other BitOffset or BitLength): use `read_bits(data, <BitOffset>, <BitLength>)` (unsigned) or `read_signed_bits(...)` (signed), then multiply by Resolution.
- Date/Time field pairs → wrap in `N2kDateTime { date, time }`.
- Multiply raw integer by `Resolution` from pgns.json to get the physical value.

### `Display` impl
One-line human-readable summary. Show the most important fields with their units.

### Optional constructors
Add a `pub fn new(...)` constructor only if the struct is also constructed outside of parsing (e.g. for test fixtures or SignalK broadcasting).

## Step 3 — Wire into `nmea2k/src/pgns/mod.rs`

1. Add `pub mod pgn<NUMBER>;` in the `pub mod` block (keep numerically ordered).
2. `pub use pgn<NUMBER>::<StructName>;` in the re-exports section if the type is used outside the crate.

## Step 4 — Register Fast Packet PGNs (if `Type` is `"Fast"`)

If the pgns.json entry has `"Type": "Fast"`, the PGN must be registered in `nmea2k/src/stream_reader.rs` so the stream reader reassembles multi-frame CAN messages before dispatching. Without this, all frames for the PGN are silently discarded.

Open `stream_reader.rs` and find the `is_fast_packet_pgn` method. Add the new PGN number to the `matches!` arm, keeping the list numerically ordered:

```rust
fn is_fast_packet_pgn(&self, pgn: u32) -> bool {
    matches!(
        pgn,
        ... | <NUMBER> | ...
    )
}
```

Skip this step if `Type` is `"Single"`.

## Step 5 — Wire into `nmea2k/src/pgns/message.rs` (if applicable)

Look at `message.rs` to see if there is a `N2kMessage` enum or dispatch table. If the PGN should be decoded in the main loop, add a variant and the dispatch arm. If `message.rs` does not exist or does not use an enum, skip this step.

## Step 6 — Write tests (inline `#[cfg(test)]` module at bottom of the new file)

### Test strategy

**If you can simulate the frame from the field definitions:**

Calculate the expected byte sequence by hand from the pgns.json BitOffset/BitLength/Resolution values, constructing a minimal known-value payload. Write at least:
1. `test_<pgn>_from_bytes_known_values` — construct a byte slice with specific values, call `from_bytes`, assert every public field equals the expected decoded value.
2. `test_<pgn>_from_bytes_too_short` — pass a truncated slice, assert `None`.

To construct a test byte buffer:
- For each field, compute `raw = physical_value / Resolution` (round to integer).
- Write the raw integer into the correct byte positions using the BitOffset as the starting bit.
- For byte-aligned fields: `data[byte] = raw as u8`, `data[byte..byte+2].copy_from_slice(&(raw as u16).to_le_bytes())`, etc.
- For bit-packed fields: set individual bits using `data[bit/8] |= (bit_value << (bit%8))`.

**If the frame is too complex to simulate (e.g. AIS payloads, encryption, proprietary encodings):**

Ask the user: "I need a real CAN frame payload for PGN <N> to write accurate parse tests. Please capture one from your vessel using `candump` or the SignalK browser, and paste the hex bytes here."

Then write the test using the literal hex bytes provided.

### Test template
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_<snake_name>_from_bytes_known_values() {
        // <field>: <physical_value> → raw = <raw_value>, bytes = [<hex>]
        // <field>: <physical_value> → raw = <raw_value>, bytes = [<hex>]
        let data: Vec<u8> = vec![
            /* byte-by-byte with comments explaining each field */
        ];
        let msg = <StructName>::from_bytes(&data).unwrap();
        assert_eq!(msg.pgn, <NUMBER>);
        assert!((msg.<float_field> - <expected>).abs() < 1e-9);
        assert_eq!(msg.<int_field>, <expected>);
    }

    #[test]
    fn test_<snake_name>_from_bytes_too_short() {
        let data = vec![0u8; <Length - 1>];
        assert!(<StructName>::from_bytes(&data).is_none());
    }
}
```

## Checklist before finishing
- [ ] `cargo build -p nmea2k` passes with no warnings
- [ ] `cargo test -p nmea2k pgn<NUMBER>` passes
- [ ] If `Type` is `"Fast"`: PGN is registered in `is_fast_packet_pgn` in `stream_reader.rs`
- [ ] Field units match CLAUDE.md rules: internal fields in SI (m/s, radians, Kelvin, Pa); document any conversion needed at the call site
- [ ] No unused imports
- [ ] `#[allow(dead_code)]` only on fields that are genuinely internal (sid, integrity, etc.)
- [ ] Update `AGENTS.md` — add the new PGN to the **NMEA2000** supported messages list with its number, name, and the fields/data it contributes to the application
- [ ] Update `docs/APPLICATION_SPECS.md` — add a row to the appropriate PGN table (Navigation / Environmental / System) under **NMEA2000 Message Support** with: PGN number, struct name, one-line description, and typical update rate
