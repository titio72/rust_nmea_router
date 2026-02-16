# UDP Broadcaster Specification

## Overview

The UDP Broadcaster is a module that serializes NMEA2000 messages to JSON format and broadcasts them over UDP to the local network. This enables external applications to receive real-time marine data updates from the NMEA Router.

## Architecture

### Components

1. **UdpBroadcaster Module** (`src/udp_broadcaster.rs`)
   - Implements the `MessageHandler` trait
   - Manages UDP socket lifecycle
   - Serializes NMEA2000 messages to JSON
   - Broadcasts messages with frame metadata

2. **Configuration** (`src/config.rs`)
   - `UdpConfig` struct with enable flag and destination address
   - Integrated into main `Config` structure
   - Safe defaults with `#[serde(default)]`

3. **Integration** (`src/main.rs`)
   - Instantiated in main message processing loop
   - Receives all processed NMEA2000 messages
   - Passes frame metadata (source address, priority)

## Configuration

### UdpConfig Structure

```rust
pub struct UdpConfig {
    pub enabled: bool,      // Enable/disable UDP broadcasting
    pub address: String,    // UDP destination (broadcast or unicast)
}
```

### Default Values

- **enabled**: `false` (disabled by default for safety)
- **address**: `"192.168.1.255:10110"` (broadcast address on port 10110)

### Configuration File Example

```json
{
  "udp": {
    "enabled": true,
    "address": "192.168.1.255:10110"
  }
}
```

### Supported Address Formats

- **Broadcast**: `192.168.1.255:10110` (subnet broadcast)
- **Multicast**: `224.0.0.1:10110` (IP multicast)
- **Unicast**: `192.168.1.100:10110` (specific host)

## JSON Message Format

### Message Wrapper Structure

```json
{
  "message_type": "PositionRapidUpdate",
  "pgn": 129025,
  "source": 15,
  "priority": 3,
  "data": {
    "latitude": 43.630142,
    "longitude": 10.293372
  }
}
```

### Fields

- **message_type** (string): Human-readable message type name
- **pgn** (u32): Parameter Group Number from NMEA2000 standard
- **source** (u8): CAN source address (0-253)
- **priority** (u8): Message priority (0-7, lower is higher priority)
- **data** (object): Message-specific payload

## Supported Message Types

### Navigation Messages

#### PositionRapidUpdate (PGN 129025)
```json
{
  "message_type": "PositionRapidUpdate",
  "pgn": 129025,
  "data": {
    "latitude": 43.630142,
    "longitude": 10.293372
  }
}
```
- **latitude**: Decimal degrees
- **longitude**: Decimal degrees

#### CogSogRapidUpdate (PGN 129026)
```json
{
  "message_type": "CogSogRapidUpdate",
  "pgn": 129026,
  "data": {
    "sog": 2.5,
    "cog": 1.5708,
    "cog_reference": true
  }
}
```
- **sog**: Speed over ground in m/s
- **cog**: Course over ground in radians
- **cog_reference**: true=True North, false=Magnetic

#### GnssPositionData (PGN 129029)
```json
{
  "message_type": "GnssPositionData",
  "pgn": 129029,
  "data": {
    "date": "Date(2026, 1, 26)",
    "time": "Time(9, 30, 45.123)",
    "latitude": 43.630142,
    "longitude": 10.293372,
    "altitude": 5.2
  }
}
```
- **date**: NMEA2000 date format (debug format)
- **time**: NMEA2000 time format (debug format)
- **altitude**: Meters above sea level

#### VesselHeading (PGN 127250)
```json
{
  "message_type": "VesselHeading",
  "pgn": 127250,
  "data": {
    "heading": 1.5708,
    "reference": "True"
  }
}
```
- **heading**: Heading in radians
- **reference**: Reference type (debug format)

#### RateOfTurn (PGN 127251)
```json
{
  "message_type": "RateOfTurn",
  "pgn": 127251,
  "data": {
    "rate": 0.05
  }
}
```
- **rate**: Rate of turn in radians per second

#### Attitude (PGN 127257)
```json
{
  "message_type": "Attitude",
  "pgn": 127257,
  "data": {
    "yaw": 0.1,
    "pitch": 0.05,
    "roll": 0.02
  }
}
```
- **yaw**: Yaw in radians
- **pitch**: Pitch in radians
- **roll**: Roll in radians

#### SpeedWaterReferenced (PGN 128259)
```json
{
  "message_type": "SpeedWaterReferenced",
  "pgn": 128259,
  "data": {
    "speed": 2.3
  }
}
```
- **speed**: Speed through water in m/s

#### WaterDepth (PGN 128267)
```json
{
  "message_type": "WaterDepth",
  "pgn": 128267,
  "data": {
    "depth": 15.5,
    "offset": -0.5
  }
}
```
- **depth**: Water depth in meters
- **offset**: Transducer offset in meters

### Environmental Messages

#### WindData (PGN 130306)
```json
{
  "message_type": "WindData",
  "pgn": 130306,
  "data": {
    "speed": 5.2,
    "angle": 0.785,
    "reference": "Apparent"
  }
}
```
- **speed**: Wind speed in m/s
- **angle**: Wind angle in radians
- **reference**: Wind reference (debug format)

#### Temperature (PGN 130312)
```json
{
  "message_type": "Temperature",
  "pgn": 130312,
  "data": {
    "instance": 0,
    "source": 1,
    "temperature": 293.15,
    "set_temperature": null
  }
}
```
- **instance**: Sensor instance number
- **source**: Temperature source code
- **temperature**: Temperature in Kelvin
- **set_temperature**: Optional set temperature in Kelvin

#### Humidity (PGN 130313)
```json
{
  "message_type": "Humidity",
  "pgn": 130313,
  "data": {
    "instance": 0,
    "source": 1,
    "actual_humidity": 65.5,
    "set_humidity": null
  }
}
```
- **instance**: Sensor instance number
- **source**: Humidity source code
- **actual_humidity**: Relative humidity percentage (0-100)
- **set_humidity**: Optional set humidity percentage

#### ActualPressure (PGN 130314)
```json
{
  "message_type": "ActualPressure",
  "pgn": 130314,
  "data": {
    "instance": 0,
    "source": 1,
    "pressure": 101325.0
  }
}
```
- **instance**: Sensor instance number
- **source**: Pressure source code
- **pressure**: Pressure in Pascals

### System Messages

#### NMEASystemTime (PGN 126992)
```json
{
  "message_type": "NMEASystemTime",
  "pgn": 126992,
  "data": {
    "date": "Date(2026, 1, 26)",
    "time": "Time(9, 30, 45.123)"
  }
}
```
- **date**: NMEA2000 date format (debug format)
- **time**: NMEA2000 time format (debug format)

#### EngineRapidUpdate (PGN 127488)
```json
{
  "message_type": "EngineRapidUpdate",
  "pgn": 127488,
  "data": {
    "engine_instance": 0,
    "engine_speed": 1800.0,
    "engine_boost_pressure": 0.0,
    "engine_tilt_trim": 0.0
  }
}
```
- **engine_instance**: Engine instance number
- **engine_speed**: RPM
- **engine_boost_pressure**: Boost pressure
- **engine_tilt_trim**: Tilt/trim value

#### Unknown Messages
```json
{
  "message_type": "Unknown",
  "pgn": 123456,
  "data": {
    "raw": [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
  }
}
```
- **raw**: Raw byte array for unrecognized PGNs

## Technical Implementation

### Socket Configuration

- **Socket Type**: UDP (User Datagram Protocol)
- **Binding**: `0.0.0.0:0` (any address, ephemeral port)
- **Mode**: Non-blocking
- **Broadcast**: Enabled automatically for `.255` addresses
- **Buffer**: No explicit buffer size (uses OS defaults)

### Error Handling

- **Socket Creation Failures**: Logged as warnings, broadcaster disabled
- **Serialization Errors**: Logged (up to 10 errors), message skipped
- **Send Failures**: Ignored (UDP is fire-and-forget)
- **Error Counters**: Tracked for monitoring

### Statistics

The broadcaster tracks:
- **message_count**: Total messages successfully broadcast
- **error_count**: Total serialization/send errors

Access via `UdpBroadcaster::stats()` method (currently unused).

### Performance Characteristics

- **Non-blocking I/O**: Does not block main message processing loop
- **Zero-copy**: Messages serialized directly to JSON without intermediate buffers
- **Minimal Overhead**: Only enabled messages are processed
- **Fire-and-forget**: UDP does not wait for acknowledgment

## Integration Points

### Main Processing Loop

```rust
// In main.rs message processing loop
udp_broadcaster.handle_message_with_metadata(
    &n2k_frame.message,
    n2k_frame.identifier.source(),
    n2k_frame.identifier.priority()
);
```

### MessageHandler Trait

```rust
impl MessageHandler for UdpBroadcaster {
    fn handle_message(&mut self, message: &N2kMessage) {
        // Uses default source=0, priority=0
        self.broadcast_message(message, 0, 0);
    }
}
```

### Extended API

```rust
pub fn handle_message_with_metadata(
    &mut self,
    message: &N2kMessage,
    source: u8,
    priority: u8
) -> Result<(), std::io::Error>
```

## Security Considerations

### Network Exposure

- **Default**: Disabled by default (`enabled: false`)
- **Broadcast**: Exposes data to entire subnet
- **No Authentication**: UDP packets are unauthenticated
- **No Encryption**: Data sent in plaintext JSON

### Recommendations

1. **Enable only on trusted networks** (private vessel network)
2. **Use firewall rules** to restrict outbound UDP
3. **Consider VPN** for remote access instead of UDP forwarding
4. **Monitor network traffic** for unexpected listeners
5. **Use unicast** instead of broadcast when possible

## Client Implementation Guide

### Python Example

```python
import socket
import json

# Create UDP socket
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('', 10110))

while True:
    data, addr = sock.recvfrom(4096)
    message = json.loads(data.decode('utf-8'))
    
    print(f"PGN: {message['pgn']}")
    print(f"Source: {message['source']}")
    print(f"Data: {message['data']}")
```

### Node.js Example

```javascript
const dgram = require('dgram');
const server = dgram.createSocket('udp4');

server.on('message', (msg, rinfo) => {
    const message = JSON.parse(msg.toString());
    console.log(`PGN ${message.pgn} from source ${message.source}`);
    console.log(`Data:`, message.data);
});

server.bind(10110);
```

### C++ Example

```cpp
#include <sys/socket.h>
#include <netinet/in.h>
#include <nlohmann/json.hpp>

int sock = socket(AF_INET, SOCK_DGRAM, 0);
struct sockaddr_in addr;
addr.sin_family = AF_INET;
addr.sin_port = htons(10110);
addr.sin_addr.s_addr = INADDR_ANY;

bind(sock, (struct sockaddr*)&addr, sizeof(addr));

char buffer[4096];
while (true) {
    int len = recvfrom(sock, buffer, sizeof(buffer), 0, nullptr, nullptr);
    auto message = nlohmann::json::parse(buffer, buffer + len);
    
    std::cout << "PGN: " << message["pgn"] << std::endl;
    std::cout << "Data: " << message["data"] << std::endl;
}
```

## Testing

### Manual Testing

1. **Enable UDP broadcaster**:
   ```json
   {
     "udp": {
       "enabled": true,
       "address": "192.168.1.255:10110"
     }
   }
   ```

2. **Monitor UDP traffic**:
   ```bash
   # Using tcpdump
   sudo tcpdump -i any -n udp port 10110 -X
   
   # Using netcat
   nc -u -l 10110
   
   # Using socat
   socat UDP-RECV:10110 -
   ```

3. **Verify JSON format**:
   ```bash
   nc -u -l 10110 | jq .
   ```

### Unit Tests

Located in `src/udp_broadcaster.rs`:

- `test_disabled_broadcaster`: Verifies broadcaster respects enabled flag
- `test_serialize_position`: Tests JSON serialization format

### Integration Testing

1. Configure test CAN interface with known data
2. Enable UDP broadcaster
3. Capture UDP packets
4. Verify message format and content match input

## Troubleshooting

### No UDP Packets Received

1. **Check configuration**: Verify `udp.enabled = true`
2. **Check firewall**: Ensure UDP port 10110 is open
3. **Check network**: Verify broadcast address matches subnet
4. **Check binding**: Ensure no other process uses port 10110
5. **Check logs**: Look for UDP broadcaster warnings

### Malformed JSON

1. **Check receiver**: Ensure complete packet received (check buffer size)
2. **Check encoding**: Verify UTF-8 decoding
3. **Check logs**: Look for serialization errors in nmea_router logs

### High CPU Usage

1. **Check message rate**: NMEA2000 can produce 100+ msg/sec
2. **Disable if not needed**: Set `enabled: false`
3. **Filter messages**: Consider filtering by PGN in future version

### Network Congestion

1. **Use unicast**: Replace broadcast with specific destination
2. **Reduce update rate**: Consider message filtering
3. **Increase buffer**: On receiver side, increase socket buffer

## Future Enhancements

### Potential Features

1. **Message Filtering**: Allow filtering by PGN or message type
2. **Rate Limiting**: Configurable maximum messages per second
3. **Compression**: Optional gzip compression for large messages
4. **Batching**: Send multiple messages in single UDP packet
5. **Multicast Support**: Proper multicast group management
6. **Statistics API**: Expose message_count and error_count
7. **UDP Authentication**: Optional HMAC for message verification
8. **TLS/DTLS**: Encrypted UDP variant
9. **Message Queueing**: Buffer messages during temporary network issues
10. **Alternative Formats**: Support Protocol Buffers or MessagePack

### Backwards Compatibility

Any future changes should:
- Maintain JSON format for default configuration
- Add new features as optional configuration
- Preserve existing field names and types
- Document breaking changes with migration guide

## Version History

- **v1.0** (January 2026): Initial implementation
  - Basic UDP broadcasting
  - JSON serialization for all NMEA2000 message types
  - Configurable enable/disable and destination address
  - Frame metadata (source, priority) included
  - Non-blocking socket operation
  - Error tracking and statistics

## Related Documentation

- [NMEA2000 Standard](http://www.nmea.org/)
- [CAN Bus Protocol](https://en.wikipedia.org/wiki/CAN_bus)
- [UDP Protocol RFC 768](https://tools.ietf.org/html/rfc768)
- [JSON Standard RFC 8259](https://tools.ietf.org/html/rfc8259)

## Contact & Support

For issues, questions, or contributions related to the UDP broadcaster:
- Review code in `src/udp_broadcaster.rs`
- Check configuration in `config.example.json`
- Monitor logs for error messages
- Test with minimal configuration first
