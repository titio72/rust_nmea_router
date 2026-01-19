# NMEA2000 Router

A robust Rust application for monitoring NMEA2000 marine data networks, with intelligent database persistence and automatic time synchronization.
This is a new project spawned from https://github.com/titio72/nmearouter, mostly to learn rust and get familiar with my friend Claude.

## Features

- **CAN Bus Integration**: Reads NMEA2000 messages from SocketCAN interfaces
- **Comprehensive PGN Support**: 
  - Position (129025, 129029)
  - Speed & Heading (129026, 127250, 127251)
  - Environmental Data (130306, 130312, 130313, 130314)
  - Attitude/Roll (127257)
  - Depth & Water Speed (128267, 128259)
  - System Time (126992)
- **Adaptive Database Persistence**:
  - Moored vessels: 30-minute intervals
  - Underway vessels: 30-second intervals
  - Per-metric environmental intervals
- **Time Synchronization Protection**: Blocks database writes when NMEA2000 time differs from system time by more than 500ms (configurable)
- **Automatic Reconnection**: Retries CAN interface connection every 10 seconds on failure
- **JSON Configuration**: Externalized configuration for all runtime parameters
- **Mooring Detection**: Automatically detects when vessel is moored based on position history
- **Comprehensive Unit Tests**: 58 tests covering core functionality

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

See [Database Setup Guide](README_DATABASE.md) for detailed instructions.

Quick setup:

```bash
# Create database and user
sudo mysql -e "CREATE DATABASE nmea_router;"
sudo mysql -e "CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'nmea';"
sudo mysql -e "GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';"

# Load schema
mysql -u nmea -pnmea nmea_router < schema.sql
mysql -u nmea -pnmea nmea_router < create_environmental_table.sql
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

#### Time Synchronization
- `skew_threshold_ms`: Maximum allowed time difference between NMEA2000 and system time in milliseconds. Database writes are blocked when exceeded (default: 500ms)

#### Database Connection
- `host`: Database server hostname
- `port`: Database server port (default: 3306)
- `username`: Database username
- `password`: Database password
- `database_name`: Target database name

#### Vessel Status Intervals
- `interval_moored_seconds`: DB write interval when vessel is moored (default: 1800 = 30 minutes)
- `interval_underway_seconds`: DB write interval when vessel is underway (default: 30 seconds)

#### Environmental Metrics Intervals
Individual persistence intervals for each environmental metric:
- `wind_speed_seconds`: Wind speed persistence interval (default: 30)
- `wind_direction_seconds`: Wind direction persistence interval (default: 30)
- `roll_seconds`: Roll angle persistence interval (default: 30)
- `pressure_seconds`: Atmospheric pressure persistence interval (default: 120)
- `cabin_temp_seconds`: Cabin temperature persistence interval (default: 300)
- `water_temp_seconds`: Water temperature persistence interval (default: 300)
- `humidity_seconds`: Humidity persistence interval (default: 300)

## Usage

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
...
```

### Testing

Run the comprehensive test suite:

```bash
cargo test
```

Expected output:
```
running 58 tests
test result: ok. 58 passed; 0 failed; 0 ignored; 0 measured
```

## Architecture

### Core Components

1. **Main Loop** ([main.rs](src/main.rs))
   - CAN frame reading with automatic reconnection
   - Message processing and routing
   - Database write coordination

2. **Configuration** ([config.rs](src/config.rs))
   - JSON-based configuration loading
   - Default values with type-safe access
   - Duration conversions for intervals

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
   - Calculates 1-minute averages (avg, min, max)
   - Per-metric persistence intervals

6. **Database** ([db.rs](src/db.rs))
   - Connection pool management
   - Vessel status inserts
   - Environmental metrics inserts

7. **PGN Decoders** ([pgns/](src/pgns/))
   - Individual decoders for each supported PGN
   - Binary data parsing with validation
   - Unit conversions (radians to degrees, Kelvin to Celsius, etc.)

### Data Flow

```
CAN Bus (vcan0/can0)
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
4. **Cooldown**: Warnings are displayed every 10 seconds to avoid console spam
5. **Automatic Recovery**: When time resynchronizes, database writes resume automatically

### Adaptive Persistence

Environmental metrics are persisted at different intervals based on their update frequency and importance:

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

Stores vessel position, speed, and navigation data.

```sql
CREATE TABLE vessel_status (
    id INT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    latitude DECIMAL(10, 7),
    longitude DECIMAL(11, 7),
    avg_speed_30s DECIMAL(6, 2),
    max_speed_30s DECIMAL(6, 2),
    is_moored BOOLEAN DEFAULT FALSE,
    INDEX idx_timestamp (timestamp)
);
```

### `environmental_metrics` Table

Stores environmental sensor data with 1-minute averages.

```sql
CREATE TABLE environmental_metrics (
    id INT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    metric_id TINYINT UNSIGNED NOT NULL,
    avg_value DECIMAL(10, 2),
    min_value DECIMAL(10, 2),
    max_value DECIMAL(10, 2),
    INDEX idx_timestamp_metric (timestamp, metric_id)
);
```

**Metric IDs:**
- 1: Pressure (Pa)
- 2: Cabin Temperature (°C)
- 3: Water Temperature (°C)
- 4: Humidity (%)
- 5: Wind Speed (m/s)
- 6: Wind Direction (degrees)
- 7: Roll Angle (degrees)

See [README_DATABASE.md](README_DATABASE.md) for detailed schema information.

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

## Development

### Running Tests

```bash
# All tests
cargo test

# With output
cargo test -- --nocapture

# Specific module
cargo test config::tests
cargo test vessel_monitor::tests
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

[Add your license here]

## Contributing

[Add contributing guidelines here]

## Authors

[Add author information here]
