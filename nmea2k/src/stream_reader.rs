use nmea2000::{FastPacket, Identifier};
use socketcan::ExtendedId;
use std::collections::HashMap;

use crate::pgns::N2kMessage;

/// NMEA2000 Stream Reader
/// 
/// This module provides a stateful stream reader for NMEA2000 CAN frames.
/// It handles:
/// - Single-frame messages (decoded immediately)
/// - Fast packet messages (assembled from multiple frames)
/// 
/// # Usage
/// 
/// ```no_run
/// use nmea2k::N2kStreamReader;
/// use socketcan::ExtendedId;
/// 
/// let mut reader = N2kStreamReader::new();
/// 
/// // Example: Push frames into the reader
/// let can_id = ExtendedId::new(0x09F80001).unwrap();
/// let data = vec![0xC0, 0x0F, 0x7B, 0x26, 0x36, 0xD0, 0x86, 0x3A];
/// 
/// if let Some(complete_message) = reader.process_frame(can_id, &data) {
///     // A complete message is available
///     println!("PGN: {}", complete_message.identifier.pgn());
///     println!("Message: {}", complete_message.message);
/// }
/// ```
// Key for tracking multi-frame messages: (PGN, Source)
type FastPacketKey = (u32, u8);

struct FastPacketBuffer {
    frames: Vec<Vec<u8>>,
    total_len: usize,
    expected_frames: usize,
}

impl FastPacketBuffer {
    fn new(total_len: usize) -> Self {
        // First frame has 6 bytes of data (2 bytes overhead)
        // Subsequent frames have 7 bytes of data (1 byte overhead)
        let expected_frames = if total_len <= 6 {
            1
        } else {
            1 + (total_len - 6).div_ceil(7)
        };
        
        Self {
            frames: Vec::new(),
            total_len,
            expected_frames,
        }
    }
    
    fn add_frame(&mut self, frame_index: usize, frame_data: &[u8]) {
        // Explicit extraction of payload bytes:
        // Frame 0: bytes [2..8] = 6 bytes of payload
        // Frames 1+: bytes [1..8] = 7 bytes of payload
        let payload = if frame_index == 0 {
            // First frame: skip 2 bytes of header (sequence + length)
            if frame_data.len() >= 2 {
                frame_data[2..].to_vec()
            } else {
                frame_data.to_vec()
            }
        } else {
            // Subsequent frames: skip 1 byte of header (sequence + frame counter)
            if frame_data.len() >= 1 {
                frame_data[1..].to_vec()
            } else {
                frame_data.to_vec()
            }
        };
        
        self.frames.push(payload);
    }
    
    fn is_complete(&self) -> bool {
        self.frames.len() >= self.expected_frames
    }
    
    fn get_complete_data(&self) -> Vec<u8> {
        let mut data = Vec::new();
        for frame in &self.frames {
            data.extend_from_slice(frame);
        }
        // Truncate to actual message length
        data.truncate(self.total_len);
        data
    }
}

/// A decoded NMEA2000 message with metadata
pub struct N2kFrame {
    pub identifier: Identifier,
    pub message: N2kMessage,
    #[allow(dead_code)]
    pub is_fast_packet: bool,
    #[allow(dead_code)]
    pub data: Vec<u8>, // Complete assembled data
}

/// NMEA2000 stream reader that processes CAN frames and assembles fast packets
pub struct N2kStreamReader {
    fast_packet_buffers: HashMap<FastPacketKey, FastPacketBuffer>,
}

impl N2kStreamReader {
    /// Create a new NMEA2000 stream reader
    pub fn new() -> Self {
        Self {
            fast_packet_buffers: HashMap::new(),
        }
    }

    /// Process a CAN frame and return a complete message if available
    /// 
    /// # Arguments
    /// * `can_id` - The extended CAN ID
    /// * `data` - The CAN frame data
    /// 
    /// # Returns
    /// `Some(N2kFrame)` if a complete message is ready, `None` otherwise
    pub fn process_frame(&mut self, can_id: ExtendedId, data: &[u8]) -> Option<N2kFrame> {
        let identifier = Identifier::from_can_id(can_id);
        let pgn = identifier.pgn();
        
        // Check if this is a fast packet PGN
        if self.is_fast_packet_pgn(pgn) && data.len() == 8 {
            self.process_fast_packet(identifier, data)
        } else {
            // Regular single-frame message
            let message = N2kMessage::from_pgn(pgn, data);
            Some(N2kFrame {
                identifier,
                message,
                is_fast_packet: false,
                data: data.to_vec(),
            })
        }
    }

    fn process_fast_packet(&mut self, identifier: Identifier, data: &[u8]) -> Option<N2kFrame> {
        // Parse as FastPacket
        let mut packet_data = [0u8; 8];
        packet_data.copy_from_slice(data);
        let fast_packet = FastPacket(packet_data);
        
        let pgn = identifier.pgn();
        let source = identifier.source();
        let key = (pgn, source);
        
        if fast_packet.is_first() {
            // First frame - start new buffer
            if let Some(total_len) = fast_packet.total_len() {
                let mut buffer = FastPacketBuffer::new(total_len as usize);
                buffer.add_frame(0, &packet_data);
                
                if buffer.is_complete() {
                    // Single-frame fast packet
                    let complete_data = buffer.get_complete_data();
                    let message = N2kMessage::from_pgn(pgn, &complete_data);
                    return Some(N2kFrame {
                        identifier,
                        message,
                        is_fast_packet: true,
                        data: complete_data,
                    });
                } else {
                    self.fast_packet_buffers.insert(key, buffer);
                }
            }
        } else if let Some(buffer) = self.fast_packet_buffers.get_mut(&key) {
            // Subsequent frame - add to existing buffer
            let frame_index = buffer.frames.len();
            buffer.add_frame(frame_index, &packet_data);
            
            if buffer.is_complete() {
                let complete_data = buffer.get_complete_data();
                self.fast_packet_buffers.remove(&key);
                let message = N2kMessage::from_pgn(pgn, &complete_data);
                return Some(N2kFrame {
                    identifier,
                    message,
                    is_fast_packet: true,
                    data: complete_data,
                });
            }
        }
        
        None
    }

    fn is_fast_packet_pgn(&self, pgn: u32) -> bool {
        matches!(
            pgn,
            126996 | 127233 | 127237 | 127489 | 127493 | 127505 | 128275 | 129029
                | 129038 | 129039 | 129040 | 129041 | 129540 | 129793 | 129794 | 129809 | 129810
        )
    }
}

impl Default for N2kStreamReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pgns::N2kMessage;

    /// Build an ExtendedId encoding the given NMEA2000 priority, PGN, and source.
    ///
    /// Layout (29-bit CAN extended ID):
    ///   bits [28:26] = priority (3 bits)
    ///   bits [25:8]  = PGN (18 bits)
    ///   bits  [7:0]  = source address
    fn make_can_id(priority: u32, pgn: u32, source: u32) -> ExtendedId {
        let raw = (priority << 26) | ((pgn & 0x3FFFF) << 8) | (source & 0xFF);
        ExtendedId::new(raw).expect("invalid CAN extended ID")
    }

    /// Feed the four real CAN frames for one PGN 129039 fast-packet message into
    /// N2kStreamReader and verify that the assembled AisClassBPositionReport is
    /// decoded correctly.
    ///
    /// Frames captured on 2026-04-06 16:44:18 UTC (source 0, destination 255,
    /// priority 4):
    ///   60 1a 12 5f e4 d4 0d 3b   <- first frame (frame_no=0, total_len=26)
    ///   61 2e 1f 06 5c a0 08 1a   <- frame 1
    ///   62 43 ac 60 05 00 03 00   <- frame 2
    ///   63 00 ff ff 00 fc ff ff   <- frame 3
    ///
    /// Assembled 26-byte payload decodes to:
    ///   MMSI 232055903, lat ≈ 43.6773°N, lon ≈ 10.2707°E
    #[test]
    fn test_fast_packet_pgn129039_four_frames() {
        use crate::pgns::N2kMessage;

        let mut reader = N2kStreamReader::new();

        // CAN ID: priority=4, PGN=129039, source=0
        let can_id = make_can_id(4, 129039, 0);

        let frames: [&[u8]; 4] = [
            &[0x60, 0x1a, 0x12, 0x5f, 0xe4, 0xd4, 0x0d, 0x3b],
            &[0x61, 0x2e, 0x1f, 0x06, 0x5c, 0xa0, 0x08, 0x1a],
            &[0x62, 0x43, 0xac, 0x60, 0x05, 0x00, 0x03, 0x00],
            &[0x63, 0x00, 0xff, 0xff, 0x00, 0xfc, 0xff, 0xff],
        ];

        // Frames 1–3: message is incomplete, reader returns None
        assert!(reader.process_frame(can_id, frames[0]).is_none(), "frame 0 should be incomplete");
        assert!(reader.process_frame(can_id, frames[1]).is_none(), "frame 1 should be incomplete");
        assert!(reader.process_frame(can_id, frames[2]).is_none(), "frame 2 should be incomplete");

        // Frame 4 completes the message
        let result = reader.process_frame(can_id, frames[3]);
        assert!(result.is_some(), "frame 3 should complete the message");

        let frame = result.unwrap();
        assert_eq!(frame.identifier.pgn(), 129039);
        assert_eq!(frame.identifier.source(), 0);
        assert!(frame.is_fast_packet);

        match frame.message {
            N2kMessage::AisClassBPositionReport(ref report) => {
                assert_eq!(report.mmsi, 232055903, "unexpected MMSI");
                assert!(
                    (report.get_latitude_degrees() - 43.6772956).abs() < 0.00001,
                    "unexpected latitude: {}",
                    report.get_latitude_degrees()
                );
                assert!(
                    (report.get_longitude_degrees() - 10.2706747).abs() < 0.00001,
                    "unexpected longitude: {}",
                    report.get_longitude_degrees()
                );
            }
            other => panic!("expected AisClassBPositionReport, got {:?}", other),
        }
    }

    /// Feed the five real CAN frames for one PGN 129810 fast-packet message into
    /// N2kStreamReader and verify that the assembled AisClassBStaticDataPartB is
    /// decoded correctly.
    ///
    /// Frames captured on 2026-04-06 16:54:47 UTC (source 0, destination 255,
    /// priority 6):
    ///   c0 21 18 5f e4 d4 0d 25   <- first frame (frame_no=0, total_len=33)
    ///   c1 72 8a 15 54 2a 00 2e   <- frame 1
    ///   c2 4d 50 4e 42 32 40 40   <- frame 2
    ///   c3 c8 00 3c 00 1e 00 64   <- frame 3
    ///   c4 00 00 00 00 00 a7 ff   <- frame 4
    ///
    /// Assembled 33-byte payload decodes to:
    ///   MMSI 232055903, callsign "MPNB2", type_of_ship 37
    #[test]
    fn test_fast_packet_pgn129810_five_frames() {
        use crate::pgns::N2kMessage;

        let mut reader = N2kStreamReader::new();

        // CAN ID: priority=6, PGN=129810, source=0
        let can_id = make_can_id(6, 129810, 0);

        let frames: [&[u8]; 5] = [
            &[0xc0, 0x21, 0x18, 0x5f, 0xe4, 0xd4, 0x0d, 0x25],
            &[0xc1, 0x72, 0x8a, 0x15, 0x54, 0x2a, 0x00, 0x2e],
            &[0xc2, 0x4d, 0x50, 0x4e, 0x42, 0x32, 0x40, 0x40],
            &[0xc3, 0xc8, 0x00, 0x3c, 0x00, 0x1e, 0x00, 0x64],
            &[0xc4, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa7, 0xff],
        ];

        // Frames 1–4: message is incomplete
        assert!(reader.process_frame(can_id, frames[0]).is_none(), "frame 0 should be incomplete");
        assert!(reader.process_frame(can_id, frames[1]).is_none(), "frame 1 should be incomplete");
        assert!(reader.process_frame(can_id, frames[2]).is_none(), "frame 2 should be incomplete");
        assert!(reader.process_frame(can_id, frames[3]).is_none(), "frame 3 should be incomplete");

        // Frame 5 completes the message
        let result = reader.process_frame(can_id, frames[4]);
        assert!(result.is_some(), "frame 4 should complete the message");

        let frame = result.unwrap();
        assert_eq!(frame.identifier.pgn(), 129810);
        assert_eq!(frame.identifier.source(), 0);
        assert!(frame.is_fast_packet);

        match frame.message {
            N2kMessage::AisClassBStaticDataPartB(ref report) => {
                assert_eq!(report.mmsi, 232055903, "unexpected MMSI");
                assert_eq!(report.callsign, "MPNB2", "unexpected callsign");
                assert_eq!(report.type_of_ship, 37, "unexpected type_of_ship");
            }
            other => panic!("expected AisClassBStaticDataPartB, got {:?}", other),
        }
    }

    /// Feed the four real CAN frames for one PGN 129809 fast-packet message into
    /// N2kStreamReader and verify that the assembled AisClassBStaticDataPartA is
    /// decoded correctly.
    ///
    /// Frames captured on 2026-04-06 16:51:37 UTC (source 0, destination 255,
    /// priority 6):
    ///   c0 19 18 78 08 cb 0e 41   <- first frame (frame_no=0, total_len=25)
    ///   c1 54 4c 41 4e 54 49 43   <- frame 1
    ///   c2 4f 20 20 20 20 20 20   <- frame 2
    ///   c3 20 20 20 20 20 ff ff   <- frame 3
    ///
    /// Assembled 25-byte payload decodes to:
    ///   MMSI 248187000, name "ATLANTICO"
    #[test]
    fn test_fast_packet_pgn129809_four_frames() {
        use crate::pgns::N2kMessage;

        let mut reader = N2kStreamReader::new();

        // CAN ID: priority=6, PGN=129809, source=0
        let can_id = make_can_id(6, 129809, 0);

        let frames: [&[u8]; 4] = [
            &[0xc0, 0x19, 0x18, 0x78, 0x08, 0xcb, 0x0e, 0x41],
            &[0xc1, 0x54, 0x4c, 0x41, 0x4e, 0x54, 0x49, 0x43],
            &[0xc2, 0x4f, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20],
            &[0xc3, 0x20, 0x20, 0x20, 0x20, 0x20, 0xff, 0xff],
        ];

        // Frames 1–3: message is incomplete
        assert!(reader.process_frame(can_id, frames[0]).is_none(), "frame 0 should be incomplete");
        assert!(reader.process_frame(can_id, frames[1]).is_none(), "frame 1 should be incomplete");
        assert!(reader.process_frame(can_id, frames[2]).is_none(), "frame 2 should be incomplete");

        // Frame 4 completes the message
        let result = reader.process_frame(can_id, frames[3]);
        assert!(result.is_some(), "frame 3 should complete the message");

        let frame = result.unwrap();
        assert_eq!(frame.identifier.pgn(), 129809);
        assert_eq!(frame.identifier.source(), 0);
        assert!(frame.is_fast_packet);

        match frame.message {
            N2kMessage::AisClassBStaticDataPartA(ref report) => {
                assert_eq!(report.mmsi, 248187000, "unexpected MMSI");
                assert_eq!(report.name, "ATLANTICO", "unexpected name");
            }
            other => panic!("expected AisClassBStaticDataPartA, got {:?}", other),
        }
    }

    /// Feed the seven real CAN frames for one PGN 129029 fast-packet message into
    /// N2kStreamReader and verify that the assembled GnssPositionData is decoded
    /// correctly.
    ///
    /// Frames captured on 2026-04-10 08:30:26 UTC (source 22, destination 255,
    /// priority 3):
    ///   e0 2b 04 49 50 d8 33 41   <- first frame (frame_no=0, total_len=43)
    ///   e1 12 40 2d 82 a2 c2 7a   <- frame 1
    ///   e2 f9 05 80 28 71 18 09   <- frame 2
    ///   e3 71 5d 01 ff ff ff ff   <- frame 3
    ///   e4 ff ff ff 7f 10 fd 00   <- frame 4
    ///   e5 52 00 00 00 00 00 00   <- frame 5
    ///   e6 00 00 ff ff ff ff ff   <- frame 6
    ///
    /// Assembled 43-byte payload decodes to:
    ///   lat ≈ 43.0510216°N, lon ≈ 9.8359051°E, HDOP 0.82
    #[test]
    fn test_fast_packet_pgn129029_seven_frames() {
        use crate::pgns::N2kMessage;

        let mut reader = N2kStreamReader::new();

        // CAN ID: priority=3, PGN=129029, source=22
        let can_id = make_can_id(3, 129029, 22);

        let frames: [&[u8]; 7] = [
            &[0xe0, 0x2b, 0x04, 0x49, 0x50, 0xd8, 0x33, 0x41],
            &[0xe1, 0x12, 0x40, 0x2d, 0x82, 0xa2, 0xc2, 0x7a],
            &[0xe2, 0xf9, 0x05, 0x80, 0x28, 0x71, 0x18, 0x09],
            &[0xe3, 0x71, 0x5d, 0x01, 0xff, 0xff, 0xff, 0xff],
            &[0xe4, 0xff, 0xff, 0xff, 0x7f, 0x10, 0xfd, 0x00],
            &[0xe5, 0x52, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            &[0xe6, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff],
        ];

        // Frames 0–5: message is incomplete, reader returns None
        assert!(reader.process_frame(can_id, frames[0]).is_none(), "frame 0 should be incomplete");
        assert!(reader.process_frame(can_id, frames[1]).is_none(), "frame 1 should be incomplete");
        assert!(reader.process_frame(can_id, frames[2]).is_none(), "frame 2 should be incomplete");
        assert!(reader.process_frame(can_id, frames[3]).is_none(), "frame 3 should be incomplete");
        assert!(reader.process_frame(can_id, frames[4]).is_none(), "frame 4 should be incomplete");
        assert!(reader.process_frame(can_id, frames[5]).is_none(), "frame 5 should be incomplete");

        // Frame 7 completes the message
        let result = reader.process_frame(can_id, frames[6]);
        assert!(result.is_some(), "frame 6 should complete the message");

        let frame = result.unwrap();
        assert_eq!(frame.identifier.pgn(), 129029);
        assert_eq!(frame.identifier.source(), 22);
        assert!(frame.is_fast_packet);

        match frame.message {
            N2kMessage::GnssPositionData(ref report) => {
                assert!(
                    (report.latitude - 43.0510216).abs() < 0.00001,
                    "unexpected latitude: {}",
                    report.latitude
                );
                assert!(
                    (report.longitude - 9.8359051).abs() < 0.00001,
                    "unexpected longitude: {}",
                    report.longitude
                );
                assert!(
                    (report.hdop - 0.82).abs() < 0.01,
                    "unexpected hdop: {}",
                    report.hdop
                );
            }
            other => panic!("expected GnssPositionData, got {:?}", other),
        }
    }

    /// Feed the four real CAN frames for one PGN 129038 fast-packet message into
    /// N2kStreamReader and verify that:
    ///   - the first three frames return None (message is still incomplete), and
    ///   - the fourth frame returns a fully assembled AisClassAPositionReport with
    ///     the expected field values.
    ///
    /// Frames captured on 2026-04-06 16:13:59 UTC (source 0, destination 255,
    /// priority 4):
    ///   80 1b c3 d8 4f 8a 0c 45   <- first frame  (frame_no=0, total_len=27)
    ///   81 9a 23 06 c2 2a f7 19   <- frame 1
    ///   82 e8 b6 dd 00 00 00 00   <- frame 2
    ///   83 00 22 06 00 00 f5 fe   <- frame 3
    ///
    /// Assembled 27-byte payload decodes to:
    ///   MMSI 210391000, lat ≈ 43.5629°N, lon ≈ 10.2997°E
    #[test]
    fn test_fast_packet_pgn129038_four_frames() {
        let mut reader = N2kStreamReader::new();

        // CAN ID: priority=4, PGN=129038, source=0
        let can_id = make_can_id(4, 129038, 0);

        let frames: [&[u8]; 4] = [
            &[0x80, 0x1b, 0xc3, 0xd8, 0x4f, 0x8a, 0x0c, 0x45],
            &[0x81, 0x9a, 0x23, 0x06, 0xc2, 0x2a, 0xf7, 0x19],
            &[0x82, 0xe8, 0xb6, 0xdd, 0x00, 0x00, 0x00, 0x00],
            &[0x83, 0x00, 0x22, 0x06, 0x00, 0x00, 0xf5, 0xfe],
        ];

        // Frames 1–3: message is incomplete, reader returns None
        assert!(reader.process_frame(can_id, frames[0]).is_none(), "frame 0 should be incomplete");
        assert!(reader.process_frame(can_id, frames[1]).is_none(), "frame 1 should be incomplete");
        assert!(reader.process_frame(can_id, frames[2]).is_none(), "frame 2 should be incomplete");

        // Frame 4 completes the message
        let result = reader.process_frame(can_id, frames[3]);
        assert!(result.is_some(), "frame 3 should complete the message");

        let frame = result.unwrap();
        assert_eq!(frame.identifier.pgn(), 129038);
        assert_eq!(frame.identifier.source(), 0);
        assert!(frame.is_fast_packet);

        match frame.message {
            N2kMessage::AisClassAPositionReport(ref report) => {
                assert_eq!(report.mmsi, 210391000, "unexpected MMSI");
                assert!(
                    (report.get_latitude_degrees() - 43.5628738).abs() < 0.00001,
                    "unexpected latitude: {}",
                    report.get_latitude_degrees()
                );
                assert!(
                    (report.get_longitude_degrees() - 10.2996549).abs() < 0.00001,
                    "unexpected longitude: {}",
                    report.get_longitude_degrees()
                );
            }
            other => panic!("expected AisClassAPositionReport, got {:?}", other),
        }
    }
}
