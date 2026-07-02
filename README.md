# NMEA2000 Router


A robust Rust application for monitoring NMEA2000 marine data networks, with intelligent database persistence, automatic time synchronization, and advanced wind data/statistics handling.

This project is a learning and production-grade effort, inspired by https://github.com/titio72/nmearouter and leveraging the excellent reverse engineering work at https://github.com/canboat/canboat.

## Features

- **CAN Bus Integration**: Reads NMEA2000 messages from SocketCAN interfaces
- **Comprehensive PGN Support**: 
  - Position (129025, 129029)
  - Speed & Heading (129026, 127250, 127251)
  - Environmental Data (130306, 130312, 130313, 130314)
  - Attitude/Roll (127257)
  - Depth & Water Speed (128267, 128259)
  - Engine Data (127488)
  - System Time (126992)
  - AIS Position Reports (129038, 129039, 129040)
  - AIS Aid-to-Navigation (129041)
  - AIS Static Data (129794, 129809, 129810)
  - AIS UTC & Date Report (129793)
- **REST API & Web Interface**: JSON endpoints for trips, track data, environmental metrics, and speed distribution analysis, plus responsive web dashboard with interactive charts and Google Maps integration
- **SignalK v1.7.0 Broadcaster**: Real-time WebSocket streaming of vessel data in SignalK delta format with SI units (m/s, radians, Kelvin, Pa) for position, speed, heading, wind, temperature, humidity, and pressure
- **Adaptive Database Persistence**:
  - Moored vessels: 5-minute intervals
  - Underway vessels: 30-second intervals
  - Per-metric environmental intervals
- **Database Resilience**:
  - Health checks every 60 seconds
  - Automatic retry on failed writes
  - Transaction atomicity for vessel status and trip updates
  - Continues operation if database unavailable
- **Time Synchronization Protection**: Blocks database writes when NMEA2000 time differs from system time by more than 1000ms (configurable)
- **Configuration Validation**: Comprehensive validation with auto-correction and sensible defaults
- **CLI Options**: Test configuration (--validate-config), display help (--help)
- **Automatic Reconnection**: Retries CAN interface connection every 10 seconds on failure
- **JSON Configuration**: Externalized configuration for all runtime parameters
- **Mooring Detection**: Automatically detects when vessel is moored based on position history
- **Comprehensive Unit Tests**: 80+ tests covering core functionality, wind calculations, configuration validation, and safe deserialization
- **Advanced Wind Data Handling**: Calculates and persists true wind speed/angle, with robust rolling window averaging and test coverage
- **AIS Target Tracking**: Decodes and broadcasts AIS position reports, static data, and navigation information via SignalK with live web dashboard for monitoring nearby vessels and navigation aids
- **Trips Viewer Sync**: One-command push of collected trips from the boat to a cloud-hosted read-only viewer (`POST /api/sync/push`). Incremental transfer, full reconciliation (handles deletes), authenticated with a shared API key. See [TRIPS_VIEWER.md](TRIPS_VIEWER.md) for setup.

## Requirements

- **Rust**: Edition 2024 or later
- **SocketCAN**: Linux CAN bus interface (use `vcan0` for testing)
- **MariaDB/MySQL**: Version 10.5+ or MySQL 8.0+

## Installation

### 1. Clone and Build

```bash
git clone <repository-url>
cd rust_nmea_router
cargo build --release
```


### 2. Set Up Database

See [README_DATABASE.md](README_DATABASE.md) for detailed instructions and schema explanations.

Quick setup:

```bash
# Create database and user
sudo mysql -e "CREATE DATABASE nmea_router;"
sudo mysql -e "CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';"
sudo mysql -e "GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';"

# Load schema
mysql -u nmea -pnmea nmea_router < schema.sql
```

### 3. Configure CAN Interface

For testing with virtual CAN:

```bash
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan
sudo ip link set up vcan0
```

For real CAN interface:

```bash
sudo ip link set can0 up type can bitrate 250000
```

## Configuration

Edit `config.json` to customize settings:

```json
{
  "can_interface": "vcan0",
  "time": {
    "skew_threshold_ms": 500
  },
  "database": {
    "connection": {
      "host": "localhost",
      "port": 3306,
      "username": "nmea",
      "password": "nmea",
      "database_name": "nmea_router"
    },
    "vessel_status": {
      "interval_moored_seconds": 1800,
      "interval_underway_seconds": 30
    },
    "environmental": {
      "wind_speed_seconds": 30,
      "wind_direction_seconds": 30,
      "roll_seconds": 30,
      "pressure_seconds": 120,
      "cabin_temp_seconds": 300,
      "water_temp_seconds": 300,
      "humidity_seconds": 300
    }
  }
}
```

### Configuration Options

#### CAN Interface
- `can_interface`: Name of the SocketCAN interface (e.g., `can0`, `vcan0`)
  - Must be alphanumeric, underscore, or hyphen characters only
  - Cannot be empty
  - Invalid values will cause startup failure

#### Time Synchronization
- `skew_threshold_ms`: Maximum allowed time difference between NMEA2000 and system time in milliseconds. Database writes are blocked when exceeded (default: 1000ms, minimum: 100ms)
- `set_system_time`: Enable automatic system time synchronization from NMEA2000 GPS time (default: false)
  - **Important**: Requires root/sudo privileges to set system time
  - Useful for systems without NTP or other time synchronization
  - When enabled and time skew is detected, automatically sets system time to NMEA2000 time
  - Recommended for embedded systems or vessels without internet connectivity
  - **Safe parsing**: Accepts boolean (`true`/`false`), strings (`"true"`, `"yes"`, `"1"`, `"on"`, `"enabled"`, or their negatives), or numbers (`1`/`0`)
  - **Error handling**: Any malformed or invalid value defaults to `false` (safe behavior)

#### Database Connection
- `host`: Database server hostname
- `port`: Database server port (default: 3306)
- `username`: Database username
- `password`: Database password
- `database_name`: Target database name

#### Vessel Status Intervals
- `interval_moored_seconds`: DB write interval when vessel is moored (default: 300, valid range: 30-3600)
- `interval_underway_seconds`: DB write interval when vessel is underway (default: 30, valid range: 10-600)

#### Environmental Metrics Intervals
Individual persistence intervals for each environmental metric (all values in seconds, valid range: 30-600):
- `wind_speed_seconds`: Wind speed persistence interval (default: 30)
- `wind_direction_seconds`: Wind direction persistence interval (default: 30)
- `roll_seconds`: Roll angle persistence interval (default: 30)
- `pressure_seconds`: Atmospheric pressure persistence interval (default: 120)
- `cabin_temp_seconds`: Cabin temperature persistence interval (default: 300)
- `water_temp_seconds`: Water temperature persistence interval (default: 300)
- `humidity_seconds`: Humidity persistence interval (default: 300)

### Configuration Validation

The application automatically validates the configuration on startup and applies the following rules:

#### PGN Filter Rules
- **Valid Range**: 50,000 - 200,000
- **Invalid Entries**: Automatically removed with a warning
- **Example**: PGN 12345 (too low) or PGN 250000 (too high) will be filtered out

#### Source Filter Rules
- **Valid Range**: 1 - 254
- **Invalid Entries**: Automatically removed with a warning
- **Example**: Source 0 or source 300 will be filtered out

#### Interval Validation
- **Valid Range**: 30 - 600 seconds
- **Out of Range**: Reverts to default value with a warning
- **Example**: `wind_speed_seconds: 10` (too low) will revert to default 30 seconds

#### Skew Threshold Validation
- **Minimum Value**: 100 milliseconds
- **Below Minimum**: Reverts to default 1000ms with a warning
- **Example**: `skew_threshold_ms: 50` will revert to 1000ms

All validation errors are logged with warnings but do not prevent startup (except for invalid CAN interface names).

### Environment Variable Overrides

Any config file value can be overridden at runtime via environment variables. This is the recommended approach for cloud deployments (e.g. Railway) where editing files after deployment is impractical.

Environment variables are applied **after** the config file is loaded, so the file acts as the baseline and env vars take precedence.

#### Database

| Variable | Config equivalent | Notes |
|---|---|---|
| `DATABASE_URL` | `database.connection.*` | Full URL: `mysql://user:pass@host:port/db` — also accepted: `mysql2://`, `mysqls://` |
| `MYSQL_URL` | `database.connection.*` | Alias for `DATABASE_URL` (tried second) |
| `MYSQL_PUBLIC_URL` | `database.connection.*` | Alias for `DATABASE_URL` (tried third) |
| `MYSQLHOST` | `database.connection.host` | Used only when no URL variable is set |
| `MYSQLPORT` | `database.connection.port` | Used only when no URL variable is set |
| `MYSQLUSER` | `database.connection.username` | Used only when no URL variable is set |
| `MYSQLPASSWORD` | `database.connection.password` | Used only when no URL variable is set |
| `MYSQLDATABASE` | `database.connection.database_name` | Used only when no URL variable is set |

#### Web Server

| Variable | Config equivalent | Notes |
|---|---|---|
| `PORT` | `web.port` | Listening port (Railway injects this automatically) |
| `AUTH_PASSWORD` | `web.auth_password` | Empty string disables authentication |
| `SECURE_COOKIES` | `web.secure_cookies` | Accepts `true`, `1`, `yes` |

#### Sync

| Variable | Config equivalent | Notes |
|---|---|---|
| `SYNC_API_KEY` | `sync.api_key` | Shared secret for push sync authentication |
| `SYNC_ENABLED` | `sync.enabled` | Accepts `true`, `1`, `yes` |
| `SYNC_TARGET_URL` | `sync.target_url` | URL of the remote trips-viewer instance |

#### Logging

| Variable | Config equivalent | Notes |
|---|---|---|
| `LOG_LEVEL` | `logging.level` | e.g. `info`, `debug`, `warn` |

## Usage

### Command Line Options

```bash
# Run the application normally
./target/release/nmea_router

# Validate configuration without running
./target/release/nmea_router --validate-config
# or
./target/release/nmea_router --validate
./target/release/nmea_router -v

# Display help
./target/release/nmea_router --help
./target/release/nmea_router -h
```

#### Validation Mode (--validate-config)

Tests the configuration file for errors without starting the application. Displays:
- CAN interface name
- All configured PGN filters
- All configured source filters
- Time skew threshold
- All database intervals (vessel status and environmental metrics)

Example output:
```
Configuration is valid!

Configuration Summary:
  CAN Interface: vcan0
  PGN Filters: 126992, 127250, 129025, 129026, 130306, 130312
  Source Filters: 1, 2, 3, 10
  Time Skew Threshold: 500 ms
  Vessel Status Intervals:
    Moored: 1800 seconds
    Underway: 30 seconds
  Environmental Intervals:
    Wind Speed: 30 seconds
    Wind Direction: 30 seconds
    Roll: 30 seconds
    Pressure: 120 seconds
    Cabin Temperature: 300 seconds
    Water Temperature: 300 seconds
    Humidity: 300 seconds
```

### Run the Application

```bash
./target/release/nmea_router
```

Or with cargo:

```bash
cargo run --release
```

### Expected Output

```
NMEA2000 Router - Starting...
Opening CAN interface: vcan0
✓ Successfully opened CAN interface: vcan0
Database connection established
Listening for NMEA2000 messages...

Position: 45.123456° N, -122.654321° W | Alt: 15.5m
Speed: 5.2 m/s (10.1 knots) | COG: 245° (Magnetic)
Heading: 243° (Magnetic) | ROT: 2.5°/s (right)

--- Metrics (60s) ---
  CAN Frames: 1234 (20.6/s)
  NMEA Messages: 987 (16.5/s)
  Vessel Reports: 12 (0.2/s)
  Env Reports: 45 (0.8/s)
  Errors: 0
-------------------
...
```

## Web Interface

The application includes a built-in web dashboard for visualizing trips, tracks, and environmental metrics.

### Configuration

Add web settings to your `config.json`:

```json
{
  "web": {
    "enabled": true,
    "port": 8080
  }
}
```

- `enabled`: Enable or disable the web server (default: `true`)
- `port`: HTTP port to listen on (default: `8080`)

### Accessing the Dashboard

Once the application is running, open your browser and navigate to:

```
http://localhost:8080
```

Or from another device on the same network:

```
http://<your-server-ip>:8080
```

### Google Maps Integration

The trip detail pages include interactive Google Maps showing GPS tracks. To enable this feature:

1. **Get a Google Maps API Key**:
   - Visit the [Google Cloud Console](https://console.cloud.google.com/)
   - Create a new project or select an existing one
   - Enable the "Maps JavaScript API"
   - Create credentials (API Key)
   - Optionally restrict the API key to your domain for security

2. **Configure the API Key**:
   - Open `static/trip.html` in your web browser's developer tools or a text editor
   - Replace `YOUR_API_KEY_HERE` in the Google Maps script tag with your actual API key:
   ```html
   <script async defer src="https://maps.googleapis.com/maps/api/js?key=YOUR_ACTUAL_API_KEY"></script>
   ```

3. **API Key Security Note**: 
   - For production use, consider server-side API key management
   - Restrict the API key to specific domains and enable only the required APIs
   - Monitor API usage in the Google Cloud Console

### Features

#### Trip Dashboard (Landing Page)
- **Trip List**: View all recorded trips from the last 12 months with detailed statistics
- **Trip Cards**: Each trip shows:
  - Start/end dates and duration
  - Total distance traveled
  - Time distribution (sailing/motoring/moored) with visual progress bars and percentages
  - Distance distribution (sailing vs motoring) with percentages
  - Clickable cards that navigate to detailed trip view

#### Trip Detail View
- **Trip Overview**: Complete trip information including start/end times, total distance, and activity breakdowns
- **Interactive Map**: Google Maps integration showing the complete GPS track with:
  - Red track line showing the vessel's path
  - Green start marker and red end marker
  - Satellite view by default with map type controls
  - Clickable markers showing start/end timestamps
  - Automatic zoom to fit the entire track
- **Interactive Charts**: Four charts displaying:
  - **Boat Speed Chart**: Average speed over time (filtered for underway periods)
  - **Boat Heading Chart**: Compass heading in degrees (0-360°)
  - **Wind Speed Chart**: Wind speed in knots over time
  - **Wind Direction Chart**: Wind direction in degrees (0-360°) with compass labels
- **Data Source**: Charts use track data retrieved via `/api/track?trip_id=X` endpoint
- **Responsive Design**: Charts adapt to screen size, arranged in a 2x2 grid on larger screens

#### Summary Statistics
- Total number of trips in the selected period
- Combined distance, time, and activity breakdowns
- Real-time updates when new data is recorded

#### REST API Endpoints

The web interface exposes JSON endpoints for programmatic access:

##### GET /api/trips
List all trips with filtering options.

Query parameters:
- `year`: Filter by specific year (e.g., `?year=2024`)
- `last_months`: Show trips from last N months (e.g., `?last_months=12`)

Example response:
```json
{
  "status": "ok",
  "data": [
    {
      "id": 1,
      "start_date": "2024-01-15 08:30:00",
      "end_date": "2024-01-15 17:45:00",
      "total_distance_nm": 25.3,
      "total_time_ms": 33300000,
      "sailing_time_ms": 20000000,
      "motoring_time_ms": 10000000,
      "moored_time_ms": 3300000,
      "sailing_distance_nm": 18.5,
      "motoring_distance_nm": 6.8
    }
  ]
}
```

##### GET /api/track
Retrieve vessel track data (GPS points).

Query parameters:
- `trip_id`: Get track for specific trip (e.g., `?trip_id=1`)
- `start` & `end`: Get track for date range (e.g., `?start=2024-01-15&end=2024-01-16`)

Example response:
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2024-01-15 08:30:00",
      "latitude": 43.630127,
      "longitude": 10.293377,
      "avg_speed_ms": 2.5,
      "max_speed_ms": 3.2,
      "moored": false,
      "engine_on": true
    }
  ]
}
```

##### GET /api/metrics
Retrieve environmental metric time series.

Query parameters:
- `metric`: Metric ID (required) - e.g., `wind_speed`, `cabin_temp`, `pressure`, `humidity`
- `trip_id`: Filter by trip
- `start` & `end`: Filter by date range

Example response:
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2024-01-15 08:30:00",
      "metric_id": "wind_speed",
      "avg_value": 5.2,
      "max_value": 7.8,
      "min_value": 3.1,
      "count": 120
    }
  ]
##### GET /api/speed_distribution
Retrieve speed distribution histogram data for a trip, showing time spent at different speed ranges.

Query parameters:
- `id`: Trip ID (required) - e.g., `?id=1`

Example response:
```json
{
  "status": "ok",
  "data": {
    "labels": ["0.0-0.5", "0.5-1.0", "1.0-1.5", ...],
    "sailing": [0.02, 0.0, 0.0, ...],
    "motoring": [0.002, 0.02, 0.1, ...]
  }
}
```

The response contains:
- `labels`: Speed range buckets in knots (0.5 knot increments)
- `sailing`: Percentage of time spent sailing in each speed range
- `motoring`: Percentage of time spent motoring in each speed range

## SignalK WebSocket Streaming

The application includes a SignalK v1.7.0 broadcaster that streams real-time vessel data in SignalK delta format over WebSocket connections.

### What is SignalK?

SignalK is a modern, open-source data format for marine systems that provides a standardized way to exchange sensor and navigation data. It uses JSON for data representation and WebSocket for real-time streaming.

### Configuration

Enable SignalK broadcasting in your `config.json`:

```json
{
  "signalk": {
    "enabled": true,
    "rate_limit_ms": 100
  }
}
```

- `enabled`: Enable or disable the SignalK broadcaster (default: `false`)
- `rate_limit_ms`: Minimum milliseconds between updates for each SignalK path (default: `100` = 10Hz)

### WebSocket Endpoint

Once enabled and the application is running, connect to:

```
ws://localhost:8080/signalk/v1/stream
```

Or from another device on the same network:

```
ws://<your-server-ip>:8080/signalk/v1/stream
```

### Testing the Stream

**Visual Test Page:**

The application includes a built-in test page for viewing the SignalK stream:

```
http://localhost:8080/signalk-browser.html
```

Features:
- Live connection status indicator
- Real-time message display with timestamps
- Message and update counters
- Path-based filtering (filter by specific SignalK paths)
- Auto-scroll with newest messages at top
- Theme support (dark/light mode)
- JSON formatting for easy reading

**Command-Line Testing:**

Use `wscat` or any WebSocket client to connect:

```bash
wscat -c ws://localhost:8080/signalk/v1/stream
```

### Supported Data Paths

The broadcaster converts NMEA2000 messages to SignalK paths with SI units:

| NMEA2000 PGN | SignalK Path | Units | Description |
|--------------|--------------|-------|-------------|
| 129025 | navigation.position | degrees | Latitude/Longitude |
| 129026 | navigation.speedOverGround | m/s | Speed over ground |
| 129026 | navigation.courseOverGroundTrue | radians | Course over ground (true) |
| 127250 | navigation.headingTrue | radians | Vessel heading (true) |
| 130306 | environment.wind.speedOverGround | m/s | True wind speed |
| 130306 | environment.wind.angleTrueGround | radians | True wind angle |
| 130306 | environment.wind.speedApparent | m/s | Apparent wind speed |
| 130306 | environment.wind.angleApparent | radians | Apparent wind angle |
| 130312 | environment.water.temperature | Kelvin | Water temperature |
| 130312 | environment.outside.temperature | Kelvin | Air/cabin temperature |
| 130313 | environment.outside.relativeHumidity | ratio (0-1) | Relative humidity |
| 130314 | environment.outside.pressure | Pascals | Atmospheric pressure |
| 126992 | navigation.datetime | ISO 8601 | GPS date/time |

### Message Format

SignalK delta messages follow the v1.7.0 specification:

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

### Features

- **SI Units**: All values in standard international units (m/s, radians, Kelvin, Pascals)
- **Rate Limiting**: Independent per-path throttling to prevent overwhelming clients
- **True Wind Calculation**: Automatic conversion from apparent wind using vessel speed
- **Source Metadata**: Each update includes NMEA2000 source address and PGN
- **RFC 3339 Timestamps**: Millisecond-precision ISO 8601 timestamps
- **Multi-Client Support**: Broadcast to multiple WebSocket clients simultaneously

### Integration

The SignalK stream integrates with:
- **SignalK Server**: Connect as a data provider
- **OpenPlotter**: Direct WebSocket connection
- **Navionics**: Via SignalK plugin
- **Custom Applications**: Any WebSocket client supporting SignalK v1.7.0

For detailed path mappings, unit conversions, and examples, see [docs/SIGNALK_MAPPING.md](docs/SIGNALK_MAPPING.md).

## AIS Target Tracking

The application decodes Automatic Identification System (AIS) messages from the NMEA2000 bus and broadcasts them via SignalK, enabling real-time monitoring of nearby vessels, navigation aids, and mobile units.

### What is AIS?

AIS is a maritime mobile messaging system intended for identifying and locating vessels. It broadcasts position, course, speed, and static vessel information, allowing mariners to track nearby traffic and aid-to-navigation markers.

### Supported AIS PGNs

The application decodes the following AIS message types:

| PGN | Message Type | Content |
|-----|--------------|---------|
| 129038 | Class A Position Report | Position, speed, course, heading, navigation status for Class A vessels |
| 129039 | Class B Position Report | Position, speed, course, heading for Class B (smaller) vessels |
| 129040 | Class B Extended Position Report | Class B position + vessel dimensions and name |
| 129041 | Aid-to-Navigation Report | Fixed navigation aids: buoys, beacons, lights, landmarks |
| 129793 | UTC Date Report | UTC time and date synchronization for AIS targets |
| 129794 | Class A Static Data | IMO number, call sign, vessel name, type, dimensions, ETA, destination |
| 129809 | Class B Static Data Part A | Vessel name and sequence information |
| 129810 | Class B Static Data Part B | Vessel type, call sign, dimensions, mothership ID |

### AIS Web Dashboard

Access the live AIS targets page:

```
http://localhost:8080/ais.html
```

**Features:**
- **Live Target Table**: Real-time list of detected AIS targets with:
  - MMSI (Maritime Mobile Service Identity)
  - Vessel name, call sign, and type
  - Position (latitude/longitude)
  - Speed over ground (knots)
  - Course over ground (degrees)
  - Heading (degrees, where available)
  - Navigation status (e.g., underway, at anchor, moored)
  - Last update timestamp
- **Search & Filter**: Filter targets by MMSI, name, or call sign
- **Sortable Columns**: Click column headers to sort by any field
- **Stale Detection**:
  - Grayed out: Targets not updated for 30+ minutes
  - Hidden: Targets not updated for 60+ minutes
  - Automatic removal prevents clutter from lost signals
- **Connection Status**: Indicator showing WebSocket connection status to SignalK stream
- **Theme Support**: Dark and light mode themes

### SignalK Integration for AIS

AIS targets are broadcast via SignalK using unique, per-target contexts rather than the shared `vessels.self` context:

**Context Format:**
```
vessels.urn:mrn:imo:mmsi:<MMSI>
```

Example: A vessel with MMSI 123456789 uses context `vessels.urn:mrn:imo:mmsi:123456789`.

**Supported SignalK Paths for AIS Targets:**

| PGN | SignalK Path | Units | Description |
|-----|--------------|-------|-------------|
| 129038, 129039, 129040 | navigation.position | degrees | Target position (latitude/longitude) |
| 129038, 129039, 129040 | navigation.speedOverGround | m/s | Vessel speed over ground |
| 129038, 129039, 129040 | navigation.courseOverGroundTrue | radians | Course over ground (true) |
| 129038, 129039, 129040 | navigation.headingTrue | radians | Vessel heading (true) |
| 129038 | navigation.state | string | Navigation status (e.g., "UnderWayEngine", "AtAnchor") |
| 129040, 129039 | design.length | meters | Vessel length |
| 129040, 129039 | design.beam | meters | Vessel beam (width) |
| 129041 | navigation.atonType | string | Aid-to-navigation type |
| 129794 | name | string | Vessel name |
| 129794, 129809, 129810 | communication.callsignVhf | string | VHF call sign |
| 129794, 129810 | design.aisShipType | string | IEC 61162-1 ship type |
| 129794 | design.draft | meters | Vessel draft |
| 129794 | navigation.destination | string | Destination port |
| All | sensors.ais.class | string | AIS class: "A", "B", or "AtoN" |

### AIS Message Format

SignalK delta messages for AIS targets follow the v1.7.0 specification with custom `vessels.urn:mrn:imo:mmsi:<MMSI>` context:

**Example: Class A Position Report (PGN 129038):**
```json
{
  "context": "vessels.urn:mrn:imo:mmsi:123456789",
  "updates": [{
    "source": {
      "label": "N2K-205",
      "type": "NMEA2000",
      "src": "205",
      "pgn": 129038
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [
      { "path": "navigation.position", "value": { "latitude": 43.630127, "longitude": 10.293377 } },
      { "path": "navigation.speedOverGround", "value": 5.144 },
      { "path": "navigation.courseOverGroundTrue", "value": 0.7854 },
      { "path": "navigation.headingTrue", "value": 0.8727 },
      { "path": "navigation.state", "value": "UnderWayEngine" },
      { "path": "sensors.ais.class", "value": "A" }
    ]
  }]
}
```

**Example: Aid-to-Navigation Report (PGN 129041):**
```json
{
  "context": "vessels.urn:mrn:imo:mmsi:993700001",
  "updates": [{
    "source": {
      "label": "N2K-205",
      "type": "NMEA2000",
      "src": "205",
      "pgn": 129041
    },
    "timestamp": "2026-02-17T12:00:00.000Z",
    "values": [
      { "path": "navigation.position", "value": { "latitude": 43.625000, "longitude": 10.295000 } },
      { "path": "navigation.atonType", "value": "Light" },
      { "path": "name", "value": "North Buoy #1" },
      { "path": "sensors.ais.class", "value": "AtoN" }
    ]
  }]
}
```

### Integration Points

- **WebSocket Clients**: Connect to `ws://localhost:8080/signalk/v1/stream` to receive AIS deltas
- **SignalK Server**: The application can act as a SignalK data provider for AIS targets
- **OpenPlotter**: Direct WebSocket integration for vessel monitoring
- **Custom Applications**: Any WebSocket client supporting SignalK v1.7.0 can consume AIS data

### Technical Details

- **No Database Storage**: AIS data is decoded and broadcast in real-time only (not persisted)
- **Per-Target Context**: Each AIS target uses a unique SignalK context, enabling multi-target tracking
- **Rate Limiting**: AIS messages respect the global SignalK rate limit (default 100ms per update)
- **Automatic Unit Conversion**: All values converted to SI units (m/s, radians, meters)
- **Source Tracking**: Each AIS update includes the NMEA2000 source address for multi-transponder scenarios

## NMEA0183 UDP Broadcasting

The application broadcasts NMEA2000 data as standard NMEA0183 sentences over UDP, enabling integration with chart plotters, marine instrumentation, and other legacy NMEA0183-compatible devices.

### What is NMEA0183?

NMEA0183 is the industry-standard marine data format used by GPS receivers, chart plotters, fish finders, and other marine electronics. UDP broadcasting allows multi-device reception on the local network.

### Configuration

Enable UDP broadcasting in your `config.json`:

```json
{
  "udp": {
    "enabled": true,
    "address": "192.168.1.255:10110"
  }
}
```

- `enabled`: Enable or disable UDP broadcasting (default: `false`)
- `address`: Broadcast destination (use `255.255.255.255:10110` for network-wide broadcast, or specific multicast/address)

### Rate Limiting

To prevent overwhelming receiving devices, the broadcaster enforces a **1 message per second per topic** rate limit. Each NMEA0183 sentence type (RMC, GGA, MWV, etc.) has its own 1-second throttle.

### Emitted Sentence Types

| Sentence | PGN Source | Content | Rate |
|----------|------------|---------|------|
| **RMC** | 129029 | Position, Speed over Ground, Course over Ground, Date/Time | 1/s |
| **GGA** | 129029 | Position, Altitude, Fix Quality, Satellite Count, HDOP | 1/s |
| **ZDA** | 126992 | Date and Time (UTC) | 1/s |
| **MWV** | 130306 | Wind Speed (knots) and Angle (degrees); Apparent (R) or True (T) | 1/s per type |
| **HDT** | 127250 | True Heading (degrees) | 1/s |
| **HDM** | 127250 | Magnetic Heading (degrees) | 1/s |
| **ROT** | 127251 | Rate of Turn (degrees/minute) | 1/s |
| **XDR** | 127257 | Attitude: Yaw, Pitch, Roll (degrees) | 1/s |
| **XDR** | 130312 | Temperature (water, air, cabin) in Celsius | 1/s per instance |
| **XDR** | 130313 | Humidity (percent) | 1/s per instance |
| **XDR** | 130314 | Barometric Pressure (bar) | 1/s per instance |
| **RPM** | 127488 | Engine RPM | 1/s per engine |
| **VHW** | 128259 | Water Speed (knots) | 1/s |
| **DPT** | 128267 | Water Depth (meters) and Offset | 1/s |

### Data Aggregation

**RMC & GGA Sentences**: These sentences combine data from multiple sources:
- **Position** (129025): Latitude/Longitude in DDMM.MMMM format
- **COG/SOG** (129026): Course over ground (degrees), Speed over ground (knots)
- **System Time** (126992): UTC date/time
- **GNSS Data** (129029): Altitude, fix quality, satellite count, HDOP (triggers RMC and GGA emission)

State is buffered across messages and emitted when GnssPositionData arrives, ensuring consistent position, speed, course, and time across both sentences.

### Conversion Details

- **Units**: Knots for speed, degrees for angles, meters for depth/altitude, Celsius for temperature, percent for humidity, bar for pressure
- **Wind**: Apparent wind converted from PGN 130306 reference fields; true wind when reference indicates true wind
- **Heading**: Magnetic headings sent as HDM; true headings sent as HDT
- **Talker ID**: All sentences use `II` (Integrated Instrumentation) talker ID for multi-device aggregation

### Example Usage

**Chart Plotter Integration:**
Most chart plotters can accept NMEA0183 input via UDP. Configure with:
- Destination: Broadcast address of the router (e.g., `192.168.1.100:10110`)
- Protocol: UDP
- Sentence filters: Select desired sentence types (RMC, GGA, MWV, etc.)

**Custom Software:**
Receive UDP packets and parse NMEA0183 sentences:

```bash
# On Linux/Mac
nc -ul 10110  # Listen and display raw NMEA0183 sentences
```

**Example sentences:**
```
$IIRMC,120000.00,A,5223.3876,N,01324.2288,E,5.2,45.0,T,,,140226
$IIGGA,120000.00,5223.3876,N,01324.2288,E,1,12,1.5,100.0,M,,M,,
$IIMWV,45.0,R,,5.2,N
$IIHDT,45.0,T
```

### Future Enhancements

Planned features for UDP broadcasting:
- Optional NMEA0183 checksum attachment (`*XX` format)
- GST sentences (GNSS Positional Error)
- Multiple transducer support for XDR sentences
- Configurable rate limits per sentence type

### Future Enhancements for Web Interface

Planned features:

- Trip comparison tools
- Export functionality (CSV, GPX)
- Additional environmental metric charts (temperature, pressure, humidity)
- Real-time data streaming
- User authentication and trip management
- Mobile app companion

### Database Resilience Features

#### Health Checks
The application performs database health checks every 60 seconds using a lightweight query (`SELECT 1`). If the check fails:
1. A warning is logged
2. Automatic reconnection is attempted with exponential backoff (1s, 2s, 4s)
3. Application continues reading CAN data during reconnection attempts

#### Automatic Retry
When a database write fails (e.g., connection lost):
1. The failed data is retained in memory
2. Reconnection is attempted (up to 3 attempts with exponential backoff)
3. The write operation is retried automatically (up to 2 attempts)
4. If all retries fail, the data is discarded and a warning is logged

This ensures that transient database issues don't cause data loss.

#### Transaction Atomicity
Vessel status and trip updates are wrapped in a database transaction:
- Both operations succeed together, or
- Both operations roll back together
- This prevents inconsistent data (e.g., status saved but trip update failed)

### Non-Blocking Operation
The CAN socket uses a 500ms read timeout to prevent blocking. This ensures that:
- Metrics are logged every 60 seconds even without CAN activity
- Database health checks run on schedule
- The application remains responsive

### Testing

Run the comprehensive test suite:

```bash
cargo test
```

Expected output:
```
running 86 tests
test result: ok. 86 passed; 0 failed; 0 ignored; 0 measured
```

Tests cover:
- Configuration validation (PGN ranges, source ranges, intervals, skew threshold)
- NMEA2000 message parsing for all 13 supported PGNs
- Fast packet assembly
- Vessel monitoring (position tracking, mooring detection, speed calculations)
- Environmental monitoring (statistics, per-metric intervals)
- Time synchronization
- Database operations

## Architecture

### Core Components

1. **Main Loop** ([main.rs](src/main.rs))
   - CAN frame reading with automatic reconnection and 500ms timeout
   - Message processing and routing
   - Database write coordination with retry logic
   - Health checks every 60 seconds
   - Metrics logging every 60 seconds

2. **Configuration** ([config.rs](src/config.rs))
   - JSON-based configuration loading
   - Comprehensive validation with auto-correction
   - Default values with type-safe access
   - Duration conversions for intervals
   - CLI validation mode support

3. **Time Monitor** ([time_monitor.rs](src/time_monitor.rs))
   - Tracks time skew between NMEA2000 and system time
   - Blocks database writes when time is not synchronized
   - Configurable threshold with warning cooldown

4. **Vessel Monitor** ([vessel_monitor.rs](src/vessel_monitor.rs))
   - Tracks vessel position, speed, and heading
   - Detects mooring status using the mooring detection algorithm
   - Adaptive database persistence (moored vs underway)

5. **Environmental Monitor** ([environmental_monitor.rs](src/environmental_monitor.rs))
   - Tracks wind, temperature, pressure, humidity, roll
   - Calculates statistics (avg, min, max, count) on demand per metric
   - Per-metric persistence intervals for efficient storage
   - Metric-by-metric database writes for optimal performance

6. **Database** ([src/db/mod.rs](src/db/mod.rs))
   - Connection pool management with health checks
   - Transaction support for atomic operations
   - Vessel status and trip inserts (atomic)
   - Environmental metrics inserts
   - Automatic reconnection with exponential backoff

7. **PGN Decoders** ([nmea2k/src/pgns/](nmea2k/src/pgns/))
   - Individual decoders for each supported PGN
   - Binary data parsing with validation
   - Unit conversions (radians to degrees, Kelvin to Celsius, etc.)

### Data Flow

```
CAN Bus (vcan0/can0) [500ms timeout]
    ↓
SocketCAN Interface
    ↓
StreamReader (Fast Packet Assembly)
    ↓
PGN Decoders (Binary → Structured Data)
    ↓
Monitors (Vessel/Environmental/Time)
    ↓
Database (MariaDB) [if time synchronized]
    ├─ Health Check (every 60s)
    ├─ Retry Logic (up to 2 attempts)
    └─ Transactions (atomic vessel status + trip)
```

### Mooring Detection Algorithm

The application automatically detects when a vessel is moored using a velocity-based algorithm:

1.  Maintains a 3-minute sliding window of speed (Velocity Made Good) samples.
2.  If 90% of the samples in the window are below 0.1 knots, the vessel is considered moored.
3.  Mooring status affects the database persistence interval:
    *   **Moored**: 5-minute intervals (reduces database load)
    *   **Underway**: 30-second intervals (higher resolution tracking)

### Time Synchronization Protection

To prevent incorrect timestamps in the database:

1. **Time Skew Monitoring**: Compares NMEA2000 system time (PGN 126992) with server system time
2. **Threshold Check**: If skew exceeds configured threshold (default 500ms), database writes are blocked
3. **Warning Display**: Shows formatted warning with current skew and threshold
4. **Automatic System Time Setting** (optional):
   - Enable `set_system_time: true` in config to automatically sync system clock
   - Requires root/sudo privileges: `sudo ./nmea_router`
   - Ideal for embedded systems or vessels without NTP/internet connectivity
   - When time skew is detected, sets system time to NMEA2000 GPS time
   - Success/failure messages displayed with detailed information
5. **Cooldown**: Warnings are displayed every 10 seconds to avoid console spam
6. **Automatic Recovery**: When time resynchronizes, database writes resume automatically

**Example Configuration**:
```json
{
  "time": {
    "skew_threshold_ms": 1000,
    "set_system_time": true
  }
}
```

**Flexible Configuration Formats**:
The `set_system_time` field accepts various formats for convenience:
```json
// Boolean values
"set_system_time": true
"set_system_time": false

// String values (case-insensitive)
"set_system_time": "true"
"set_system_time": "yes"
"set_system_time": "enabled"
"set_system_time": "on"
"set_system_time": "1"

// Numeric values
"set_system_time": 1    // treated as true
"set_system_time": 0    // treated as false
```

**Error Handling**:
- Invalid or malformed values default to `false` (safe behavior)
- Missing field defaults to `false`
- Application logs a warning for unrecognized values but continues to run

**Running with System Time Setting**:
```bash
# Requires root privileges
sudo ./target/release/nmea_router

# Or use capabilities (Linux)
sudo setcap 'cap_sys_time=ep' ./target/release/nmea_router
./target/release/nmea_router
```

### Adaptive Persistence

Environmental metrics are persisted individually to the database at different intervals based on their update frequency and importance. When a metric's persistence interval is reached, the system:

1. **Calculates Statistics**: Computes avg, min, max, and count from collected samples
2. **Writes to Database**: Inserts a single row with the calculated statistics
3. **Clears Samples**: Removes processed samples to conserve memory
4. **Updates Timestamp**: Marks the metric as persisted for interval tracking

This metric-by-metric approach provides:
- **Efficient Storage**: Only stores aggregated statistics, not every sample
- **Flexible Intervals**: Each metric can have its own persistence rate
- **Memory Efficiency**: Samples are cleared after processing
- **Better Query Performance**: Pre-aggregated data reduces database load

| Metric | Default Interval | Rationale |
|--------|-----------------|-----------|
| Wind Speed | 30s | Changes rapidly, important for sailing |
| Wind Direction | 30s | Changes rapidly, important for sailing |
| Roll Angle | 30s | Important for stability monitoring |
| Pressure | 120s | Changes slowly, weather trends |
| Cabin Temp | 300s | Changes very slowly |
| Water Temp | 300s | Changes very slowly |
| Humidity | 300s | Changes very slowly |

Each metric is persisted independently based on its configured interval.

## Supported PGNs

| PGN | Name | Data |
|-----|------|------|
| 126992 | System Time | Date, Time, Milliseconds |
| 127250 | Vessel Heading | Heading (Magnetic/True) |
| 127251 | Rate of Turn | ROT (degrees/second) |
| 127257 | Attitude | Yaw, Pitch, Roll |
| 127488 | Engine Data | Engine RPM |
| 128259 | Speed (Water Referenced) | Speed through water |
| 128267 | Water Depth | Depth, Offset |
| 129025 | Position Rapid Update | Latitude, Longitude |
| 129026 | COG & SOG Rapid Update | Course, Speed over ground |
| 129029 | GNSS Position Data | Lat, Lon, Altitude |
| 130306 | Wind Data | Speed, Direction, Reference |
| 130312 | Temperature | Various sources (cabin, water, etc.) |
| 130313 | Humidity | Relative humidity |
| 130314 | Actual Pressure | Atmospheric pressure |

## Database Schema

### `vessel_status` Table

Stores vessel position, speed, wind, and navigation data.

```sql
CREATE TABLE vessel_status (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  timestamp DATETIME(3) NOT NULL COMMENT 'Report generation time in UTC with millisecond precision',
  latitude DOUBLE COMMENT 'Vessel latitude in decimal degrees (NULL if no position fix)',
  longitude DOUBLE COMMENT 'Vessel longitude in decimal degrees (NULL if no position fix)',
  average_speed_kn DECIMAL(6,3) NOT NULL COMMENT 'Average speed over reporting period in knots',
  max_speed_kn DECIMAL(6,3) NOT NULL COMMENT 'Maximum speed over reporting period in knots',
  average_wind_speed_kn DECIMAL(6,3) COMMENT 'Average wind speed over reporting period in knots (NULL if no wind data)',
  average_wind_angle_deg DECIMAL(6,3) COMMENT 'Average wind direction over reporting period in degrees (NULL if no wind data)',
  is_moored BOOLEAN NOT NULL COMMENT 'TRUE if vessel is moored (position stable for 2+ minutes within 30m radius)',
  engine_on TINYINT NOT NULL DEFAULT 2 COMMENT '1 if engine is running, 0 if the engine is off, and 2 if unknown',
  total_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled since last report in nautical miles (straight-line Haversine)',
  total_time_ms BIGINT NOT NULL DEFAULT 0 COMMENT 'Time elapsed since last report in milliseconds',
  INDEX idx_timestamp (timestamp),
  INDEX idx_moored (is_moored, timestamp)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci
COMMENT='Stores vessel navigation status reports';
```

### `environmental_data` Table

Stores environmental sensor data with calculated statistics per metric per persistence interval.

```sql
CREATE TABLE environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    metric_id TINYINT UNSIGNED NOT NULL,
    value_avg FLOAT,
    value_max FLOAT,
    value_min FLOAT,
    unit CHAR(3),
    UNIQUE KEY unique_metric_time (timestamp, metric_id),
    INDEX idx_timestamp (timestamp),
    INDEX idx_metric_timestamp (metric_id, timestamp)
);
```

**Metric IDs:**
- 1: Pressure (Pa)
- 2: Cabin Temperature (°C)
- 3: Water Temperature (°C)
- 4: Humidity (%)
- 5: Wind Speed (knots)
- 6: Wind Direction (degrees)
- 7: Roll Angle (degrees)

**Storage Approach:**
- Each metric is persisted independently at its configured interval
- Statistics (avg, min, max) are calculated from collected samples
- Samples are cleared after persistence to conserve memory
- `UNIQUE KEY` prevents duplicate data for the same metric/timestamp

See [README_DATABASE.md](README_DATABASE.md) for detailed schema information and query examples.

## Troubleshooting

### CAN Interface Not Found

```
⚠️  Failed to open CAN interface 'vcan0': No such device
   Retrying in 10 seconds...
```

**Solution**: Set up the CAN interface (see Configuration section)

### Database Connection Failed

```
Warning: Failed to connect to database: Access denied
Continuing without database logging...
```

**Solution**: Verify database credentials in `config.json` and ensure database exists

**Note**: The application will automatically retry connection every 60 seconds via health checks

### Database Write Failed

```
Warning: Failed to insert vessel status, will retry...
Attempting database reconnection (attempt 1/3)...
```

**Solution**: 
- Check database server is running
- Verify network connectivity
- The application will automatically retry up to 2 times with 3 reconnection attempts each
- Failed data will be retained and retried after successful reconnection

### Configuration Validation Errors

```
Warning: PGN 12345 is outside valid range (50000-200000), removing from filter
Warning: Source 300 is outside valid range (1-254), removing from filter
Warning: wind_speed_seconds (10) is outside valid range (30-600), using default: 30
```

**Solution**: 
- Check `config.json` for invalid values
- Run `./target/release/nmea_router --validate-config` to test configuration
- The application will auto-correct invalid values but may not behave as expected

### Time Skew Warning

```
╔════════════════════════════════════════════════════════════╗
║  ⚠️  TIME SKEW WARNING                                     ║
╠════════════════════════════════════════════════════════════╣
║  NMEA2000 time is BEHIND system time by 1250 ms           ║
║  ⚠️  DATABASE WRITES DISABLED UNTIL TIME SYNC              ║
╚════════════════════════════════════════════════════════════╝
```

**Solution**: 
- Check NMEA2000 network time source (GPS)
- Verify system time is correct
- Adjust `skew_threshold_ms` in config if needed


## Development & Testing

### Running Tests

```bash
# All tests
cargo test

# With output
cargo test -- --nocapture

# Specific module
cargo test config::tests
cargo test vessel_monitor::tests
cargo test utilities::tests  # wind calculation and angle math
```

### Code Structure

```
src/
├── main.rs                    # Application entry point
├── config.rs                  # Configuration management
├── db.rs                      # Database operations
├── time_monitor.rs            # Time synchronization
├── vessel_monitor.rs          # Vessel tracking
├── environmental_monitor.rs   # Environmental data
├── stream_reader.rs          # NMEA2000 frame assembly
└── pgns/                     # PGN decoders
    ├── mod.rs
    ├── attitude.rs          # Attitude
    ├── cog_sog.rs           # COG & SOG Rapid
    ├── depth.rs          # Water Depth
    ├── engine.rs          # Engine Data
    ├── heading.rs          # Vessel Heading
    ├── humidity.rs          # Humidity
    ├── position.rs          # Position Rapid
    ├── pressure.rs          # Actual Pressure
    ├── rate_of_turn.rs          # Rate of Turn
    ├── speed.rs          # Speed (Water)
    ├── system_time.rs          # System Time
    ├── temperature.rs          # Temperature
    └── wind.rs          # Wind Data
```

## Development Guidelines & Architecture

For detailed architectural guidance, coding conventions, testing strategies, and best practices, refer to **[AGENTS.md](AGENTS.md)**. This document contains:

- **Code Style & Conventions**: Function naming (`snake_case`), struct naming (`PascalCase`), module organization
- **Timestamp & Duration Rules**: UTC-only timestamps, millisecond durations, event handler patterns
- **Unit Standards**: Knots, nautical miles, Celsius, Pascals, decimal degrees
- **Database Patterns**: Parameterized queries, transaction patterns, test infrastructure
- **Testing Strategy**: Test helpers, data generation, tolerance-based assertions, serial execution
- **Architecture Principles**: Layered design, state management, event-driven patterns
- **Common Patterns**: Error handling, type conversions, Haversine calculations
- **Performance & Security**: Best practices and guidelines

### Running Database Tests

Database tests require serial execution due to shared test database:

```bash
# Run all database tests (single-threaded required)
cargo test -- --test-threads=1

# Run only database tests
cargo test --package nmea_router --bin nmea_router db:: -- --test-threads=1 --ignored

# Run specific database test module
cargo test --package nmea_router --bin nmea_router db::operations::trip -- --test-threads=1
```

The test infrastructure automatically loads test configuration and manages database reset between tests.

## License

MIT or Apache-2.0 (choose one and update as appropriate)

## Contributing

Pull requests and issues welcome! See CONTRIBUTING.md for guidelines.

## Authors

See AUTHORS.md or repository contributors.
