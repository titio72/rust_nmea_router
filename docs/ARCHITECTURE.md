# NMEA2000 Router — Architecture & Functioning Principles

## Table of Contents

1. [System Overview](#1-system-overview)
2. [High-Level Architecture](#2-high-level-architecture)
3. [Startup Sequence](#3-startup-sequence)
4. [Data Ingestion Pipeline](#4-data-ingestion-pipeline)
5. [Core Subsystems](#5-core-subsystems)
   - 5.1 [Time Monitor](#51-time-monitor)
   - 5.2 [Vessel Monitor](#52-vessel-monitor)
   - 5.3 [Mooring Detection](#53-mooring-detection)
   - 5.4 [Vessel Status Handler](#54-vessel-status-handler)
   - 5.5 [Environmental Monitor](#55-environmental-monitor)
   - 5.6 [Trip Manager](#56-trip-manager)
6. [Outbound Channels](#6-outbound-channels)
   - 6.1 [REST API](#61-rest-api)
   - 6.2 [SignalK WebSocket](#62-signalk-websocket)
   - 6.3 [UDP NMEA0183 Broadcaster](#63-udp-nmea0183-broadcaster)
7. [Database Layer](#7-database-layer)
8. [Configuration](#8-configuration)
9. [Key Algorithms & Principles](#9-key-algorithms--principles)
10. [Observability & Metrics](#10-observability--metrics)
11. [Gap Filler Utility](#11-gap-filler-utility)

---

## 1. System Overview

The NMEA2000 Router is a Rust application running on a Linux system aboard a vessel. It reads the ship's NMEA2000 CAN bus, decodes navigation and environmental messages from onboard instruments, and provides three services:

- **Persistence** — writes structured vessel-status and environmental records to a MariaDB database for later analysis.
- **Real-time broadcast** — re-publishes live data as SignalK deltas (WebSocket) and as legacy NMEA0183 sentences (UDP) for chart plotters and other instruments.
- **Web dashboard & API** — an Axum HTTP server that serves REST endpoints and static HTML dashboards for trip review and environmental analysis.

```
CAN Bus (NMEA2000)
      │
      ▼
 nmea2k crate                 ← decoding library (workspace crate)
      │
      ▼
 router_loop.rs (event loop)
      ├──► TimeMonitor         ← gate: block data until UTC is synced
      ├──► VesselMonitor       ── state machine ──► VesselStatusHandler ──► MariaDB
      ├──► EnvironmentalMonitor ──────────────────► EnvStatusHandler    ──► MariaDB
      ├──► UdpBroadcaster      ← NMEA0183 sentences → UDP
      └──► SignalKBroadcaster  ← SignalK deltas → WebSocket clients
                                         ▲
                              Web server (Axum) ─── REST API ─── Dashboards
```

---

## 2. High-Level Architecture

The application follows a **layered, event-driven** design:

| Layer | Files | Responsibility |
|---|---|---|
| **Transport** | `nmea2k/` | CAN frame assembly, NMEA2000 PGN parsing |
| **Event loop** | `src/router_loop.rs` | Message dispatch, reconnection, metrics |
| **Business logic** | `src/vessel_monitor.rs`, `src/environmental_monitor.rs`, `src/trip.rs`, `src/mooring_detection.rs` | Aggregation, state machines, calculations |
| **Persistence** | `src/vessel_status_handler.rs`, `src/environmental_status_handler.rs`, `src/db/` | DB writes, trip lifecycle |
| **Output** | `src/signalk_broadcaster.rs`, `src/udp_broadcaster.rs` | Real-time broadcast |
| **API** | `src/web/` | HTTP REST + WebSocket |

Dependencies flow strictly downward. Business logic does not import from the API or broadcast layers.

---

## 3. Startup Sequence

On launch `main()` performs the following steps in order:

1. **Load configuration** from `./config.json` → `/etc/nmea_router/config.json` → defaults.
2. **Initialize logging** — daily rolling file in the configured directory, plus stdout.
3. **Open the CAN socket** via `CanBus::open_can_socket_with_retry()`. Retries indefinitely with a minimum 5 s gap between attempts.
4. **Connect to MariaDB**. If the connection fails the application continues without persistence, logging a warning.
5. **Restore in-memory state** from the database:
   - `VesselStatusHandler::load_last_trip()` — restores the current trip so accumulated distances are not lost.
   - `VesselStatusHandler::load_last_vessel_status()` — restores the last persisted position/timestamp so the first status report can compute a correct displacement vector. Only records younger than 6 hours are considered.
6. **Start the Axum web server** in a dedicated OS thread (with its own Tokio runtime) if `web.enabled = true` and a DB connection is available.
7. **Enter the main read loop**.

---

## 4. Data Ingestion Pipeline

The main loop calls `CanBus::read_nmea2k_frame()` in a tight blocking loop. Each raw CAN frame is passed through the following pipeline:

```
raw CAN frame
     │
     ▼
FrameFilter (source filter)      ← discard frames from unwanted device addresses
     │
     ▼
N2kStreamReader::process_frame() ← reassemble multi-packet fast-packets
     │
     ▼ (complete N2kFrame)
MessageFilter (PGN source map)   ← per-PGN source whitelist
     │
     ▼
TimeMonitor::handle_message()    ← always called, regardless of sync status
     │
     ├── UdpBroadcaster::handle_message()
     ├── SignalKBroadcaster::handle_message()
     │
     └── [only when time is Synchronized]
           ├── VesselMonitor::handle_message()
           └── EnvironmentalMonitor::handle_message()
```

**Time synchronization is a hard gate.** Until `TimeMonitor` reports `Synchronized`, vessel and environmental data are not processed. This prevents records with incorrect timestamps from being written to the database.

**CAN bus reconnection** — on any socket error that is not a timeout, the loop logs a warning and calls `open_can_socket_with_retry()` again before resuming.

---

## 5. Core Subsystems

### 5.1 Time Monitor

**File:** [src/time_monitor.rs](../src/time_monitor.rs)

The `TimeMonitor` listens for PGN 126992 (System Time). On each message it compares the GNSS-derived UTC time with the Linux system clock. If the skew exceeds the configured threshold (default 1000 ms) it attempts to set the system clock via `clock_settime()` and marks the status as `TimeSkewDetected`. Once the skew falls below the threshold it transitions to `Synchronized`.

The `TimeSyncStatus` enum has three states:

| State | Meaning |
|---|---|
| `NotInitialized` | No System Time PGN received yet |
| `TimeSkewDetected` | A skew was found; attempted correction |
| `Synchronized` | Skew < threshold; data processing is allowed |

The current status and last skew value are read by `router_loop.rs` after every message and used to gate vessel/environmental processing.

---

### 5.2 Vessel Monitor

**File:** [src/vessel_monitor.rs](../src/vessel_monitor.rs)

`VesselMonitor` is the core state machine for navigation data. It implements `MessageHandler` and maintains several time-bounded rolling queues:

| Queue | Fed by PGN | Content |
|---|---|---|
| `positions` | 129025 | `(Instant, Position)` samples |
| `speeds` | 129026 | SOG in knots |
| `vmg_for_mooring` | 129026 | VMG (velocity made good toward heading) |
| `wind_speeds` | 130306 | True wind speed (converted from apparent) |
| `wind_angles` | 130306 | True wind angle (converted from apparent) |
| `headings` | 127250 | True heading (converted from magnetic via WMM 2025) |

All queues are bounded by the longer of the underway and moored reporting periods plus a 30 s margin, so older samples are automatically discarded.

**Reporting cadence** — a status report is triggered when the reporting period has elapsed *and* at least 10 position samples have accumulated. The period is shorter when underway (default 30 s) and longer when moored (default 1800 s / 30 min).

**Noise filtering** — two filters protect data quality:
- SOG readings above 25 kn are discarded.
- Position samples more than 100 m from the rolling 10-second median are rejected (bootstrapped: filter is inactive until 10 baseline samples exist).

**Wind processing** — only apparent wind messages are accepted from PGN 130306. The apparent wind is converted to true wind using the most recent SOG reading. If the SOG sample is older than 5 s the wind message is discarded.

**Heading processing** — only magnetic headings (PGN 127250) are accepted. The WMM 2025 model converts them to true heading using the latest known position. If no position is available, the magnetic heading is used as-is.

**Engine status** — PGN 127488 engine RPM is compared to 50 RPM to determine on/off. A 5-second hysteresis prevents rapid toggling.

---

### 5.3 Mooring Detection

**File:** [src/mooring_detection.rs](../src/mooring_detection.rs)

`MooringDetectionQueue` maintains a 180-second rolling window of VMG (velocity made good) samples. The vessel is considered moored if at least 85% of the samples in the window are below 0.25 kn. This approach is robust to lateral drift at anchor (sideways movement does not count as forward progress).

Before 100 samples are accumulated the detector conservatively returns `true` only if *all* samples are below the threshold.

The mooring status gates two downstream behaviors:
- Which position to report (last position vs. median position for the period).
- Which reporting period to use (underway vs. moored interval).
- How trip time is categorized (sailing, motoring, or moored).

---

### 5.4 Vessel Status Handler

**File:** [src/vessel_status_handler.rs](../src/vessel_status_handler.rs)

After `VesselMonitor::generate_status()` produces a `VesselStatus`, the `VesselStatusHandler` converts it into a `VesselStatusOperation` and writes it to the database.

The key calculation performed here is the **displacement vector**: the handler compares the effective position in the new status against the position from the last persisted status to compute:
- Distance traveled (Haversine, nautical miles).
- Average SOG (distance / elapsed time).
- COG (bearing from previous to current position).

The previous status is cached in memory. If it is older than 6 hours (e.g., after a long application restart) it is discarded and the vector calculation is skipped for that period.

After writing the status record the handler updates the current **trip** (see §5.6) with the incremental distance and time breakdown.

---

### 5.5 Environmental Monitor

**File:** [src/environmental_monitor.rs](../src/environmental_monitor.rs)

`EnvironmentalMonitor` collects seven environmental metrics from the CAN bus in rolling `TimedQueue` buffers:

| Metric ID | PGN | Unit |
|---|---|---|
| Pressure | 130314 | Pa |
| Cabin temperature | 130312 | °C |
| Water temperature | 130312 | °C |
| Humidity | 130313 | % |
| Wind speed | 130306 | kn (true) |
| Wind direction | 130306 | degrees |
| Roll (attitude) | 127257 | degrees |

The `EnvironmentalStatusHandler` flushes each metric at its configured interval, writing an `avg/max/min/count` aggregate record to the `environmental_data` table.

---

### 5.6 Trip Manager

**File:** [src/trip.rs](../src/trip.rs)

A `Trip` represents a continuous voyage. Consecutive legs are grouped into the same trip if the time gap between the end of one leg and the start of the next is less than 24 hours.

`VesselStatusHandler` maintains a single `current_trip` in memory. After each status record is written it calls `Trip::update()` with:
- The incremental distance.
- The period duration.
- The engine status (on/off/unknown).
- The mooring status.

`Trip::update()` categorizes the period:

```
is_moored = true  →  total_time_moored  += period
engine_on  = On   →  total_distance_motoring, total_time_motoring  += ...
otherwise         →  total_distance_sailed, total_time_sailing  += ...
```

The trip record is persisted to the `trips` table and kept up to date after every status write. The `start_timestamp` is written once and never modified; `end_timestamp` is updated on every write.

---

## 6. Outbound Channels

### 6.1 REST API

**Files:** [src/web/api.rs](../src/web/api.rs), [src/web/server.rs](../src/web/server.rs)

The Axum web server runs in a dedicated OS thread with its own Tokio runtime. It exposes endpoints under `/api` and serves static HTML/JS dashboards from the `static/` directory.

Key endpoint groups:

| Group | Description |
|---|---|
| `/api/trips` | List, fetch, update, delete trips; import/export |
| `/api/track` | GPS track points for a trip or date range |
| `/api/metrics` | Environmental time-series data |
| `/api/speed_distribution` | Speed histogram for a trip |
| `/api/wind_statistics` | Wind rose / statistics for a trip |
| `/api/heatmap` | Position density heatmap |
| `/api/system` | System status flags (tracking, SignalK, metrics enabled) |
| `/api/backup` | Trigger database backup |

All responses use the envelope `{ "status": "ok"|"error", "data": ..., "error": ... }`.

Feature flags (`tracking_enabled`, `metrics_enabled`, `signalk_enabled`) are read from the `system_status` table in the database and checked on every message, allowing runtime toggling without a restart.

---

### 6.2 SignalK WebSocket

**Files:** [src/web/signalk.rs](../src/web/signalk_messages.rs), [src/signalk_broadcaster.rs](../src/signalk_broadcaster.rs)

The SignalK broadcaster translates NMEA2000 messages to SignalK v1.7.0 delta format and publishes them to an internal broadcast channel. The WebSocket endpoint at `/signalk/v1/stream` subscribes each client to that channel.

All values are transmitted in **SI units** regardless of internal representation (m/s not knots, radians not degrees, Kelvin not Celsius, Pascals). Broadcasting is rate-limited per path to the configured interval (`signalk.rate_limit_ms`).

Mooring status is broadcast as a non-standard path `vessel.mooringStatus` (0 = underway, 1 = moored) after each vessel status write.

---

### 6.3 UDP NMEA0183 Broadcaster

**File:** [src/udp_broadcaster.rs](../src/udp_broadcaster.rs)

Converts NMEA2000 messages to legacy NMEA0183 sentences and broadcasts them over UDP for compatibility with chart plotters and external navigation software.

Supported sentences: `RMC`, `GGA`, `MWV`, `HDT`, `HDM`, `ROT`, `XDR`, `RPM`, `VHW`, `DPT`.

Rate-limited to 1 message per second per sentence type. Configurable broadcast address and bind address in `config.json`.

---

## 7. Database Layer

**Files:** [src/db/](../src/db/)

The database layer uses the `mysql` crate (synchronous) against a MariaDB instance.

### Key Tables

| Table | Purpose |
|---|---|
| `vessel_status` | One row per status report (position, speed, heading, wind, mooring flag, engine status, period) |
| `trips` | Trip summary (start/end timestamps, total distance split by sailing/motoring, time split) |
| `environmental_data` | Aggregated environmental metrics (avg/min/max per interval per metric type) |
| `system_status` | Key-value store for runtime feature flags |

### Patterns

- All queries use the `params!` macro — no string interpolation of user data.
- Multi-statement writes use explicit `start_transaction()` / `commit()` transactions.
- `DECIMAL` columns returned by MySQL arrive as `Value::Bytes`; they are converted via `String::from_utf8(b)?.parse::<f64>()`.
- `VesselDatabase` wraps a `mysql::Pool` and is cloned (`Arc`) between the main thread and the web server thread.

---

## 8. Configuration

**File:** [src/config.rs](../src/config.rs), [config.example.json](../config.example.json)

Configuration is loaded at startup from `./config.json`, then `/etc/nmea_router/config.json`, then compiled-in defaults. It is **read-only at runtime** — all mutable application state lives in the database.

Notable configuration sections:

| Section | Key settings |
|---|---|
| `can_interface` | Name of the Linux SocketCAN interface (e.g. `can0`) |
| `database.connection` | Host, port, user, password, database name |
| `database.vessel_status` | `interval_underway_seconds`, `interval_moored_seconds` |
| `database.environmental` | Per-metric aggregation interval |
| `time` | `skew_threshold_ms`, `set_system_time` |
| `web` | `enabled`, `port` |
| `signalk` | `enabled`, `rate_limit_ms`, `vessel_uuid` |
| `udp` | `enabled`, `address`, `bind_address` |
| `source_filter` | Per-PGN source address whitelist |
| `logging` | `directory`, `file_prefix`, `level` |

---

## 9. Key Algorithms & Principles

### Angle Averaging

Angles are never averaged arithmetically. The correct method is:

```
avg_angle = atan2( mean(sin(angles)), mean(cos(angles)) )
```

This avoids the discontinuity at 0°/360° and produces the correct circular mean.

### Distance & Bearing

All distance and bearing calculations use the **Haversine formula**. No flat-earth approximations.

### True Wind Calculation

Apparent wind (from PGN 130306) is converted to true wind using vector arithmetic:

```
TWS, TWA = f(AWS, AWA, SOG)
```

where SOG is the most recent sample from PGN 129026. If the SOG sample is older than 5 seconds the wind message is discarded.

### Position Reporting

The position included in a vessel status report depends on mooring status:

- **Underway**: the most recent GPS fix.
- **Moored**: the median position of all fixes in the reporting period (reduces GPS wander noise).

### Mooring Detection

VMG (velocity made good relative to true heading) over a 180-second rolling window. The vessel is moored if ≥ 85% of samples have |VMG| < 0.25 kn. Using VMG rather than raw SOG correctly handles a vessel swinging sideways at anchor.

### COG / SOG Computation

COG and average SOG in a status report are computed geometrically from the displacement between the current and previous status positions, not as arithmetic averages of bus values:

```
distance_nm  = haversine(prev_pos, curr_pos)
avg_sog_kn   = distance_nm / elapsed_hours
cog_deg      = bearing(prev_pos, curr_pos)
```

If no previous report exists the bus-reported values are used as a fallback.

### Heading — Magnetic to True

Magnetic heading (PGN 127250) is corrected using the **WMM 2025** (World Magnetic Model) at the current vessel position and UTC date.

### Timestamps

`now()` is called **only** in event handlers (`MessageHandler::handle_message()`), where the CAN frame arrival time is captured as an `Instant`. All downstream functions receive this `Instant` as a parameter. `SystemTime` (wall clock) is used only where UTC calendar time is required (database writes, trip timestamps).

---

## 10. Observability & Metrics

**File:** [src/app_metrics.rs](../src/app_metrics.rs)

Every 60 seconds `MetricsLogger` logs a summary to stdout and the rolling log file:

- CAN frames received / processed / error count.
- NMEA messages decoded / processed.
- Per-PGN parse success/failure counts.
- Vessel status reports written.
- Environmental reports written.
- GNSS time skew (ms) and synchronization status.

Database health is also checked every 60 seconds (`HealthCheckManager`). If the connection has dropped, reconnection is attempted and the last trip is reloaded from the database.

---

## 11. Gap Filler Utility

**File:** [src/bin/gap_filler.rs](../src/bin/gap_filler.rs), [src/db/operations/gap_fill.rs](../src/db/operations/gap_fill.rs)

A standalone binary (`gap_filler`) that back-fills missing vessel status records by interpolating between known positions. Invoked manually or from scripts:

```
gap_filler --logs <dir> --from YYYY-MM-DD --to YYYY-MM-DD [--dry-run]
```

The `--dry-run` flag reports what would be written without touching the database.
