# NMEA2000 Router: User Stories and Business Guide

## Table of Contents
1. [What This Software Does](#what-this-software-does)
2. [Specs](#specs)
3. [Rules](#rules)
4. [Agent Guidelines](#agent-guidelines)
   - [Code Hygiene & Cleanup](#code-hygiene--cleanup)
   - [Cleanup Guidelines (Mandatory Process)](#cleanup-guidelines-mandatory-process)
5. [When in Doubt](#when-in-doubt)

---

## What This Software Does

### Overview

The **NMEA2000 Router** is a marine vessel data collection and analysis system that listens to NMEA2000 (CAN bus) network messages from your boat's navigation equipment and stores comprehensive information about your vessel's operations, environment, and performance.

### Core Function

The software continuously monitors the NMEA2000 bus (the standard marine networking protocol), processes navigation and environmental data from marine instruments (GPS, depth sounder, wind sensor, thermometer, barometer, etc.), decodes AIS messages for vessel tracking, and broadcasts data in SignalK and NMEA0183 formats. Data is persisted to a database for analysis and visualization.

### Key Data Captured

**Navigation & Trip Data:**
- Position (latitude, longitude)
- Speed over ground (SOG) and heading
- Distance traveled while sailing and motoring
- Time spent sailing, motoring, and moored
- Automatic trip segmentation (tracking journeys with 24-hour boundaries)

**Environmental Metrics:**
- Wind speed and direction (true wind calculations)
- Atmospheric pressure (barometric trends)
- Water and cabin temperature
- Humidity levels
- Boat attitude (roll angle)

**System Data:**
- Depth and water speed
- System time synchronization
- Vessel mooring status
- AIS target tracking (nearby vessels and navigation aids)

### Primary Use Cases

1. **Performance Analysis**: Review how your boat performed during specific trips (average speed, sail vs. motor percentage, fastest segments)
2. **Fleet Management**: Track multiple vessels' movements and environmental conditions
3. **Route Planning**: Analyze historical speed, wind patterns, and performance data for future trips
4. **Environmental Monitoring**: Track water and air conditions over time
5. **Maintenance & Diagnostics**: Monitor system health and performance trends
6. **Data Integration**: Export vessel data to external systems via REST API
7. **AIS Monitoring**: Track nearby vessels and navigation aids for collision avoidance and situational awareness

## Specs

### NMEA2000
The supported messages are:
1. 126992 System Time
2. 127250 Boat heading
3. 127251 Rate of Turn
4. 127257 Boat attitude
5. 127488 Engine rapid update
6. 128259 Boat speed through water
7. 128267 Water Depth
8. 129025 Position rapid update
9. 129026 COG and SOG Rapid update
10. 129029 GNSS Position Data
11. 129284 Navigation Data (distance/bearing to waypoint, ETA, destination lat/lon, closing velocity)
12. 129038 AIS Class A Position Report
13. 129039 AIS Class B Position Report
14. 129040 AIS Class B Extended Position Report
15. 129041 AIS Aid-to-Navigation Report
16. 129539 GNSS DOPs
17. 129793 AIS UTC Date Report
18. 129794 AIS Class A Static Data
19. 129809 AIS Class B Static Data Part A
20. 129810 AIS Class B Static Data Part B
21. 130306 Wind speed and direction
22. 130312 Temperature
23. 130313 Humidity
24. 130314 Pressure
The application constantly monitors the status of the CAN bus, and, in case of failure, start retrying until reconnection. In case of repeated failure, each retry must be at least 5s apart.

NMEA 2000 messages reference is available in pgns.json

### Boat Status Report
By "Status report" we mean the data representing the status of the boat for a certain period of time. A report refers always to a period of time which varies depending on the conditions, in particular, when the boat is moving, the period is shorter (1s to 30s), while it is longer (1 minute to 60 minutes) when the boat is moored or at anchor.
A report is considered valid only if the time synchronization status is "synced" (see below).
A report is also invalid if it didn't accumulate a history of at least 10 position messages during the period.
The report is persisted in the database.
It includes:
1. The position expressed in longitude and latitude with a precision under 1 meter.
2. The timestamp. It represents the timestamp at the moment the report is generated.
3. The duration of period (which typically is the time elapsed since the previous report)
4. The average and maximum SOG (Speed over ground) - typically, it is calculated as the distance between the reported positions and the previously reported position
5. The average COG (Course over ground) - typically, it is calculated as the bearing from the previously reported position and the currently reported position
6. The average heading (this is typically calculated as average of the heading). The heading is always True heading, not magnetic
7. The average wind speed in the period - this is the True Wind speed (TWS)
8. The average wind angle in the period - this is the True Wind angle (TWA), hence the reference is the bow of the boat
9. The total distance traveled in the period. This is typically the distance between the reported position and the previously reported position
10. The engine status - it can be 1 if the engine is on, and 0 if the engine is off. When unknown, it's 2
11. The mooring status - it is 0 when the boat is under way, and 1 when the boat is stationary

#### Calculation of the average COG and SOG
Calculation of average COG and SOG. If a precedent report is available (to be kept in cache in memory for reference), they should be calculated as the distance between the previous report position and the new position divided by the time elapsed from the previous report. The average COG instead is the bearing between the previous position and the new one.
If a previous report is not available, use the average of the SOG and COG received from the bus.

#### What position must be in the report
The position reported in the report is:
1. The last position received from the NMEA 2000 bus if the boat is underway (mooring status is 0)
2. The median position within the period in case the mooring status is 1

#### Recovery of the last Boat Status Report from the database.
As most calculations depend on the previous report, the application is at risk of producing inconsistent or incomplete data in case of restart, as the previous report is lost and no longer in memory.
For this reason, as soon as the application starts, it should load in memory the last boat status report from the database, but only if it is not older than 6 hours (we do not expect the application to stay down for more than 6 hours).
In this way, the total time, the total distance, and average COG and SOG will be calculated correctly even if the application bounces.

### Time synchronization
The application receives NMEA 2000 messages 126992 and compares the timestamp received with the message with the system time. If the skew is larger than 1 second, the application will try to set the system using the timestamp received with NMEA 2000 message.
Two global states are continuously updated in memory and available to all the subsystems of the application: the time skew in milliseconds and the boolean status indicating if it is synced up (i.e. skew < 1000ms).

### Data collection and Boat Status Report generation
The application uses NMEA messages:
1. 130306 Wind speed and direction
2. 127250 Boat heading
3. 127488 Engine rapid update
4. 129025 Position rapid update
5. 129026 COG and SOG Rapid update
The application collects the relevant values in queues storing the pairs (timestamp at the time the message is received, value). The queue is a time bound and rolling and maintains samples for a time window that is the maximum of the underway period (see below) and moored period.
Depending on the engine status the application determines the period of the report. The duration of the period is configurable by the user in a JSON configuration file. In particular, the underway period will be in the JSON attribute "underway_period_ms" and the moored period in "moored_period_ms". The defaults are respectively 30000ms and 300000ms.
When the period expires the report is generated as described in "Boat Status Report" and stored in the database.
The reported position is the median position of the period in case of mooring status = 1, and the last tracked position in case mooring status = 0

#### Engine status
The engine status (0/1) is determined as 1 is the engine's RPM are > 100 and 0 if the engine RPM are <= 100

#### Mooring status
The mooring status is determined by calculating the median position of the last 180 seconds, and it's 1 if X% of the last 180 seconds' positions are within 50 meters from the median position.
X is to be determined in the field, but expected to be in the range 70 to 90.
In alternative (ti be observed in the field which one is more accurate), the check is performed on the speed over the 180s period, instead of position.

#### Wind data collection
The wind data is received through PGN 130306 frm the bus. Only apparent wind messages are considered for the Boat Status Report. The application will automatically convert the apparent wind to true wind using the last received SOG.
In other words, the application will filter out the true wind, coming from the bus, but it will convert the apparent into true using the "rapid update sog" received with 129026 to calculate the true wind.
Only the calculated true wind will queued up for processing.

#### Heading data colleciton
The heading is received from the bus with 127250 PGN. The application will accept only the Magnetic heading (consider unreliable the true heading coming from the bus) and apply the WMM 2025 declination to obtain the true heading.
To calculate the declination, use the last received position from the bus.
The conversion must be applied before the heading is queued up. If the position is not available, the heading is discarded and not added to the queue for processing.

### Trips
A trip is an expedition that can last a few hours or days. A leg of a trip indicates a period of navigation between two moorings.
We consider legs belonging to the same trip if the start of a leg is less than 24 hours from the end of the preceding leg.
The application automatically calculates trips and, for every Boat Status Report, updates the persistent image of the trip in the database reporting:
1. The start timestamp of the trip (this is written once and never updated)
2. The end timestamp (this is updated for each written Boat Status Report)
3. The total time of the trip, and the breakdown into time sailing, time motoring, and time moored
4. The total distance sailed and motored

### AIS Target Tracking
The application decodes Automatic Identification System (AIS) messages from the NMEA2000 bus for monitoring nearby vessels and navigation aids. Supported PGNs include Class A and B position reports, extended position reports, aid-to-navigation reports, static data, and UTC date reports. AIS data is broadcast in real-time via SignalK but not persisted to the database.

### SignalK Broadcasting
The application broadcasts real-time vessel data in SignalK v1.7.0 delta format over WebSocket connections. Data includes navigation (position, speed, heading), environment (wind, temperature, pressure), and AIS targets with SI units (m/s, radians, Kelvin, Pascals). Broadcasting is rate-limited per path with configurable intervals.

### UDP NMEA0183 Broadcasting
The application converts NMEA2000 data to legacy NMEA0183 sentences and broadcasts them over UDP for compatibility with marine instruments and chart plotters. Supported sentences include RMC, GGA, MWV, HDT, HDM, ROT, XDR, RPM, VHW, DPT. Broadcasting is rate-limited to 1 message per second per sentence type.

### Web Interface
The application provides a REST API for programmatic access to trip data, vessel tracks, environmental metrics, and speed distributions. An HTML dashboard offers interactive visualization with Google Maps integration for trip tracks and real-time AIS target monitoring.

## Logging
The logs are written on daily files in a folder configurable (default is the application folder).
It must report all the operations (database read/write operations, open and close of ports/sockets/database).
Every 60 seconds, it traces a report with the number of NMEA messages received (broken down by type, and whether or not they succeeded to parse), the number of records written in the database of each type, and the time sync status.
The output is expected to be in the console as well.

#### Periodic stats
Every 60 seconds, the following stats are to be reported:
1. Number of messages received and parsed successfully for each supported PGN (ignore not supported messages)
2. Number of messages failed to parse for each supported PGN (ignore unsupported PGN)
3. Number of records written for each type
4. Time synchronization status (last skew in milliseconds, and sync status)

## Rules

Coding convention:
1. Backend is written in Rust
2. Frontend is html and javascript
3. Use underscore _ to separate words in function names
4. Use camel notation for struct names
5. Never use now() in function, unless the function is a handler that generates an event (for example, when a NMEA 2000 message is received, it's legit to use now() to generate the timestamp of the event. If a function in invoked because an event is generated, it will have the timestamp as parameter)
6. Configuration fields are read only - any status that under control of the application is to be read and written from the database
All the code, AI or human generated, must follow this rules.
7. All the timestamps are in UTC
8. All the durations are milliseconds
9. Longitude and latitude are in decimal degrees and with the precision of 1 meter or better
10. The speeds are in Knots and distances in nautical miles
11. Temperatures are in Celsius
12. Barometric pressure is in Pa
13. Humidity is in percentage
14. Average angles are calculated by averaging sin(angle) and cos(angle), and calculating the atan of the average sin and average cos
15. For other measures, the preferred aggregation is the median, unless it's too expensive and we can revert to average
16. Distances between two locations are calculated using the Haversine formula
17. Bearing from a position to another is calculated using the Haversine formula
18. Angles are in decimal degrees and normalized to 0..360 degrees range
19. NMEA2000 messages use different units. The structure and classes representing NMEA2000 messages will expose accessors to read the values in the original units (the unit must be explicit in the name of the accessor, like get_angle_radiants), and will expose additional accessors to read values in the application units (like: get_angle_degrees)
20. The application relies on a MySQL database for persistence
21. Conversion from magnetic angles and true angles are performed using WMM 2025

## Agent Guidelines

### Overview
These guidelines are for AI agents (including GitHub Copilot) working on the NMEA2000 Router codebase. Follow these principles to maintain code quality, consistency, and architectural integrity.

### Code Style & Conventions

**Naming & Structure:**
- Function names: `snake_case` (e.g., `calculate_position_from_bearing`)
- Struct/Type names: `PascalCase` (e.g., `VesselStatusRecord`, `TripSummary`)
- Module names: `snake_case`
- Constants: `UPPER_CASE`

**Timestamp & Duration Rules:**
- All timestamps are `SystemTime` in UTC (never local time)
- All durations are in **milliseconds** as `u64` or `Duration`
- Never call `std::time::SystemTime::now()` except in event handlers
- Pass timestamps as parameters to functions; never compute them internally

**Units (Non-Negotiable):**
- Position: decimal degrees (latitude/longitude), precision ≥ 1 meter
- Speed: knots
- Distance: nautical miles
- Temperature: Celsius
- Pressure: Pascals (Pa)
- Humidity: percentage (0-100)
- Angles: decimal degrees (0-360 range)
- Durations: milliseconds

**Calculations:**
- Use Haversine formula for: distance between positions, bearing/heading
- Average angles: `atan2(avg_sin, avg_cos)` not simple arithmetic mean
- Prefer median over arithmetic mean for data aggregation (unless performance-critical)
- NMEA2000 accessors: `get_<measurement>_<original_units>()` → returns in original units; also provided: `get_<measurement>_<app_units>()`

### Database & Persistence

**Key Rules:**
- MySQL/MariaDB only; never hardcode connection strings in code
- Configuration is read-only; application state goes in database
- All database queries use parameterized queries (`params!` macro)
- Transactions required for multi-statement operations
- Use individual `exec_drop()` calls within transaction, not multi-statement SQL

**Test Database:**
- Use `test_config.json` (loaded automatically in test mode)
- Run tests with `--test-threads=1` for database tests
- Call `reset_test_db()` at start of each test for clean state
- Mark database tests with `#[test]` and `#[ignore]` (require explicit `--ignored` flag)

### Testing Strategy

**Use the Test Infrastructure:** (`src/db/test_helpers.rs`)
- `setup_db()` → Creates test database and resets it
- `add_test_trip()` → Insert test trip with realistic data
- `add_test_vessel_status()` → Add vessel status records
- `generate_track()` → Create track with waypoints
- `assert_approx_equal()` → Safe floating-point assertions (handles precision loss)

**Test Example:**
```rust
#[test]
#[ignore]
fn test_my_feature() {
    let db = setup_db();  // Auto-loads test_config.json, resets DB
    
    // Create test data
    let trip_id = add_test_trip(&db, "Test Trip".to_string(), 
        start_time, end_time, 10.5, 2.3, 3600000, 600000, 0)?;
    
    // Run tests
    let result = db.fetch_trip(trip_id)?;
    assert_eq!(result.description, "Test Trip");
}
```

### File Organization

**Core Modules:**
- `src/db/` - Database layer (types, operations, test helpers)
- `src/db/operations/` - CRUD operations (trip.rs, vessel_status.rs, query.rs, etc.)
- `src/nmea2k/` - NMEA2000 message handling and parsing
- `src/web/` - REST API (api.rs, server.rs, websocket.rs)
- `src/*.rs` - Business logic (vessel_monitor, environmental_monitor, trip.rs)

**Test Files:**
- `src/db/test_helpers.rs` - Helper functions and utilities
- `src/db/test_examples.rs` - Example/integration tests
- `src/db/operations/test_data.rs` - Batch test data operations
- Inline tests in implementation files (with `#[cfg(test)]`)

**Documentation:**
- `doc/*.md` is the folder for documentation
- Only `README.md` and `AGENTS.md` (this file) are in the root folder
- When a new feature is added, `README.md` must updated automatically 

### Common Patterns

**Error Handling:**
- Use `Result<T, Box<dyn Error>>` for fallible operations
- Chain errors with `.map_err()` for context
- Panic only in tests or truly fatal scenarios

**Type Conversions:**
```rust
// MySQL returns DECIMAL as Bytes, need to convert
match row.take::<mysql::Value, _>("field_name") {
    Some(mysql::Value::Float(f)) => f as f64,
    Some(mysql::Value::Double(d)) => d,
    Some(mysql::Value::Bytes(b)) => String::from_utf8(b)?.parse::<f64>()?,
    Some(mysql::Value::Int(i)) => i as f64,
    Some(mysql::Value::UInt(u)) => u as f64,
    None => return Err("Missing field"),
}
```

**Transactions:**
```rust
let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
tx.exec_drop("SELECT @var := value", [])?;  // Session variables
tx.exec_drop("UPDATE table SET ...", params!{...})?;  // Multiple statements
tx.commit()?;  // Atomic
```

### Architecture Principles

1. **Layered Design:** 
   - Data layer (DB) → Business logic → REST API
   - Dependencies flow downward only

2. **State Management:**
   - Immutable configuration only in memory
   - All mutable state lives in database
   - Use in-memory caches sparingly (e.g., system_status for 0-latency access)

3. **Event-Driven:**
   - NMEA2000 messages trigger events
   - Event handlers generate timestamps
   - Business logic receives timestamps as parameters

4. **Separation of Concerns:**
   - Parsing (NMEA2000 decoding) separate from business logic
   - Database operations separate from queries
   - Web API thin layer over business logic

### Performance Considerations

- Minimize database round-trips; use transactions
- Batch operations when possible (`populate_multi_leg_trip`)
- Use indexes on frequently queried columns (timestamps, trip_id)
- Session variables in transactions to compute values server-side
- Median calculation is expensive; use only when justified

### Security

- No hardcoded credentials; use config files
- Parameterize all SQL queries
- Validate input ranges (e.g., latitude -90..90, longitude -180..180)
- No sensitive data in logs

### UI structure

**Layout & Styling:**
- Dark and bright themes managed by `shared-theme.js` (loaded in all pages)
- All pages share the same style - common classes must be in `shared.css`
- Use page-specific styles sparingly (inline `<style>` tags for page-only features)
- Pages are 1500px wide, centered with consistent margin/padding

**Page Structure:**
All pages must follow this hierarchical structure:
```html
<body>
    <div class="header-bar">
        <!-- Logo, title, navigation, theme toggle -->
    </div>
    <div class="level-1-container">
        <!-- Primary content section -->
    </div>
    <!-- Additional level-1-container divs as needed -->
</body>
```

**Header Bar (`.header-bar`):**
- Fixed max-width of 1500px, centered with `margin: 0 auto 20px auto`
- Left side: Branding image (`id="brandLogo"`) and application title
- Right side: Theme selector button (`id="themeBtn"`, class `theme-toggle`)
- Navigation links to other pages (optional, can be in title area)
- Avoid page-specific widgets in the header when possible

**Content Containers (`.level-1-container`):**
- Wraps all main content sections
- Provides consistent styling: background, border, border-radius, padding, box-shadow
- Automatically centers with max-width: 1500px
- Multiple containers can stack on a single page (e.g., heatmap + trips sections)

**Theme Management (`shared-theme.js`):**
- All pages must load `<script src="/shared-theme.js"></script>` in the `<head>`
- Global theme functions provided: `initializeTheme()`, `updateBrandLogo()`, `updateThemeButton()`, `baseToggleTheme()`
- Pages that need custom behavior when theme changes should override `toggleTheme()`:
  ```javascript
  const baseToggleTheme = toggleTheme;  // Save the base function
  function toggleTheme() {
      baseToggleTheme();  // Call the shared theme toggle
      // Add page-specific logic here
      // Example: reload charts, refresh heatmap, reload trip details, etc.
  }
  ```
- Call `initializeTheme()` on page load to restore user's theme preference
- Theme preference is persisted in `localStorage` with key `'theme'` (value: `'light'` or `'dark'`)

**Button & DOM ID Conventions:**
- Theme toggle button: `id="themeBtn"`, class `class="theme-toggle"`
- Brand logo: `id="brandLogo"` for dynamic SVG swapping on theme change
- Theme icon: `id="theme-icon"` displaying `◐` (light) or `◑` (dark)
- Theme text: `id="theme-text"` displaying `Dark` or `Light`

### Documentation

- Document non-obvious calculations (bearing, true wind conversions)
- Include units in variable names or comments (`speed_kn`, `distance_nm`)
- Link to specifications (WMM 2025, Haversine, NMEA2000 specs)
- Explain test data generation strategies

### Code Hygiene & Cleanup

**CRITICAL - Do NOT leave unused code behind:**
- If implementing a feature/refactor, complete it fully or revert it completely
- Never commit unused imports, abandoned modules, or partial implementations
- Never leave infrastructure (like importmaps, module scripts, component libraries) without using them
- If transitioning between approaches (e.g., plain HTML → Lit):
  - Either fully migrate and test the new approach
  - OR completely revert to the original and remove all intermediate code
- Remove all debugging console.log() statements before committing
- Clean up attempt branches: use `git stash` or create feature branches, don't merge partial work

**When Work is Incomplete:**
1. Create a feature branch (`git checkout -b feature/lit-refactor`)
2. Work fully to completion on that branch
3. Only merge to main when fully tested and working
4. If abandoning, use `git revert` to clean up, don't leave debris

### Cleanup Guidelines (Mandatory Process)

When removing an abandoned feature, tool, or approach, follow this exhaustive process to ensure nothing is left behind:

**Step 1: Search Comprehensively**
Before deleting anything, search for ALL references to the feature across the entire workspace:
```bash
# Replace "angular" with the feature name being removed
grep -r "angular\|Angular\|ng-trip\|frontend" \
  --include="*.md" --include="*.rs" --include="*.js" \
  --include="*.html" --include="*.json" --include="*.toml" \
  --include="*.lock" --include="*.xml" \
  . 2>/dev/null | grep -v ".git"
```

**Step 2: Categorize All Artifacts**
Systematically document every artifact found in the search:
- [ ] **Source code**: Directories, modules, files (e.g., `/frontend/src/`, `src/app/`)
- [ ] **Configuration**: Build configs, `Cargo.toml`, `package.json`, `tsconfig.json`, etc.
- [ ] **Tests**: Test files, test helpers, test data
- [ ] **Documentation**: README sections, design docs, migration guides (e.g., `docs/ANGULAR_MIGRATION_*.md`)
- [ ] **Static files**: HTML debug pages, CSS for feature, SVGs, assets
- [ ] **Build outputs**: `target/`, `dist/`, `node_modules/`, compiled artifacts
- [ ] **Debug statements**: `console.log()`, print statements, logging entries
- [ ] **Comments/docstrings**: Code comments referencing the feature
- [ ] **Dependencies**: Package lock files, version constraints in Cargo.toml
- [ ] **Database migrations**: If applicable, old migration files

**Step 3: Remove Everything**
Delete all artifacts identified in Step 1 and categorized in Step 2:
```bash
# Example cleanup for Angular migration
rm -rf /path/to/frontend/
rm /path/to/docs/ANGULAR_MIGRATION_*.md
rm /path/to/src/app/
rm -f /path/to/static/debug-angular.html
# ... remove all others
```

**Step 4: Verify Cleanup is Complete**
```bash
# Run the verification to ensure NO references remain
FEATURE="angular"
MATCHES=$(grep -r "$FEATURE" --include="*.md" --include="*.rs" --include="*.js" \
  --include="*.html" --include="*.json" --include="*.toml" . 2>/dev/null | grep -v ".git" | wc -l)

if [ $MATCHES -eq 0 ]; then
  echo "✓ Complete: No references to '$FEATURE' remain"
else
  echo "✗ Incomplete: Found $MATCHES references. Review and remove:"
  grep -r "$FEATURE" --include="*.md" --include="*.rs" --include="*.js" \
    --include="*.html" --include="*.json" . 2>/dev/null | grep -v ".git"
fi
```

**Step 5: Update Core Documentation**
- [ ] Update `README.md` if feature is mentioned
- [ ] Update Table of Contents in `AGENTS.md` if sections were added
- [ ] Update project status in relevant documentation

**Examples of Incomplete Cleanups (What NOT to do):**
- **Bad**: Deleted source code but left documentation → Search for "angular" still finds references
- **Bad**: Removed code but left `console.log()` debug statements → Code still has artifacts
- **Bad**: Removed `/frontend/` but left `/docs/ANGULAR_MIGRATION_*.md` → Confusing documentation remains
- **Bad**: Deleted files but kept build outputs → `/target/`, `/dist/` still contain old artifacts

**Rule of Thumb:**
If `grep -r "FEATURE_NAME"` returns zero results, cleanup is complete. If it returns any results, you're not done.

### When in Doubt

1. Check `AGENTS.md` (this file) for conventions
2. Look at similar existing code (patterns are established)
3. Run all tests with `cargo test -- --test-threads=1`
4. Consult project documentation files (README.md, DATABASE_TESTING.md, etc.)
