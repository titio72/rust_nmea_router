# SignalK Path Mapping

This document describes the mapping between NMEA2000 PGNs and SignalK v1.7.0 paths for the NMEA2000 Router SignalK broadcaster.

## Overview

The SignalK broadcaster converts NMEA2000 messages to SignalK delta format in real-time. All values are provided in SI units as required by the SignalK specification.

**SignalK Endpoint:** `ws://localhost:8080/signalk/v1/stream`  
**Message Format:** Delta (incremental updates)  
**Context:** `vessels.self` (single vessel mode)  
**Specification:** SignalK v1.7.0

## Supported PGN to SignalK Path Mappings

### PGN 129025 - Position Rapid Update

**SignalK Path:** `navigation.position`  
**Units:** Decimal degrees  
**NMEA2000 Fields:** `latitude`, `longitude` (already in decimal degrees)  
**Value Format:** Object with `latitude` and `longitude` properties

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 129025
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "navigation.position",
      "value": {
        "latitude": 43.6332,
        "longitude": 10.2891
      }
    }]
  }]
}
```

---

### PGN 129026 - COG and SOG Rapid Update

**SignalK Paths:**
- `navigation.speedOverGround` - Speed in m/s
- `navigation.courseOverGroundTrue` - Course in radians

**Units:**
- Speed: meters per second (m/s)
- Course: radians

**NMEA2000 Fields:**
- `sog` - Already in m/s
- `cog` - Already in radians

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 129026
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [
      {
        "path": "navigation.speedOverGround",
        "value": 2.572
      },
      {
        "path": "navigation.courseOverGroundTrue",
        "value": 0.7854
      }
    ]
  }]
}
```

---

### PGN 127250 - Vessel Heading

**SignalK Path:** `navigation.headingTrue`  
**Units:** Radians  
**NMEA2000 Fields:**
- `heading` - In radians
- `variation` - Magnetic variation in radians  
- `reference` - Must be `Magnetic` to be processed

**Conversion:** `heading_true = heading + variation`

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 127250
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "navigation.headingTrue",
      "value": 0.7854
    }]
  }]
}
```

---

### PGN 130306 - Wind Data

**SignalK Paths:**
- `environment.wind.speedOverGround` - True wind speed (m/s)
- `environment.wind.angleTrueGround` - True wind angle (radians)
- `environment.wind.speedApparent` - Apparent wind speed (m/s)
- `environment.wind.angleApparent` - Apparent wind angle (radians)

**Units:** All speeds in m/s, all angles in radians

**NMEA2000 Fields:**
- `speed` - Already in m/s
- `angle` - Already in radians
- `reference` - Must be `Apparent` to be processed

**Processing:**
1. Apparent wind values are extracted directly from NMEA2000 message
2. True wind is calculated using cached SOG (from PGN 129026)
3. Conversion uses vector subtraction in knots, then converts back to m/s

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 130306
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [
      {
        "path": "environment.wind.speedOverGround",
        "value": 5.144
      },
      {
        "path": "environment.wind.angleTrueGround",
        "value": 0.5236
      },
      {
        "path": "environment.wind.speedApparent",
        "value": 6.172
      },
      {
        "path": "environment.wind.angleApparent",
        "value": 0.6981
      }
    ]
  }]
}
```

---

### PGN 130312 - Temperature

**SignalK Paths:**
- `environment.water.temperature` - Water temperature (instance 0)
- `environment.outside.temperature` - Air/cabin temperature (other instances)

**Units:** Kelvin  
**NMEA2000 Fields:** `temperature` (already in Kelvin), `instance`

**Instance Mapping:**
- Instance 0 → Water temperature
- Other instances → Outside/cabin temperature

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 130312
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "environment.water.temperature",
      "value": 288.15
    }]
  }]
}
```

---

### PGN 130313 - Humidity

**SignalK Path:** `environment.outside.relativeHumidity`  
**Units:** Ratio (0-1)  
**NMEA2000 Fields:** `actual_humidity` (percentage 0-100)

**Conversion:** `ratio = percentage / 100`

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 130313
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "environment.outside.relativeHumidity",
      "value": 0.65
    }]
  }]
}
```

---

### PGN 130314 - Actual Pressure

**SignalK Path:** `environment.outside.pressure`  
**Units:** Pascals (Pa)  
**NMEA2000 Fields:** `pressure` (already in Pa)

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 130314
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "environment.outside.pressure",
      "value": 101325.0
    }]
  }]
}
```

---

### PGN 126992 - System Time

**SignalK Path:** `navigation.datetime`  
**Units:** ISO 8601 / RFC 3339 string  
**NMEA2000 Fields:** `date_time` (NMEA datetime object)

**Conversion:** NMEA datetime → Unix timestamp → ISO 8601 string with milliseconds

**Example Delta:**
```json
{
  "context": "vessels.self",
  "updates": [{
    "source": {
      "label": "N2K-22",
      "type": "NMEA2000",
      "src": "22",
      "pgn": 126992
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [{
      "path": "navigation.datetime",
      "value": "2026-02-17T12:00:00.000Z"
    }]
  }]
}
```

---

## Unit Conversions

### Native SI Units (No Conversion Required)
Most NMEA2000 wire format values are already in SI units:
- **Position:** Decimal degrees (not conversion needed)
- **Speed:** m/s in NMEA2000 `sog`, `speed` fields
- **Angles:** Radians in NMEA2000 `cog`, `heading`, `angle` fields
- **Pressure:** Pascals in NMEA2000 `pressure` field
- **Temperature:** Kelvin in NMEA2000 `temperature` field

### Conversions Applied
- **Humidity:** Percentage (0-100) → Ratio (0-1): `ratio = percentage / 100`
- **Heading:** Magnetic + variation → True: `true_heading = magnetic_heading + variation`
- **True Wind:** Calculated from apparent wind and SOG using vector subtraction

### Accessor Methods Used
The broadcaster uses raw NMEA2000 struct fields when they're already in SI units:
- `pos.latitude`, `pos.longitude` (decimal degrees)
- `cog_sog.sog`, `cog_sog.cog` (m/s, radians)
- `wind.speed`, `wind.angle` (m/s, radians)
- `pressure.pressure` (Pa)
- `temp.temperature` (K)

---

## Rate Limiting

Each SignalK path is rate-limited independently based on the `rate_limit_ms` configuration parameter (default 100ms = 10Hz).

This prevents overwhelming WebSocket clients with rapid updates while allowing different signal types to update at their natural rates.

---

## Source Metadata

Each delta update includes source information identifying the NMEA2000 device:

- **label:** `N2K-{source_address}` (e.g., "N2K-22")
- **type:** Always "NMEA2000"
- **src:** Source address as string (e.g., "22")
- **pgn:** PGN number (e.g., 129025)

---

## Configuration

Enable SignalK broadcasting in `config.json`:

```json
{
  "signalk": {
    "enabled": true,
    "rate_limit_ms": 100
  }
}
```

**Parameters:**
- `enabled`: Boolean to enable/disable SignalK broadcaster
- `rate_limit_ms`: Minimum milliseconds between updates for each path (default 100)

---

## Testing

### Visual Test Page

The application includes a built-in test page for viewing the SignalK stream in real-time:

```
http://localhost:8080/signalk-browser.html
```

**Features:**
- Live WebSocket connection management (Connect/Disconnect buttons)
- Real-time connection status indicator (connected/disconnected/connecting)
- Message display with timestamps and JSON formatting
- Statistics: total messages received, total updates, unique paths
- Path-based filtering to view specific SignalK paths
- Auto-scroll option for newest messages
- Clear messages button
- Dark/Light theme support
- Displays last 100 messages (automatic cleanup)

**Screenshots:**
The test page shows:
1. Connection status with visual indicator
2. Statistics dashboard (message count, update count, path count)
3. Control buttons (Connect, Disconnect, Clear)
4. Path filter tags (click to filter by specific SignalK path)
5. Real-time message feed with JSON-formatted delta messages

### Command-Line Testing

Connect to the SignalK stream using a WebSocket client:

```bash
wscat -c ws://localhost:8080/signalk/v1/stream
```

You should receive SignalK delta messages as JSON:

```json
{
  "context": "vessels.self",
  "updates": [...]
}
```

---

## References

- **SignalK Specification:** https://signalk.org/specification/1.7.0/doc/
- **NMEA2000 PGN Reference:** See `pgns.json`
- **Implementation:** `src/signalk_broadcaster.rs`
