//! PDU-aware helpers for NMEA2000 29-bit CAN identifiers.
//!
//! `nmea2000::Identifier::pgn()` returns `(id >> 8) & 0x3FFFF`, so for PDU1 (addressed) frames the
//! destination address stays in the PGN's low byte. The helpers here follow ISO 11783 instead:
//! PDU1 PGNs have the destination split out, PDU2 PGNs keep both PF and PS.

use embedded_can::ExtendedId;

/// Destination address meaning "all nodes".
pub const GLOBAL_ADDRESS: u8 = 255;
/// Source address of a node that has not (or could not) claim an address.
pub const NULL_ADDRESS: u8 = 254;

/// PF values below this are PDU1 (addressed); at or above are PDU2 (broadcast).
const PDU2_MIN_PF: u32 = 240;

fn pdu_format(raw: u32) -> u32 {
    (raw >> 16) & 0xFF
}

/// PGN of a frame, with the destination byte zeroed for PDU1 frames.
pub fn pgn_of(id: ExtendedId) -> u32 {
    let raw = id.as_raw();
    let data_page = (raw >> 24) & 0x3;
    let pf = pdu_format(raw);
    if pf < PDU2_MIN_PF {
        (data_page << 16) | (pf << 8)
    } else {
        (data_page << 16) | (pf << 8) | ((raw >> 8) & 0xFF)
    }
}

/// Destination address: PS for PDU1 frames, [`GLOBAL_ADDRESS`] for PDU2 frames.
pub fn destination_of(id: ExtendedId) -> u8 {
    let raw = id.as_raw();
    if pdu_format(raw) < PDU2_MIN_PF {
        ((raw >> 8) & 0xFF) as u8
    } else {
        GLOBAL_ADDRESS
    }
}

/// Source address of a frame.
pub fn source_of(id: ExtendedId) -> u8 {
    (id.as_raw() & 0xFF) as u8
}

/// Build a 29-bit identifier. `destination` is used only for PDU1 PGNs and ignored for PDU2.
/// `priority` is masked to 3 bits and `pgn` to 18 bits.
pub fn build_id(priority: u8, pgn: u32, destination: u8, source: u8) -> ExtendedId {
    let pgn = pgn & 0x3FFFF;
    let pf = (pgn >> 8) & 0xFF;
    let ps = if pf < PDU2_MIN_PF { destination as u32 } else { pgn & 0xFF };
    let raw = ((priority as u32 & 0x7) << 26) | ((pgn & 0x30000) << 8) | (pf << 16) | (ps << 8) | source as u32;
    ExtendedId::new(raw).expect("29-bit identifier by construction")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_address_claim_is_60928() {
        let id = ExtendedId::new(0x18EEFF05).unwrap(); // prio 6, PGN 60928 -> 255, src 5
        assert_eq!(pgn_of(id), 60928);
        assert_eq!(destination_of(id), 255);
        assert_eq!(source_of(id), 5);
    }

    #[test]
    fn addressed_request_keeps_destination() {
        let id = ExtendedId::new(0x18EA0564).unwrap(); // 59904 to 5 from 100
        assert_eq!(pgn_of(id), 59904);
        assert_eq!(destination_of(id), 5);
        assert_eq!(source_of(id), 100);
    }

    #[test]
    fn pdu2_pgn_unchanged() {
        let id = ExtendedId::new(0x09F80116).unwrap(); // 129025 from 22
        assert_eq!(pgn_of(id), 129025);
        assert_eq!(destination_of(id), GLOBAL_ADDRESS);
    }

    #[test]
    fn build_id_round_trips() {
        for (p, pgn, d, s) in [(6, 59904, 5, 100), (6, 60928, 255, 100), (7, 126993, 255, 100), (6, 126996, 255, 100)] {
            let id = build_id(p, pgn, d, s);
            assert_eq!(pgn_of(id), pgn);
            assert_eq!(source_of(id), s);
            let is_pdu1 = ((pgn >> 8) & 0xFF) < 240;
            assert_eq!(destination_of(id), if is_pdu1 { d } else { GLOBAL_ADDRESS });
            assert_eq!(id.as_raw() >> 26, p as u32);
        }
    }
}
