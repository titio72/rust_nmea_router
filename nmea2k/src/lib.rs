//! NMEA2000 Protocol Library
//!
//! This library provides a complete implementation for working with NMEA2000 marine data networks:
//! - CAN bus interface utilities
//! - NMEA2000 stream reader with fast packet assembly
//! - PGN (Parameter Group Number) decoders for 13+ message types
//! - Message handler trait for processing NMEA2000 messages
//!
//! # Features
//!
//! - **CAN Bus Support**: Open, configure, and read from SocketCAN interfaces
//! - **Fast Packet Assembly**: Automatic reassembly of multi-frame messages
//! - **Comprehensive PGN Decoders**: Position, speed, heading, environmental data, and more
//! - **Message Filtering**: Filter messages by PGN and source
//!
//! # Example
//!
//! ```no_run
//! use nmea2k::{CanBus, N2kStreamReader};
//!
//! // Open CAN interface
//! let mut socket = CanBus::open_can_socket_with_retry("can0");
//! CanBus::configure_nmea2k_socket(&mut socket).unwrap();
//!
//! // Create stream reader
//! let mut reader = N2kStreamReader::new();
//!
//! // Process frames
//! loop {
//!     match CanBus::read_nmea2k_frame(&socket) {
//!         Ok((id, data)) => {
//!             if let Some(frame) = reader.process_frame(id, &data) {
//!                 println!("PGN: {}", frame.identifier.pgn());
//!             }
//!         }
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! ```

pub mod pgns;
pub mod stream_reader;
pub mod message_handler;
pub mod canbus;

// Re-export commonly used types
pub use stream_reader::{N2kStreamReader, N2kFrame};
pub use message_handler::MessageHandler;
pub use pgns::N2kMessage;
pub use canbus as CanBus;

// Re-export external types for convenience
pub use nmea2000::{Identifier, FastPacket};
pub use socketcan::ExtendedId;
