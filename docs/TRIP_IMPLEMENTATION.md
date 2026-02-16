# Trip Entity Implementation Summary

## Overview
Added automatic trip tracking functionality to the NMEA2000 Router. The system now automatically records vessel journeys, separating sailing, motoring, and moored time.

## Files Created

### 1. `src/trip.rs`
New module containing the Trip entity with:
- **Fields:**
  - `id`: Database identifier (Option<i64>)
  - `description`: Human-readable trip name
  - `start_timestamp`: Trip start time (Instant)
  - `end_timestamp`: Trip end time (Instant)
  - `total_distance_sailed`: Distance under sail in nautical miles (f64)
  - `total_distance_motoring`: Distance with engine in nautical miles (f64)
  - `total_time_sailing`: Time sailing in milliseconds (u64)
  - `total_time_motoring`: Time motoring in milliseconds (u64)
  - `total_time_moored`: Time moored in milliseconds (u64)

- **Methods:**
  - `new()`: Create a new trip with start timestamp
  - `update()`: Update trip with new status data (distance, time, engine state, moored state)
  - `is_active()`: Check if trip is within 24 hours of current time
  - `total_distance()`: Get combined sailing + motoring distance
  - `total_time()`: Get total time (sailing + motoring + moored)

- **Tests:** 8 comprehensive unit tests covering all functionality

## Files Modified

### 1. `src/main.rs`
- Added `trip` module import
- Added `Trip` type import
- Added `current_trip: Option<Trip>` variable to main loop
- Added logic to load last trip from database on startup
- Modified `handle_vessel_status()` signature to accept `current_trip` parameter
- Created new `handle_trip_update()` function with trip management logic:
  - Checks if current trip is still active (within 24 hours)
  - Creates new trip if no active trip exists or last trip is older than 24 hours
  - Updates existing trip if still active
  - Persists trips to database

### 2. `src/db.rs`
Added three new database methods:

- **`get_last_trip()`**: Retrieves the most recent trip from database
  - Converts database timestamps to Instant types
  - Returns Option<Trip>

- **`insert_trip()`**: Inserts a new trip into database
  - Converts Instant to SystemTime for database storage
  - Returns the new trip ID

- **`update_trip()`**: Updates an existing trip in database
  - Updates end timestamp and all distance/time fields
  - Requires trip to have an ID

### 3. `README_DATABASE.md`
- Added "Trip Tracking" section explaining:
  - Trip logic and 24-hour boundary rule
  - Database schema
  - Migration instructions
- Added SQL query examples:
  - Current trip summary with formatted times
  - All trips summary with percentages
  - Trip statistics

## Trip Logic Flow

1. **On Application Start:**
   - Attempt to load the last trip from database
   - If found and within 24 hours, continue updating it
   - If not found or older than 24 hours, will create new on first status write

2. **On Each Vessel Status Write:**
   - After successful database write, call `handle_trip_update()`
   - Check if current trip exists and is active (within 24 hours)
   - **If no trip or inactive:** Create new trip with:
     - Description: "Trip YYYY-MM-DD"
     - Start timestamp: current report timestamp
     - Initial update with current status data
   - **If trip is active:** Update existing trip with:
     - End timestamp: current report timestamp
     - Accumulated distance (to sailed or motoring based on engine state)
     - Accumulated time (to sailing, motoring, or moored based on state)

3. **State Classification:**
   - **Moored:** `is_moored = true` → accumulates to `total_time_moored`
   - **Motoring:** `is_moored = false && engine_on = true` → accumulates to `total_distance_motoring` and `total_time_motoring`
   - **Sailing:** `is_moored = false && engine_on = false` → accumulates to `total_distance_sailed` and `total_time_sailing`

## Database Schema

```sql
CREATE TABLE trips (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    description VARCHAR(255) NOT NULL,
    start_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    end_timestamp DATETIME(3) NOT NULL COMMENT 'UTC timezone',
    total_distance_sailed DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    total_distance_motoring DOUBLE NOT NULL DEFAULT 0 COMMENT 'nautical miles',
    total_time_sailing BIGINT NOT NULL DEFAULT 0,
    total_time_motoring BIGINT NOT NULL DEFAULT 0,
    total_time_moored BIGINT NOT NULL DEFAULT 0,
    INDEX idx_end_timestamp (end_timestamp),
    INDEX idx_start_timestamp (start_timestamp)
);
```

## Example Usage

### Query Current Trip
```sql
SELECT 
    description,
    start_timestamp,
    end_timestamp,
    ROUND((total_distance_sailed + total_distance_motoring) / 1852.0, 2) as total_nm,
    ROUND(total_distance_sailed / 1852.0, 2) as sailed_nm,
    ROUND(total_distance_motoring / 1852.0, 2) as motored_nm
FROM trips 
ORDER BY end_timestamp DESC 
LIMIT 1;
```

### Calculate Sail vs Motor Percentage
```sql
SELECT 
    description,
    ROUND(total_distance_sailed / (total_distance_sailed + total_distance_motoring) * 100, 1) as sail_percentage,
    ROUND(total_distance_motoring / (total_distance_sailed + total_distance_motoring) * 100, 1) as motor_percentage
FROM trips 
WHERE id = <trip_id>;
```

## Testing

All 76 tests pass, including:
- 8 new tests in `trip::tests` module
- All existing tests continue to pass
- Trip creation, update, and active status detection verified

## Deployment

1. The trips table schema is included in the main schema.sql file.

2. Restart the application:
   ```bash
   cargo build --release
   ./target/release/nmea_router
   ```

3. The application will automatically:
   - Load any existing trip from database
   - Start a new trip on first vessel status write if no active trip exists
   - Continue tracking trips indefinitely

## Notes

- Trips are automatically bounded by 24-hour inactivity
- Trip descriptions are auto-generated as "Trip YYYY-MM-DD" using the start date
- All timestamps stored in UTC timezone
- Distance values in nautical miles, time values in milliseconds in database
- The system is resilient to database failures (continues if trip write fails)
