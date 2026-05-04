# Technical Debt & Improvement Backlog

Items from the April 2026 code review that were deferred. Ordered by category.

---

## Performance

### Fixed 500 ms sleep in CAN reconnect path
**File**: `src/router_loop.rs` — `run()`, after `open_can_socket_with_retry`

A fixed `thread::sleep(500ms)` stalls message processing during recovery. Replace with exponential backoff capped at ~5 s so burst-recovery is faster and sustained failure doesn't busy-spin.

---

## Test Coverage

### Error path tests
No tests exercise:
- Database connection failure mid-operation
- `VesselDatabase` returning an error from `insert_*` methods
- Invalid/NaN GPS coordinates flowing into `PositionQueue`
- Clock skew detection edge cases (skew exactly at threshold, skew decreasing)

### Concurrent write tests
No tests for concurrent access to the `Arc<RwLock<VesselDatabase>>`. Add a test that spawns N threads each calling `set_system_status` simultaneously to verify no deadlock or data corruption.

### Benchmark tests (criterion)
No performance baselines exist. Candidate benchmarks:
- `PositionQueue::get_rolling_median_position` — called on every NMEA position fix
- `utilities::true_wind_*` calculations
- `VesselDatabase::fetch_track` with a large result set

### End-to-end integration tests
No test exercises the full message pipeline: synthetic `N2kFrame` → `RouterLoop::process_n2k_message` → DB write → REST API read-back. A test using `RouterLoop`'s `pub(crate) process_n2k_message` entry point and the existing test DB helpers could cover this without a live CAN bus.
