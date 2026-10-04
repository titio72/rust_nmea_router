// Health monitor: records when each data stream, DB write and loop iteration last happened,
// and evaluates that state into alarms. The router loop only calls the cheap `record_*`
// methods; evaluation happens elsewhere (publisher task, /api/health) so that a hung loop
// is itself detectable. See docs/superpowers/specs/2026-10-03-health-monitor-design.md.
//
// Every method takes `now` explicitly: nothing in here reads the clock.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::config::HealthConfig;
use crate::time_monitor::TimeSyncStatus;
use crate::web::signalk_messages::{vessel_context, SignalKDelta, SignalKUpdate, SignalKValue};

/// NMEA2000 data streams the health monitor tracks. Engine data is deliberately not tracked:
/// engine gateways go silent whenever the engine is switched off, which is normal operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stream {
    Position,
    CogSog,
    SystemTime,
    Heading,
    Wind,
}

impl Stream {
    pub const ALL: [Stream; 5] = [
        Stream::Position,
        Stream::CogSog,
        Stream::SystemTime,
        Stream::Heading,
        Stream::Wind,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stream::Position => "position",
            Stream::CogSog => "cog_sog",
            Stream::SystemTime => "system_time",
            Stream::Heading => "heading",
            Stream::Wind => "wind",
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
    /// Busy-time accounting: work done since `busy_window_start`, and the ratio of the last
    /// completed window (when it completed, busy fraction).
    busy_window_start: Instant,
    busy_acc: Duration,
    last_busy: Option<(Instant, f64)>,
    time_status: TimeSyncStatus,
    /// Last time the status was observed `Synchronized`.
    last_synced: Option<Instant>,
}

/// Length of the window over which loop busy time is averaged.
const BUSY_WINDOW: Duration = Duration::from_secs(10);

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
            busy_window_start: now,
            busy_acc: Duration::ZERO,
            last_busy: None,
            time_status: TimeSyncStatus::NotInitialized,
            last_synced: None,
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
        self.busy_acc += duration;
        let elapsed = now.saturating_duration_since(self.busy_window_start);
        if elapsed >= BUSY_WINDOW {
            let ratio = self.busy_acc.as_secs_f64() / elapsed.as_secs_f64();
            self.last_busy = Some((now, ratio));
            self.busy_acc = Duration::ZERO;
            self.busy_window_start = now;
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
        self.time_status = status;
        if status == TimeSyncStatus::Synchronized {
            self.last_synced = Some(now);
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
        let grace_end = self.started + limit(cfg.startup_grace_secs);

        // A stalled loop cannot tell us anything about the bus or the streams.
        let heartbeat_ref = self.last_heartbeat.unwrap_or(self.started);
        let stalled = age(heartbeat_ref) > limit(cfg.loop_stall_secs);
        if stalled {
            alarms.push(Alarm {
                id: "loop_stalled".to_string(),
                severity: Severity::Alarm,
                since: heartbeat_ref,
                message: format!(
                    "Router loop has not completed an iteration for {} s (possibly waiting for the CAN interface to come back)",
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
                    // Never seen: the clock starts when the grace period ends (a GPS cold start can
                    // legitimately take a minute before the first message).
                    (None, true) => (grace_end, cfg.required_stream_timeout_secs, Severity::Alarm),
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
            if let Some(first) = rec.first_failure {
                if age(first) >= limit(cfg.db_failure_secs) {
                    alarms.push(Alarm {
                        id: format!("db_failing:{}", kind.name()),
                        severity: Severity::Alarm,
                        since: first,
                        message: format!(
                            "{} database writes failing for {} s ({} failures, no success since)",
                            kind.name(),
                            age(first).as_secs(),
                            rec.consecutive_failures
                        ),
                    });
                }
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

        if let Some((at, ratio)) = self.last_busy {
            if ratio > cfg.loop_busy_ratio && age(at) < limit(cfg.loop_lag_window_secs) {
                alarms.push(Alarm {
                    id: "loop_overloaded".to_string(),
                    severity: Severity::Warn,
                    since: at,
                    message: format!(
                        "Router loop busy {:.0}% of the last {} s window (limit {:.0}%): CAN frames may be dropped",
                        ratio * 100.0,
                        BUSY_WINDOW.as_secs(),
                        cfg.loop_busy_ratio * 100.0
                    ),
                });
            }
        }

        // Skipped while the bus is silent or the loop stalled: the root cause is already alarmed.
        let time_ref = self.last_synced.unwrap_or(self.started);
        if !stalled
            && !can_silent
            && self.time_status != TimeSyncStatus::Synchronized
            && age(time_ref) > limit(cfg.time_unsynced_secs)
        {
            alarms.push(Alarm {
                id: "time_not_synced".to_string(),
                severity: Severity::Alarm,
                since: time_ref,
                message: format!(
                    "System time not synchronized with NMEA time ({}); database writes are blocked",
                    self.time_status
                ),
            });
        }

        alarms
    }

    /// Snapshot for `/api/health`. Ages are milliseconds; `None` means "never seen".
    pub fn report(&self, now: Instant) -> HealthReport {
        let alarms = self.evaluate(now);
        let age_ms = |t: Instant| now.saturating_duration_since(t).as_millis() as u64;

        // Alarm-severity conditions mean data is not being captured: Down. Warn-only: Degraded.
        let status = if alarms.iter().any(|a| a.severity == Severity::Alarm) {
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

    pub fn report(&self, now: Instant) -> HealthReport {
        self.lock().report(now)
    }
}

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
        st.record_time_sync(TimeSyncStatus::Synchronized, now);
        (st, now)
    }

    /// Keep the bus, loop, required streams and time sync fresh at `t`.
    fn refresh(st: &mut HealthState, t: Instant) {
        st.record_frame(t);
        st.record_heartbeat(t);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, t);
        }
        st.record_time_sync(TimeSyncStatus::Synchronized, t);
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
        st.record_time_sync(TimeSyncStatus::Synchronized, now);
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
    fn db_failing_only_after_failing_continuously_and_clears_on_success() {
        let (mut st, now) = healthy(HealthConfig::default());
        // A burst of failures in the same instant (env retries at CAN rate) is not an alarm yet.
        for _ in 0..50 {
            st.record_db_result(DbKind::Vessel, false, now);
        }
        assert!(st.evaluate(now).is_empty());
        let later = now + secs(59);
        refresh(&mut st, later);
        assert!(st.evaluate(later).is_empty());
        let later = now + secs(60);
        refresh(&mut st, later);
        assert_eq!(ids(&st.evaluate(later)), vec!["db_failing:vessel"]);
        st.record_db_result(DbKind::Vessel, true, later);
        assert!(st.evaluate(later).is_empty());
    }

    #[test]
    fn single_failed_write_alarms_after_the_window() {
        // A moored boat writes every 30 min: one failed write (after reconnect+retry) must
        // alarm after db_failure_secs, not after N more 30-minute intervals.
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_db_result(DbKind::Vessel, false, now);
        let later = now + secs(61);
        refresh(&mut st, later);
        assert_eq!(ids(&st.evaluate(later)), vec!["db_failing:vessel"]);
    }

    #[test]
    fn db_failures_are_tracked_per_kind() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_db_result(DbKind::Env, false, now);
        st.record_db_result(DbKind::Vessel, true, now);
        let later = now + secs(61);
        refresh(&mut st, later);
        assert_eq!(ids(&st.evaluate(later)), vec!["db_failing:env"]);
    }

    #[test]
    fn busy_loop_raises_overloaded_even_when_no_single_unit_is_slow() {
        let t0 = Instant::now();
        let cfg = HealthConfig { startup_grace_secs: 0, ..HealthConfig::default() };
        let mut st = HealthState::new(cfg, true, t0);
        // 100 units of 95 ms spread over 10 s (95% busy), none anywhere near loop_lag_secs.
        for i in 1..=100u64 {
            st.record_work(t0 + Duration::from_millis(i * 100), Duration::from_millis(95));
        }
        let eval_at = t0 + secs(10) + Duration::from_millis(1);
        refresh(&mut st, eval_at);
        let alarms = st.evaluate(eval_at);
        assert_eq!(ids(&alarms), vec!["loop_overloaded"]);
        assert_eq!(alarms[0].severity, Severity::Warn);
    }

    #[test]
    fn mostly_idle_loop_is_not_overloaded() {
        let t0 = Instant::now();
        let cfg = HealthConfig { startup_grace_secs: 0, ..HealthConfig::default() };
        let mut st = HealthState::new(cfg, true, t0);
        // 100 units of 20 ms over 10 s: 20% busy.
        for i in 1..=100u64 {
            st.record_work(t0 + Duration::from_millis(i * 100), Duration::from_millis(20));
        }
        let eval_at = t0 + secs(10) + Duration::from_millis(1);
        refresh(&mut st, eval_at);
        assert!(st.evaluate(eval_at).is_empty());
    }

    #[test]
    fn overloaded_clears_when_load_drops() {
        let t0 = Instant::now();
        let cfg = HealthConfig { startup_grace_secs: 0, ..HealthConfig::default() };
        let mut st = HealthState::new(cfg, true, t0);
        for i in 1..=100u64 {
            st.record_work(t0 + Duration::from_millis(i * 100), Duration::from_millis(95));
        }
        // next 10 s window nearly idle
        for i in 101..=200u64 {
            st.record_work(t0 + Duration::from_millis(i * 100), Duration::from_millis(5));
        }
        let eval_at = t0 + secs(20) + Duration::from_millis(1);
        refresh(&mut st, eval_at);
        assert!(st.evaluate(eval_at).is_empty());
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
        let t = now + secs(59);
        st.record_frame(t);
        st.record_heartbeat(t);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, t);
        }
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, t);
        assert!(st.evaluate(t).is_empty());
        let later = now + secs(61);
        st.record_frame(later);
        st.record_heartbeat(later);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, later);
        }
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, later);
        assert_eq!(ids(&st.evaluate(later)), vec!["time_not_synced"]);
        st.record_time_sync(TimeSyncStatus::Synchronized, later);
        assert!(st.evaluate(later).is_empty());
    }

    #[test]
    fn flapping_time_sync_cannot_dodge_the_alarm() {
        // Skewed, one Synchronized blip, skewed again: the alarm measures time since the last
        // Synchronized observation, so flapping around the threshold cannot hide a bad clock.
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, now + secs(1));
        st.record_time_sync(TimeSyncStatus::Synchronized, now + secs(30));
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, now + secs(31));
        let t = now + secs(100); // 70 s after the last Synchronized observation
        st.record_frame(t);
        st.record_heartbeat(t);
        for s in [Stream::Position, Stream::CogSog, Stream::SystemTime] {
            st.record_stream(s, t);
        }
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, t);
        assert_eq!(ids(&st.evaluate(t)), vec!["time_not_synced"]);
    }

    #[test]
    fn time_not_synced_suppressed_when_bus_is_silent() {
        let t0 = Instant::now();
        let st = HealthState::new(HealthConfig::default(), true, t0);
        // Bus off from boot: can_silent (and stalled/streams) but not a second root-cause alarm.
        let mut st = st;
        let now = t0 + secs(100);
        st.record_heartbeat(now);
        assert_eq!(ids(&st.evaluate(now)), vec!["can_silent"]);
    }

    #[test]
    fn never_seen_required_stream_waits_for_grace_plus_timeout() {
        // GPS cold start: no alarm until grace (30 s) + required timeout (30 s) have passed.
        let t0 = Instant::now();
        let mut st = HealthState::new(HealthConfig::default(), true, t0);
        let t = t0 + secs(45);
        st.record_frame(t);
        st.record_heartbeat(t);
        st.record_time_sync(TimeSyncStatus::Synchronized, t);
        assert!(st.evaluate(t).is_empty());
        let t = t0 + secs(61);
        st.record_frame(t);
        st.record_heartbeat(t);
        st.record_time_sync(TimeSyncStatus::Synchronized, t);
        assert_eq!(
            ids(&st.evaluate(t)),
            vec!["stream_stale:position", "stream_stale:cog_sog", "stream_stale:system_time"]
        );
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
        let cfg = HealthConfig { startup_grace_secs: 0, ..HealthConfig::default() };
        let mut st = HealthState::new(cfg, true, t0 + secs(50));
        // Everything is recorded *after* the instant we evaluate at, so every age path
        // (heartbeat, frame, streams, db, lag, time) sees a recorded instant in the future.
        let future = t0 + secs(100);
        refresh(&mut st, future);
        st.record_work(future, Duration::from_secs(3));
        st.record_db_result(DbKind::Vessel, false, future);
        st.record_time_sync(TimeSyncStatus::TimeSkewDetected, future);
        let _ = st.evaluate(t0 + secs(60));
        let _ = st.report(t0 + secs(60));
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
        let later = now + Duration::from_secs(61);
        refresh(&mut st, later);
        let r = st.report(later);
        assert_eq!(r.status, HealthStatus::Down);
        assert_eq!(r.db["vessel"].consecutive_failures, 3);
    }

    #[test]
    fn report_status_down_when_nothing_is_being_captured() {
        // A required stream missing (no vessel status can be generated) is Down, not Degraded.
        let t0 = Instant::now();
        let mut st = HealthState::new(HealthConfig::default(), true, t0);
        let now = t0 + Duration::from_secs(100);
        st.record_frame(now);
        st.record_heartbeat(now);
        st.record_time_sync(TimeSyncStatus::Synchronized, now);
        st.record_stream(Stream::CogSog, now);
        st.record_stream(Stream::SystemTime, now);
        assert_eq!(st.report(now).status, HealthStatus::Down);
    }

    #[test]
    fn report_status_degraded_for_warn_only() {
        let (mut st, now) = healthy(HealthConfig::default());
        st.record_stream(Stream::Wind, now);
        let later = now + Duration::from_secs(61);
        refresh(&mut st, later);
        let r = st.report(later);
        assert_eq!(r.alarms.len(), 1);
        assert_eq!(r.status, HealthStatus::Degraded);
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
}
