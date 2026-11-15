//! No-op CAN backend for platforms without SocketCAN (macOS, Windows).
//!
//! Mirrors the API of the Linux [`canbus`](../canbus/index.html) module so the router compiles
//! and runs unchanged off-vessel: the socket "opens" successfully and every read reports a
//! timeout, which the router treats as an idle bus. No frame is ever delivered, so nothing is
//! recorded or broadcast — the web API and database layer behave exactly as they do on a real
//! bus that happens to be silent.

use embedded_can::ExtendedId;
use std::{error::Error, time::Duration};
use tracing::{info, warn};

pub use crate::stream_reader::N2kFrame;

/// Matches the read timeout `configure_nmea2k_socket` sets on Linux, so reads block for the
/// same interval instead of spinning the caller's loop.
const MOCK_READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Stand-in for `socketcan::CanSocket` that never delivers a frame.
pub struct CanSocket {
    read_timeout: Duration,
}

/// Pretends to open a CAN interface. Never fails, so it never retries.
///
/// # Arguments
/// * `interface` - Name of the CAN interface, used only for logging
///
/// # Returns
/// A mock CanSocket
pub fn open_can_socket_with_retry(interface: &str) -> CanSocket {
    warn!(
        "SocketCAN is unavailable on this platform: opening a no-op mock for interface '{}'",
        interface
    );
    warn!("No NMEA2000 messages will be received, so no vessel or environmental data is recorded");
    CanSocket {
        read_timeout: MOCK_READ_TIMEOUT,
    }
}

/// Applies the NMEA2000 read timeout to a mock socket. Cannot fail. There is no write timeout:
/// mock writes never block.
///
/// # Arguments
/// * `socket` - The mock CAN socket to configure
///
/// # Returns
/// Result indicating success or failure
pub fn configure_nmea2k_socket(socket: &mut CanSocket) -> Result<(), Box<dyn Error>> {
    socket.read_timeout = MOCK_READ_TIMEOUT;
    info!(
        "Mock CAN socket configured with a {} ms read timeout",
        socket.read_timeout.as_millis()
    );
    Ok(())
}

/// Blocks for the configured read timeout, then reports it the way SocketCAN does.
///
/// Callers treat `WouldBlock` as "no traffic on the bus" and fall through to their periodic
/// work, so this keeps the router loop ticking at its real cadence rather than busy-waiting.
///
/// # Arguments
/// * `socket` - The mock CAN socket to read from
///
/// # Returns
/// Always a timeout error — the mock bus is permanently silent
pub fn read_nmea2k_frame(socket: &CanSocket) -> Result<(ExtendedId, Vec<u8>), std::io::Error> {
    std::thread::sleep(socket.read_timeout);

    Err(std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        "mock CAN interface never receives frames",
    ))
}

/// Accepts a frame and discards it, validating the length the way the Linux backend does.
///
/// # Arguments
/// * `socket` - The mock CAN socket (unused: nothing is ever transmitted)
/// * `id` - The 29-bit extended identifier (unused)
/// * `data` - The frame payload, at most 8 bytes
///
/// # Returns
/// `InvalidInput` when `data` is longer than 8 bytes, otherwise `Ok(())`
pub fn write_nmea2k_frame(
    _socket: &CanSocket,
    _id: ExtendedId,
    data: &[u8],
) -> Result<(), std::io::Error> {
    if data.len() > 8 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CAN data frame payload must be at most 8 bytes",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A short timeout keeps the timing assertions fast; the production value is 500 ms.
    const TEST_TIMEOUT: Duration = Duration::from_millis(5);

    fn test_socket() -> CanSocket {
        CanSocket {
            read_timeout: TEST_TIMEOUT,
        }
    }

    #[test]
    fn test_configure_socket_sets_timeout() {
        let mut socket = test_socket();
        configure_nmea2k_socket(&mut socket).expect("mock configuration must not fail");
        assert_eq!(socket.read_timeout, MOCK_READ_TIMEOUT);
    }

    #[test]
    fn test_read_reports_a_timeout_the_router_can_ignore() {
        let error = read_nmea2k_frame(&test_socket()).expect_err("mock bus must never yield a frame");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::WouldBlock,
            "RouterLoop only tolerates WouldBlock/TimedOut without reopening the socket"
        );
    }

    #[test]
    fn test_read_blocks_for_the_configured_timeout() {
        let socket = test_socket();
        let start = std::time::Instant::now();
        let _ = read_nmea2k_frame(&socket);
        assert!(
            start.elapsed() >= TEST_TIMEOUT,
            "read must block so the caller's loop does not spin"
        );
    }

    #[test]
    fn write_accepts_up_to_8_bytes() {
        let id = ExtendedId::new(0x18EE_FF01).expect("valid 29-bit id");
        assert!(write_nmea2k_frame(&test_socket(), id, &[0u8; 8]).is_ok());
    }

    #[test]
    fn write_rejects_9_bytes() {
        let id = ExtendedId::new(0x18EE_FF01).expect("valid 29-bit id");
        let error = write_nmea2k_frame(&test_socket(), id, &[0u8; 9])
            .expect_err("a CAN data frame cannot carry 9 bytes");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
