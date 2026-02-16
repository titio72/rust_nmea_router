# Multi-Leg Trip Helper - Documentation

## Overview

The `populate_multi_leg_trip()` function is a powerful test helper that creates realistic multi-leg sailing trips with automatic vessel status records, mooring periods, and proper time/distance tracking.

This is useful for testing complex scenarios involving:
- Multi-day voyages with multiple sailing legs
- Port mooring periods between legs
- Variable wind conditions across different legs
- Realistic speed calculations based on leg distance and time

## Function Signature

```rust
pub fn populate_multi_leg_trip(
    db: &VesselDatabase,
    trip_name: String,
    legs: Vec<(Position, SystemTime, Position, SystemTime, f64, f64)>,
) -> Result<i64, Box<dyn Error>>
```

## Parameters

- **`db`** - Database connection reference
- **`trip_name`** - Description of the trip (stored in trips.description)
- **`legs`** - Vector of leg tuples: `(start_pos, start_time, end_pos, end_time, wind_speed_kn, wind_angle_deg)`

## How It Works

### Timeline Structure

The function creates the following timeline:

```
5 min moored
    |
    v
[LEG 1: Sailing records at 30-sec intervals]
    |
    v
[Mooring period: 5-min intervals until next leg]
    |
    v
[LEG 2: Sailing records at 30-sec intervals]
    |
    v
[Final mooring: Two 5-min records]
```

### Vessel Status Records

For each leg:
1. **Sailing phase** - Records generated at 30-second intervals
   - Speed calculated from leg distance ÷ leg time
   - Max SOG = Average SOG + 10%
   - Heading set equal to COG
   - Wind speed/angle from leg parameters
   - Moored status = false
   - Engine status = Off (sailing only)

2. **Mooring phase** - Records generated at 5-minute intervals
   - Between legs: from leg end until next leg start
   - After final leg: two records 5 minutes apart
   - All fields except wind set to zero/null
   - Wind persists from previous leg (realistic behavior)
   - Moored status = true
   - Engine status = Off

### Trip Record

The trip record is calculated with:
- **Total distance** - Sum of all leg distances
- **Total sailing time** - Sum of all leg durations + initial 5-min mooring
- **Total moored time** - Sum of all mooring periods
- **Engine status** - All Off (pure sailing trip)

## Usage Example

```rust
let base_time = UNIX_EPOCH + Duration::from_secs(1609459200); // Jan 1, 2021

let legs = vec![
    (
        Position { latitude: 41.0, longitude: 2.0 },    // Start Barcelona
        base_time,
        Position { latitude: 41.3, longitude: 2.4 },    // End position
        base_time + Duration::from_secs(3600),          // 1 hour sailing
        10.0,  // Wind 10 knots
        45.0,  // Wind angle 45°
    ),
    (
        Position { latitude: 41.3, longitude: 2.4 },
        base_time + Duration::from_secs(7200),          // Start after 2-hour break
        Position { latitude: 41.6, longitude: 2.8 },
        base_time + Duration::from_secs(8400),          // 20 minutes sailing
        12.0,  // Wind 12 knots
        50.0,  // Wind angle 50°
    ),
];

let trip_id = populate_multi_leg_trip(
    &db,
    "Mediterranean sailing trip".to_string(),
    legs,
)?;

println!("Created trip with ID: {}", trip_id);
```

## Key Behaviors

### Speed Calculation
- **Leg speed** = Distance (Haversine) ÷ Time
- **Segment speed** = Actual distance between 30-sec points ÷ 30 seconds
- Max SOG automatically = Average SOG × 1.1

### Wind Handling
- Wind from the current leg applies to all sailing records in that leg
- Wind persists into mooring periods from the previous leg
- This creates realistic behavior (wind doesn't magically change when vessel moors)

### Timing
- Each record is timestamped accurately based on segment duration
- Mooring periods use exact time deltas (5 minutes where possible)
- All phases are connected chronologically

### Distance Tracking
- All distances use Haversine formula (accurate for marine navigation)
- COG (Course Over Ground) auto-calculated between consecutive positions
- Heading set to COG (simplified for testing)

## Examples

### Simple Two-Leg Trip

```rust
let legs = vec![
    // 30 minute leg from A to B, 8 knot wind
    (pos_a, start_time, pos_b, start_time + 30_min, 8.0, 45.0),
    // 1 hour leg from B to C starting 2 hours later, 12 knot wind
    (pos_b, start_time + 2_hr, pos_c, start_time + 3_hr, 12.0, 50.0),
];

let trip_id = populate_multi_leg_trip(&db, "Trip A-B-C".to_string(), legs)?;
```

### Multi-Day Voyage

```rust
let mut legs = Vec::new();

// Day 1: Multiple short legs
legs.push((pos1, day1_0930, pos2, day1_1200, 8.0, 30.0));  // Morning leg
legs.push((pos2, day1_1400, pos3, day1_1700, 10.0, 35.0)); // Afternoon leg

// Day 2: Longer passage
legs.push((pos3, day2_0900, pos4, day2_1500, 12.0, 45.0)); // Long leg

// Day 3: Final approach
legs.push((pos4, day3_1000, destination, day3_1300, 8.0, 40.0));

let trip_id = populate_multi_leg_trip(&db, "3-Day Passage".to_string(), legs)?;
```

## Testing Scenarios

### Scenario 1: Verify Trip Totals
```rust
let trip = fetch_trip_by_timestamp(&db, trip_start_time - 5_min)?
    .expect("Trip not found");

assert_approx_equal(trip.total_distance_sailed, expected_distance, 0.1, "Distance");
assert!(trip.total_time_moored > 0, "Should have moored time");
```

### Scenario 2: Verify Mooring Periods
```rust
// Check for moored status between legs
let mooring_time = leg1_end + 5_min;
let status = fetch_vessel_status_by_timestamp(&db, mooring_time)?
    .expect("Status not found");

assert!(status.is_moored);
assert_approx_equal(status.average_speed_kn, 0.0, 0.001, "Speed while moored");
```

### Scenario 3: Wind Persistence
```rust
// Wind from leg 1 carries into mooring period
let moored_status = fetch_vessel_status_by_timestamp(&db, leg1_end + 2_min)?
    .expect("Moored status not found");

assert_approx_equal(
    moored_status.average_wind_speed_kn.unwrap_or(0.0),
    leg1_wind_speed,
    0.1,
    "Wind persists to mooring"
);
```

## Limitations & Notes

1. **Engine Status**: Always set to Off (pure sailing trips)
   - For motoring trips, use `insert_simulated_motoring_trip()` instead
   - Mixed sailing/motoring trips require custom code

2. **Wind Variation**: Uses exact values provided
   - For realistic variation, pre-calculate varied values before calling
   - Or use `generate_realistic_wind()` helper

3. **Record Density**: Fixed at 30-second intervals
   - Cannot change interval per leg
   - Must be at least 30 seconds between points

4. **Mooring Interval**: Fixed at 5-minute intervals between legs
   - Designed for realistic mooring periods
   - Last mooring is always two 5-minute records

5. **Track Resolution**: 30-second intervals may not capture all turns
   - For sharp course changes, consider increasing leg count with waypoints

## Performance

- Generates ~120 records per nautical mile sailed (at 30-sec intervals, 6 knots)
- Mooring records: 1 per 5 minutes
- Typically 200-400 vessel status records per leg
- Database inserts are optimized (batch-friendly architecture)

## Integration with Other Tests

Combine with other helpers:

```rust
// Create multi-leg trip
let trip_id = populate_multi_leg_trip(&db, "Test Trip", legs)?;

// Add environmental data for the same time period
add_test_env(&db, timestamp, 1, Some(101325.0), None, None, "Pa")?;

// Query and verify
let track = fetch_track(&db, Some(trip_id as u32), None, None)?;
assert!(track.len() > 100, "Should have many track points");
```
