# NMEA2K - NMEA2000 Protocol Library

A comprehensive Rust library for working with NMEA2000 marine data networks over CAN bus.

## Features

- **CAN Bus Interface**: Utilities for opening, configuring, and reading from SocketCAN interfaces
- **Fast Packet Assembly**: Automatic reassembly of multi-frame NMEA2000 messages
- **Comprehensive PGN Decoders**: 13+ Parameter Group Number (PGN) decoders including:
  - Position (129025, 129029)
  - Speed & Heading (129026, 127250, 127251)
  - Environmental Data (130306, 130312, 130313, 130314)
  - Attitude/Roll (127257)
  - Depth & Water Speed (128267, 128259)
  - System Time (126992)
  - Engine Data (127488)
- **Message Handler Trait**: Clean abstraction for processing NMEA2000 messages
- **Message Filtering**: Filter frames by PGN and source

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
nmea2k = { path = "path/to/nmea2k" }
```

### Basic Example

```rust
use nmea2k::{CanBus, N2kStreamReader, MessageHandler};

// Open and configure CAN socket
let mut socket = CanBus::open_can_socket_with_retry("can0");
CanBus::configure_nmea2k_socket(&mut socket).unwrap();

// Create stream reader for frame assembly
let mut reader = N2kStreamReader::new();

// Read and process frames
loop {
    match CanBus::read_nmea2k_frame(&socket) {
        Ok((id, data)) => {
            if let Some(frame) = reader.process_frame(id, &data) {
                // Process complete message
                println!("PGN: {}", frame.identifier.pgn());
                println!("Source: {}", frame.identifier.source());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            // Timeout is normal with configured read timeout
            continue;
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
```

### Implementing a Message Handler

```rust
use nmea2k::{MessageHandler, N2kMessage};

struct MyMonitor {
    // Your state here
}

impl MessageHandler for MyMonitor {
    fn handle_message(&mut self, message: &N2kMessage) {
        match message {
            N2kMessage::PositionRapidUpdate(pos) => {
                println!("Lat: {}, Lon: {}", pos.latitude(), pos.longitude());
            }
            N2kMessage::WindData(wind) => {
                println!("Wind speed: {} m/s", wind.wind_speed());
            }
            _ => {} // Ignore other messages
        }
    }
}
```

## Architecture

### Modules

- **canbus**: CAN socket operations (open, configure, read)
- **stream_reader**: NMEA2000 stream reader with fast packet assembly
- **pgns**: PGN decoders for various NMEA2000 message types
- **message_handler**: Trait for implementing message processors

### Fast Packet Assembly

NMEA2000 messages can span multiple CAN frames. The `N2kStreamReader` automatically:
1. Detects single-frame vs multi-frame messages
2. Buffers multi-frame messages
3. Assembles complete messages
4. Decodes into typed message structs

## Supported PGNs

| PGN | Name | Data |
|-----|------|------|
| 126992 | System Time | Date, Time, Milliseconds |
| 127250 | Vessel Heading | Heading (Magnetic/True) |
| 127251 | Rate of Turn | ROT (degrees/second) |
| 127257 | Attitude | Yaw, Pitch, Roll |
| 127488 | Engine Rapid Update | RPM, boost pressure, tilt/trim |
| 128259 | Speed (Water Referenced) | Speed through water |
| 128267 | Water Depth | Depth, Offset |
| 129025 | Position Rapid Update | Latitude, Longitude |
| 129026 | COG & SOG Rapid Update | Course, Speed over ground |
| 129029 | GNSS Position Data | Lat, Lon, Altitude |
| 130306 | Wind Data | Speed, Direction, Reference |
| 130312 | Temperature | Various sources (cabin, water, etc.) |
| 130313 | Humidity | Relative humidity |
| 130314 | Actual Pressure | Atmospheric pressure |

## Dependencies

- `nmea2000`: Core NMEA2000 protocol types
- `socketcan`: Linux SocketCAN interface
- `tracing`: Logging framework
- `chrono`: Date and time handling

## License

MIT OR Apache-2.0

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues.
