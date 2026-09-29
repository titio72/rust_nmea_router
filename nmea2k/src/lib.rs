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
//! - **CAN Bus Support**: Open, configure, and read from SocketCAN interfaces on Linux, with a
//!   no-op mock backend elsewhere so the same code runs on a development machine
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

/// CAN bus I/O.
///
/// Two interchangeable backends live behind this path. On Linux it is real SocketCAN; on every
/// other platform it is `canbus_mock`, a silent stand-in that lets the router run on a
/// development machine without a CAN interface. Both expose the same functions and the same
/// `CanSocket` type, so callers never need to know which one they got.
#[cfg(target_os = "linux")]
pub mod canbus;
#[cfg(not(target_os = "linux"))]
#[path = "canbus_mock.rs"]
pub mod canbus;

// Re-export commonly used types
pub use stream_reader::{N2kStreamReader, N2kFrame};
pub use message_handler::MessageHandler;
pub use pgns::N2kMessage;
pub use canbus as CanBus;
pub use canbus::CanSocket;

// Re-export external types for convenience
pub use nmea2000::{Identifier, FastPacket};
// `ExtendedId` comes from `embedded-can` (which `socketcan` and `nmea2000` both re-export),
// so it stays available on platforms without SocketCAN.
pub use embedded_can::ExtendedId;
