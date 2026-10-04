# Health Monitor — Design

Date: 2026-10-03
Status: draft, awaiting review

## Goal

The system must tell the operator when it has stopped capturing data, so a silent gap cannot go unnoticed. Three conditions are covered:

1. **Feed loss** — the whole CAN bus going quiet, or an individual stream (GPS, system time, ...) going quiet while the bus is alive.
2. **Consistent DB failure** — writes failing persistently (not a single transient error, which `with_db_retry` already handles).
3. **Processing delay** — the router loop falling behind, which distorts the time-windowed sampling (status averaging, environmental aggregation, mooring detection). **Detect and alarm only**; no change to processing behaviour.

Plus one cheap addition: **time not synchronized**, because it silently blocks all DB writes today.

### Delivery

Error-level log lines, `GET /api/health`, a dashboard status indicator, and SignalK notifications. No push notifications and no systemd watchdog.

### Out of scope

- DB I/O blocking the CAN read loop (known; not touched now).
- Fast-packet reassembly robustness.
- `CAP_SYS_TIME` for the service (system time is set only on the on-board RPi).
- Frame-drop counting (`SO_RXQ_OVFL`).
- Shedding work under lag.

## Architecture

New module `src/health.rs`, three units:

- **`HealthState`** — shared as `Arc<Mutex<HealthState>>`. Written by the router loop, read by the web layer. Holds:
  - per-stream last-seen `Instant` plus an ever-seen flag
  - last CAN frame `Instant`
  - per DB-write kind (vessel, env): last success `Instant`, consecutive failure count, last failure `Instant`
  - last loop heartbeat `Instant`
  - the most recent unit of work that exceeded `loop_lag_secs` (when, how long)
  - time-sync status and the `Instant` it last changed
  - process start `Instant`
- **`HealthState::evaluate(&self, now: Instant) -> Vec<Alarm>`** — pure (the state holds a copy of `HealthConfig`); `now` is injected (project rule: never call `Instant::now()` inside business logic).
- **`AlarmPublisher`** — holds the previous active alarm set; on each evaluation emits only differences: start → `error!` log + SignalK `alarm`/`warn`; clear → `info!` log + SignalK `normal`. Re-sends active alarms to SignalK every 60 s so late-joining clients see them.

The router loop only calls cheap `record_*` methods; it never evaluates. Evaluation happens outside the loop so that a hung loop is itself detectable (stale heartbeat).

### Wiring

- `main` creates the `Arc<Mutex<HealthState>>` before the web server starts and passes it to both `start_web_server` (stored in `AppState`, like `poller_status`) and `run_can_pipeline`.
- A `tokio::spawn` 1 Hz task in `start_web_server` runs `evaluate` + `AlarmPublisher`.
- `/api/health` also calls `evaluate` on read, so the endpoint is correct even if the publisher task dies.
- With `can.enabled = false`, CAN/stream/loop/DB-write alarms are suppressed (no pipeline); the endpoint reports `"can": "disabled"`.

### Recording points in `router_loop.rs`

| Call | Where |
|---|---|
| `record_heartbeat(now)` + iteration duration | top/bottom of each `run()` iteration |
| `record_frame(now)` | on every successful CAN read |
| `record_stream(stream, now)` | in `process_n2k_message`, by message type |
| `record_db_result(kind, ok, duration, now)` | inside `with_db_retry` (final outcome, after the retry) |
| `record_time_sync(status, now)` | next to the existing `time_sync_status()` call |

The mutex is held only for field writes, never across DB or I/O calls. A poisoned mutex is recovered with `into_inner`, as elsewhere in the codebase.

## Alarms

Each alarm has: `id`, `severity` (`warn` | `alarm`), `since`, `message`.

| Id | Fires when | Clears when |
|---|---|---|
| `can_silent` | no CAN frame for `can_silence_secs` | any frame |
| `stream_stale:<name>` | a tracked stream is silent beyond its timeout (see below) | next message of that stream |
| `db_failing:<vessel\|env>` | writes of that kind failing continuously (no success since the first failure) for `db_failure_secs` (time-based, not a count: env writes retry at CAN-message rate, moored vessel writes every 30 min; a "no write for N seconds" trigger was rejected: writes legitimately stop when moored, when tracking is off, or while time is unsynced) | first successful write on that kind |
| `loop_overloaded` | the loop spent more than `loop_busy_ratio` of a 10 s window processing messages (accumulating delay: no single unit is slow, but the loop is saturating and the kernel CAN buffer may overflow) | next window below the ratio |
| `loop_lagging` | one unit of work (message processing including its DB write, or the periodic DB health check) took longer than `loop_lag_secs`; stays active for `loop_lag_window_secs` | window passes |
| `loop_stalled` | no heartbeat for `loop_stall_secs` | heartbeat |
| `time_not_synced` | time uninitialized or skewed for `time_unsynced_secs` | synchronized |

### Streams

Required (alarm if silent past timeout, even if never seen once past startup grace): `position` (PGN 129025), `cog_sog` (129026), `system_time` (126992).
Optional (alarm only if seen at least once since start): `heading` (127250), `wind` (130306). Engine (127488) is not tracked: engine gateways go silent whenever the engine is off. A never-seen required stream's timeout starts when the grace period ends (GPS cold start).
Environmental sensors (pressure, temperature, humidity) are not tracked as streams in this iteration; their silence is not alarmed.

### Suppression rules

- No stream, `can_silent`, `loop_*` or `db_failing` alarms during `startup_grace_secs` after process start.
- When `can_silent` is active, per-stream and `time_not_synced` alarms are suppressed (one root cause, one alarm). When `loop_stalled` is active, `can_silent`, stream and time alarms are suppressed too.
- `time_not_synced` measures time since the last `Synchronized` observation, so flapping cannot hide a bad clock.
- `db_failing` and stream alarms are independent.

## Config

New `health` section in `Config`, `config.example.json` and AGENTS.md (per the config-change checklist):

| Field | Default |
|---|---|
| `enabled` | true |
| `startup_grace_secs` | 30 |
| `can_silence_secs` | 10 |
| `required_stream_timeout_secs` | 30 |
| `optional_stream_timeout_secs` | 60 |
| `db_failure_secs` | 60 |
| `loop_lag_secs` | 2 |
| `loop_lag_window_secs` | 60 |
| `loop_busy_ratio` | 0.8 |
| `loop_stall_secs` | 10 |
| `time_unsynced_secs` | 60 |

Config is read-only after load. A zero value reverts to the default with a warning (except `startup_grace_secs`, where 0 is allowed), matching the other config sections.

## Surfaces

### `GET /api/health`

```json
{
  "status": "ok | degraded | down",
  "can": "ok | silent | disabled",
  "alarms": [{"id": "...", "severity": "alarm", "since": "RFC3339", "message": "..."}],
  "streams": {"position": 400, "cog_sog": 500, "system_time": 1200, "wind": null},
  "db": {"vessel": {"consecutive_failures": 0, "last_success_age_ms": 12000}, "env": {...}},
  "loop": {"heartbeat_age_ms": 300, "last_slow_ms": null, "last_slow_age_ms": null}
}
```

HTTP 200 when no alarms, 503 otherwise. `status` is `down` when any alarm-severity alarm is active (data is not being captured), `degraded` when only warn-severity alarms are active (`loop_lagging`, `loop_overloaded`, optional streams). Added to `is_public_path` (exposes only ages and alarm ids, no vessel position).

### SignalK

Delta on `notifications.router.<id>` with `{state: "alarm"|"warn"|"normal", message}`, using the existing broadcast channel.

### Dashboard

A status dot in the shared header built by `static/js/shared-theme.js`: polls `/api/health` every 10 s; green = ok, amber = degraded, red = down; tooltip lists active alarms (no click-through). Follows the existing theme/header conventions in AGENTS.md §UI structure.

## Testing

- Unit tests on `evaluate()` with synthetic `Instant`s: each alarm's fire/clear, startup grace, `can_silent` suppressing stream alarms, optional-stream ever-seen latch, `db_failing` both triggers.
- `AlarmPublisher`: emits only on change; periodic SignalK re-send.
- `/api/health` handler: 200/503 mapping, `status` derivation, `can: disabled`.
- Config: defaults, validation of bad values.
- No DB and no CAN required for any of these.

## Failure modes of the monitor itself

- Mutex poisoned: recover with `into_inner`.
- Publisher task dies: `/api/health` still evaluates on read.
- Router loop hangs: detected as `loop_stalled` from the external evaluator.
- `health.enabled = false`: `evaluate` returns no alarms; endpoint returns `status: "ok"` with `"enabled": false`.
- Total DB outage: the router exits when reconnection fails (existing behaviour, systemd restarts it), so `db_failing` covers persistent write failures that do not trigger a reconnect. An external poller sees the outage as an unreachable endpoint.
