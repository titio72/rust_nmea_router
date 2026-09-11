# CLAUDE.md — NMEA2000 Router

A marine vessel data collection and monitoring system. Reads NMEA2000 (CAN bus) messages from onboard instruments, persists navigation and environmental data to MariaDB, and serves a REST/WebSocket API with a web dashboard.

For full architecture, specs, and business rules see [AGENTS.md](AGENTS.md).
For trip data analysis, modification protocols, and entity relationships see [DB_ANALYST.md](DB_ANALYST.md).

**Before any task involving the database, trips, vessel status, queries, or schema changes: read DB_ANALYST.md in full before proceeding.**

---

## Directories

```
docs/   # All the documentation goes here. Only docs relevant to agents are in the root: README.md, AGENTS.md, CLAUDE.md, DB_ANALYST.md, TODO.md.
```

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

## Mandatory Coding Rules, Key Patterns & UI Conventions

These apply to all code, AI-generated or otherwise. Full detail lives in AGENTS.md — don't restate it here, read it there:
- Naming, units, timestamp/duration rules, angle averaging, Haversine, config read-only, SQL/transaction rules → AGENTS.md §Rules and §Code Style & Conventions.
- Error handling, MySQL `DECIMAL`→`Bytes` conversion, transaction pattern → AGENTS.md §Common Patterns.
- Code hygiene / no partial implementations → AGENTS.md §Code Hygiene & Cleanup.
- Page layout, `shared-theme.js`/`shared.css`, `header-bar`/`level-1-container`, theme toggle IDs → AGENTS.md §UI structure.

---

## Production Deployment

```bash
sudo ./install.sh          # Installs to /opt/nmea_router, /etc/nmea_router, /var/log/nmea_router
sudo systemctl enable nmea_router.service
sudo systemctl start nmea_router.service
```

## Git Rules
- Do NOT run `git commit` or `git push` at any point.
- Do NOT stage or commit files automatically.
- When you finish writing code, stop. I will review and commit myself.
