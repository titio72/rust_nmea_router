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
/// use stream_reader::N2kStreamReader;
/// 
/// let mut reader = N2kStreamReader::new();
/// 
/// // Push frames into the reader
/// if let Some(complete_message) = reader.process_frame(can_id, data) {
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
    
    fn add_frame(&mut self, frame_data: Vec<u8>) {
        self.frames.push(frame_data);
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
                buffer.add_frame(fast_packet.data().to_vec());
                
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
            buffer.add_frame(fast_packet.data().to_vec());
            
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
                | 129038 | 129039 | 129540 | 129794 | 129809 | 129810
        )
    }
}

impl Default for N2kStreamReader {
    fn default() -> Self {
        Self::new()
    }
}
