# NMEA2000 Router - Application Specifications

**Version**: 0.1.0  
**Edition**: Rust 2024  
**Last Updated**: January 26, 2026

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [System Architecture](#system-architecture)
3. [Core Features](#core-features)
4. [Technical Stack](#technical-stack)
5. [Component Specifications](#component-specifications)
6. [Data Flow](#data-flow)
7. [Database Schema](#database-schema)
8. [Configuration](#configuration)
9. [Web Interface](#web-interface)
10. [REST API](#rest-api)
11. [UDP Broadcasting](#udp-broadcasting)
12. [Unit Consistency](#unit-consistency)
13. [Performance & Reliability](#performance--reliability)
14. [Security Considerations](#security-considerations)
15. [Deployment](#deployment)
16. [Testing Strategy](#testing-strategy)
17. [Future Enhancements](#future-enhancements)

---

## Executive Summary

The NMEA2000 Router is a robust Rust application designed for comprehensive monitoring and analysis of marine vessel data networks. It provides real-time data acquisition from NMEA2000 CAN bus networks, intelligent data persistence with adaptive reporting intervals, automatic trip tracking, environmental monitoring, and modern web-based visualization.

### Primary Objectives

- **Data Acquisition**: Capture and process NMEA2000 messages from CAN bus networks
- **Intelligent Persistence**: Store vessel navigation and environmental data with adaptive intervals
- **Trip Management**: Automatically detect and track vessel trips with sailing/motoring breakdown
- **Real-time Monitoring**: Provide web dashboard and REST API for live data access
- **Data Broadcasting**: Distribute NMEA2000 messages via UDP for external integrations
- **Reliability**: Maintain operation through database failures and CAN interface disruptions

### Key Capabilities

- Supports 15+ NMEA2000 Parameter Group Numbers (PGNs)
- Adaptive database persistence (30s underway, 30min moored)
- Automatic mooring detection (position stability analysis)
- Per-metric environmental data collection with configurable intervals
- Web dashboard with trip visualization and track display
- RESTful API for external integrations
- UDP JSON broadcasting for real-time data streaming
- Time synchronization protection
- Configuration validation and safe defaults

---

## System Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        NMEA2000 Router                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐    │
│  │  CAN Bus     │───▶│  N2K Stream  │───▶│   Message    │    │
│  │  Interface   │    │   Reader     │    │   Router     │    │
│  │ (SocketCAN)  │    │              │    │              │    │
│  └──────────────┘    └──────────────┘    └──────┬───────┘    │
│                                                   │             │
│                          ┌────────────────────────┼────────┐   │
│                          │                        │        │   │
│                          ▼                        ▼        ▼   │
│                  ┌──────────────┐        ┌──────────┐  ┌────┐│
│                  │   Vessel     │        │  Enviro  │  │UDP ││
│                  │   Monitor    │        │  Monitor │  │Bcast││
│                  └──────┬───────┘        └────┬─────┘  └─┬──┘│
│                         │                     │           │   │
│                         ▼                     ▼           │   │
│                  ┌──────────────┐        ┌──────────┐    │   │
│                  │   Vessel     │        │  Enviro  │    │   │
│                  │   Handler    │        │  Handler │    │   │
│                  └──────┬───────┘        └────┬─────┘    │   │
│                         │                     │           │   │
│                         └─────────┬───────────┘           │   │
│                                   ▼                       │   │
│                            ┌─────────────┐               │   │
│                            │  Database   │               │   │
│                            │  Manager    │               │   │
│                            └─────────────┘               │   │
│                                                           │   │
│  ┌──────────────────────────────────────────────────┐   │   │
│  │            Web Server (Axum)                      │   │   │
│  │  ┌─────────────┐  ┌──────────────┐              │   │   │
│  │  │  Dashboard  │  │   REST API   │              │   │   │
│  │  │   (HTML)    │  │   (JSON)     │              │   │   │
│  │  └─────────────┘  └──────────────┘              │   │   │
│  └──────────────────────────────────────────────────┘   │   │
│                                                           │   │
│                                                           ▼   │
│                                                    ┌──────────┐
│                                                    │  UDP     │
│                                                    │ Network  │
│                                                    └──────────┘
└─────────────────────────────────────────────────────────────────┘

External Systems:
  - MariaDB/MySQL Database
  - Web Browsers (Dashboard clients)
  - HTTP Clients (API consumers)
  - UDP Listeners (Real-time data consumers)
```

### Component Overview

| Component | Purpose | Technology |
|-----------|---------|------------|
| **CAN Bus Interface** | Hardware interface to NMEA2000 network | SocketCAN (Linux) |
| **N2K Stream Reader** | NMEA2000 protocol decoder | Custom `nmea2k` crate |
| **Message Router** | Distributes messages to handlers | Event-driven dispatch |
| **Vessel Monitor** | Tracks position, speed, mooring status | State machine |
| **Environmental Monitor** | Collects sensor data with aggregation | Time-series sampling |
| **Trip Manager** | Automatic trip detection and tracking | 24-hour gap detection |
| **Database Manager** | Persistent storage with health checks | MySQL/MariaDB |
| **UDP Broadcaster** | Real-time JSON message streaming | UDP/IP |
| **Web Server** | Dashboard and REST API | Axum (Tokio) |
| **Configuration Manager** | JSON-based configuration with validation | Serde |

---

## Core Features

### 1. CAN Bus Integration

- **Interface**: SocketCAN (Linux native CAN bus support)
- **Protocol**: NMEA2000 (based on SAE J1939)
- **Bitrate**: 250 kbps (NMEA2000 standard)
- **Auto-reconnection**: 10-second retry interval on connection failure
- **Frame Filtering**: Configurable PGN-based source filtering

### 2. NMEA2000 Message Support

#### Navigation Messages

| PGN | Message Type | Description | Update Rate |
|-----|--------------|-------------|-------------|
| 129025 | PositionRapidUpdate | Latitude, Longitude | 100ms |
| 129026 | CogSogRapidUpdate | Course, Speed | 100ms |
| 129029 | GnssPositionData | Full GPS data with altitude | 1s |
| 127250 | VesselHeading | True/Magnetic heading | 100ms |
| 127251 | RateOfTurn | Rate of turn | 100ms |
| 127257 | Attitude | Yaw, Pitch, Roll | 100ms |
| 128259 | SpeedWaterReferenced | Speed through water | 1s |
| 128267 | WaterDepth | Depth, transducer offset | 1s |

#### Environmental Messages

| PGN | Message Type | Description | Update Rate |
|-----|--------------|-------------|-------------|
| 130306 | WindData | Wind speed, angle, reference | 100ms |
| 130312 | Temperature | Multi-instance temperature | 2s |
| 130313 | Humidity | Relative humidity | 2s |
| 130314 | ActualPressure | Barometric pressure | 2s |

#### System Messages

| PGN | Message Type | Description | Update Rate |
|-----|--------------|-------------|-------------|
| 126992 | NMEASystemTime | System date and time | 1s |
| 127488 | EngineRapidUpdate | RPM, boost, tilt/trim | 100ms |

### 3. Adaptive Database Persistence

The system intelligently adjusts data collection frequency based on vessel activity:

#### Vessel Status Reporting

- **Underway**: 30 seconds (configurable)
- **Moored**: 30 minutes (configurable, range: 1-120 minutes)
- **Stored Data**: Position, speed (avg/max), mooring state, engine state, distance, time

#### Environmental Data Reporting

Per-metric configurable intervals (independent):

- **Pressure**: Default 5 minutes
- **Temperature (Cabin)**: Default 5 minutes  
- **Temperature (Water)**: Default 5 minutes
- **Humidity**: Default 5 minutes
- **Wind Speed**: Default 1 minute
- **Wind Direction**: Default 1 minute
- **Roll**: Default 1 minute

Each metric stores: average, minimum, maximum values over collection period

### 4. Mooring Detection

Automatic detection of moored vessel using position stability analysis:

- **Detection Window**: 2 minutes of position history
- **Radius Threshold**: 30 meters
- **Accuracy Requirement**: 90% of positions within threshold
- **Noise Filtering**: 
  - Maximum valid SOG: 25 knots
  - Position deviation from median: 100m max
  - Validation window: 10 seconds with 10+ samples

### 5. Trip Management

Automatic trip detection and tracking with sailing/motoring classification:

- **Trip Boundary**: 24 hours of inactivity (moored or no data)
- **Trip Naming**: Auto-generated as "Trip YYYY-MM-DD"
- **Tracked Metrics**:
  - Start and end timestamps
  - Distance sailed (nautical miles)
  - Distance motoring (nautical miles)
  - Time sailing (milliseconds)
  - Time motoring (milliseconds)
  - Time moored (milliseconds)
- **Engine Detection**: Based on PGN 127488 (EngineRapidUpdate)

### 6. Web Dashboard

Modern responsive web interface with:

- **Trip Browser**: List and select historical trips
- **Interactive Map**: Leaflet-based track visualization
- **Environmental Charts**: Time-series graphs for all metrics
- **Real-time Updates**: Live data display (requires periodic refresh)
- **Trip Statistics**: Distance, speed, duration, sail/motor breakdown
- **Responsive Design**: Works on desktop and mobile devices

### 7. REST API

JSON-based RESTful API for external integrations:

#### Endpoints

- `GET /api/trips` - List trips with optional filtering
- `GET /api/track` - Retrieve track points for time range or trip
- `GET /api/metrics` - Environmental time-series data
- `GET /api/latest` - Latest vessel and environmental status

All responses follow standard format:
```json
{
  "status": "ok|error",
  "data": { ... },
  "error": "error message if status=error"
}
```

### 8. UDP Broadcasting

Real-time JSON streaming of all NMEA2000 messages:

- **Protocol**: UDP (User Datagram Protocol)
- **Format**: JSON with message metadata
- **Configurable**: Enable/disable, destination address
- **Use Cases**: External monitoring, data logging, third-party integrations
- **See**: [UDP_BROADCASTER_SPECS.md](UDP_BROADCASTER_SPECS.md) for details

### 9. Time Synchronization Protection

Prevents database corruption from time changes:

- **Threshold**: 500ms (configurable)
- **Comparison**: NMEA2000 time vs system time
- **Action**: Block database writes when skew exceeds threshold
- **Logging**: Warnings logged for time discrepancies
- **Recovery**: Automatic resume when time synchronizes

### 10. Source Filtering

Optional PGN-based source address filtering:

- **Configuration**: Map of PGN → allowed source address
- **Behavior**: 
  - If PGN in map: Only accept from specified source
  - If PGN not in map: Accept from all sources
- **Use Case**: Filter duplicate sensors on same network

---

## Technical Stack

### Core Technologies

| Technology | Version | Purpose |
|------------|---------|---------|
| **Rust** | Edition 2024 | Primary language |
| **SocketCAN** | 3.0 | CAN bus interface |
| **Tokio** | 1.x | Async runtime |
| **Axum** | 0.7 | Web framework |
| **MariaDB/MySQL** | 10.5+ / 8.0+ | Database |
| **Serde** | 1.0 | Serialization |
| **Chrono** | 0.4 | Date/time handling |
| **Tracing** | 0.1 | Logging framework |

### Dependencies

```toml
[dependencies]
nmea2k = { path = "nmea2k" }          # Custom NMEA2000 decoder
nmea2000 = "0.2.2"                     # NMEA2000 types
socketcan = "3.0"                      # CAN interface
mysql = "25.0"                         # Database client
chrono = "0.4"                         # DateTime
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"                     # JSON
log = "0.4"                            # Logging facade
tracing = "0.1"                        # Structured logging
tracing-subscriber = "0.3"             # Log subscriber
tracing-appender = "0.2"               # File appender
time = "0.3"                           # Time utilities
nix = "0.29"                           # Unix system calls
axum = "0.7"                           # Web framework
tokio = { version = "1", features = ["full"] }
tower = "0.4"                          # Service middleware
tower-http = "0.5"                     # HTTP middleware
```

### Build Requirements

- **Rust Toolchain**: 1.75+ (Edition 2024)
- **Target**: Linux (SocketCAN dependency)
- **Build Profile**: Release optimized for embedded systems

---

## Component Specifications

### 1. Vessel Monitor (`vessel_monitor.rs`)

#### Purpose
Tracks vessel position, speed, and mooring status with position validation and noise filtering.

#### State Management

```rust
pub struct VesselMonitor {
    current_position: Option<Position>,
    current_sog_kn: f64,
    max_speed_kn: f64,
    last_report_time: Instant,
    position_history: VecDeque<PositionSample>,
    is_moored: bool,
    engine_on: bool,
    // ... validation fields
}
```

#### Key Methods

- `process_position()`: Update from PGN 129025/129029
- `process_cog_sog()`: Update from PGN 129026
- `check_mooring_status()`: Analyze position stability
- `validate_position()`: Noise filtering using median
- `get_status()`: Generate vessel status report
- `requires_report()`: Check if reporting interval elapsed

#### Position Validation

1. **Median Calculation**: 10-second window with 10+ samples
2. **Deviation Check**: Max 100m from median position
3. **SOG Validation**: Reject speeds > 25 knots
4. **Outlier Rejection**: Log and discard invalid positions

#### Mooring Detection Algorithm

```
1. Collect last 2 minutes of positions
2. Calculate average position (centroid)
3. Measure distance of each position from centroid
4. Count positions within 30m radius
5. If 90%+ within radius: MOORED
6. Otherwise: UNDERWAY
```

### 2. Environmental Monitor (`environmental_monitor.rs`)

#### Purpose
Collects and aggregates environmental sensor data with per-metric intervals.

#### Metric Types

```rust
pub enum MetricId {
    Pressure = 1,      // Pa
    CabinTemp = 2,     // °C
    WaterTemp = 3,     // °C  
    Humidity = 4,      // %
    WindSpeed = 5,     // Knots
    WindDir = 6,       // Degrees
    Roll = 7,          // Degrees
}
```

#### State Management

```rust
pub struct EnvironmentalMonitor {
    db_periods: [Duration; 7],           // Per-metric intervals
    data_samples: [VecDeque<Sample<f64>>; 7],  // Sample buffers
}
```

#### Aggregation

For each metric, calculates over collection period:
- **Average**: Mean of all samples
- **Minimum**: Lowest value
- **Maximum**: Highest value
- **Count**: Number of samples

#### Message Processing

| PGN | Handler | Metrics Updated |
|-----|---------|-----------------|
| 130306 | `handle_wind_data()` | WindSpeed, WindDir |
| 130312 | `handle_temperature()` | CabinTemp or WaterTemp (by instance) |
| 130313 | `handle_humidity()` | Humidity |
| 130314 | `handle_pressure()` | Pressure |
| 127257 | `handle_attitude()` | Roll |

### 3. Trip Manager (`trip.rs`)

#### Purpose
Automatic trip lifecycle management with sailing/motoring classification.

#### Trip Structure

```rust
pub struct Trip {
    pub id: Option<i64>,
    pub description: String,
    pub start_timestamp: Instant,
    pub end_timestamp: Instant,
    pub total_distance_sailed: f64,     // nautical miles
    pub total_distance_motoring: f64,   // nautical miles
    pub total_time_sailing: u64,        // milliseconds
    pub total_time_motoring: u64,       // milliseconds
    pub total_time_moored: u64,         // milliseconds
}
```

#### Trip Lifecycle

1. **Creation**: First vessel status after 24h+ gap
2. **Update**: Each vessel status report updates current trip
3. **Classification**:
   - **Moored**: Add to `total_time_moored`
   - **Engine On**: Add to `total_distance_motoring` + `total_time_motoring`
   - **Engine Off**: Add to `total_distance_sailed` + `total_time_sailing`
4. **Closure**: Implicitly closed after 24h inactivity
5. **Naming**: Auto-generated as "Trip YYYY-MM-DD" from start date

#### Activity Detection

```rust
pub fn is_active(&self, current_time: Instant) -> bool {
    let duration = current_time.duration_since(self.end_timestamp);
    duration.as_secs() <= 24 * 60 * 60 // 24 hours
}
```

### 4. Database Manager (`db.rs`)

#### Purpose
Manages all database operations with health checks and retry logic.

#### Components

##### VesselDatabase

Main database interface:

```rust
pub struct VesselDatabase {
    pool: Option<Pool>,
    health_check: HealthCheckManager,
    last_error: Option<String>,
}
```

**Methods:**
- `new()` - Initialize with connection string
- `insert_vessel_status()` - Persist vessel status
- `insert_environmental_data()` - Persist environmental metrics
- `ensure_trip()` - Get or create current trip
- `update_trip()` - Update trip statistics
- `fetch_trips()` - Query historical trips
- `fetch_track()` - Query track points
- `fetch_metrics()` - Query environmental time series
- `fetch_latest_status()` - Get most recent vessel status

##### HealthCheckManager

Monitors database connectivity:

```rust
pub struct HealthCheckManager {
    last_check: Instant,
    is_healthy: bool,
    check_interval: Duration,  // 60 seconds
}
```

**Behavior:**
- Check every 60 seconds
- Execute `SELECT 1` test query
- Mark unhealthy on failure
- Attempt reconnection on next operation
- Log health state changes

#### Transaction Management

Critical operations use MySQL transactions for atomicity:

```rust
// Example: Vessel status + trip update
conn.query_drop("START TRANSACTION")?;
// Insert vessel status
// Update trip
conn.query_drop("COMMIT")?;
```

#### Error Handling

- **Connection Failure**: Mark unhealthy, continue operation
- **Query Failure**: Log error, retry on next cycle
- **Transaction Failure**: Rollback, log error
- **Never Panic**: Application continues even if database unavailable

### 5. UDP Broadcaster (`udp_broadcaster.rs`)

#### Purpose
Broadcast all NMEA2000 messages as JSON over UDP network.

#### Implementation

```rust
pub struct UdpBroadcaster {
    socket: Option<UdpSocket>,
    destination: SocketAddr,
    enabled: bool,
    message_count: u64,
    error_count: u64,
}
```

**See**: [UDP_BROADCASTER_SPECS.md](UDP_BROADCASTER_SPECS.md) for complete specification.

### 6. Web Server (`web/`)

#### Architecture

```
web/
├── mod.rs          # Module exports
├── server.rs       # Axum server setup
└── api.rs          # REST API handlers
```

#### Server Configuration

- **Framework**: Axum with Tokio async runtime
- **Port**: 8080 (configurable)
- **CORS**: Enabled for cross-origin requests
- **Static Files**: Served from `./static/` directory
- **Routing**: Path-based with middleware

#### Middleware Stack

1. **CORS Layer**: Allow all origins (development mode)
2. **Static Files**: Serve HTML/CSS/JS from `static/`
3. **API Routes**: JSON endpoints under `/api/`
4. **Error Handling**: JSON error responses

---

## Data Flow

### Message Processing Pipeline

```
1. CAN Frame Received (SocketCAN)
   │
   ├─▶ Parse NMEA2000 Identifier
   │   ├─ Extract PGN
   │   ├─ Extract Source Address
   │   └─ Extract Priority
   │
   ├─▶ Apply Source Filter (optional)
   │   └─ Check if PGN+Source allowed
   │
   ├─▶ Decode to N2kMessage
   │   └─ PGN-specific decoder
   │
   ├─▶ Time Sync Check (if PGN 126992)
   │   ├─ Compare NMEA time vs system time
   │   └─ Block DB writes if skew > threshold
   │
   ├─▶ Dispatch to Handlers
   │   │
   │   ├─▶ Vessel Monitor
   │   │   ├─ Update position (PGN 129025/129029)
   │   │   ├─ Update speed (PGN 129026)
   │   │   ├─ Update engine (PGN 127488)
   │   │   ├─ Check mooring status
   │   │   └─ Generate status if interval elapsed
   │   │
   │   ├─▶ Environmental Monitor
   │   │   ├─ Add sample to metric queue
   │   │   ├─ Check if interval elapsed
   │   │   ├─ Calculate avg/min/max
   │   │   └─ Generate metric data
   │   │
   │   └─▶ UDP Broadcaster (if enabled)
   │       ├─ Serialize to JSON
   │       ├─ Add frame metadata
   │       └─ Send UDP packet
   │
   └─▶ Status/Metric Generated
       │
       ├─▶ Vessel Status Handler
       │   ├─ Store in database
       │   ├─ Get/create current trip
       │   ├─ Update trip statistics
       │   └─ Store trip in database
       │
       └─▶ Environmental Status Handler
           └─ Store metrics in database
```

### Database Write Flow

```
1. Status/Metric Ready
   │
   ├─▶ Check Time Sync Status
   │   ├─ If NOT OK: Skip write, log warning
   │   └─ If OK: Continue
   │
   ├─▶ Check Database Health
   │   ├─ If unhealthy and check interval elapsed:
   │   │   └─ Attempt health check
   │   └─ If healthy: Continue
   │
   ├─▶ Execute Write Operation
   │   ├─ Vessel Status:
   │   │   ├─ BEGIN TRANSACTION
   │   │   ├─ INSERT vessel_status
   │   │   ├─ SELECT current trip (24h window)
   │   │   ├─ INSERT or UPDATE trips
   │   │   └─ COMMIT
   │   │
   │   └─ Environmental Data:
   │       └─ INSERT IGNORE environmental_data
   │           (unique constraint on timestamp+metric)
   │
   └─▶ Handle Result
       ├─ Success: Log write
       ├─ Error: Log error, mark DB unhealthy
       └─ Continue operation regardless
```

### Web Request Flow

```
1. HTTP Request Received
   │
   ├─▶ Static File? (*.html, *.css, *.js)
   │   ├─ Serve from ./static/ directory
   │   └─ Return 200 OK or 404 Not Found
   │
   └─▶ API Route? (/api/*)
       │
       ├─▶ Parse Query Parameters
       │   ├─ Trip ID filter
       │   ├─ Date range (start/end)
       │   ├─ Metric name
       │   └─ Year filter
       │
       ├─▶ Query Database
       │   ├─ GET /api/trips → fetch_trips()
       │   ├─ GET /api/track → fetch_track()
       │   ├─ GET /api/metrics → fetch_metrics()
       │   └─ GET /api/latest → fetch_latest_status()
       │
       └─▶ Return JSON Response
           ├─ Success: { status: "ok", data: {...} }
           └─ Error: { status: "error", error: "..." }
```

---

## Database Schema

### Tables Overview

| Table | Purpose | Size Estimate |
|-------|---------|---------------|
| `vessel_status` | Navigation reports | ~1 MB/day underway |
| `environmental_data` | Sensor readings | ~500 KB/day |
| `trips` | Trip summaries | ~1 KB/trip |

### vessel_status Table

Stores vessel navigation status reports.

```sql
CREATE TABLE vessel_status (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    latitude DOUBLE,
    longitude DOUBLE,
    average_speed_kn DOUBLE NOT NULL,     -- knots
    max_speed_kn DOUBLE NOT NULL,         -- knots
    is_moored BOOLEAN NOT NULL,
    engine_on BOOLEAN NOT NULL DEFAULT FALSE,
    total_distance_nm DOUBLE NOT NULL DEFAULT 0,  -- nautical miles
    total_time_ms BIGINT NOT NULL DEFAULT 0,      -- milliseconds
    
    INDEX idx_timestamp (timestamp),
    INDEX idx_moored (is_moored, timestamp)
);
```

**Update Frequency:**
- Underway: Every 30 seconds (default)
- Moored: Every 30 minutes (default)

**Key Fields:**
- `average_speed_kn`: Speed averaged over reporting period
- `max_speed_kn`: Maximum speed observed in period
- `total_distance_nm`: Distance since last report (Haversine)
- `total_time_ms`: Time elapsed since last report

### environmental_data Table

Stores environmental sensor readings with aggregation.

```sql
CREATE TABLE environmental_data (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    timestamp DATETIME(3) NOT NULL,
    metric_id TINYINT UNSIGNED NOT NULL,
    value_avg FLOAT,
    value_max FLOAT,
    value_min FLOAT,
    unit CHAR(10),
    
    UNIQUE KEY unique_metric_time (timestamp, metric_id),
    INDEX idx_timestamp (timestamp),
    INDEX idx_metric (metric_id, timestamp)
);
```

**Metric IDs:**
- 1 = Pressure (Pa)
- 2 = Cabin Temperature (°C)
- 3 = Water Temperature (°C)
- 4 = Humidity (%)
- 5 = Wind Speed (Knots)
- 6 = Wind Direction (Degrees)
- 7 = Roll (Degrees)

**Update Frequency:** Per-metric configurable (1-60 minutes)

### trips Table

Stores vessel trips with sailing/motoring breakdown.

```sql
CREATE TABLE trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL,
    start_timestamp DATETIME(3) NOT NULL,
    end_timestamp DATETIME(3) NOT NULL,
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0,      -- nautical miles
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0,    -- nautical miles
    total_time_sailing BIGINT NOT NULL DEFAULT 0,         -- milliseconds
    total_time_motoring BIGINT NOT NULL DEFAULT 0,        -- milliseconds
    total_time_moored BIGINT NOT NULL DEFAULT 0,          -- milliseconds
    
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp)
);
```

**Trip Detection:** 24-hour gap in activity

**Auto-naming:** "Trip YYYY-MM-DD" from start date

---

## Configuration

### Configuration File Structure

`config.json` - JSON format with nested sections:

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
      "pressure_interval_seconds": 300,
      "cabin_temp_interval_seconds": 300,
      "water_temp_interval_seconds": 300,
      "humidity_interval_seconds": 300,
      "wind_speed_interval_seconds": 60,
      "wind_dir_interval_seconds": 60,
      "roll_interval_seconds": 60
    }
  },
  "source_filter": {
    "pgn_source_map": {
      "129025": 10,
      "129026": 10
    }
  },
  "logging": {
    "directory": "./logs",
    "file_prefix": "nmea_router",
    "level": "info"
  },
  "web": {
    "enabled": true,
    "port": 8080
  },
  "udp": {
    "enabled": false,
    "address": "192.168.1.255:10110"
  }
}
```

### Configuration Sections

#### CAN Interface

```json
{
  "can_interface": "vcan0"
}
```

- **Type**: String
- **Required**: Yes
- **Validation**: Must exist as network interface
- **Examples**: `"can0"`, `"vcan0"`

#### Time Configuration

```json
{
  "time": {
    "skew_threshold_ms": 500
  }
}
```

- **skew_threshold_ms**: Maximum allowed difference between NMEA2000 time and system time
- **Range**: 0-60000 ms
- **Default**: 500 ms
- **Effect**: Database writes blocked when exceeded

#### Database Configuration

##### Connection

```json
{
  "connection": {
    "host": "localhost",
    "port": 3306,
    "username": "nmea",
    "password": "nmea",
    "database_name": "nmea_router"
  }
}
```

- **host**: Database hostname or IP
- **port**: MySQL port (default 3306)
- **username**: Database user
- **password**: Database password
- **database_name**: Database name

##### Vessel Status Intervals

```json
{
  "vessel_status": {
    "interval_moored_seconds": 1800,
    "interval_underway_seconds": 30
  }
}
```

- **interval_moored_seconds**: Report interval when moored
  - Range: 60-7200 seconds (1 min - 2 hours)
  - Default: 1800 (30 minutes)
- **interval_underway_seconds**: Report interval when underway
  - Range: 1-300 seconds
  - Default: 30 seconds

##### Environmental Intervals

```json
{
  "environmental": {
    "pressure_interval_seconds": 300,
    "cabin_temp_interval_seconds": 300,
    "water_temp_interval_seconds": 300,
    "humidity_interval_seconds": 300,
    "wind_speed_interval_seconds": 60,
    "wind_dir_interval_seconds": 60,
    "roll_interval_seconds": 60
  }
}
```

- **Range (all)**: 1-3600 seconds (1 second - 1 hour)
- **Defaults**: See above
- **Independent**: Each metric has its own interval

#### Source Filtering

```json
{
  "source_filter": {
    "pgn_source_map": {
      "129025": 10,
      "129026": 10,
      "130306": 15
    }
  }
}
```

- **pgn_source_map**: Map of PGN (as string) to source address (0-253)
- **Behavior**:
  - PGN in map: Only accept from specified source
  - PGN not in map: Accept from all sources
- **Use Case**: Filter duplicate sensors

#### Logging

```json
{
  "logging": {
    "directory": "./logs",
    "file_prefix": "nmea_router",
    "level": "info"
  }
}
```

- **directory**: Log file directory (created if missing)
- **file_prefix**: Log file name prefix (date appended)
- **level**: Log verbosity
  - Values: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`
  - Default: `"info"`

#### Web Server

```json
{
  "web": {
    "enabled": true,
    "port": 8080
  }
}
```

- **enabled**: Enable/disable web interface
- **port**: TCP port for web server (1024-65535)
- **Default**: Enabled on port 8080

#### UDP Broadcasting

```json
{
  "udp": {
    "enabled": false,
    "address": "192.168.1.255:10110"
  }
}
```

- **enabled**: Enable/disable UDP broadcasting
- **address**: Destination address (broadcast, multicast, or unicast)
- **Default**: Disabled
- **See**: [UDP_BROADCASTER_SPECS.md](UDP_BROADCASTER_SPECS.md)

### Configuration Validation

The application validates configuration on startup:

```bash
# Validate without running
./nmea_router --validate-config

# Show help
./nmea_router --help
```

**Validation Checks:**
- CAN interface exists
- Database connection parameters valid
- Interval ranges valid
- Port numbers valid
- Required fields present
- Type correctness

**Auto-correction:**
- Clamps intervals to valid ranges
- Provides defaults for missing optional fields
- Warns about corrections in logs

### Environment Variables

Logging can be controlled via environment variable:

```bash
RUST_LOG=debug ./nmea_router
```

Overrides `logging.level` in config.

---

## Web Interface

### Dashboard Features

Located in `static/` directory:

- **index.html**: Main dashboard
- **Dashboard components**:
  - Trip selector dropdown
  - Interactive map (Leaflet.js)
  - Environmental charts (Chart.js)
  - Trip statistics panel
  - Real-time data display

### Map Visualization

- **Library**: Leaflet.js with OpenStreetMap tiles
- **Features**:
  - Track polyline with color coding
  - Start/end markers
  - Zoom controls
  - Pan and zoom
  - Responsive sizing

### Charts

- **Library**: Chart.js
- **Metric Charts**:
  - Pressure (Pa)
  - Cabin Temperature (°C)
  - Water Temperature (°C)
  - Humidity (%)
  - Wind Speed (Knots)
  - Wind Direction (Degrees)
  - Roll (Degrees)
- **Features**:
  - Time-series line charts
  - Min/max/avg display
  - Interactive tooltips
  - Responsive sizing

### Trip Statistics

Displays for selected trip:
- Trip name and dates
- Total distance (sailed + motoring)
- Sailing percentage
- Average speed
- Maximum speed
- Duration breakdown

### Data Refresh

- **Method**: Manual refresh button or periodic polling
- **Update Frequency**: User-controlled
- **API Calls**: Fetches latest data from REST API

---

## REST API

### API Design Principles

- **Protocol**: HTTP/1.1
- **Format**: JSON
- **Style**: RESTful
- **CORS**: Enabled (development mode)
- **Error Handling**: Standard JSON error responses

### Response Format

All API responses follow this structure:

```json
{
  "status": "ok",
  "data": { ... },
  "error": null
}
```

Or on error:

```json
{
  "status": "error",
  "data": null,
  "error": "Error message"
}
```

### Endpoints

#### GET /api/trips

List trips with optional filtering.

**Query Parameters:**
- `year` (optional): Filter by year (e.g., `2026`)
- `last_months` (optional): Last N months (e.g., `6`)

**Response:**
```json
{
  "status": "ok",
  "data": [
    {
      "id": 123,
      "description": "Trip 2026-01-15",
      "start_timestamp": "2026-01-15T08:00:00Z",
      "end_timestamp": "2026-01-15T18:30:00Z",
      "total_distance_sailed": 25.3,
      "total_distance_motoring": 3.2,
      "total_time_sailing": 32400000,
      "total_time_motoring": 5400000,
      "total_time_moored": 0
    }
  ]
}
```

**Example:**
```bash
curl http://localhost:8080/api/trips?last_months=3
```

#### GET /api/track

Retrieve track points (vessel positions).

**Query Parameters:**
- `trip_id` (optional): Filter by trip ID
- `start` (optional): Start datetime (ISO 8601)
- `end` (optional): End datetime (ISO 8601)

**Response:**
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2026-01-15T08:00:00Z",
      "latitude": 43.630142,
      "longitude": 10.293372,
      "speed_kn": 5.2,
      "is_moored": false
    }
  ]
}
```

**Example:**
```bash
curl "http://localhost:8080/api/track?trip_id=123"
curl "http://localhost:8080/api/track?start=2026-01-15T00:00:00Z&end=2026-01-16T00:00:00Z"
```

#### GET /api/metrics

Retrieve environmental metric time-series.

**Query Parameters:**
- `metric` (required): Metric name
  - Values: `pressure`, `cabin_temp`, `water_temp`, `humidity`, `wind_speed`, `wind_dir`, `roll`
- `trip_id` (optional): Filter by trip ID
- `start` (optional): Start datetime (ISO 8601)
- `end` (optional): End datetime (ISO 8601)

**Response:**
```json
{
  "status": "ok",
  "data": [
    {
      "timestamp": "2026-01-15T08:00:00Z",
      "value_avg": 101325.0,
      "value_max": 101350.0,
      "value_min": 101300.0,
      "unit": "Pa"
    }
  ]
}
```

**Example:**
```bash
curl "http://localhost:8080/api/metrics?metric=pressure&trip_id=123"
```

#### GET /api/latest

Get latest vessel status and environmental data (not yet implemented in current codebase - planned feature).

---

## UDP Broadcasting

### Overview

The UDP broadcaster provides real-time streaming of all NMEA2000 messages in JSON format over UDP network protocol.

### Complete Specification

See [UDP_BROADCASTER_SPECS.md](UDP_BROADCASTER_SPECS.md) for:
- Message format details
- All supported message types
- Configuration options
- Client implementation examples
- Security considerations
- Troubleshooting guide

### Quick Reference

**Configuration:**
```json
{
  "udp": {
    "enabled": true,
    "address": "192.168.1.255:10110"
  }
}
```

**Message Format:**
```json
{
  "message_type": "PositionRapidUpdate",
  "pgn": 129025,
  "source": 10,
  "priority": 3,
  "data": {
    "latitude": 43.630142,
    "longitude": 10.293372
  }
}
```

**Use Cases:**
- Real-time external monitoring
- Data logging to file
- Third-party application integration
- Network-based data distribution

---

## Unit Consistency

### Fundamental Principle

**All distances are stored in nautical miles. All speeds are stored in knots.**

This consistency is maintained throughout the entire application lifecycle from data acquisition to database storage to API responses.

### Unit Conversion Points

#### 1. NMEA2000 Protocol → Internal Representation

**Speed (SOG - Speed Over Ground):**
- **Wire format**: meters/second (m/s)
- **Conversion**: `sog_knots = sog_ms * 1.94384`
- **Location**: `nmea2k/src/pgns/pgn129026.rs`

```rust
pub fn sog_knots(&self) -> f64 {
    self.sog * 1.94384  // m/s to knots
}
```

**Distance (Position changes):**
- **Calculation**: Haversine formula in meters
- **Conversion**: `distance_nm = distance_m / 1852.0`
- **Location**: `src/vessel_monitor.rs`

```rust
pub fn distance_to_nm(&self, other: &Position) -> f64 {
    let r = 6371000.0; // Earth radius in meters
    // ... haversine calculation ...
    (r * c) / 1852.0  // Convert meters to nautical miles
}
```

#### 2. Database Storage

**Schema Comments (Explicit Documentation):**
```sql
average_speed_kn DOUBLE NOT NULL COMMENT 'Average speed over reporting period in knots'
max_speed_kn DOUBLE NOT NULL COMMENT 'Maximum speed over reporting period in knots'
total_distance_nm DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled in nautical miles'
total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled under sail in nautical miles'
total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'Distance traveled with engine in nautical miles'
```

**Naming Convention:**
- `_kn` suffix for speeds in knots
- `_nm` suffix for distances in nautical miles

#### 3. Internal Structures

**VesselStatus:**
```rust
pub struct VesselStatus {
    pub max_speed_kn: f64,  // Knots
    // ...
}
```

**Trip:**
```rust
pub struct Trip {
    pub total_distance_sailed: f64,   // nautical miles
    pub total_distance_motoring: f64, // nautical miles
    // ...
}
```

#### 4. API Responses

All JSON responses maintain unit consistency:

```json
{
  "average_speed_kn": 5.2,      // knots
  "max_speed_kn": 7.8,          // knots
  "total_distance_nm": 28.5     // nautical miles
}
```

### Conversion Factors (Reference)

| Conversion | Factor | Accuracy |
|------------|--------|----------|
| m/s → knots | × 1.94384 | Exact |
| knots → m/s | ÷ 1.94384 | Exact |
| meters → nm | ÷ 1852.0 | Exact (by definition) |
| nm → meters | × 1852.0 | Exact (by definition) |
| km → nm | ÷ 1.852 | Exact |
| nm → km | × 1.852 | Exact |

### Validation

Unit consistency has been verified across:
- ✅ All source code files
- ✅ Database schema and comments
- ✅ Configuration documentation
- ✅ API responses
- ✅ Variable naming conventions
- ✅ Conversion factor correctness

**Reference**: Previous conversation audit found no inconsistencies.

---

## Performance & Reliability

### Performance Characteristics

#### Message Processing

- **Throughput**: 100+ messages/second
- **Latency**: < 10ms per message (typical)
- **Memory**: ~50 MB typical, ~100 MB max
- **CPU**: < 5% on modern systems (idle state)

#### Database Operations

- **Write Rate**: 
  - Vessel status: 1 write/30s underway, 1 write/30min moored
  - Environmental: 7 writes/minute max (one per metric)
- **Query Performance**:
  - Latest status: < 10ms
  - Track query (1 day): < 100ms
  - Trip list: < 50ms
- **Connection Pooling**: Single connection with health checks

#### Web Server

- **Concurrent Connections**: 100+ (Tokio async)
- **Response Time**: < 50ms typical
- **Static File Serving**: < 10ms
- **API Endpoints**: < 100ms including database query

### Reliability Features

#### 1. Database Resilience

**Health Checking:**
- Check interval: 60 seconds
- Test query: `SELECT 1`
- Automatic marking of healthy/unhealthy state

**Failure Handling:**
- Connection failures logged but don't stop application
- Retry on next write attempt
- Application continues collecting data in memory
- Automatic reconnection when database recovers

**Transaction Safety:**
- Vessel status + trip update in single transaction
- Rollback on any failure
- Maintains data consistency

#### 2. CAN Interface Resilience

**Auto-reconnection:**
- 10-second retry interval
- Continues retrying indefinitely
- Logs connection attempts
- Resumes normal operation on success

**Error Handling:**
- Invalid frames logged and skipped
- Decode errors don't stop processing
- Continues processing subsequent frames

#### 3. Time Synchronization Protection

**Prevents corrupted timestamps:**
- Compares NMEA2000 time vs system time
- Threshold: 500ms (configurable)
- Blocks database writes when exceeded
- Logs warnings for time skew
- Automatic resume when synchronized

#### 4. Data Validation

**Position Validation:**
- Median filter (10 samples, 10-second window)
- Maximum deviation: 100m from median
- Outlier rejection and logging

**Speed Validation:**
- Maximum valid SOG: 25 knots
- Rejects obvious noise
- Logs rejected values

**Mooring Detection:**
- 2-minute position history
- 90% positions within 30m radius
- Prevents false positives from GPS drift

#### 5. Configuration Safety

**Validation:**
- All values range-checked
- Auto-correction with warnings
- Safe defaults for missing values
- Fails early on critical errors (CAN interface)

**Testing:**
- `--validate-config` flag for pre-flight checks
- Comprehensive unit tests (73 tests)
- Safe deserialization with Serde

### Resource Management

#### Memory

**Bounded Buffers:**
- Position history: 120 samples (2 minutes)
- Environmental samples: Per-metric configurable
- No unbounded growth

**Cleanup:**
- Old samples removed automatically
- Transaction scope limits memory usage

#### Disk

**Database Growth:**
- ~1 MB/day underway (vessel_status)
- ~500 KB/day (environmental_data)
- Minimal trip table growth

**Log Files:**
- Daily rotation
- Configurable directory
- No automatic cleanup (external tool recommended)

#### Network

**UDP Broadcasting:**
- Fire-and-forget (no buffering)
- Non-blocking socket
- Minimal network overhead

**Web Server:**
- Async I/O (Tokio)
- Efficient connection handling
- Static file caching by browser

---

## Security Considerations

### Attack Surface

#### 1. Network Exposure

**CAN Bus:**
- **Risk**: Direct access to vessel's NMEA2000 network
- **Mitigation**: Physical isolation, SocketCAN permissions
- **Recommendation**: Run as non-root user with CAN group access

**Web Server (Port 8080):**
- **Risk**: HTTP endpoint exposed to network
- **Mitigation**: No authentication (trusted network assumed)
- **Recommendation**: 
  - Bind to localhost only for single-machine access
  - Use firewall rules to restrict access
  - Deploy reverse proxy with authentication for internet exposure

**UDP Broadcaster (Port 10110):**
- **Risk**: Unencrypted broadcast of vessel data
- **Mitigation**: Disabled by default
- **Recommendation**:
  - Enable only on private vessel network
  - Use unicast instead of broadcast when possible
  - Firewall outbound UDP if not needed

#### 2. Database Security

**Connection:**
- **Risk**: Database credentials in config file
- **Mitigation**: File permissions (0600 recommended)
- **Recommendation**:
  - Use dedicated database user with minimal privileges
  - Restrict database access to localhost
  - Use strong password

**SQL Injection:**
- **Risk**: SQL injection through malformed data
- **Mitigation**: Parameterized queries via MySQL crate
- **Status**: All queries use safe parameter binding

#### 3. File System Access

**Configuration File:**
- **Risk**: Sensitive credentials in `config.json`
- **Mitigation**: User responsible for permissions
- **Recommendation**: `chmod 600 config.json`

**Log Files:**
- **Risk**: Information disclosure through logs
- **Mitigation**: Logs written to configurable directory
- **Recommendation**: 
  - Restrict log directory permissions
  - Rotate and archive logs regularly
  - Review log verbosity level

**Static Files:**
- **Risk**: Path traversal in static file serving
- **Mitigation**: Tower-HTTP static file middleware
- **Status**: Library handles path validation

#### 4. Code Security

**Memory Safety:**
- **Rust guarantees**: No buffer overflows, use-after-free, data races
- **Status**: All safe Rust code (no `unsafe` blocks)

**Dependencies:**
- **Risk**: Vulnerabilities in third-party crates
- **Mitigation**: Regular `cargo audit` checks
- **Recommendation**: Keep dependencies updated

**Input Validation:**
- **CAN frames**: Validated by nmea2k decoder
- **Configuration**: Validated with range checks
- **API parameters**: Validated by Axum deserializer

### Security Best Practices

#### Deployment Checklist

- [ ] Run as non-root user with CAN group membership
- [ ] Set `config.json` permissions to 0600
- [ ] Bind web server to localhost if single-machine access
- [ ] Configure firewall rules for web server port
- [ ] Use strong database password
- [ ] Restrict database access to localhost
- [ ] Grant minimal database privileges (no DROP, no admin)
- [ ] Disable UDP broadcaster if not needed
- [ ] Use private network for UDP broadcasts
- [ ] Restrict log directory permissions
- [ ] Regular `cargo audit` for vulnerabilities
- [ ] Keep Rust toolchain updated
- [ ] Monitor logs for anomalies

#### Production Hardening

1. **Reverse Proxy**: Use Nginx/Apache with authentication
2. **TLS**: Terminate HTTPS at reverse proxy
3. **Rate Limiting**: Implement at reverse proxy level
4. **Monitoring**: Set up alerting for crashes/errors
5. **Backups**: Regular database backups
6. **Updates**: Patch OS and dependencies regularly

---

## Deployment

### System Requirements

#### Hardware

- **Architecture**: x86_64 or ARM (Linux)
- **CPU**: 1 core minimum, 2+ recommended
- **RAM**: 512 MB minimum, 1 GB recommended
- **Storage**: 10 GB minimum (database growth)
- **Network**: CAN bus interface required

#### Software

- **OS**: Linux (kernel 2.6.25+ for SocketCAN)
- **Database**: MariaDB 10.5+ or MySQL 8.0+
- **CAN Tools**: can-utils (for diagnostics)

### Installation Steps

#### 1. Install Dependencies

```bash
# Debian/Ubuntu
sudo apt update
sudo apt install -y mariadb-server can-utils

# RHEL/CentOS
sudo yum install -y mariadb-server can-utils
```

#### 2. Set Up Database

```bash
# Create database and user
sudo mysql <<EOF
CREATE DATABASE nmea_router;
CREATE USER 'nmea'@'localhost' IDENTIFIED BY 'your-secure-password';
GRANT ALL PRIVILEGES ON nmea_router.* TO 'nmea'@'localhost';
FLUSH PRIVILEGES;
EOF

# Load schema
mysql -u nmea -p nmea_router < schema.sql
mysql -u nmea -p nmea_router < create_environmental_table.sql
```

**See**: [README_DATABASE.md](README_DATABASE.md) for detailed instructions.

#### 3. Configure CAN Interface

**For testing (virtual CAN):**
```bash
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan
sudo ip link set up vcan0
```

**For production (real CAN):**
```bash
# Find CAN interface
ip link show

# Configure bitrate and bring up
sudo ip link set can0 type can bitrate 250000
sudo ip link set up can0

# Make persistent (systemd)
cat << EOF | sudo tee /etc/systemd/network/80-can.network
[Match]
Name=can0

[CAN]
BitRate=250000
EOF

sudo systemctl enable systemd-networkd
```

#### 4. Build Application

```bash
# Clone repository
git clone <repository-url>
cd rust_nmea_router

# Build release binary
cargo build --release

# Binary located at: target/release/nmea_router
```

#### 5. Configure Application

```bash
# Copy example config
cp config.example.json config.json

# Edit configuration
nano config.json

# Set permissions
chmod 600 config.json

# Validate configuration
./target/release/nmea_router --validate-config
```

#### 6. Create Systemd Service

```bash
# Create service file
sudo cat << EOF > /etc/systemd/system/nmea_router.service
[Unit]
Description=NMEA2000 Router
After=network.target mysql.service

[Service]
Type=simple
User=nmea
Group=nmea
WorkingDirectory=/opt/nmea_router
ExecStart=/opt/nmea_router/nmea_router
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Create user
sudo useradd -r -s /bin/false nmea
sudo usermod -a -G dialout nmea  # For CAN access

# Install application
sudo mkdir -p /opt/nmea_router
sudo cp target/release/nmea_router /opt/nmea_router/
sudo cp config.json /opt/nmea_router/
sudo cp -r static /opt/nmea_router/
sudo chown -R nmea:nmea /opt/nmea_router

# Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable nmea_router
sudo systemctl start nmea_router

# Check status
sudo systemctl status nmea_router
```

#### 7. Verify Operation

```bash
# Check logs
sudo journalctl -u nmea_router -f

# Check CAN traffic
candump can0

# Test web interface
curl http://localhost:8080/api/trips

# Open dashboard in browser
firefox http://localhost:8080
```

### Docker Deployment (Optional)

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libmariadb3 \
    can-utils \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/nmea_router /usr/local/bin/
COPY config.json /etc/nmea_router/
COPY static /usr/share/nmea_router/static
WORKDIR /usr/share/nmea_router
CMD ["nmea_router"]
```

**Note**: Docker requires `--network=host` and `--privileged` for CAN access.

### Backup Strategy

#### Database Backup

```bash
# Daily backup script
#!/bin/bash
BACKUP_DIR=/var/backups/nmea_router
DATE=$(date +%Y%m%d_%H%M%S)

mkdir -p $BACKUP_DIR
mysqldump -u nmea -p'password' nmea_router | gzip > $BACKUP_DIR/nmea_router_$DATE.sql.gz

# Keep last 30 days
find $BACKUP_DIR -name "*.sql.gz" -mtime +30 -delete
```

**Recommended**: Run via cron daily

#### Configuration Backup

```bash
# Backup config and logs
tar -czf nmea_router_config_$(date +%Y%m%d).tar.gz \
    /opt/nmea_router/config.json \
    /opt/nmea_router/logs
```

### Monitoring

#### Health Checks

```bash
# Check process running
systemctl is-active nmea_router

# Check web server responding
curl -f http://localhost:8080/api/trips || echo "API unhealthy"

# Check database connectivity
mysql -u nmea -p'password' nmea_router -e "SELECT 1" || echo "DB unhealthy"

# Check CAN interface
ip link show can0 | grep -q UP || echo "CAN interface down"
```

#### Log Monitoring

```bash
# Watch for errors
journalctl -u nmea_router | grep ERROR

# Watch database health
journalctl -u nmea_router | grep -i "database.*health"

# Watch for time sync issues
journalctl -u nmea_router | grep -i "time.*skew"
```

---

## Testing Strategy

### Test Coverage

#### Unit Tests

- **Location**: Inline in source files (73 tests total)
- **Coverage**:
  - Configuration validation and deserialization
  - Position distance calculations (Haversine)
  - Mooring detection logic
  - Trip management (creation, updates, closure)
  - Environmental metric aggregation
  - UDP message serialization
  - Time sync threshold checking

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_mooring_detection
```

#### Integration Tests

**Manual integration testing workflow:**

1. **CAN Interface Test**:
```bash
# Terminal 1: Generate test data
cangen vcan0 -g 100 -I 09F10F00 -D 080102030405060708

# Terminal 2: Run application
cargo run

# Terminal 3: Monitor
candump vcan0
```

2. **Database Integration**:
```bash
# Verify writes
mysql -u nmea -p nmea_router -e "SELECT * FROM vessel_status ORDER BY timestamp DESC LIMIT 5;"
```

3. **API Integration**:
```bash
# Test endpoints
curl http://localhost:8080/api/trips | jq
curl "http://localhost:8080/api/track?trip_id=1" | jq
curl "http://localhost:8080/api/metrics?metric=pressure" | jq
```

4. **UDP Broadcasting**:
```bash
# Terminal 1: Enable UDP in config, run application
cargo run

# Terminal 2: Listen for UDP
nc -u -l 10110 | jq

# Or use socat
socat UDP-RECV:10110 - | jq
```

#### System Tests

**Full system test scenarios:**

1. **Normal Operation**:
   - Start with clean database
   - Generate position/speed messages
   - Verify vessel status reports
   - Verify trip creation
   - Check web dashboard displays data

2. **Mooring Detection**:
   - Send stable position for 2+ minutes
   - Verify `is_moored` flag set
   - Verify reporting interval changes to 30 minutes
   - Send changing position
   - Verify `is_moored` flag cleared

3. **Database Failure Recovery**:
   - Stop MariaDB service
   - Verify application continues running
   - Verify errors logged
   - Restart MariaDB
   - Verify automatic reconnection
   - Verify writes resume

4. **Time Sync Protection**:
   - Send NMEA time with >500ms skew
   - Verify database writes blocked
   - Verify warning logged
   - Send correct NMEA time
   - Verify writes resume

5. **Configuration Validation**:
   - Test invalid CAN interface
   - Test out-of-range intervals
   - Test missing required fields
   - Test invalid JSON syntax
   - Verify appropriate errors/warnings

### Test Data Generation

#### NMEA2000 Message Generation

**Using can-utils:**

```bash
# Position (PGN 129025)
# Source=10, PGN=129025, Data=lat/lon
cansend vcan0 09F80910#80112233445566FF

# COG/SOG (PGN 129026)
cansend vcan0 09F80A10#FF0102030405FF06

# System Time (PGN 126992)
cansend vcan0 09F03010#0123456789ABCDFF
```

**Using Python script:**

```python
import can
import struct
import time

bus = can.interface.Bus('vcan0', bustype='socketcan')

# Generate position updates
lat = 43.630142
lon = 10.293372

while True:
    # PGN 129025, source 10, priority 3
    arbitration_id = 0x09F80910
    
    # Encode lat/lon (simplified)
    data = struct.pack('<II', 
        int((lat + 90) * 1e7),
        int((lon + 180) * 1e7))
    
    msg = can.Message(arbitration_id=arbitration_id, 
                     data=data, is_extended_id=True)
    bus.send(msg)
    time.sleep(0.1)
```

### Performance Testing

#### Load Testing

```bash
# Generate high message rate
cangen vcan0 -g 10 -n 10000 -I 09F10F00

# Monitor CPU/memory
top -p $(pgrep nmea_router)

# Check message processing rate
journalctl -u nmea_router | grep "processed" | tail
```

#### Database Performance

```bash
# Measure write latency
mysql -u nmea -p nmea_router -e "
SET profiling = 1;
INSERT INTO vessel_status (timestamp, latitude, longitude, average_speed_kn, max_speed_kn, is_moored, engine_on) VALUES (NOW(3), 43.63, 10.29, 5.2, 7.8, 0, 0);
SHOW PROFILES;
"

# Check query performance
mysql -u nmea -p nmea_router -e "
EXPLAIN SELECT * FROM vessel_status WHERE timestamp >= NOW() - INTERVAL 1 DAY;
"
```

#### Web Server Performance

```bash
# Using Apache Bench
ab -n 1000 -c 10 http://localhost:8080/api/trips

# Using wrk
wrk -t4 -c100 -d30s http://localhost:8080/api/trips
```

---

## Future Enhancements

### Planned Features

#### Short Term

1. **Enhanced Web Dashboard**
   - Real-time updates via WebSocket
   - Wind rose visualization
   - Track animation playback
   - Export data to GPX/KML

2. **API Improvements**
   - `/api/latest` endpoint implementation
   - Pagination for large datasets
   - GraphQL support
   - Authentication (JWT)

3. **Database Optimizations**
   - Automatic data archival (older than N months)
   - Partitioning by date
   - Read replicas support
   - Connection pooling

4. **Configuration Enhancements**
   - Environment variable overrides
   - Hot-reload configuration changes
   - Multiple configuration profiles
   - GUI configuration editor

#### Medium Term

5. **Advanced Trip Analytics**
   - Automatic waypoint detection
   - Tacking analysis (sailing efficiency)
   - Weather correlation
   - Fuel consumption estimation

6. **Alerting System**
   - Anchor drag detection
   - Geofencing alerts
   - Environmental threshold alerts
   - Email/SMS notifications

7. **Data Export**
   - CSV export for all tables
   - GPX track export
   - PDF trip reports
   - Excel workbook generation

8. **Mobile Support**
   - Progressive Web App (PWA)
   - Responsive dashboard
   - Offline capability
   - Mobile-optimized charts

#### Long Term

9. **Multi-Vessel Support**
   - Track multiple vessels
   - Fleet management dashboard
   - Vessel comparison analytics
   - Central data aggregation

10. **Machine Learning**
    - Arrival time prediction
    - Weather routing suggestions
    - Anomaly detection
    - Predictive maintenance

11. **Integration Ecosystem**
    - SignalK integration
    - Weather service APIs
    - AIS data integration
    - Marina/port information

12. **Cloud Deployment**
    - Kubernetes deployment
    - Cloud database support (AWS RDS, Azure SQL)
    - Object storage for backups (S3, Azure Blob)
    - Horizontal scaling

### Community Contributions

**Welcome contributions in:**
- Additional PGN support
- Alternative database backends
- Dashboard enhancements
- Documentation improvements
- Bug fixes and optimizations

**See**: CONTRIBUTING.md (to be created)

---

## Appendix

### Glossary

| Term | Definition |
|------|------------|
| **CAN** | Controller Area Network - automotive/marine bus protocol |
| **COG** | Course Over Ground - direction of travel |
| **GNSS** | Global Navigation Satellite System (GPS, GLONASS, etc.) |
| **Knot** | Nautical mile per hour (1.852 km/h) |
| **Nautical Mile** | 1852 meters (1.15078 statute miles) |
| **NMEA2000** | Marine data communication standard |
| **PGN** | Parameter Group Number - NMEA2000 message type ID |
| **SOG** | Speed Over Ground - GPS-measured speed |
| **SocketCAN** | Linux CAN bus interface |

### References

- **NMEA2000 Standard**: http://www.nmea.org/
- **CAN Bus Protocol**: ISO 11898-1
- **SocketCAN Documentation**: https://www.kernel.org/doc/html/latest/networking/can.html
- **Rust Language**: https://www.rust-lang.org/
- **Axum Framework**: https://docs.rs/axum/
- **Tokio Runtime**: https://tokio.rs/

### Related Documentation

Project documentation files:

- [README.md](README.md) - Main project README
- [README_DATABASE.md](README_DATABASE.md) - Database setup guide
- [UDP_BROADCASTER_SPECS.md](UDP_BROADCASTER_SPECS.md) - UDP broadcaster specification
- [TRIP_IMPLEMENTATION.md](TRIP_IMPLEMENTATION.md) - Trip management details
- [ENVIRONMENTAL_MONITORING.md](ENVIRONMENTAL_MONITORING.md) - Environmental monitoring details
- [SOURCE_FILTER.md](SOURCE_FILTER.md) - Source filtering configuration
- [LOGGING.md](LOGGING.md) - Logging configuration guide
- [nmea2k/README.md](nmea2k/README.md) - NMEA2000 decoder library

### Version History

- **0.1.0** (January 2026)
  - Initial release
  - Core NMEA2000 support (15+ PGNs)
  - Adaptive database persistence
  - Trip management with sailing/motoring breakdown
  - Environmental monitoring (7 metrics)
  - Web dashboard and REST API
  - UDP broadcasting
  - Time synchronization protection
  - Source filtering
  - Configuration validation
  - 73 unit tests

### License

(License information to be added)

### Authors

(Author information to be added)

### Acknowledgments

- Original Python project: https://github.com/titio72/nmearouter
- Rust community for excellent tooling
- Claude AI for development assistance
- Marine open-source community

---

**Document Version**: 1.0  
**Generated**: January 26, 2026  
**Status**: Complete and Current
