# WebSocket Real-Time Data Streaming

## Overview

The NMEA2000 Router includes a WebSocket endpoint for streaming real-time vessel data to connected clients. This enables live dashboards, mobile apps, and other real-time monitoring applications to receive updates on vessel position, navigation, wind, environmental conditions, and system time synchronization.

## Endpoint

- **URL**: `ws://localhost:PORT/api/ws/realtime`
- **Default Port**: 8080 (configurable in config.json)
- **Protocol**: WebSocket (RFC 6455)

## Message Format

All messages are sent as JSON text frames with the following structure:

```json
{
  "message_type": "position|wind|environmental|navigation|system_time",
  "timestamp": 1700000000000,
  "data": {
    "latitude": 52.3667,
    "longitude": 4.9045,
    "cog_deg": 180.5,
    "sog_kn": 5.2,
    "heading_deg": 179.8,
    ...
  }
}
```

## Message Types and Data Fields

### System Time Updates (`message_type: "system_time"`)

Sent when system or GNSS time synchronization data is available. Throttled to max 1 update per second.

```json
{
  "system_timestamp": 1700000000000,
  "gnss_timestamp": 1700000000050,
  "time_skew_ms": 50,
  "time_sync_status": "synced"
}
```

**Fields:**
- `system_timestamp` (integer, optional): System time in Unix milliseconds
- `gnss_timestamp` (integer, optional): GNSS/GPS time in Unix milliseconds
- `time_skew_ms` (integer, optional): Time difference between system and GNSS time in milliseconds
- `time_sync_status` (string, optional): Time synchronization status: "synced" or "not_synced"

### Position Updates (`message_type: "position"`)

Sent when vessel position changes. Throttled to max 1 update per second.

```json
{
  "latitude": 52.3667,
  "longitude": 4.9045,
  "cog_deg": 180.5,
  "sog_kn": 5.2,
  "heading_deg": 179.8
}
```

**Fields:**
- `latitude` (float, optional): Latitude in decimal degrees
- `longitude` (float, optional): Longitude in decimal degrees
- `cog_deg` (float, optional): Course over ground in degrees (0-360)
- `sog_kn` (float, optional): Speed over ground in knots
- `heading_deg` (float, optional): True heading in degrees (0-360)

### Wind Updates (`message_type: "wind"`)

Sent when wind data is available. Throttled to max 1 update per second.

```json
{
  "apparent_wind_speed_kn": 8.5,
  "apparent_wind_angle_deg": 45.2,
  "true_wind_speed_kn": 10.2,
  "true_wind_angle_deg": 30.5
}
```

**Fields:**
- `apparent_wind_speed_kn` (float, optional): Apparent wind speed in knots
- `apparent_wind_angle_deg` (float, optional): Apparent wind angle in degrees, relative to bow (0-360)
- `true_wind_speed_kn` (float, optional): True wind speed in knots
- `true_wind_angle_deg` (float, optional): True wind angle in degrees, relative to bow (0-360)

### Environmental Updates (`message_type: "environmental"`)

Sent when environmental sensor data is available. Throttled to max 1 update per second.

```json
{
  "barometric_pressure_pa": 101325.0,
  "cabin_temperature_c": 20.5,
  "sea_temperature_c": 15.3,
  "humidity_percent": 65.2,
  "depth_m": 12.4
}
```

**Fields:**
- `barometric_pressure_pa` (float, optional): Barometric pressure in Pascals
- `cabin_temperature_c` (float, optional): Cabin temperature in Celsius
- `sea_temperature_c` (float, optional): Sea/water temperature in Celsius
- `humidity_percent` (float, optional): Relative humidity in percentage (0-100)
- `depth_m` (float, optional): Water depth in meters

### Navigation Updates (`message_type: "navigation"`)

Sent for combined navigation data. Throttled to max 1 update per second.

```json
{
  "cog_deg": 180.5,
  "sog_kn": 5.2,
  "heading_deg": 179.8
}
```

**Fields:**
- `cog_deg` (float, optional): Course over ground in degrees
- `sog_deg` (float, optional): Speed over ground in knots
- `heading_deg` (float, optional): True heading in degrees

## Throttling

Each message type is independently throttled to prevent excessive data transmission:
- Maximum 1 update per second per message type
- If a new value arrives before 1 second has passed since the last send, it is silently discarded
- This prevents network congestion and reduces client processing load

## Client Example (JavaScript)

```javascript
// Connect to the WebSocket
const ws = new WebSocket('ws://localhost:8080/api/ws/realtime');

ws.onopen = function(event) {
  console.log('Connected to real-time data stream');
};

ws.onmessage = function(event) {
  const message = JSON.parse(event.data);
  
  switch (message.message_type) {
    case 'position':
      console.log('Position:', message.data.latitude, message.data.longitude);
      updateMap(message.data);
      break;
      
    case 'wind':
      console.log('Wind:', message.data.true_wind_speed_kn, 'knots');
      updateWindDisplay(message.data);
      break;
      
    case 'environmental':
      console.log('Temperature:', message.data.sea_temperature_c, '°C');
      updateEnvironmentalDisplay(message.data);
      break;
      
    case 'navigation':
      console.log('Heading:', message.data.heading_deg, '°');
      updateCompass(message.data);
      break;
      
    case 'system_time':
      console.log('Time status:', message.data.time_sync_status, 'Skew:', message.data.time_skew_ms, 'ms');
      updateTimeDisplay(message.data);
      break;
  }
};

ws.onerror = function(event) {
  console.error('WebSocket error:', event);
};

ws.onclose = function(event) {
  console.log('Disconnected from real-time data stream');
};
```

## Data Integration Guide

To broadcast real-time data from the NMEA2000 message handlers, use the `get_broadcast_channels()` function from the main event loop:

```rust
use crate::web::get_broadcast_channels;
use crate::web::websocket::RealtimeVesselData;

// In your message handler or data processing code:
if let Some(channels) = get_broadcast_channels() {
    // Create a RealtimeVesselData struct with current values
    let data = RealtimeVesselData {
        timestamp: now.elapsed().as_millis() as i64,
        latitude: Some(position.lat),
        longitude: Some(position.lon),
        cog_deg: Some(cog),
        sog_kn: Some(sog),
        heading_deg: Some(heading),
        // ... set other fields as available
        ..Default::new()  // or set remaining fields to None
    };
    
    // Broadcast the data
    if let Err(e) = channels.broadcast_position(data) {
        warn!("Failed to broadcast position: {}", e);
    }
}
```

### Integration Points

The following NMEA2000 messages can be used as data sources:

1. **Position Data** (PGN 129025 - Position Rapid Update)
   - Use for `latitude`, `longitude` in position updates

2. **COG/SOG** (PGN 129026 - COG and SOG Rapid Update)
   - Use for `cog_deg`, `sog_kn` in position and navigation updates

3. **Heading** (PGN 127250 - Vessel Heading)
   - Use for `heading_deg` in position and navigation updates

4. **Wind** (PGN 130306 - Wind Speed and Direction)
   - Use for `apparent_wind_speed_kn`, `apparent_wind_angle_deg`, `true_wind_speed_kn`, `true_wind_angle_deg` in wind updates

5. **Temperature** (PGN 130312 - Temperature)
   - Use for `cabin_temperature_c`, `sea_temperature_c` in environmental updates

6. **Humidity** (PGN 130313 - Humidity)
   - Use for `humidity_percent` in environmental updates

7. **Pressure** (PGN 130314 - Atmospheric Pressure)
   - Use for `barometric_pressure_pa` in environmental updates

8. **Depth** (PGN 128267 - Water Depth)
   - Use for `depth_m` in environmental updates

9. **System Time** (PGN 126992 - System Time)
   - Use for `system_timestamp`, `gnss_timestamp`, `time_skew_ms`, `time_sync_status` in system_time updates

## Performance Considerations

- Broadcast channels use an in-memory buffer of 10 messages per type
- If there are no subscribers, messages are discarded after the buffer fills
- Each connected client spawns a tokio task to handle outgoing messages
- Throttling is applied per message type, not per field
- Clients can connect/disconnect at any time without affecting other clients

## Status

This endpoint is currently in the infrastructure phase. The following has been implemented:

✅ WebSocket endpoint (`/api/ws/realtime`)
✅ Message type definitions and JSON serialization  
✅ Per-type throttling (1 msg/sec max)
✅ Broadcast channel infrastructure
✅ Global broadcast manager

⏳ **To be implemented:**
- Data source integration in message handlers
- Real-time data population from NMEA2000 messages
- Client validation and authentication (if needed)
- Bandwidth optimization (delta/diff messaging)
- Message compression for high-speed data

## Troubleshooting

### No messages received
- Check that the web server is running: `curl http://localhost:8080/api/trips`
- Verify the WebSocket URL is correct and port matches config.json
- Check browser/client WebSocket support (use `ws://` not `wss://` for unencrypted)

### Connection refused
- Ensure web server is enabled in config.json: `"enabled": true` under `[web]`
- Check firewall rules - WebSocket uses HTTP upgrade
- Verify no other process is using the configured port

### Throttling too aggressive
- The 1-second throttle per message type is hardcoded. To change it, modify `ThrottleTracker::min_interval` in `src/web/websocket.rs`
- This can be made configurable via config.json if needed
