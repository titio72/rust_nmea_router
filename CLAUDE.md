# CLAUDE.md — NMEA2000 Router

A marine vessel data collection and monitoring system. Reads NMEA2000 (CAN bus) messages from onboard instruments, persists navigation and environmental data to MariaDB, and serves a REST/WebSocket API with a web dashboard.

For full architecture, specs, and business rules see [AGENTS.md](AGENTS.md).

---

## Build & Run

```bash
cargo build --release
./target/release/nmea_router                    # Run with config.json in CWD
./target/release/nmea_router --validate-config  # Validate config only
./target/release/gap_filler --logs <dir> --from YYYY-MM-DD --to YYYY-MM-DD [--dry-run]
```

Configuration is loaded from `./config.json`, then `/etc/nmea_router/config.json`, then defaults. See `config.example.json` for all options.

---

## Testing

```bash
cargo test                    # All non-DB unit tests
cargo test -- --test-threads=1 --include-ignored   # Include DB integration tests (serial required)
cargo test config::tests      # Single module
```

**Database tests** are marked `#[ignore]` and require:
- A live MariaDB instance configured in `test_config.json`
- `--test-threads=1` (shared DB state — parallel runs corrupt results)
- Each test calls `reset_test_db()` at the start for a clean slate

---

## Project Structure

```
src/
  main.rs                     # Entry point, CAN socket, async runtime
  config.rs                   # Config loading/validation/defaults
  vessel_monitor.rs           # Core state machine: position, speed, heading, mooring
  vessel_status_handler.rs    # Persistence: status reports, trip state transitions
  trip.rs                     # Trip lifecycle, sailing/motoring breakdown
  mooring_detection.rs        # VMG-based mooring detection (180s window, 85% threshold)
  environmental_monitor.rs    # Wind, pressure, temperature, humidity aggregation
  environmental_status_handler.rs
  time_monitor.rs             # System time vs NMEA time skew detection
  utilities.rs                # True wind calc, angle averaging, haversine
  position_utils.rs
  db/                         # Database layer (types, connection, operations)
    operations/               # CRUD: trip.rs, vessel_status.rs, query.rs, gap_fill.rs
    test_helpers.rs           # setup_db(), add_test_trip(), assert_approx_equal()
  web/                        # Axum REST API + WebSocket (SignalK)
    api.rs                    # Endpoint handlers
    server.rs                 # Router setup, CORS, static files
    signalk.rs                # SignalK delta broadcaster
  bin/gap_filler.rs           # Standalone backfill binary
nmea2k/                       # Internal workspace crate: NMEA2000 parsing
static/                       # Frontend HTML/JS/CSS dashboards
scripts/backup.sh             # MySQL dump utility
schema.sql                    # DB schema
pgns.json                     # NMEA2000 PGN reference (1.3 MB)
```

---

## Mandatory Coding Rules

These apply to all code, AI-generated or otherwise (see AGENTS.md §Rules for the full list):

1. **Backend**: Rust only. **Frontend**: HTML + vanilla JavaScript.
2. **Naming**: `snake_case` functions/modules, `PascalCase` structs, `UPPER_CASE` constants.
3. **Never call `now()`** inside business logic — pass timestamps as parameters. Only call `now()` in event handlers (e.g., on NMEA message receipt).
4. **Units are non-negotiable**:
   - Speed → knots, Distance → nautical miles, Position → decimal degrees
   - Temperature → Celsius, Pressure → Pascals, Humidity → percentage
   - Angles → decimal degrees (0–360), Durations → milliseconds
5. **Angle averaging**: use `atan2(avg_sin, avg_cos)` — never simple arithmetic mean.
6. **Distance/bearing**: Haversine formula only.
7. **All timestamps in UTC**; all durations in milliseconds as `u64` or `Duration`.
8. **Configuration is read-only** at runtime — mutable application state lives in the database.
9. **SQL**: parameterized queries (`params!` macro) always; transactions for multi-statement ops.
10. **SignalK broadcasts use SI units** (m/s, radians, Kelvin, Pa) regardless of internal units.

---

## Key Patterns

**Error handling**: `Result<T, Box<dyn Error>>`; chain with `.map_err()`; panic only in tests or truly fatal paths.

**MySQL DECIMAL rows** come back as `mysql::Value::Bytes` — convert via `String::from_utf8(b)?.parse::<f64>()`.

**Transactions**:
```rust
let mut tx = conn.start_transaction(mysql::TxOpts::default())?;
tx.exec_drop("UPDATE ...", params!{...})?;
tx.commit()?;
```

**Code hygiene**: no unused imports, no abandoned `console.log()`, no partial implementations committed to main. If a refactor is incomplete, put it on a feature branch.

---

## UI Conventions (static/)

- Pages are 1500 px wide, centered.
- All pages load `shared-theme.js` and `shared.css`.
- Structure: `<div class="header-bar">` then one or more `<div class="level-1-container">`.
- Theme toggle: `id="themeBtn"` with `class="theme-toggle"`.
- Brand logo: `id="brandLogo"` (swapped on theme change).
- Pages that need custom theme behavior override `toggleTheme()` and call `baseToggleTheme()` first.

---

## Production Deployment

```bash
sudo ./install.sh          # Installs to /opt/nmea_router, /etc/nmea_router, /var/log/nmea_router
sudo systemctl enable nmea_router.service
sudo systemctl start nmea_router.service
```
