# Health Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise, expose and publish alarms when the CAN feed goes quiet, DB writes keep failing, or the router loop lags or stalls.

**Architecture:** The router loop only calls cheap `record_*` methods on a shared `HealthHandle`. A pure `HealthState::evaluate(now)` turns the recorded timestamps and counters into alarms. A 1 Hz task on the web runtime feeds an `AlarmPublisher` (edge-triggered: log + SignalK notifications), and `GET /api/health` evaluates on read. A status dot in the shared dashboard header polls the endpoint.

**Tech Stack:** Rust (std `Mutex`, `serde`, axum, tokio), plain JS in `static/js/shared-theme.js`.

**Spec:** `docs/superpowers/specs/2026-10-03-health-monitor-design.md` (read it first). Deviations from the spec made while planning, all deliberate simplifications:
- `HealthState` holds a clone of `HealthConfig`, so the entry point is `evaluate(now)` instead of `evaluate(&state, &config, now)`.
- `db_failing` fires only on consecutive final write failures. The "no vessel write for N seconds" trigger is dropped: writes legitimately stop when moored (default interval 1800 s), when tracking is switched off, or while time is unsynced, so it would raise false alarms.
- `loop_stalled` also suppresses `can_silent` and stream alarms (a stalled loop cannot tell you anything about the bus).
- DB-op duration is not tracked separately; "lag" is the duration of one unit of work (message processing incl. its DB write, or the periodic DB health check) exceeding `loop_lag_secs`.
- Invalid config values are warned about and reverted to defaults (the existing codebase pattern) instead of rejected.
- Report durations are in milliseconds (AGENTS.md rule 8); config fields stay in seconds like the other interval fields.
- Dashboard dot has a tooltip only, no click-through.
- `health.enabled = false` makes `evaluate` return no alarms; `record_*` still run (they are trivial).

## Global Constraints

- **Do NOT run `git add`, `git commit` or `git push`** (CLAUDE.md Git Rules). Leave all changes in the working tree for the owner to review. There are intentionally no commit steps in this plan.
- Never call `Instant::now()` inside business logic: `now` is a parameter everywhere in `src/health.rs`, except inside `run_alarm_publisher` (an event-generating task) and the router loop (I/O layer).
- Function names use underscores; structs are PascalCase.
- All durations in API output are milliseconds; all timestamps UTC RFC3339 (use `crate::utilities::instant_to_rfc3339`).
- Configuration is read-only after load.
- Any change to config structs requires updating `config.example.json` and docs (README.md "Configuration Options", AGENTS.md).
- Frontend pages use `shared-theme.js` / `shared.css`; do not add per-page header markup.
- DB tests are `#[ignore]` and need a live MariaDB; none of the new tests may need a DB or CAN.
- Run tests with `cargo test <filter>`; the non-DB suite must stay green.

## Review Focus

Failure modes the spec implies but a straightforward implementation would miss:

1. **Cold boot:** no alarms during `startup_grace_secs`, then a never-seen *required* stream alarms (GPS dead at boot). Pinned in Task 2 (`grace_suppresses_everything`, `required_stream_never_seen_alarms_after_grace`).
2. **Clock/`Instant` ordering:** `now` earlier than a recorded instant must not panic (`saturating_duration_since`). Pinned in Task 2 (`evaluate_never_panics_on_earlier_now`).
3. **One root cause, one alarm:** bus silent or loop stalled must not also raise per-stream alarms. Pinned in Task 2.
4. **Optional sensor not fitted:** heading/wind/engine never seen → no alarm. Pinned in Task 2.
5. **Health disabled / CAN disabled:** web-only mode must not report alarms for a pipeline that does not exist. Pinned in Tasks 2 and 3.
6. **Poisoned mutex:** the loop panicking elsewhere must not make recording or `/api/health` panic. Pinned in Task 2 (`handle_survives_poisoned_mutex`).
7. **SignalK path safety:** alarm ids contain `:`, which is not valid in a SignalK path. Pinned in Task 3.

---

## File Structure

- **Create `src/health.rs`** — everything health-related: `HealthState`, `HealthHandle`, `Alarm`, `HealthReport`, `AlarmPublisher`, SignalK delta builder, `run_alarm_publisher`. One file, ~500 lines with tests; the units are small and share types.
- **Modify `src/config.rs`** — `HealthConfig`, field on `Config`, validation, tests.
- **Modify `config.example.json`, `README.md`, `AGENTS.md`** — docs for the new section.
- **Modify `src/main.rs`** — `mod health;`, create the handle, pass it on.
- **Modify `src/router_loop.rs`** — recording calls.
- **Modify `src/web/api.rs`, `src/web/server.rs`, `src/web/auth.rs`** — `AppState.health`, `/health` route, public path, publisher task.
- **Modify `static/js/shared-theme.js`, `static/shared.css`** — dashboard dot.

---

### Task 1: `HealthConfig`

**Files:**
- Modify: `src/config.rs` (struct near `TimeConfig` ~line 360, `Config` ~line 41, `new_default_instance` ~line 740, `validate_and_fix` ~line 552, tests module ~line 812)
- Modify: `config.example.json`, `README.md` (after "Time Synchronization" section, ~line 140)

**Interfaces:**
- Produces: `crate::config::HealthConfig` with pub fields `enabled: bool`, `startup_grace_secs: u64`, `can_silence_secs: u64`, `required_stream_timeout_secs: u64`, `optional_stream_timeout_secs: u64`, `db_failure_threshold: u32`, `loop_lag_secs: u64`, `loop_lag_window_secs: u64`, `loop_stall_secs: u64`, `time_unsynced_secs: u64`; `impl Default`; `Config.health: HealthConfig`.

- [ ] **Step 1: Write the failing tests** — append inside `mod tests` in `src/config.rs`:

```rust
    #[test]
    fn test_health_config_defaults() {
        let h = HealthConfig::default();
        assert!(h.enabled);
        assert_eq!(h.startup_grace_secs, 30);
        assert_eq!(h.can_silence_secs, 10);
        assert_eq!(h.required_stream_timeout_secs, 30);
        assert_eq!(h.optional_stream_timeout_secs, 60);
        assert_eq!(h.db_failure_threshold, 3);
        assert_eq!(h.loop_lag_secs, 2);
        assert_eq!(h.loop_lag_window_secs, 60);
        assert_eq!(h.loop_stall_secs, 10);
        assert_eq!(h.time_unsynced_secs, 60);
    }

    #[test]
    fn test_health_config_absent_section_uses_defaults() {
        let json = r#"{
            "can": {"interface": "vcan0", "enabled": false},
            "time": {"skew_threshold_ms": 500},
            "database": {
                "connection": {"host": "localhost", "port": 3306, "username": "nmea", "password": "nmea", "database_name": "nmea_router"},
                "vessel_status": {"interval_moored_seconds": 1800, "interval_underway_seconds": 30},
                "environmental": {"wind_speed_seconds": 30, "wind_direction_seconds": 30, "roll_seconds": 30, "pressure_seconds": 120, "cabin_temp_seconds": 300, "water_temp_seconds": 300, "humidity_seconds": 300}
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.health.can_silence_secs, 10);
    }

    #[test]
    fn test_health_config_partial_section_keeps_other_defaults() {
        let h: HealthConfig = serde_json::from_str(r#"{"can_silence_secs": 20}"#).unwrap();
        assert_eq!(h.can_silence_secs, 20);
        assert_eq!(h.loop_stall_secs, 10);
    }

    #[test]
    fn test_health_config_zero_values_revert_to_defaults() {
        let mut config = Config::new_default_instance();
        config.health.can_silence_secs = 0;
        config.health.db_failure_threshold = 0;
        config.health.loop_lag_secs = 0;
        config.health.startup_grace_secs = 0; // zero grace is legitimate
        config.validate_and_fix().unwrap();
        assert_eq!(config.health.can_silence_secs, 10);
        assert_eq!(config.health.db_failure_threshold, 3);
        assert_eq!(config.health.loop_lag_secs, 2);
        assert_eq!(config.health.startup_grace_secs, 0);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test config::tests::test_health`
Expected: FAIL to compile — `HealthConfig` not found.

- [ ] **Step 3: Implement.** Add after `TimeConfig`'s `impl Default` block:

```rust
/// Health monitor thresholds. All durations are in seconds (like the other interval
/// settings); `/api/health` reports ages in milliseconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthConfig {
    /// Master switch. When false, no alarms are ever reported.
    pub enabled: bool,
    /// No alarms for this long after process start (cold boot: bus and sensors need time).
    pub startup_grace_secs: u64,
    /// `can_silent` fires when no CAN frame has arrived for this long.
    pub can_silence_secs: u64,
    /// Required streams (position, COG/SOG, system time) alarm after this much silence.
    pub required_stream_timeout_secs: u64,
    /// Optional streams (heading, wind, engine) alarm after this much silence, but only if
    /// they have been seen at least once since start.
    pub optional_stream_timeout_secs: u64,
    /// `db_failing` fires after this many consecutive final write failures of one kind.
    pub db_failure_threshold: u32,
    /// `loop_lagging` fires when one unit of work takes longer than this.
    pub loop_lag_secs: u64,
    /// `loop_lagging` stays active this long after the last slow unit of work.
    pub loop_lag_window_secs: u64,
    /// `loop_stalled` fires when the router loop has not completed an iteration for this long.
    pub loop_stall_secs: u64,
    /// `time_not_synced` fires when time is uninitialized or skewed for this long.
    pub time_unsynced_secs: u64,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            startup_grace_secs: 30,
            can_silence_secs: 10,
            required_stream_timeout_secs: 30,
            optional_stream_timeout_secs: 60,
            db_failure_threshold: 3,
            loop_lag_secs: 2,
            loop_lag_window_secs: 60,
            loop_stall_secs: 10,
            time_unsynced_secs: 60,
        }
    }
}
```

In `pub struct Config`, add after the `sync` field:

```rust
    #[serde(default)]
    pub health: HealthConfig,
```

In `new_default_instance`, add after `sync: SyncConfig::default(),`:

```rust
            health: HealthConfig::default(),
```

In `validate_and_fix`, after the line `self.validate_environmental_intervals();` (~line 555) add:

```rust
        self.validate_health();
```

Add the method next to `validate_environmental_intervals`:

```rust
    fn validate_health(&mut self) {
        let defaults = HealthConfig::default();
        let h = &mut self.health;
        let mut fix = |name: &str, value: &mut u64, default: u64| {
            if *value == 0 {
                warn!("Configuration warning: health.{} must be > 0. Reverting to default {}.", name, default);
                *value = default;
            }
        };
        fix("can_silence_secs", &mut h.can_silence_secs, defaults.can_silence_secs);
        fix("required_stream_timeout_secs", &mut h.required_stream_timeout_secs, defaults.required_stream_timeout_secs);
        fix("optional_stream_timeout_secs", &mut h.optional_stream_timeout_secs, defaults.optional_stream_timeout_secs);
        fix("loop_lag_secs", &mut h.loop_lag_secs, defaults.loop_lag_secs);
        fix("loop_lag_window_secs", &mut h.loop_lag_window_secs, defaults.loop_lag_window_secs);
        fix("loop_stall_secs", &mut h.loop_stall_secs, defaults.loop_stall_secs);
        fix("time_unsynced_secs", &mut h.time_unsynced_secs, defaults.time_unsynced_secs);
        if h.db_failure_threshold == 0 {
            warn!("Configuration warning: health.db_failure_threshold must be > 0. Reverting to default {}.", defaults.db_failure_threshold);
            h.db_failure_threshold = defaults.db_failure_threshold;
        }
    }
```

In `config.example.json`, add a `"health"` section after the `"time"` section:

```json
  "health": {
    "enabled": true,
    "startup_grace_secs": 30,
    "can_silence_secs": 10,
    "required_stream_timeout_secs": 30,
    "optional_stream_timeout_secs": 60,
    "db_failure_threshold": 3,
    "loop_lag_secs": 2,
    "loop_lag_window_secs": 60,
    "loop_stall_secs": 10,
    "time_unsynced_secs": 60
  },
```

In `README.md`, add before `#### Database Connection`:

```markdown
#### Health Monitor
Alarms for lost data feeds, failing database writes and a lagging or stalled router loop (see `GET /api/health`). All values in seconds; every field is optional and a value of 0 reverts to the default (except `startup_grace_secs`, where 0 is allowed).
- `enabled`: Master switch (default: true)
- `startup_grace_secs`: No alarms for this long after start (default: 30)
- `can_silence_secs`: Alarm when no CAN frame arrives for this long (default: 10)
- `required_stream_timeout_secs`: Position, COG/SOG and system time must be seen within this window (default: 30)
- `optional_stream_timeout_secs`: Heading, wind and engine; only alarmed once seen at least once (default: 60)
- `db_failure_threshold`: Consecutive failed writes before `db_failing` (default: 3)
- `loop_lag_secs`: One unit of work taking longer than this raises `loop_lagging` (default: 2)
- `loop_lag_window_secs`: `loop_lagging` stays active this long after the last slow unit of work (default: 60)
- `loop_stall_secs`: No loop iteration for this long raises `loop_stalled` (default: 10)
- `time_unsynced_secs`: Time uninitialized or skewed for this long raises `time_not_synced` (default: 60)

```

- [ ] **Step 4: Run tests**

Run: `cargo test config::tests`
Expected: PASS. If the compiler reports a missing `health` field in some other `Config { .. }` literal, add `health: HealthConfig::default()` there.

---

### Task 2: `HealthState`, evaluation and `HealthHandle`

**Files:**
- Create: `src/health.rs`
- Modify: `src/main.rs` (add `mod health;` in the module list, alphabetically after `mod frame_filter;`)

**Interfaces:**
- Consumes: `crate::config::HealthConfig` (Task 1), `crate::time_monitor::TimeSyncStatus`.
- Produces (used by Tasks 3-5):
  - `enum Stream { Position, CogSog, SystemTime, Heading, Wind, Engine }` with `ALL`, `name(self) -> &'static str`, `is_required(self) -> bool`
  - `enum DbKind { Vessel, Env }` with `ALL`, `name(self) -> &'static str`
  - `enum Severity { Warn, Alarm }`; `struct Alarm { id: String, severity: Severity, since: Instant, message: String }`
  - `struct HealthState` with `new(config: HealthConfig, can_enabled: bool, now: Instant)`, `record_heartbeat(&mut self, now)`, `record_frame(&mut self, now)`, `record_stream(&mut self, Stream, now)`, `record_work(&mut self, now, Duration)`, `record_db_result(&mut self, DbKind, ok: bool, now)`, `record_time_sync(&mut self, TimeSyncStatus, now)`, `evaluate(&self, now) -> Vec<Alarm>`
  - `#[derive(Clone)] struct HealthHandle` wrapping `Arc<Mutex<HealthState>>` with the same `record_*` methods (taking `&self`), plus `evaluate(&self, now) -> Vec<Alarm>`

- [ ] **Step 1: Write the failing tests.** Create `src/health.rs` containing only the test module below (the implementation follows in Step 3, so the file will not compile yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// State past the startup grace with every required stream and the bus healthy.
    fn healthy(cfg: HealthConfig) -> (HealthState, Instant) {
        let t0 = Instant::now();
        let mut st = HealthState::new(cfg, true, t0);
        let now = t0 + secs(100);
        st.record_frame(now);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, now);
        }
        st.record_heartbeat(now);
        st.record_time_sync(TimeSyncStatus::Synchronized, t0);
        (st, now)
    }

    fn ids(alarms: &[Alarm]) -> Vec<&str> {
        alarms.iter().map(|a| a.id.as_str()).collect()
    }

    #[test]
    fn healthy_system_has_no_alarms() {
        let (st, now) = healthy(HealthConfig::default());
        assert!(st.evaluate(now).is_empty());
    }

    #[test]
    fn grace_suppresses_everything() {
        let t0 = Instant::now();
        let st = HealthState::new(HealthConfig::default(), true, t0);
        // Nothing has ever been recorded, but we are inside the 30 s grace.
        assert!(st.evaluate(t0 + secs(29)).is_empty());
    }

    #[test]
    fn required_stream_never_seen_alarms_after_grace() {
        let t0 = Instant::now();
        let mut st = HealthState::new(HealthConfig::default(), true, t0);
        // Bus alive and loop alive, but GPS never spoke.
        let now = t0 + secs(100);
        st.record_frame(now);
        st.record_heartbeat(now);
        st.record_time_sync(TimeSyncStatus::Synchronized, t0);
        st.record_stream(Stream::CogSog, now);
        st.record_stream(Stream::SystemTime, now);
        let alarms = st.evaluate(now);
        assert_eq!(ids(&alarms), vec!["stream_stale:position"]);
        assert_eq!(alarms[0].severity, Severity::Alarm);
    }

    #[test]
    fn stream_goes_stale_and_recovers() {
        let (mut st, now) = healthy(HealthConfig::default());
        // Keep the bus and loop alive, let position age out (timeout 30 s).
        let later = now + secs(31);
        st.record_frame(later);
        st.record_heartbeat(later);
        st.record_stream(Stream::CogSog, later);
        st.record_stream(Stream::SystemTime, later);
        assert_eq!(ids(&st.evaluate(later)), vec!["stream_stale:position"]);
        st.record_stream(Stream::Position, later);
        assert!(st.evaluate(later).is_empty());
    }

    #[test]
    fn optional_stream_not_fitted_never_alarms() {
        let (st, now) = healthy(HealthConfig::default());
        // Heading/wind/engine were never seen: no alarm, however long we wait
        // (keeping the others fresh is not needed: only optional ones are checked here).
        let alarms = st.evaluate(now);
        assert!(!ids(&alarms).iter().any(|i| i.contains("heading") || i.contains("wind") || i.contains("engine")));
    }

    #[test]
    fn optional_stream_seen_then_silent_warns() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_stream(Stream::Wind, now);
        let later = now + secs(61);
        st.record_frame(later);
        st.record_heartbeat(later);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, later);
        }
        let alarms = st.evaluate(later);
        assert_eq!(ids(&alarms), vec!["stream_stale:wind"]);
        assert_eq!(alarms[0].severity, Severity::Warn);
    }

    #[test]
    fn can_silent_suppresses_stream_alarms() {
        let (mut st, now) = healthy(HealthConfig::default());
        let later = now + secs(40); // bus silent 40 s, every stream also stale
        st.record_heartbeat(later); // loop is alive, bus is not
        let alarms = st.evaluate(later);
        assert_eq!(ids(&alarms), vec!["can_silent"]);
        st.record_frame(later);
        assert!(ids(&st.evaluate(later)).contains(&"stream_stale:position"));
    }

    #[test]
    fn loop_stalled_suppresses_can_and_stream_alarms() {
        let (st, now) = healthy(HealthConfig::default());
        let later = now + secs(40); // nothing recorded at all for 40 s
        assert_eq!(ids(&st.evaluate(later)), vec!["loop_stalled"]);
    }

    #[test]
    fn db_failing_after_threshold_and_clears_on_success() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_db_result(DbKind::Vessel, false, now);
        st.record_db_result(DbKind::Vessel, false, now);
        assert!(st.evaluate(now).is_empty());
        st.record_db_result(DbKind::Vessel, false, now);
        assert_eq!(ids(&st.evaluate(now)), vec!["db_failing:vessel"]);
        st.record_db_result(DbKind::Vessel, true, now);
        assert!(st.evaluate(now).is_empty());
    }

    #[test]
    fn db_failures_are_tracked_per_kind() {
        let (mut st, now) = healthy(HealthConfig::default());
        for _ in 0..3 {
            st.record_db_result(DbKind::Env, false, now);
        }
        st.record_db_result(DbKind::Vessel, true, now);
        assert_eq!(ids(&st.evaluate(now)), vec!["db_failing:env"]);
    }

    #[test]
    fn slow_work_raises_loop_lagging_for_the_window_then_clears() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_work(now, Duration::from_millis(500)); // under 2 s: ignored
        assert!(st.evaluate(now).is_empty());
        st.record_work(now, Duration::from_millis(2500));
        assert_eq!(ids(&st.evaluate(now)), vec!["loop_lagging"]);

        let mut later = now + secs(59);
        st.record_frame(later);
        st.record_heartbeat(later);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, later);
        }
        assert_eq!(ids(&st.evaluate(later)), vec!["loop_lagging"]);
        later += secs(2); // 61 s after the slow event, window is 60 s
        st.record_frame(later);
        st.record_heartbeat(later);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, later);
        }
        assert!(st.evaluate(later).is_empty());
    }

    #[test]
    fn time_not_synced_after_threshold() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, now);
        assert!(st.evaluate(now + secs(59)).iter().all(|a| a.id != "time_not_synced"));
        let later = now + secs(61);
        st.record_frame(later);
        st.record_heartbeat(later);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, later);
        }
        assert_eq!(ids(&st.evaluate(later)), vec!["time_not_synced"]);
        st.record_time_sync(TimeSyncStatus::Synchronized, later);
        assert!(st.evaluate(later).is_empty());
    }

    #[test]
    fn disabled_config_or_disabled_can_yields_no_alarms() {
        let t0 = Instant::now();
        let cfg = HealthConfig { enabled: false, ..HealthConfig::default() };
        let st = HealthState::new(cfg, true, t0);
        assert!(st.evaluate(t0 + secs(1000)).is_empty());

        let st = HealthState::new(HealthConfig::default(), false, t0);
        assert!(st.evaluate(t0 + secs(1000)).is_empty());
    }

    #[test]
    fn evaluate_never_panics_on_earlier_now() {
        let t0 = Instant::now();
        let mut st = HealthState::new(HealthConfig::default(), true, t0 + secs(50));
        st.record_frame(t0 + secs(60));
        // `now` precedes recorded instants: must not panic.
        let _ = st.evaluate(t0);
    }

    #[test]
    fn handle_survives_poisoned_mutex() {
        let t0 = Instant::now();
        let handle = HealthHandle::new(HealthConfig::default(), true, t0);
        let poisoner = handle.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("poison the mutex");
        })
        .join();
        handle.record_frame(t0);
        let _ = handle.evaluate(t0 + secs(100));
    }
}
```

Also add `mod health;` to `src/main.rs` (after `mod frame_filter;`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test health::tests`
Expected: FAIL to compile — `HealthState`, `Stream`, etc. not found.

- [ ] **Step 3: Implement.** Insert at the top of `src/health.rs`, above the `#[cfg(test)]` module:

```rust
// Health monitor: records when each data stream, DB write and loop iteration last happened,
// and evaluates that state into alarms. The router loop only calls the cheap `record_*`
// methods; evaluation happens elsewhere (publisher task, /api/health) so that a hung loop
// is itself detectable. See docs/superpowers/specs/2026-10-03-health-monitor-design.md.
//
// Every method takes `now` explicitly: nothing in here reads the clock.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::config::HealthConfig;
use crate::time_monitor::TimeSyncStatus;

/// NMEA2000 data streams the health monitor tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stream {
    Position,
    CogSog,
    SystemTime,
    Heading,
    Wind,
    Engine,
}

impl Stream {
    pub const ALL: [Stream; 6] = [
        Stream::Position,
        Stream::CogSog,
        Stream::SystemTime,
        Stream::Heading,
        Stream::Wind,
        Stream::Engine,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stream::Position => "position",
            Stream::CogSog => "cog_sog",
            Stream::SystemTime => "system_time",
            Stream::Heading => "heading",
            Stream::Wind => "wind",
            Stream::Engine => "engine",
        }
    }

    /// Required streams alarm even if never seen; optional ones only once seen.
    pub fn is_required(self) -> bool {
        matches!(self, Stream::Position | Stream::CogSog | Stream::SystemTime)
    }
}

/// Kinds of database writes whose failures are tracked separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DbKind {
    Vessel,
    Env,
}

impl DbKind {
    pub const ALL: [DbKind; 2] = [DbKind::Vessel, DbKind::Env];

    pub fn name(self) -> &'static str {
        match self {
            DbKind::Vessel => "vessel",
            DbKind::Env => "env",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warn,
    Alarm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alarm {
    pub id: String,
    pub severity: Severity,
    /// When the condition began (the last good event, or process start).
    pub since: Instant,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct DbRecord {
    last_success: Option<Instant>,
    consecutive_failures: u32,
    first_failure: Option<Instant>,
}

#[derive(Debug)]
pub struct HealthState {
    config: HealthConfig,
    can_enabled: bool,
    started: Instant,
    last_frame: Option<Instant>,
    streams: BTreeMap<Stream, Instant>,
    db: BTreeMap<DbKind, DbRecord>,
    last_heartbeat: Option<Instant>,
    /// Most recent unit of work that exceeded `loop_lag_secs`: (when, how long).
    last_slow: Option<(Instant, Duration)>,
    time_status: TimeSyncStatus,
    time_status_since: Instant,
}

impl HealthState {
    pub fn new(config: HealthConfig, can_enabled: bool, now: Instant) -> Self {
        Self {
            config,
            can_enabled,
            started: now,
            last_frame: None,
            streams: BTreeMap::new(),
            db: BTreeMap::new(),
            last_heartbeat: None,
            last_slow: None,
            time_status: TimeSyncStatus::NotInitialized,
            time_status_since: now,
        }
    }

    pub fn record_heartbeat(&mut self, now: Instant) {
        self.last_heartbeat = Some(now);
    }

    pub fn record_frame(&mut self, now: Instant) {
        self.last_frame = Some(now);
    }

    pub fn record_stream(&mut self, stream: Stream, now: Instant) {
        self.streams.insert(stream, now);
    }

    /// Record how long one unit of work took (message processing including its DB write,
    /// or the periodic DB health check). Only durations above `loop_lag_secs` are kept.
    pub fn record_work(&mut self, now: Instant, duration: Duration) {
        if duration > Duration::from_secs(self.config.loop_lag_secs) {
            self.last_slow = Some((now, duration));
        }
    }

    /// Record the final outcome of a DB write (after any reconnect-and-retry).
    pub fn record_db_result(&mut self, kind: DbKind, ok: bool, now: Instant) {
        let rec = self.db.entry(kind).or_default();
        if ok {
            rec.last_success = Some(now);
            rec.consecutive_failures = 0;
            rec.first_failure = None;
        } else {
            rec.consecutive_failures = rec.consecutive_failures.saturating_add(1);
            rec.first_failure.get_or_insert(now);
        }
    }

    pub fn record_time_sync(&mut self, status: TimeSyncStatus, now: Instant) {
        if status != self.time_status {
            self.time_status = status;
            self.time_status_since = now;
        }
    }

    /// Turn the recorded state into the list of currently active alarms.
    pub fn evaluate(&self, now: Instant) -> Vec<Alarm> {
        let mut alarms = Vec::new();
        if !self.config.enabled || !self.can_enabled {
            return alarms;
        }
        let cfg = &self.config;
        if now.saturating_duration_since(self.started) < Duration::from_secs(cfg.startup_grace_secs) {
            return alarms;
        }
        let age = |t: Instant| now.saturating_duration_since(t);
        let limit = Duration::from_secs;

        // A stalled loop cannot tell us anything about the bus or the streams.
        let heartbeat_ref = self.last_heartbeat.unwrap_or(self.started);
        let stalled = age(heartbeat_ref) > limit(cfg.loop_stall_secs);
        if stalled {
            alarms.push(Alarm {
                id: "loop_stalled".to_string(),
                severity: Severity::Alarm,
                since: heartbeat_ref,
                message: format!(
                    "Router loop has not completed an iteration for {} s",
                    age(heartbeat_ref).as_secs()
                ),
            });
        }

        let mut can_silent = false;
        if !stalled {
            let can_ref = self.last_frame.unwrap_or(self.started);
            can_silent = age(can_ref) > limit(cfg.can_silence_secs);
            if can_silent {
                alarms.push(Alarm {
                    id: "can_silent".to_string(),
                    severity: Severity::Alarm,
                    since: can_ref,
                    message: format!("No CAN frame received for {} s", age(can_ref).as_secs()),
                });
            }
        }

        if !stalled && !can_silent {
            for stream in Stream::ALL {
                let (reference, timeout, severity) = match (self.streams.get(&stream), stream.is_required()) {
                    (Some(&seen), true) => (seen, cfg.required_stream_timeout_secs, Severity::Alarm),
                    (Some(&seen), false) => (seen, cfg.optional_stream_timeout_secs, Severity::Warn),
                    (None, true) => (self.started, cfg.required_stream_timeout_secs, Severity::Alarm),
                    (None, false) => continue,
                };
                if age(reference) > limit(timeout) {
                    alarms.push(Alarm {
                        id: format!("stream_stale:{}", stream.name()),
                        severity,
                        since: reference,
                        message: format!(
                            "No {} data for {} s",
                            stream.name(),
                            age(reference).as_secs()
                        ),
                    });
                }
            }
        }

        for kind in DbKind::ALL {
            let rec = self.db.get(&kind).copied().unwrap_or_default();
            if rec.consecutive_failures >= cfg.db_failure_threshold {
                alarms.push(Alarm {
                    id: format!("db_failing:{}", kind.name()),
                    severity: Severity::Alarm,
                    since: rec.first_failure.unwrap_or(now),
                    message: format!(
                        "{} database writes failed {} times in a row",
                        kind.name(),
                        rec.consecutive_failures
                    ),
                });
            }
        }

        if let Some((at, duration)) = self.last_slow {
            if age(at) < limit(cfg.loop_lag_window_secs) {
                alarms.push(Alarm {
                    id: "loop_lagging".to_string(),
                    severity: Severity::Warn,
                    since: at,
                    message: format!(
                        "Router loop work took {} ms (limit {} ms)",
                        duration.as_millis(),
                        limit(cfg.loop_lag_secs).as_millis()
                    ),
                });
            }
        }

        if self.time_status != TimeSyncStatus::Synchronized
            && age(self.time_status_since) > limit(cfg.time_unsynced_secs)
        {
            alarms.push(Alarm {
                id: "time_not_synced".to_string(),
                severity: Severity::Alarm,
                since: self.time_status_since,
                message: format!(
                    "System time not synchronized with NMEA time ({}); database writes are blocked",
                    self.time_status
                ),
            });
        }

        alarms
    }
}

/// Cloneable, lock-managing handle shared by the router loop, the web layer and the
/// publisher task. A poisoned mutex is recovered, never propagated.
#[derive(Clone)]
pub struct HealthHandle {
    inner: Arc<Mutex<HealthState>>,
}

impl HealthHandle {
    pub fn new(config: HealthConfig, can_enabled: bool, now: Instant) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HealthState::new(config, can_enabled, now))),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HealthState> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn record_heartbeat(&self, now: Instant) {
        self.lock().record_heartbeat(now);
    }

    pub fn record_frame(&self, now: Instant) {
        self.lock().record_frame(now);
    }

    pub fn record_stream(&self, stream: Stream, now: Instant) {
        self.lock().record_stream(stream, now);
    }

    pub fn record_work(&self, now: Instant, duration: Duration) {
        self.lock().record_work(now, duration);
    }

    pub fn record_db_result(&self, kind: DbKind, ok: bool, now: Instant) {
        self.lock().record_db_result(kind, ok, now);
    }

    pub fn record_time_sync(&self, status: TimeSyncStatus, now: Instant) {
        self.lock().record_time_sync(status, now);
    }

    pub fn evaluate(&self, now: Instant) -> Vec<Alarm> {
        self.lock().evaluate(now)
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test health::tests`
Expected: all 15 PASS. (A "dead code" warning for unused items is expected until Tasks 3-5 land.)

If `healthy_system_has_no_alarms` fails, check that `healthy()` calls `record_time_sync(Synchronized, t0)`: the initial status is `NotInitialized` and `time_not_synced` fires after 60 s.

---

### Task 3: Report, `AlarmPublisher` and SignalK notifications

**Files:**
- Modify: `src/health.rs`

**Interfaces:**
- Consumes: everything from Task 2; `crate::web::signalk_messages::{SignalKDelta, SignalKUpdate, SignalKValue, vessel_context}`; `crate::utilities::instant_to_rfc3339`.
- Produces (used by Tasks 4-5):
  - `HealthState::report(&self, now) -> HealthReport` and `HealthHandle::report(&self, now) -> HealthReport`
  - `HealthReport` (serde `Serialize`): `enabled: bool`, `status: HealthStatus`, `can: &'static str`, `alarms: Vec<AlarmView>`, `streams: BTreeMap<&'static str, Option<u64>>` (age ms), `db: BTreeMap<&'static str, DbView>`, `loop_state: LoopView` (serialized as `"loop"`); `HealthReport::http_status_code(&self) -> u16` (200 or 503)
  - `AlarmPublisher::new(reminder_interval: Duration)`, `update(&mut self, alarms: Vec<Alarm>, now: Instant) -> Vec<AlarmEvent>`
  - `AlarmEvent { id, severity, message, kind: EventKind }`, `EventKind { Started, Reminder, Cleared }`
  - `notification_path(id: &str) -> String`, `build_notification_delta(&AlarmEvent, vessel_uuid: &str, timestamp: String) -> SignalKDelta`
  - `pub async fn run_alarm_publisher(health: HealthHandle, vessel_uuid: String)`

- [ ] **Step 1: Write the failing tests.** Append inside the existing `mod tests` in `src/health.rs`:

```rust
    // ---- report ----

    #[test]
    fn report_ok_when_healthy() {
        let (st, now) = healthy(HealthConfig::default());
        let r = st.report(now);
        assert_eq!(r.status, HealthStatus::Ok);
        assert_eq!(r.http_status_code(), 200);
        assert_eq!(r.can, "ok");
        assert!(r.alarms.is_empty());
        assert_eq!(r.streams["position"], Some(0));
        assert_eq!(r.streams["wind"], None);
    }

    #[test]
    fn report_status_down_for_can_silent_degraded_for_warnings() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_work(now, Duration::from_secs(3));
        let r = st.report(now);
        assert_eq!(r.status, HealthStatus::Degraded);
        assert_eq!(r.http_status_code(), 503);

        let later = now + Duration::from_secs(40);
        st.record_heartbeat(later);
        let r = st.report(later);
        assert_eq!(r.status, HealthStatus::Down);
        assert_eq!(r.can, "silent");
        assert!(!r.alarms[0].since.is_empty());
    }

    #[test]
    fn report_status_down_for_db_failing() {
        let (mut st, now) = healthy(HealthConfig::default());
        for _ in 0..3 {
            st.record_db_result(DbKind::Vessel, false, now);
        }
        let r = st.report(now);
        assert_eq!(r.status, HealthStatus::Down);
        assert_eq!(r.db["vessel"].consecutive_failures, 3);
    }

    #[test]
    fn report_for_disabled_can_and_disabled_health() {
        let t0 = Instant::now();
        let st = HealthState::new(HealthConfig::default(), false, t0);
        let r = st.report(t0 + Duration::from_secs(500));
        assert_eq!(r.can, "disabled");
        assert_eq!(r.status, HealthStatus::Ok);

        let cfg = HealthConfig { enabled: false, ..HealthConfig::default() };
        let st = HealthState::new(cfg, true, t0);
        let r = st.report(t0 + Duration::from_secs(500));
        assert!(!r.enabled);
        assert_eq!(r.status, HealthStatus::Ok);
    }

    #[test]
    fn report_serializes_loop_key() {
        let (st, now) = healthy(HealthConfig::default());
        let json = serde_json::to_value(st.report(now)).unwrap();
        assert!(json.get("loop").is_some());
        assert_eq!(json["status"], "ok");
    }

    // ---- publisher ----

    fn alarm(id: &str, severity: Severity, t: Instant) -> Alarm {
        Alarm { id: id.to_string(), severity, since: t, message: format!("{id} message") }
    }

    #[test]
    fn publisher_emits_only_on_change_and_reminds_periodically() {
        let t0 = Instant::now();
        let mut p = AlarmPublisher::new(Duration::from_secs(60));
        let a = alarm("can_silent", Severity::Alarm, t0);

        let ev = p.update(vec![a.clone()], t0);
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, EventKind::Started);

        assert!(p.update(vec![a.clone()], t0 + Duration::from_secs(1)).is_empty());

        let ev = p.update(vec![a.clone()], t0 + Duration::from_secs(60));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, EventKind::Reminder);

        let ev = p.update(vec![], t0 + Duration::from_secs(61));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, EventKind::Cleared);
        assert_eq!(ev[0].id, "can_silent");

        assert!(p.update(vec![], t0 + Duration::from_secs(62)).is_empty());
    }

    #[test]
    fn publisher_handles_two_alarms_independently() {
        let t0 = Instant::now();
        let mut p = AlarmPublisher::new(Duration::from_secs(60));
        let a = alarm("db_failing:vessel", Severity::Alarm, t0);
        let b = alarm("loop_lagging", Severity::Warn, t0);
        let ev = p.update(vec![a.clone(), b.clone()], t0);
        assert_eq!(ev.len(), 2);
        let ev = p.update(vec![b], t0 + Duration::from_secs(1));
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].kind, EventKind::Cleared);
        assert_eq!(ev[0].id, "db_failing:vessel");
    }

    // ---- SignalK ----

    #[test]
    fn notification_path_replaces_colons() {
        assert_eq!(
            notification_path("stream_stale:position"),
            "notifications.router.stream_stale.position"
        );
        assert_eq!(notification_path("can_silent"), "notifications.router.can_silent");
    }

    #[test]
    fn notification_delta_states() {
        let ev = |kind, severity| AlarmEvent {
            id: "stream_stale:wind".to_string(),
            severity,
            message: "No wind data".to_string(),
            kind,
        };
        let state_of = |e: &AlarmEvent| {
            let d = build_notification_delta(e, "uuid-1", "2026-01-01T00:00:00.000Z".to_string());
            assert_eq!(d.context, "vessels.urn:mrn:signalk:uuid:uuid-1");
            assert_eq!(d.updates[0].values[0].path, "notifications.router.stream_stale.wind");
            d.updates[0].values[0].value["state"].as_str().unwrap().to_string()
        };
        assert_eq!(state_of(&ev(EventKind::Started, Severity::Alarm)), "alarm");
        assert_eq!(state_of(&ev(EventKind::Reminder, Severity::Warn)), "warn");
        assert_eq!(state_of(&ev(EventKind::Cleared, Severity::Alarm)), "normal");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test health::tests`
Expected: FAIL to compile — `report`, `HealthStatus`, `AlarmPublisher`, etc. not found.

- [ ] **Step 3: Implement.** Add these imports to the top of `src/health.rs` (merge with the existing `use` lines): `use serde::Serialize;` and `use crate::web::signalk_messages::{vessel_context, SignalKDelta, SignalKUpdate, SignalKValue};`.

Add inside `impl HealthState` (after `evaluate`):

```rust
    /// Snapshot for `/api/health`. Ages are milliseconds; `None` means "never seen".
    pub fn report(&self, now: Instant) -> HealthReport {
        let alarms = self.evaluate(now);
        let age_ms = |t: Instant| now.saturating_duration_since(t).as_millis() as u64;

        let status = if alarms.iter().any(|a| {
            a.id == "can_silent" || a.id == "loop_stalled" || a.id.starts_with("db_failing")
        }) {
            HealthStatus::Down
        } else if !alarms.is_empty() {
            HealthStatus::Degraded
        } else {
            HealthStatus::Ok
        };

        let can = if !self.can_enabled {
            "disabled"
        } else if alarms.iter().any(|a| a.id == "can_silent") {
            "silent"
        } else {
            "ok"
        };

        HealthReport {
            enabled: self.config.enabled,
            status,
            can,
            alarms: alarms
                .iter()
                .map(|a| AlarmView {
                    id: a.id.clone(),
                    severity: a.severity,
                    since: crate::utilities::instant_to_rfc3339(a.since),
                    message: a.message.clone(),
                })
                .collect(),
            streams: Stream::ALL
                .iter()
                .map(|s| (s.name(), self.streams.get(s).map(|&t| age_ms(t))))
                .collect(),
            db: DbKind::ALL
                .iter()
                .map(|k| {
                    let rec = self.db.get(k).copied().unwrap_or_default();
                    (
                        k.name(),
                        DbView {
                            consecutive_failures: rec.consecutive_failures,
                            last_success_age_ms: rec.last_success.map(age_ms),
                        },
                    )
                })
                .collect(),
            loop_state: LoopView {
                heartbeat_age_ms: self.last_heartbeat.map(age_ms),
                last_slow_ms: self.last_slow.map(|(_, d)| d.as_millis() as u64),
                last_slow_age_ms: self.last_slow.map(|(t, _)| age_ms(t)),
            },
        }
    }
```

Add in `impl HealthHandle`:

```rust
    pub fn report(&self, now: Instant) -> HealthReport {
        self.lock().report(now)
    }
```

Add below `HealthHandle` (before the tests module):

```rust
// ── Report ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Ok,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlarmView {
    pub id: String,
    pub severity: Severity,
    pub since: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DbView {
    pub consecutive_failures: u32,
    pub last_success_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoopView {
    pub heartbeat_age_ms: Option<u64>,
    pub last_slow_ms: Option<u64>,
    pub last_slow_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub enabled: bool,
    pub status: HealthStatus,
    pub can: &'static str,
    pub alarms: Vec<AlarmView>,
    pub streams: BTreeMap<&'static str, Option<u64>>,
    pub db: BTreeMap<&'static str, DbView>,
    #[serde(rename = "loop")]
    pub loop_state: LoopView,
}

impl HealthReport {
    /// 200 when healthy, 503 otherwise, so `curl -f` / uptime pollers work.
    pub fn http_status_code(&self) -> u16 {
        if self.status == HealthStatus::Ok {
            200
        } else {
            503
        }
    }
}

// ── Edge-triggered alarm publishing ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Started,
    Reminder,
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlarmEvent {
    pub id: String,
    pub severity: Severity,
    pub message: String,
    pub kind: EventKind,
}

/// Remembers the previously active alarms and turns each evaluation into events:
/// `Started` when an alarm appears, `Cleared` when it disappears, and `Reminder` for
/// every still-active alarm once per `reminder_interval` (so late SignalK clients see it).
pub struct AlarmPublisher {
    active: BTreeMap<String, Alarm>,
    last_reminder: Option<Instant>,
    reminder_interval: Duration,
}

impl AlarmPublisher {
    pub fn new(reminder_interval: Duration) -> Self {
        Self {
            active: BTreeMap::new(),
            last_reminder: None,
            reminder_interval,
        }
    }

    pub fn update(&mut self, alarms: Vec<Alarm>, now: Instant) -> Vec<AlarmEvent> {
        let current: BTreeMap<String, Alarm> =
            alarms.into_iter().map(|a| (a.id.clone(), a)).collect();
        let mut events = Vec::new();

        for (id, alarm) in &current {
            if !self.active.contains_key(id) {
                events.push(AlarmEvent {
                    id: id.clone(),
                    severity: alarm.severity,
                    message: alarm.message.clone(),
                    kind: EventKind::Started,
                });
            }
        }
        for (id, alarm) in &self.active {
            if !current.contains_key(id) {
                events.push(AlarmEvent {
                    id: id.clone(),
                    severity: alarm.severity,
                    message: "Condition cleared".to_string(),
                    kind: EventKind::Cleared,
                });
            }
        }

        let reminder_due = self
            .last_reminder
            .map_or(true, |t| now.saturating_duration_since(t) >= self.reminder_interval);
        if reminder_due {
            for (id, alarm) in &current {
                if self.active.contains_key(id) {
                    events.push(AlarmEvent {
                        id: id.clone(),
                        severity: alarm.severity,
                        message: alarm.message.clone(),
                        kind: EventKind::Reminder,
                    });
                }
            }
            self.last_reminder = Some(now);
        }

        self.active = current;
        events
    }
}

// ── SignalK notifications ────────────────────────────────────────────────

/// SignalK paths are dot-separated; alarm ids use `:` (e.g. `stream_stale:position`).
pub fn notification_path(id: &str) -> String {
    format!("notifications.router.{}", id.replace(':', "."))
}

pub fn build_notification_delta(
    event: &AlarmEvent,
    vessel_uuid: &str,
    timestamp: String,
) -> SignalKDelta {
    let state = match (event.kind, event.severity) {
        (EventKind::Cleared, _) => "normal",
        (_, Severity::Alarm) => "alarm",
        (_, Severity::Warn) => "warn",
    };
    SignalKDelta {
        context: vessel_context(vessel_uuid),
        updates: vec![SignalKUpdate {
            source: None,
            timestamp,
            values: vec![SignalKValue {
                path: notification_path(&event.id),
                value: serde_json::json!({
                    "state": state,
                    "method": ["visual"],
                    "message": event.message,
                }),
            }],
            source_ref: "router.health".to_string(),
        }],
    }
}

/// 1 Hz task: evaluate, publish changes to the log and the SignalK channel. Runs on the
/// web runtime so it keeps working when the router loop is hung.
pub async fn run_alarm_publisher(health: HealthHandle, vessel_uuid: String) {
    let channels = crate::web::get_signalk_channels();
    let mut publisher = AlarmPublisher::new(Duration::from_secs(60));
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    loop {
        ticker.tick().await;
        let now = Instant::now();
        for event in publisher.update(health.evaluate(now), now) {
            match (event.kind, event.severity) {
                (EventKind::Started, Severity::Alarm) => {
                    tracing::error!(alarm = %event.id, "Health alarm raised: {}", event.message)
                }
                (EventKind::Started, Severity::Warn) => {
                    tracing::warn!(alarm = %event.id, "Health warning raised: {}", event.message)
                }
                (EventKind::Cleared, _) => {
                    tracing::info!(alarm = %event.id, "Health condition cleared")
                }
                (EventKind::Reminder, _) => {}
            }
            let delta = build_notification_delta(
                &event,
                &vessel_uuid,
                crate::utilities::instant_to_rfc3339(now),
            );
            let _ = channels.send(delta);
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test health::tests`
Expected: all PASS.

---

### Task 4: Record from the router loop; create the handle in `main`

**Files:**
- Modify: `src/main.rs` (create handle ~line 193, pass to web server ~line 239 and pipeline ~line 284)
- Modify: `src/router_loop.rs`

**Interfaces:**
- Consumes: `HealthHandle`, `Stream`, `DbKind` (Task 2).
- Produces: `RouterLoop::new(..., health: HealthHandle)` (new last parameter), `run_can_pipeline(config, vessel_db, ais_target_cache, udp_broadcaster, health)`, and `start_web_server(..., health)` (Task 5 changes its signature; this task only creates and clones the handle).

There are no existing unit tests for `RouterLoop`; this task is verified by compilation, the existing suite, and the manual check in Task 6.

- [ ] **Step 1: `main.rs` — create the handle.** Insert after the `config.can.enabled` read-only block (after the `info!("CAN disabled — forcing read_only mode");` closing brace, before "Create UDP broadcaster"):

```rust
    // Health monitor shared by the router loop (recording) and the web layer (evaluation).
    let health = health::HealthHandle::new(
        config.health.clone(),
        config.can.enabled,
        std::time::Instant::now(),
    );
```

Pass it to the pipeline — change the last line of `main`:

```rust
    router_loop::run_can_pipeline(config, vessel_db, ais_cache, udp_broadcaster, health)
```

For the web server, do the `health_web` clone next to the other `*_web` clones (inside `if config.web.enabled {`, after `let ais_cache_web = ais_cache.clone();`):

```rust
        let health_web = health.clone();
```

and add `health_web` as the last argument of the `web::start_web_server(...)` call (the signature changes in Task 5, so the build stays broken until then; do Task 5 immediately after this one, or add the argument in Task 5 Step 3).

- [ ] **Step 2: `router_loop.rs` — imports and field.** Add to the imports:

```rust
use crate::health::{DbKind, HealthHandle, Stream};
```

Add a field to `RouterLoop` after `db_health_check`:

```rust
    // Health monitor (recording side)
    health: HealthHandle,
```

Add `health: HealthHandle,` as the last parameter of `RouterLoop::new` and `health,` to the `Self { .. }` initializer. Add `health: HealthHandle,` as the last parameter of `run_can_pipeline` and pass `health,` as the last argument of `RouterLoop::new(...)` inside it.

- [ ] **Step 3: `run()` — heartbeat, frame, work timing.** Replace the body of `run()` from `loop {` through the end of the `Ok((extended_id, data))` arm with the following, keeping the `Err(e)` arm and the periodic-tasks section as they are:

```rust
        loop {
            self.health.record_heartbeat(Instant::now());
            match CanBus::read_nmea2k_frame(&self.socket) {
                Ok((extended_id, data)) => {
                    self.metrics.can_frames += 1;
                    self.health.record_frame(Instant::now());

                    let id = Identifier::from_can_id(extended_id);
                    if !should_process_frame_by_id(&self.config, id) {
                        continue;
                    }

                    self.metrics.can_processed_frames += 1;

                    if let Some(n2k_frame) = self.reader.process_frame(extended_id, &data) {
                        self.metrics.nmea_messages += 1;

                        if !should_process_n2k_message(&self.config, &n2k_frame.message) {
                            continue;
                        }

                        self.metrics.nmea_processed_messages += 1;

                        let now = Instant::now();
                        self.process_n2k_message(&n2k_frame, now);
                        self.health.record_work(Instant::now(), now.elapsed());
                    }
                }
```

(The first `record_heartbeat` is before the blocking read, which is at most 500 ms; the stall limit is 10 s.)

In the periodic-tasks section, time the DB health check. Replace

```rust
            let db_url = self.config.database.connection.connection_url();
            match self.db_health_check.check_and_reconnect(&self.vessel_db, &db_url) {
```

with

```rust
            let db_url = self.config.database.connection.connection_url();
            let health_check_started = Instant::now();
            let health_check_result = self.db_health_check.check_and_reconnect(&self.vessel_db, &db_url);
            self.health.record_work(Instant::now(), health_check_started.elapsed());
            match health_check_result {
```

- [ ] **Step 4: `process_n2k_message` — stream and time-sync recording.** At the very top of the function (before `self.time_monitor.handle_message(frame, now);`) add:

```rust
        if let Some(stream) = stream_of(&frame.message) {
            self.health.record_stream(stream, now);
        }
```

and right after the existing `self.metrics.gnss_time_skew_status = sync.status;` line add:

```rust
        self.health.record_time_sync(sync.status, now);
```

Add this free function near `read_db` at the top of the file:

```rust
/// Map an assembled message to the stream the health monitor tracks, if any.
fn stream_of(message: &nmea2k::pgns::N2kMessage) -> Option<Stream> {
    use nmea2k::pgns::N2kMessage;
    match message {
        N2kMessage::PositionRapidUpdate(_) => Some(Stream::Position),
        N2kMessage::CogSogRapidUpdate(_) => Some(Stream::CogSog),
        N2kMessage::NMEASystemTime(_) => Some(Stream::SystemTime),
        N2kMessage::VesselHeading(_) => Some(Stream::Heading),
        N2kMessage::WindData(_) => Some(Stream::Wind),
        N2kMessage::EngineRapidUpdate(_) => Some(Stream::Engine),
        _ => None,
    }
}
```

(Variant names are the ones already matched in `vessel_monitor.rs`/`time_monitor.rs`.)

- [ ] **Step 5: DB write outcomes.** In `handle_vessel_status_write`, replace the `if let Some(true) = with_db_retry(...) { ... }` with:

```rust
        let result = with_db_retry(
            &self.vessel_db,
            &mut self.db_health_check,
            &db_url,
            "vessel status write",
            |db| self.vessel_status_handler.handle_vessel_status(db, &vessel_status),
        );
        self.health
            .record_db_result(DbKind::Vessel, result.is_some(), Instant::now());
        if let Some(true) = result {
            self.metrics.vessel_reports += 1;
        }
```

In `handle_env_status_write`, replace the `if let Some(count) = with_db_retry(...) { ... }` with:

```rust
        let result = with_db_retry(
            &self.vessel_db,
            &mut self.db_health_check,
            &db_url,
            "environmental write",
            |db| self.environmental_status_handler.handle_environment_status(db, &mut self.env_monitor, now),
        );
        self.health
            .record_db_result(DbKind::Env, result.is_some(), Instant::now());
        if let Some(count) = result {
            self.metrics.env_reports += count as u64;
        }
```

(`with_db_retry` returns `None` only when the final attempt failed, so `is_some()` is the final-outcome signal.)

- [ ] **Step 6: Build**

Run: `cargo build 2>&1 | tail -20`
Expected: the only error is the `start_web_server` argument count mismatch in `main.rs` (fixed in Task 5). Anything else is a mistake in this task: fix it now.

---

### Task 5: `/api/health`, public path, publisher task

**Files:**
- Modify: `src/web/api.rs` (`AppState` ~line 42, handler, route in `create_api_router` ~line 2158, the four test `AppState { .. }` literals at ~2276, ~3297, ~3321, ~4110)
- Modify: `src/web/server.rs` (signature ~line 29, `AppState` literal ~line 82, spawn ~line 99)
- Modify: `src/web/auth.rs` (`PUBLIC_PATHS` ~line 75)

**Interfaces:**
- Consumes: `HealthHandle::report`, `HealthReport::http_status_code`, `run_alarm_publisher` (Tasks 2-3).
- Produces: `AppState.health: crate::health::HealthHandle`; `GET /api/health`; `start_web_server(db, config, ais_cache, port, udp_broadcast_available, health, startup_signal)`.

- [ ] **Step 1: Failing test for the public path.** Add at the end of `src/web/auth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_endpoint_is_public() {
        assert!(is_public_path("/api/health"));
    }

    #[test]
    fn other_api_paths_stay_protected() {
        assert!(!is_public_path("/api/trips"));
        assert!(!is_public_path("/api/health/anything"));
    }
}
```

Run: `cargo test web::auth::tests` → `health_endpoint_is_public` FAILS (the build may also fail on the Task 4 leftover; if so, finish Step 3 first, then return here).

- [ ] **Step 2: Implement the public path.** Add `"/api/health",` to `PUBLIC_PATHS` in `src/web/auth.rs`.

- [ ] **Step 3: `AppState`, handler, route, server wiring.**

In `src/web/api.rs`, add to `AppState` (after `udp_broadcast_available`):

```rust
    /// Shared health monitor; evaluated on every `/api/health` request.
    pub health: crate::health::HealthHandle,
```

Add the handler next to the other simple handlers (e.g. before `get_tracking_status`):

```rust
/// GET /api/health — alarms and stream/DB/loop ages. 200 when healthy, 503 otherwise.
pub async fn get_health(State(state): State<AppState>) -> impl IntoResponse {
    let report = state.health.report(std::time::Instant::now());
    let status = StatusCode::from_u16(report.http_status_code())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(report))
}
```

(If `IntoResponse`, `StatusCode`, `Json` or `State` are not already imported at the top of `api.rs`, add them to the existing `use axum::...` lines. They are used by the other handlers, so they should be.)

Register the route in the main chain of `create_api_router` (outside the `if !read_only` block, so it is available read-only too), next to `.route("/config/read_only", get(get_read_only))`:

```rust
        .route("/health", get(get_health))
```

Add `health: crate::health::HealthHandle::new(crate::config::HealthConfig::default(), true, std::time::Instant::now()),` as the last field of each of the four test `AppState { .. }` literals in `api.rs` (search for `udp_broadcast_available: true,` — there are four, in `create_test_app_with_cache`, `create_clean_test_app`, `create_test_app_with_polars` and `create_test_app_read_only`).

In `src/web/server.rs`:

- Add a parameter to `start_web_server`, between `udp_broadcast_available: bool,` and `startup_signal`: `health: crate::health::HealthHandle,`
- Add `health: health.clone(),` to the `AppState { .. }` literal.
- Add next to the poller spawn:

```rust
    let publisher_uuid = state.config.signalk.vessel_uuid.clone();
    tokio::spawn(crate::health::run_alarm_publisher(health, publisher_uuid));
```

- In `src/main.rs`, make sure the `start_web_server(...)` call now passes `health_web` after `udp_enabled`:
  `web::start_web_server(db_arc, config_arc, ais_cache_web, web_port, udp_enabled, health_web, startup_tx)`

- [ ] **Step 4: Build and run the non-DB suite**

Run: `cargo build 2>&1 | tail -20 && cargo test 2>&1 | tail -15`
Expected: clean build; all non-ignored tests PASS, including `health::tests`, `config::tests::test_health*` and `web::auth::tests`.

- [ ] **Step 5: Cross-check the Linux build** (CLAUDE.md, only if you are not on Linux): `cargo check --workspace --all-targets --target aarch64-unknown-linux-gnu`.

---

### Task 6: Dashboard indicator, docs, final verification

**Files:**
- Modify: `static/js/shared-theme.js` (`createHeaderBar` ~line 150, plus new function and listener)
- Modify: `static/shared.css` (next to `.status-dot` ~line 180)
- Modify: `AGENTS.md` (new "Health Monitor" subsection under `## Specs`, after "Time synchronization" ~line 121)

- [ ] **Step 1: CSS.** Add after `.status-dot.disconnected { ... }` in `static/shared.css`:

```css
.status-dot.health-ok {
    background-color: #27ae60;
}

.status-dot.health-degraded {
    background-color: #f39c12;
}

.status-dot.health-down {
    background-color: #e74c3c;
}
```

- [ ] **Step 2: Header markup.** In `createHeaderBar`, immediately before the `if (showConnectionStatus) {` block, add:

```javascript
    headerHTML +=
            `<button class="theme-toggle" title="System health" tabindex="-1">
                <span class="status-dot" id="healthStatus"></span>
            </button>`;
```

- [ ] **Step 3: Polling.** Add after the `document.addEventListener('DOMContentLoaded', applyUiMode);` line in `shared-theme.js`:

```javascript
// ---------------------------- Health indicator ----------------------------

const HEALTH_POLL_MS = 10000;

async function updateHealthIndicator() {
    const dot = document.getElementById('healthStatus');
    if (!dot) return;
    dot.classList.remove('health-ok', 'health-degraded', 'health-down');
    try {
        // /api/health answers 503 when something is wrong, so do not check resp.ok.
        const resp = await fetch('/api/health', { credentials: 'same-origin' });
        const health = await resp.json();
        dot.classList.add('health-' + health.status);
        dot.parentElement.title = health.alarms.length === 0
            ? 'System health: OK'
            : health.alarms.map(a => a.message).join('\n');
    } catch (_) {
        dot.classList.add('health-down');
        dot.parentElement.title = 'System health: unreachable';
    }
}

function startHealthIndicator() {
    if (!document.getElementById('healthStatus')) return;
    updateHealthIndicator();
    setInterval(updateHealthIndicator, HEALTH_POLL_MS);
}

document.addEventListener('DOMContentLoaded', startHealthIndicator);
```

(Every page inserts the header with an inline script at the end of `<body>`, which runs before `DOMContentLoaded`, so the dot exists when `startHealthIndicator` runs. `login.html` has no header, so the function returns early.)

- [ ] **Step 4: AGENTS.md.** Add after the "Time synchronization" section:

```markdown
### Health Monitor

`src/health.rs`. The router loop records, via `HealthHandle::record_*`, when each stream (position, COG/SOG, system time, heading, wind, engine), CAN frame, loop iteration and DB write last happened. `HealthState::evaluate(now)` turns that into alarms; it is evaluated by a 1 Hz publisher task (edge-triggered: error log + SignalK `notifications.router.*`) and on every `GET /api/health` (200 when healthy, 503 otherwise; public, no login). A status dot in the shared header polls it.

Alarms: `can_silent`, `stream_stale:<name>`, `db_failing:<vessel|env>`, `loop_lagging`, `loop_stalled`, `time_not_synced`. A stalled loop suppresses the CAN and stream alarms; a silent bus suppresses the per-stream alarms. Optional streams (heading, wind, engine) only alarm once seen. No alarms during `health.startup_grace_secs` after start, nor when CAN is disabled. Thresholds: `health` section of the config (see README).

Known limit: when the database connection is lost and reconnection fails, the router exits (systemd restarts it), so `db_failing` covers persistent write failures that do not trigger a reconnect, not a total DB outage.
```

- [ ] **Step 5: Full verification**

Run: `cargo test 2>&1 | tail -15`
Expected: PASS.

Manual check (needs a reachable MariaDB per `config.json`; on Linux without a CAN interface the pipeline keeps retrying to open it, which is itself a useful test):

```bash
cargo run --release &
sleep 5;  curl -s -o /dev/null -w "%{http_code}\n" localhost:8080/api/health   # 200 (inside startup grace)
sleep 45; curl -s localhost:8080/api/health | head -c 600                      # expect status "down", alarm loop_stalled
```

With a working `vcan0` (`sudo modprobe vcan && sudo ip link add dev vcan0 type vcan && sudo ip link set up vcan0`, `config.json` pointing at `vcan0`), expect `loop_stalled` to be absent, `can_silent` to appear after ~40 s of no traffic, and `cansend vcan0` frames to clear it. Open any dashboard page and confirm the header dot is green/amber/red accordingly, and that the log shows one `Health alarm raised` / `Health condition cleared` line per transition.

Report which of these checks you could and could not run.
