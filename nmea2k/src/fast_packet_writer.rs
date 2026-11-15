//! NMEA2000 fast-packet encoder: the transmit-side counterpart of the reassembly done by
//! [`crate::stream_reader::N2kStreamReader`].

/// Largest payload a fast-packet message can carry (32 frames: 6 + 31 * 7 bytes).
pub const MAX_FAST_PACKET_PAYLOAD: usize = 223;

/// Split `payload` into 8-byte CAN frames.
///
/// Frame 0 is `[(seq & 7) << 5 | 0, len, 6 payload bytes]`; frame n is
/// `[(seq & 7) << 5 | n, 7 payload bytes]`. The last frame is padded with `0xFF`.
/// Callers never pass more than 223 bytes (checked in debug builds only).
pub fn encode_fast_packet(payload: &[u8], sequence: u8) -> Vec<[u8; 8]> {
    debug_assert!(
        payload.len() <= MAX_FAST_PACKET_PAYLOAD,
        "fast-packet payload too long: {}",
        payload.len()
    );
    let seq = (sequence & 0x07) << 5;
    let mut frames = Vec::new();

    let first_len = payload.len().min(6);
    let mut first = [0xFFu8; 8];
    first[0] = seq;
    first[1] = payload.len() as u8;
    first[2..2 + first_len].copy_from_slice(&payload[..first_len]);
    frames.push(first);

    for (n, chunk) in payload[first_len..].chunks(7).enumerate() {
        let mut frame = [0xFFu8; 8];
        frame[0] = seq | ((n + 1) as u8 & 0x1F);
        frame[1..1 + chunk.len()].copy_from_slice(chunk);
        frames.push(frame);
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{is_fast_packet_pgn, N2kStreamReader};

    #[test]
    fn product_info_reassembles() {
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

    #[test]
    fn last_frame_padded() {
        let frames = encode_fast_packet(&[1, 2, 3, 4, 5, 6, 7], 0);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1], [1, 7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn fast_packet_list_includes_identity_pgns() {
        assert!(is_fast_packet_pgn(126996) && is_fast_packet_pgn(126464) && !is_fast_packet_pgn(60928));
    }
}
