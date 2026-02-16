# Database Testing Infrastructure

This document describes the test infrastructure for database-related features in the NMEA2000 Router application.

## Overview

The test infrastructure provides a comprehensive set of tools to:
1. Create isolated, repeatable test databases
2. Generate realistic test data (trips, vessel status, environmental data)
3. Populate databases with preconfigured datasets
4. Verify database operations with helper functions

## Design Principles

All tests follow these core principles:
- **Predictable**: Test data is completely known and controlled
- **Repeatable**: Each test starts with a clean database state
- **Non-destructive**: Tests use a separate test database, never production

## Setup

### 1. Database Configuration

The test infrastructure uses a separate test database configured in `test_config.json`:

```json
{
  "database": {
    "connection": {
      "host": "localhost",
      "port": 3306,
      "username": "nmea_test",
      "password": "nmea_test",
      "database_name": "nmea_router_test"
    }
  }
}
```

### 2. Create Test Database

```sql
CREATE DATABASE nmea_router_test;
CREATE USER 'nmea_test'@'localhost' IDENTIFIED BY 'nmea_test';
GRANT ALL PRIVILEGES ON nmea_router_test.* TO 'nmea_test'@'localhost';
FLUSH PRIVILEGES;

-- Apply schema
USE nmea_router_test;
SOURCE schema.sql;
```

### 3. Configuration Loading

Tests automatically load `test_config.json` instead of `config.json`:

```rust
use crate::config::Config;

// Automatically loads test_config.json when cfg(test) is active
let config = Config::load_for_context()?;
```

## Available Test Helpers

### Database Lifecycle (`src/db/test_helpers.rs`)

```rust
use crate::db::test_helpers::*;

// Setup and reset database
let db = setup_test_db(&connection_url)?;
reset_test_db(&db)?;  // Clear all data, keep schema
teardown_test_db(&db)?;  // Same as reset for now
```

### Data Insertion

#### Add Trip
```rust
let trip_id = add_test_trip(
    &db,
    "Test Trip".to_string(),  // description
    start_timestamp,
    end_timestamp,
    10.5,  // total_distance_sailed (NM)
    2.3,   // total_distance_motoring (NM)
    3000000,  // total_time_sailing (ms)
    600000,   // total_time_motoring (ms)
    0,        // total_time_moored (ms)
)?;
```

#### Add Vessel Status
```rust
let status_id = add_test_vessel_status(
    &db,
    timestamp,
    Some(41.0),  // latitude
    Some(2.0),   // longitude
    6.5,         // average_speed_kn
    7.2,         // max_speed_kn
    Some(12.0),  // average_wind_speed_kn
    Some(45.0),  // average_wind_angle_deg
    false,       // is_moored
    EngineStatus::Off,
    1.5,         // total_distance_nm
    1800000,     // total_time_ms
    Some(90.0),  // cog_deg
    Some(92.0),  // average_heading_deg
)?;
```

#### Add Environmental Data
```rust
let env_id = add_test_env(
    &db,
    timestamp,
    1,              // metric_id (1=Pressure)
    Some(101325.0), // value_avg
    Some(101400.0), // value_max
    Some(101250.0), // value_min
    "Pa",           // unit
)?;
```

### Position & Navigation Utilities

#### Calculate Position from Bearing
```rust
use crate::db::test_helpers::calculate_position_from_bearing;

let start = Position { latitude: 40.0, longitude: -70.0 };
let end = calculate_position_from_bearing(
    start,
    90.0,   // bearing (degrees, 0=N, 90=E)
    6.0,    // speed (knots)
    3600.0, // duration (seconds)
);
```

#### Generate Track
```rust
use crate::db::test_helpers::generate_track;

let track = generate_track(
    start_position,
    end_position,
    6.0,        // speed_kn
    600,        // interval_s (10 minutes)
    start_time,
);

// Returns Vec<(Position, SystemTime)>
for (position, timestamp) in track {
    // Process each waypoint
}
```

#### Generate Realistic Wind
```rust
let (wind_speed, wind_angle) = generate_realistic_wind(
    12.0,  // base_speed_kn
    45.0,  // base_angle_deg
    2.0,   // variation
);
```

### Data Retrieval & Verification

#### Fetch Trip
```rust
let trip = fetch_trip_by_timestamp(&db, start_timestamp)?;
if let Some(trip) = trip {
    assert_eq!(trip.description, "Expected description");
    assert_approx_equal(trip.total_distance_sailed, 10.5, 0.001, "Sailed distance");
}
```

#### Fetch Vessel Status
```rust
let status = fetch_vessel_status_by_timestamp(&db, timestamp)?;
if let Some(status) = status {
    assert_approx_equal(status.latitude.unwrap(), 41.0, 0.0001, "Latitude");
    assert!(!status.is_moored);
}
```

#### Fetch Environmental Data
```rust
let env = fetch_env_data_by_timestamp(&db, timestamp, metric_id)?;
if let Some(env) = env {
    assert_approx_equal(env.value_avg.unwrap(), 101325.0, 0.1, "Pressure");
}
```

### Test Data Operations (`src/db/operations/test_data.rs`)

#### Simulated Sailing Trip
```rust
use crate::db::operations::test_data::*;

let trip_id = insert_simulated_sailing_trip(
    &db,
    start_position,
    end_position,
    start_time,
    6.0,  // speed_kn
    600,  // interval_s
)?;
```

#### Simulated Motoring Trip
```rust
let trip_id = insert_simulated_motoring_trip(
    &db,
    start_position,
    end_position,
    start_time,
    7.0,  // speed_kn
    300,  // interval_s
)?;
```

#### Moored Status
```rust
let status_id = insert_moored_status(
    &db,
    position,
    timestamp,
    3600000,  // duration_ms
)?;
```

#### Populate Sample Trips
```rust
// Creates 3 predefined realistic trips
let trip_ids = populate_sample_trips(&db)?;
```

### Assertion Helpers

```rust
use crate::db::test_helpers::{assert_approx_equal, assert_option_approx_equal};

// For floating point comparisons
assert_approx_equal(actual, expected, 0.001, "Distance mismatch");

// For optional floating point comparisons
assert_option_approx_equal(Some(41.0), Some(41.001), 0.01, "Latitude");
```

## Writing Tests

### Basic Test Template

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::test_helpers::*;

    fn setup_db() -> VesselDatabase {
        let config = Config::load_for_context()
            .expect("Failed to load test config");
        let db_url = config.database.connection.connection_url();
        let db = setup_test_db(&db_url)
            .expect("Failed to setup test database");
        reset_test_db(&db).expect("Failed to reset");
        db
    }

    #[test]
    #[ignore]  // Mark as ignored since it requires database
    fn test_my_feature() {
        let db = setup_db();
        
        // Your test code here
        
        // Database is automatically reset for next test
    }
}
```

### Example: Testing Trip Creation

```rust
#[test]
#[ignore]
fn test_trip_creation_and_update() {
    let db = setup_db();
    
    let start_time = UNIX_EPOCH + Duration::from_secs(1609459200);
    let end_time = start_time + Duration::from_secs(7200);
    
    // Insert trip
    let trip_id = add_test_trip(
        &db,
        "Coastal Cruise".to_string(),
        start_time,
        end_time,
        15.5, 3.2, 6000000, 1200000, 0,
    ).expect("Failed to insert trip");
    
    // Retrieve and verify
    let trip = fetch_trip_by_timestamp(&db, start_time)
        .expect("Query failed")
        .expect("Trip not found");
    
    assert_eq!(trip.id.unwrap(), trip_id);
    assert_eq!(trip.description, "Coastal Cruise");
    assert_approx_equal(trip.total_distance_sailed, 15.5, 0.001, "Sailed");
    assert_approx_equal(trip.total_distance_motoring, 3.2, 0.001, "Motored");
}
```

### Example: Testing Track Generation

```rust
#[test]
#[ignore]
fn test_realistic_track_generation() {
    let db = setup_db();
    
    let start = Position { latitude: 41.0, longitude: 2.0 };
    let end = Position { latitude: 41.5, longitude: 2.8 };
    let start_time = UNIX_EPOCH + Duration::from_secs(1609459200);
    
    let trip_id = insert_simulated_sailing_trip(
        &db, start, end, start_time, 6.0, 600
    ).expect("Failed to create trip");
    
    let trip = fetch_trip_by_timestamp(&db, start_time)
        .expect("Query failed")
        .expect("Trip not found");
    
    // Verify realistic values
    assert!(trip.total_distance_sailed > 0.0);
    assert!(trip.total_time_sailing > 0);
    
    // Calculate expected distance using Haversine
    use crate::utilities::haversine_distance_nm;
    let expected_distance = haversine_distance_nm(
        start.latitude, start.longitude,
        end.latitude, end.longitude
    );
    
    // Should be close to straight-line distance
    assert_approx_equal(
        trip.total_distance_sailed,
        expected_distance,
        0.5,  // Allow 0.5 NM tolerance
        "Trip distance"
    );
}
```

## Running Tests

### Run All Tests (excluding ignored database tests)
```bash
cargo test
```

### Run Database Tests
```bash
# Run specific test
cargo test test_basic_trip_insertion_and_retrieval -- --ignored

# Run all database tests
cargo test --test '*' -- --ignored

# Run tests in a specific module
cargo test db::test_examples -- --ignored
```

### Run Tests with Output
```bash
cargo test -- --ignored --nocapture
```

## Test Examples

See `src/db/test_examples.rs` for comprehensive examples including:
- Basic trip insertion and retrieval
- Vessel status operations
- Environmental data handling
- Position calculations
- Track generation
- Simulated trips (sailing and motoring)
- Database reset functionality

## Troubleshooting

### Database Connection Errors
- Ensure MySQL/MariaDB is running
- Verify test database exists (`nmea_router_test`)
- Check credentials in `test_config.json`
- Confirm schema is applied to test database

### Test Failures
- Tests are marked `#[ignore]` by default - use `--ignored` flag
- Each test resets the database - no state is shared
- Check that test_config.json exists in project root

### Compilation Errors
- Test helpers are only available in `#[cfg(test)]` context
- Ensure `rand` dependency is in Cargo.toml
- Check that all modules are properly imported

## Best Practices

1. **Always reset database** at the start of each test
2. **Use `#[ignore]`** for tests requiring database
3. **Use assertion helpers** for floating-point comparisons
4. **Generate realistic data** using provided utilities
5. **Document test purpose** with clear function names and comments
6. **Avoid hardcoded IDs** - use returned IDs from insert functions
7. **Test edge cases** (empty trips, zero speeds, null values)

## Architecture

```
src/db/
├── connection.rs           # Database connection management
├── types.rs               # Database types and structures
├── mod.rs                 # Module organization
├── test_helpers.rs        # [TEST] Core test utilities
├── test_examples.rs       # [TEST] Example tests
└── operations/
    ├── trip.rs            # Trip operations
    ├── vessel_status.rs   # Vessel status operations
    ├── environmental.rs   # Environmental data operations
    ├── query.rs           # Query operations
    ├── import_export.rs   # Import/export operations
    └── test_data.rs       # [TEST] Batch test data operations
```

## Future Enhancements

- [ ] Database snapshot/restore functionality
- [ ] Performance benchmarking utilities
- [ ] Automated test data generation from templates
- [ ] Test coverage reporting
- [ ] Mock time utilities for deterministic tests
