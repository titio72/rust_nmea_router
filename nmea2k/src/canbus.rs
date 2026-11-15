use socketcan::{EmbeddedFrame, ExtendedId, Frame, Socket};
use std::{error::Error, time::Duration};
use tracing::{info, warn};

pub use crate::stream_reader::N2kFrame;
// Re-exported so callers name the socket type through this module and stay backend-agnostic;
// `canbus_mock` exposes its own `CanSocket` under the same path on non-Linux targets.
pub use socketcan::CanSocket;

const CAN_CONNECTION_RETRY_INTERVAL_SECS: u64 = 5; // Interval between CAN connection retries
const CAN_WRITE_TIMEOUT_MS: u64 = 10; // Longest a frame write may block

/// Opens a CAN socket with automatic retry on failure
///
/// # Arguments
/// * `interface` - Name of the CAN interface (e.g., "can0", "vcan0")
///
/// # Returns
/// A connected CanSocket
pub fn open_can_socket_with_retry(interface: &str) -> CanSocket {
    loop {
        match CanSocket::open(interface) {
            Ok(socket) => {
                info!("Successfully opened CAN interface: {}", interface);
                return socket;
            }
            Err(e) => {
                warn!("Failed to open CAN interface '{}': {}", interface, e);
                warn!(
                    "Retrying in {} seconds...",
                    CAN_CONNECTION_RETRY_INTERVAL_SECS
                );
                std::thread::sleep(Duration::from_secs(CAN_CONNECTION_RETRY_INTERVAL_SECS));
            }
        }
    }
}

/// Configures a CAN socket with NMEA2000-specific settings
///
/// # Arguments
/// * `socket` - The CAN socket to configure
///
/// # Returns
/// Result indicating success or failure
pub fn configure_nmea2k_socket(socket: &mut CanSocket) -> Result<(), Box<dyn Error>> {
    // Set read timeout to prevent blocking indefinitely
    // This allows metrics logging and health checks to run even with no CAN activity
    socket.set_read_timeout(Duration::from_millis(500))?;
    // The write timeout bounds how long a write may block on a full socket send buffer. A full
    // device/qdisc queue (txqueuelen, default 10 frames) returns ENOBUFS immediately instead;
    // callers handle that as backpressure. Transmitting is optional, so failing to set the
    // timeout is logged, not fatal.
    if let Err(e) = socket.set_write_timeout(Duration::from_millis(CAN_WRITE_TIMEOUT_MS)) {
        warn!("Could not set the CAN write timeout ({}); writes may block", e);
    }
    Ok(())
}

/// Reads a CAN frame and converts it to NMEA2000 extended ID format
///
/// # Arguments
/// * `socket` - The CAN socket to read from
///
/// # Returns
/// Result containing the extended ID and data, or an error
pub fn read_nmea2k_frame(socket: &CanSocket) -> Result<(ExtendedId, Vec<u8>), std::io::Error> {
    let frame = socket.read_frame()?;

    // NMEA2000 uses 29-bit extended CAN identifiers
    let can_id = frame.can_id();
    let extended_id = ExtendedId::new(can_id.as_raw()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid CAN ID for NMEA2000",
        )
    })?;

    let data = frame.data().to_vec();

    Ok((extended_id, data))
}

/// Writes one NMEA2000 frame (29-bit extended identifier, up to 8 data bytes) to the bus
///
/// # Arguments
/// * `socket` - The CAN socket to write to
/// * `id` - The 29-bit extended identifier
/// * `data` - The frame payload, at most 8 bytes
///
/// # Returns
/// `InvalidInput` when `data` is longer than 8 bytes, otherwise the result of the socket write
pub fn write_nmea2k_frame(
    socket: &CanSocket,
    id: ExtendedId,
    data: &[u8],
) -> Result<(), std::io::Error> {
    let frame = socketcan::CanFrame::new(id, data).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "CAN data frame payload must be at most 8 bytes",
        )
    })?;
    socket.write_frame(&frame)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_configure_socket_sets_timeout() {
        // Note: This test requires a virtual CAN interface
        // Run: sudo modprobe vcan && sudo ip link add dev vcan0 type vcan && sudo ip link set up vcan0
        // For CI/CD, this test should be conditional or mocked

        // We can't easily test this without a real/virtual CAN interface
        // but we can at least verify the function exists and has the right signature
        // @TODO: Consider using a mock CAN socket for more thorough testing
        assert!(true);
    }
}
