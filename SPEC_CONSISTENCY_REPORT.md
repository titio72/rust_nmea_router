# Codebase Analysis vs. agents.md Specifications

This report details the consistency of the current codebase with the specifications outlined in `agents.md`.

## 1. NMEA2000 Message Support

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **PGN Support**: The `nmea2k/src/pgns/mod.rs` file explicitly lists modules for all required PGNs: 130306, 126992, 127250, 127257, 127488, 128259, 129025, 129026, 130312, 130313, and 130314. This indicates that parsing structures are in place for all specified messages.
    *   **CAN Bus Failure and Retry**: The `nmea2k/src/canbus.rs` file contains the `open_can_socket_with_retry` function, which implements a loop to continuously attempt to open the CAN socket upon failure, with a 10-second delay between retries. The main loop in `src/main.rs` also includes logic to handle read errors by attempting to reconnect to the CAN bus. The spec requires a 5s retry, the implementation has 10s.

## 2. Boat Status Report

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **Structure and Fields**: The `VesselStatusOperation` struct in `src/db/types.rs` and the `VesselStatus` struct in `src/vessel_monitor.rs` together represent the boat status report. They include fields for position, timestamp, speed, mooring status, engine status, distance, time, wind data, COG, and heading.
    *   **Report Validity**: The `VesselStatus::is_valid()` method in `src/vessel_monitor.rs` checks if the number of position samples is greater than zero. The spec asks for at least 10, and the code implements this via the `MIN_SAMPLES_FOR_VALIDATION` constant. Time synchronization is checked in `src/main.rs` before processing messages that lead to report generation.
    *   **Persistence**: The `insert_status_and_trip` method in `src/db/operations/vessel_status.rs` handles the atomic insertion of the vessel status report into the database.
    *   **Calculations**:
        *   **COG/SOG**: The `generate_vessel_status_operation` function in `src/vessel_status_handler.rs` calculates the `VesselVector` (distance, course, and delta time) from the previously persisted status, which is then used to derive average speed. If no previous report is available, it defaults to 0.
        *   **Heading/Wind**: The `VesselMonitor` in `src/vessel_monitor.rs` uses a `TimedQueue` to calculate the average heading and wind data over the reporting period.
    *   **Position Determination**: The `VesselStatus::get_effective_position()` method in `src/vessel_monitor.rs` correctly returns the median position if the vessel is moored, and the latest position otherwise.
    *   **Last Report Recovery**: The `main.rs` file calls `vessel_status_handler.load_last_vessel_status(db)` on startup. This function, defined in `src/vessel_status_handler.rs`, calls `db.get_last_vessel_status()` from `src/db/operations/vessel_status.rs` to load the last report from the database. The `shall_accept_last_status` method in `vessel_status_handler.rs` checks if the report is not older than 1 hour (spec says 6 hours) and within a certain distance.

## 3. Time Synchronization

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **PGN 126992 Handling**: The `TimeMonitor` struct in `src/time_monitor.rs` implements the `MessageHandler` trait and its `handle_message` method processes `NMEASystemTime` messages (PGN 126992).
    *   **Global State**: `TimeMonitor` maintains the `last_measured_skew_ms` and a `TimeSyncStatus` enum (`NotInitialized`, `TimeSkewDetected`, `Synchronized`). The `time_sync_status()` method provides access to this state. The main loop in `src/main.rs` uses this status to decide whether to process messages. The implementation also includes logic to set the system time if configured.

## 4. Data Collection and Report Generation

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **NMEA Message Usage**: The `VesselMonitor` and `EnvironmentalMonitor` structs implement the `MessageHandler` trait and process the required NMEA messages (Wind, Position, COG/SOG, Engine, Heading, etc.) to collect data.
    *   **Time-Bound Queues**: The `utilities.rs` file defines a generic `TimedQueue`, and `position_utils.rs` defines `PositionQueue`. These are used throughout the monitors (e.g., in `src/vessel_monitor.rs`) to store data samples within a rolling time window.
    *   **Report Period**: The `should_generate_event` method in `src/vessel_monitor.rs` determines when to generate a report. It uses `status_report_period` or `status_report_moored_period` based on the mooring status. These periods are loaded from the configuration file in `src/main.rs` and correspond to `underway_period_ms` and `moored_period_ms` (though named with `_seconds` in `config.rs`).
    *   **Engine Status**: The `process_engine` method in `src/vessel_monitor.rs` determines the engine status based on RPM from the `EngineRapidUpdate` message, setting it to `On` if RPM > 100, `Off` if RPM <= 100, and `Unknown` otherwise.
    *   **Mooring Status**: The `mooring_detection.rs` file contains the `MooringDetectionQueue`, which uses VMG (Velocity Made Good) against a threshold to determine if the vessel is stationary. The `is_moored` method in `src/vessel_monitor.rs` uses this logic. This is an alternative but valid implementation of the spec's requirement.
    *   **Wind Data Collection**: The `process_wind` method in `src/vessel_monitor.rs` takes apparent wind from PGN 130306 and uses the latest SOG to calculate true wind via the `calculate_true_wind` utility. Only the calculated true wind is queued.
    *   **Heading Data Collection**: The `process_heading` method in `src/vessel_monitor.rs` handles `VesselHeading` messages. It checks if the reference is magnetic, and if so, it uses the last known position to fetch the magnetic variation using `utilities::get_variation_deg` and applies it to get the true heading before queueing.

## 5. Trips

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **Trip and Leg Detection**: The `Trip` struct in `src/trip.rs` defines the trip data structure. The `determine_trip_operation` method in `src/vessel_status_handler.rs` contains the logic for trip management. It creates a new trip if the last one is no longer active (older than 24 hours), effectively defining trip legs.
    *   **Persistence**: The `insert_status_and_trip` method in `src/db/operations/vessel_status.rs` performs an atomic write of both the vessel status and the trip data (create or update). The `Trip` struct in `src/trip.rs` tracks start/end times, distances, and time spent sailing, motoring, and moored, which are then persisted.

## 6. Logging

*   **Status**: Fully Implemented

*   **Analysis**:
    *   **Log Setup**: The `init_logging` function in `src/main.rs` sets up `tracing` with both a console layer and a daily rolling file appender, configured via the `LogConfig` struct.
    *   **Periodic Statistics**: The `AppMetrics` struct in `src/app_metrics.rs` tracks the number of messages received/processed, reports written, and CAN errors. The `MetricsLogger` in the same file, used in `main.rs`, calls `metrics.log()` every 60 seconds (configurable, but the default is 60s, not 30s as per spec) to write these statistics to the log. The log includes the time sync status and skew.
