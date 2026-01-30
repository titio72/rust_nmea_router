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
  - System Time (126992)
- **REST API**: JSON endpoints for trips, track data, and environmental time series
- **Adaptive Database Persistence**:
  - Moored vessels: 30-minute intervals
  - Underway vessels: 30-second intervals
  - Per-metric environmental intervals
- **Database Resilience**:
  - Health checks every 60 seconds
  - Automatic retry on failed writes
  - Transaction atomicity for vessel status and trip updates
  - Continues operation if database unavailable
- **Time Synchronization Protection**: Blocks database writes when NMEA2000 time differs from system time by more than 500ms (configurable)
- **Configuration Validation**: Comprehensive validation with auto-correction and sensible defaults
- **CLI Options**: Test configuration (--validate-config), display help (--help)
- **Automatic Reconnection**: Retries CAN interface connection every 10 seconds on failure
- **JSON Configuration**: Externalized configuration for all runtime parameters
- **Mooring Detection**: Automatically detects when vessel is moored based on position history
- **Comprehensive Unit Tests**: 80+ tests covering core functionality, wind calculations, configuration validation, and safe deserialization
- **Advanced Wind Data Handling**: Calculates and persists true wind speed/angle, with robust rolling window averaging and test coverage

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
- `skew_threshold_ms`: Maximum allowed time difference between NMEA2000 and system time in milliseconds. Database writes are blocked when exceeded (default: 500ms, minimum: 100ms)
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
- `interval_moored_seconds`: DB write interval when vessel is moored (default: 1800, valid range: 30-600)
- `interval_underway_seconds`: DB write interval when vessel is underway (default: 30, valid range: 30-600)

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
- **Below Minimum**: Reverts to default 500ms with a warning
- **Example**: `skew_threshold_ms: 50` will revert to 500ms

All validation errors are logged with warnings but do not prevent startup (except for invalid CAN interface names).

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

### Features

#### Trip Dashboard
- **Trip List**: View all recorded trips with detailed statistics
- **Filtering**: Filter trips by year or show last 12 months
- **Trip Cards**: Each trip shows:
  - Start/end dates and duration
  - Total distance traveled
  - Time distribution (sailing/motoring/moored) with visual progress bars
  - Distance distribution (sailing vs motoring)
  - Percentages for each activity type

#### Summary Statistics
- Total number of trips
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
}
```

### Future Enhancements

Planned features for the web interface:
- Interactive map with track visualization using Leaflet or similar
- Real-time metric charts with Chart.js or similar
- Trip comparison tools
- Export functionality (CSV, GPX)
- Mobile-responsive design improvements

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
   - Detects mooring status using position history
   - Adaptive database persistence (moored vs underway)

5. **Environmental Monitor** ([environmental_monitor.rs](src/environmental_monitor.rs))
   - Tracks wind, temperature, pressure, humidity, roll
   - Calculates statistics (avg, min, max, count) on demand per metric
   - Per-metric persistence intervals for efficient storage
   - Metric-by-metric database writes for optimal performance

6. **Database** ([db.rs](src/db.rs))
   - Connection pool management with health checks
   - Transaction support for atomic operations
   - Vessel status and trip inserts (atomic)
   - Environmental metrics inserts
   - Automatic reconnection with exponential backoff

7. **PGN Decoders** ([pgns/](src/pgns/))
   - Individual decoders for each supported PGN
   - Binary data parsing with validation
   - Unit conversions (radians to degrees, Kelvin to Celsius, etc.)

### Data Flow

```
CAN Bus (vcan0/can0) [500ms timeout]
    ↓
SocketCAN Interface
    ↓
N2kStreamReader (Fast Packet Assembly)
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

The application automatically detects when a vessel is moored:

1. Maintains a 2-minute sliding window of position samples
2. Calculates maximum distance between any two positions in the window
3. If all positions are within 10 meters, vessel is considered moored
4. Mooring status affects database persistence interval:
   - Moored: 30-minute intervals (reduces database load)
   - Underway: 30-second intervals (higher resolution tracking)

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
  engine_on BOOLEAN NOT NULL DEFAULT FALSE COMMENT 'TRUE if engine is running',
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
    ├── pgn126992.rs          # System Time
    ├── pgn127250.rs          # Vessel Heading
    ├── pgn127251.rs          # Rate of Turn
    ├── pgn127257.rs          # Attitude
    ├── pgn128259.rs          # Speed (Water)
    ├── pgn128267.rs          # Water Depth
    ├── pgn129025.rs          # Position Rapid
    ├── pgn129026.rs          # COG & SOG Rapid
    ├── pgn129029.rs          # GNSS Position
    ├── pgn130306.rs          # Wind Data
    ├── pgn130312.rs          # Temperature
    ├── pgn130313.rs          # Humidity
    └── pgn130314.rs          # Actual Pressure
```


## License

MIT or Apache-2.0 (choose one and update as appropriate)

## Contributing

Pull requests and issues welcome! See CONTRIBUTING.md for guidelines.

## Authors

See AUTHORS.md or repository contributors.
