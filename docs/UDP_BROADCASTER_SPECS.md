# UDP Broadcaster Specification

## Overview

The UDP Broadcaster converts NMEA2000 messages to NMEA0183 sentences and broadcasts them over UDP. This allows standard chartplotters, navigation software, and other devices that speak NMEA0183 to consume data from the NMEA2000 bus.

Output is rate-limited to **1 sentence per second per topic** to avoid flooding receivers.

## Architecture

### Components

1. **UdpBroadcaster** (`src/udp_broadcaster.rs`)
   - Implements the `MessageHandler` trait
   - Maintains aggregated navigation state (`RmcState`) to build composite sentences (RMC, GGA)
   - Converts NMEA2000 messages to NMEA0183 and !AIVDM sentences
   - Rate-limits output per topic (1 s window, 900 ms minimum gap)
   - Cleans up stale rate-limiter entries when the map exceeds 500 entries (5-minute TTL)

2. **Configuration** (`src/config.rs`)
   - `UdpConfig` struct integrated into main `Config`
   - All fields have safe defaults via `#[serde(default)]`

3. **Integration** (`src/router_loop.rs`)
   - Called from `RouterLoop::handle_message` for every processed NMEA2000 frame

## Configuration

### UdpConfig Structure

```rust
pub struct UdpConfig {
    pub enabled: bool,        // Enable/disable UDP broadcasting
    pub address: String,      // UDP destination (broadcast, multicast, or unicast)
    pub bind_address: String, // Local interface to bind to
}
```

### Default Values

| Field          | Default               |
|----------------|-----------------------|
| `enabled`      | `false`               |
| `address`      | `192.168.1.255:10110` |
| `bind_address` | `0.0.0.0:0`           |

### Configuration File Example

```json
{
  "udp": {
    "enabled": true,
    "address": "192.168.1.255:10110",
    "bind_address": "0.0.0.0:0"
  }
}
```

### Supported Address Formats

- **Broadcast**: `192.168.1.255:10110` (subnet broadcast — `SO_BROADCAST` enabled automatically for `.255` destinations)
- **Multicast**: `224.0.0.1:10110`
- **Unicast**: `192.168.1.100:10110`

## NMEA0183 Output

All sentences use the `II` talker ID (integrated instrumentation). Each sentence ends with `\r\n` and includes an XOR checksum (`*XX`).

### Navigation

#### RMC — Recommended Minimum Navigation Information
Emitted when a `GnssPositionData` (PGN 129029) is received, provided position, SOG, COG, and date/time are all available.

```
$IIRMC,HHMMSS,A,DDMM.mmmm,N,DDDMM.mmmm,E,SOG,COG,,DDMMYY*XX\r\n
```

State sources:
- Position: `GnssPositionData` or `PositionRapidUpdate` (PGN 129025)
- SOG/COG: `CogSogRapidUpdate` (PGN 129026), converted m/s → knots and rad → degrees
- Date/time: `GnssPositionData` or `NMEASystemTime` (PGN 126992)

Rate-limit topic: `rmc`

#### GGA — Global Positioning System Fix Data
Emitted alongside RMC from `GnssPositionData`.

```
$IIGGA,HHMMSS,DDMM.mmmm,N,DDDMM.mmmm,E,fix_quality,num_sats,hdop,altitude,M,0.0,M,,*XX\r\n
```

Fix quality mapping from `GnssMethod`:

| N2K method    | GGA quality |
|---------------|-------------|
| NoGnss        | 0 (invalid) |
| GnssFix       | 1 (GPS)     |
| DGnss         | 2 (DGPS)    |
| PreciseGnss   | 4           |
| RtkFixed      | 5           |
| RtkFloat      | 6           |

Rate-limit topic: `gga`

#### ZDA — Time & Date
Emitted from `NMEASystemTime` (PGN 126992).

```
$IIZDA,HHMMSS,DD,MM,YYYY,00,00*XX\r\n
```

Rate-limit topic: `zda`

#### HDT — True Heading
Emitted from `VesselHeading` (PGN 127250) when reference is `True`.

```
$IIHDT,HHH.H,T*XX\r\n
```

Rate-limit topic: `hdt`

#### HDM — Magnetic Heading
Emitted from `VesselHeading` when reference is `Magnetic`.

```
$IIHDM,HHH.H,M*XX\r\n
```

Rate-limit topic: `hdm`

#### ROT — Rate of Turn
Emitted from `RateOfTurn` (PGN 127251). Rate converted rad/s → degrees/minute.

```
$IIROT,RRR.R,A*XX\r\n
```

Rate-limit topic: `rot`

#### VHW — Water Speed and Heading
Emitted from `SpeedWaterReferenced` (PGN 128259). Speed converted m/s → knots.

```
$IIVHW,,,,,SSS.S,N,,,K*XX\r\n
```

Rate-limit topic: `vhw`

#### DPT — Water Depth
Emitted from `WaterDepth` (PGN 128267).

```
$IIDPT,DDD.D,OOO.O,*XX\r\n
```

Fields: depth (metres), transducer offset (metres).

Rate-limit topic: `dpt`

### Wind

#### MWV — Wind Speed and Angle
Emitted from `WindData` (PGN 130306). Angle converted rad → degrees; speed converted m/s → knots.

```
$IIMWV,AAA.A,R,SSS.S,N,A*XX\r\n   (apparent)
$IIMWV,AAA.A,T,SSS.S,N,A*XX\r\n   (true)
```

Reference mapping: `Apparent` → `R`; all others (`TrueBoat`, `TrueWater`, `TrueGroundNorth`, `Magnetic`) → `T`.

Rate-limit topics: `mwv_apparent`, `mwv_true`

### Environmental

#### XDR — Transducer Data (Temperature)
Emitted from `Temperature` (PGN 130312). Temperature converted Kelvin → Celsius.

```
$IIXDR,C,TT.T,C,WATER*XX\r\n   (source 0 = sea water)
$IIXDR,C,TT.T,C,AIR*XX\r\n     (source 1 = outside air)
$IIXDR,C,TT.T,C,CABIN*XX\r\n   (source 2 = cabin)
$IIXDR,C,TT.T,C,TEMP_N*XX\r\n  (other sources)
```

Rate-limit topic: `xdr_temp_<instance>`

#### XDR — Transducer Data (Humidity)
Emitted from `Humidity` (PGN 130313).

```
$IIXDR,H,HH.H,P,HUMIDITY_N*XX\r\n
```

Rate-limit topic: `xdr_hum_<instance>`

#### XDR — Transducer Data (Barometric Pressure)
Emitted from `ActualPressure` (PGN 130314). Pressure converted Pa → bar.

```
$IIXDR,P,P.PPPP,B,BARO_N*XX\r\n
```

Rate-limit topic: `xdr_pres_<instance>`

### Attitude

#### XDR — Attitude (Yaw / Pitch / Roll)
Emitted from `Attitude` (PGN 127257). Angles converted rad → degrees.

```
$IIXDR,A,YYY.Y,D,YAW,A,PPP.P,D,PITCH,A,RRR.R,D,ROLL*XX\r\n
```

Only fields present in the N2K message are included. The sentence is skipped entirely if no fields are available.

Rate-limit topic: `xdr_attitude`

### Engine

#### RPM — Engine Speed
Emitted from `EngineRapidUpdate` (PGN 127488) when `engine_speed` is present.

```
$IIRPM,E,N,RRRR,,A*XX\r\n
```

Fields: source type `E` (engine), engine instance number, RPM.

Rate-limit topic: `rpm_<engine_instance>`

### AIS

AIS messages are re-encoded as `!AIVDM` sentences (standard AIS VHF data-link messages, single-part).

#### !AIVDM Type 1/2/3 — Class A Position Report
From `AisClassAPositionReport` (PGN 129038).

```
!AIVDM,1,1,,A,<payload>,<fill>*XX\r\n
```

Rate-limit topic: `ais_a_pos_<mmsi>`

#### !AIVDM Type 18 — Class B Position Report
From `AisClassBPositionReport` (PGN 129039).

Rate-limit topic: `ais_b_pos_<mmsi>`

#### !AIVDM Type 5 — Class A Static & Voyage Data
From `AisClassAStaticData` (PGN 129794).

Rate-limit topic: `ais_a_static_<mmsi>`

#### !AIVDM Type 24 Part A — Class B Vessel Name
From `AisClassBStaticDataPartA` (PGN 129809).

Rate-limit topic: `ais_b_static_a_<mmsi>`

#### !AIVDM Type 24 Part B — Class B Vessel Details
From `AisClassBStaticDataPartB` (PGN 129810).

Rate-limit topic: `ais_b_static_b_<mmsi>`

## Technical Details

### Socket Configuration

- **Type**: UDP
- **Binding**: configured `bind_address` (default `0.0.0.0:0`, ephemeral port)
- **Mode**: non-blocking (`set_nonblocking(true)`)
- **Broadcast**: `set_broadcast(true)` when destination contains `.255`

### Rate Limiting

Each topic has an independent 1-second rate limit. A message is sent only if at least 900 ms have elapsed since the last send on that topic. The rate-limiter map is bounded: entries older than 5 minutes are purged when the map exceeds 500 entries (primarily caused by many distinct AIS MMSIs).

### Error Handling

- **Socket creation failure**: returns `Err(String)` from `new()` when enabled
- **Lock failure**: logged as warning (first 10 occurrences), message skipped
- **Send failure**: logged as warning (first 10 occurrences), counted in `error_count`

### Statistics

```rust
pub fn stats(&self) -> (u64, u64)  // (message_count, error_count)
```

## Client Implementation Guide

### Python

```python
import socket, json

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(('', 10110))

while True:
    data, addr = sock.recvfrom(4096)
    sentence = data.decode('ascii').strip()
    print(sentence)
```

### Bash (socat)

```bash
socat UDP-RECV:10110 -
# or pipe to nmea-monitor / OpenCPN
```

## Testing

### Unit Tests (`src/udp_broadcaster.rs`)

Covers: NMEA0183 checksum, position formatting, RMC/GGA state machine, MWV/HDT/HDM/ROT/VHW/DPT/XDR formatters, AIS bit-writer, AIS raw-to-AIS unit conversions, AIS VDM sentence structure and checksum.

### Manual Testing

```bash
# Enable in config.json:
# "udp": { "enabled": true, "address": "127.0.0.1:10110" }

# Receive on loopback:
socat UDP-RECV:10110 -
# or
nc -u -l 10110
```

## Troubleshooting

| Symptom | Likely cause |
|---------|--------------|
| No sentences received | `enabled` is false, wrong port, firewall blocking UDP 10110 |
| Sentences arrive ~every 2 s instead of 1 s | Expected — 900 ms guard prevents double-sends within the same second |
| AIS sentences for many targets consume high memory | Map auto-cleans after 500 entries; not a leak |
| Socket bind error at startup | Port already in use, or `bind_address` interface not present |
