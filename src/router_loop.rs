// RouterLoop — owns all per-loop state and drives the NMEA2000 processing pipeline.
//
// `run()` is the infinite I/O loop (CAN frame read → assemble → process).
// `process_n2k_message()` is the business-logic entry point and is `pub(crate)` so
// tests can call it directly with synthetic N2kFrame values, bypassing the CAN socket.

use std::{
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tracing::{info, warn};

use socketcan::CanSocket;
use nmea2k::{CanBus, Identifier, MessageHandler, N2kFrame, N2kStreamReader};

use crate::app_metrics::{AppMetrics, MetricsLogger};
use crate::config::Config;
use crate::db::{HealthCheckManager, VesselDatabase, is_connection_error};
use crate::environmental_monitor::EnvironmentalMonitor;
use crate::environmental_status_handler::EnvironmentalStatusHandler;
use crate::frame_filter::{should_process_frame_by_id, should_process_n2k_message};
use crate::signalk_broadcaster::SignalKBroadcaster;
use crate::time_monitor::TimeSyncStatus;
use crate::udp_broadcaster::UdpBroadcaster;
use crate::vessel_monitor::VesselMonitor;
use crate::vessel_status_handler::VesselStatusHandler;

pub struct RouterLoop {
    // CAN I/O
    socket: CanSocket,
    // Message assembly
    reader: N2kStreamReader,
    // Static config (needed for frame/message filters)
    config: Config,
    // Monitors
    vessel_monitor: VesselMonitor,
    time_monitor: crate::time_monitor::TimeMonitor,
    env_monitor: EnvironmentalMonitor,
    // Persistence handlers
    vessel_status_handler: VesselStatusHandler,
    environmental_status_handler: EnvironmentalStatusHandler,
    // Broadcasters
    udp_broadcaster: UdpBroadcaster,
    signalk_broadcaster: Option<SignalKBroadcaster>,
    // Database
    vessel_db: Arc<RwLock<VesselDatabase>>,
    // Metrics & health
    pub(crate) metrics: AppMetrics,
    metrics_logger: MetricsLogger,
    db_health_check: HealthCheckManager,
}

impl RouterLoop {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        socket: CanSocket,
        reader: N2kStreamReader,
        config: Config,
        vessel_monitor: VesselMonitor,
        time_monitor: crate::time_monitor::TimeMonitor,
        env_monitor: EnvironmentalMonitor,
        vessel_status_handler: VesselStatusHandler,
        environmental_status_handler: EnvironmentalStatusHandler,
        udp_broadcaster: UdpBroadcaster,
        signalk_broadcaster: Option<SignalKBroadcaster>,
        vessel_db: Arc<RwLock<VesselDatabase>>,
        metrics: AppMetrics,
        metrics_logger: MetricsLogger,
        db_health_check: HealthCheckManager,
    ) -> Self {
        Self {
            socket,
            reader,
            config,
            vessel_monitor,
            time_monitor,
            env_monitor,
            vessel_status_handler,
            environmental_status_handler,
            udp_broadcaster,
            signalk_broadcaster,
            vessel_db,
            metrics,
            metrics_logger,
            db_health_check,
        }
    }

    /// Infinite event loop — reads CAN frames, assembles N2k messages, dispatches to
    /// `process_n2k_message`, and handles periodic tasks (metrics logging, DB health check).
    pub fn run(&mut self) -> ! {
        loop {
            match CanBus::read_nmea2k_frame(&self.socket) {
                Ok((extended_id, data)) => {
                    self.metrics.can_frames += 1;

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
                    }
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut
                    {
                        // Expected timeout — fall through to periodic tasks
                    } else {
                        self.metrics.can_errors += 1;
                        warn!("Error reading CAN frame: {}", e);
                        warn!("CAN bus connection lost. Attempting to reconnect...");

                        self.socket = CanBus::open_can_socket_with_retry(&self.config.can_interface);
                        if let Err(e) = CanBus::configure_nmea2k_socket(&mut self.socket) {
                            eprintln!("Fatal: Failed to configure CAN socket after reconnect: {}", e);
                            eprintln!("CAN interface: {}", self.config.can_interface);
                            std::process::exit(1);
                        }

                        info!("Reconnected to CAN bus. Resuming operation");
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
            }

            // Periodic tasks (run on every iteration, including timeouts)
            self.metrics_logger.check_and_log(&mut self.metrics);

            let db_url = self.config.database.connection.connection_url();
            match self.db_health_check.check_and_reconnect(&self.vessel_db, &db_url) {
                Ok(true) => {
                    // Reconnection occurred — reload trip state
                    let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
                    self.vessel_status_handler.load_last_trip(&db);
                }
                Ok(false) => {}
                Err(e) => {
                    eprintln!("Fatal: Database connection lost and reconnection failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }

    /// Route a fully-assembled N2k message through all monitors, handlers, and broadcasters.
    ///
    /// This is `pub(crate)` so test code can inject synthetic frames directly without
    /// going through the CAN socket. `now` must be passed by the caller — never call
    /// `Instant::now()` inside business logic.
    pub(crate) fn process_n2k_message(&mut self, frame: &N2kFrame, now: Instant) {
        self.time_monitor.handle_message(frame, now);
        self.udp_broadcaster.handle_message(frame, now);

        if self.is_signalk_enabled() {
            if let Some(ref mut sk) = self.signalk_broadcaster {
                sk.handle_message(frame, now);
            }
        }

        let sync = self.time_monitor.time_sync_status();
        self.metrics.gnss_time_skew = sync.skew;
        self.metrics.gnss_time_skew_status = sync.status;


        self.update_signalk_time_sync_status(&sync);

        if sync.status == TimeSyncStatus::Synchronized {
            if self.is_vessel_tracking_enabled() {
                self.vessel_monitor.handle_message(frame, now);
                if self.is_signalk_enabled() {
                    let moored = self.vessel_monitor.is_moored(now);
                    self.broadcast_mooring_status(moored, now);
                }
                if let Some(vessel_status) = self.vessel_monitor.generate_status(now) {
                    if vessel_status.is_valid() {
                        self.handle_vessel_status_write(vessel_status, now);
                    }
                }
            }

            if self.is_metrics_enabled() {
                self.env_monitor.handle_message(frame, now);
                self.handle_env_status_write(now);
            }
        } else {
            warn!("Skipping message processing - time not synchronized - skew {}", sync);
        }
    }

    
    // ── Private helpers ──────────────────────────────────────────────────────

    fn update_signalk_time_sync_status(&mut self, sync: &crate::time_monitor::TimeSyncStatusAndSkew) {
        if let Some(ref mut sk) = self.signalk_broadcaster {
            let status_str = match sync.status {
                TimeSyncStatus::Synchronized => "synced".to_string(),
                _ => "not_synced".to_string(),
            };            
            sk.update_time_sync_status(status_str, sync.skew);
        }
    }
    
    fn handle_vessel_status_write(
        &mut self,
        vessel_status: crate::vessel_monitor::VesselStatus,
        _now: Instant,
    ) {
        let db_url = self.config.database.connection.connection_url();
        let result = {
            let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
            self.vessel_status_handler.handle_vessel_status(&db, vessel_status.clone())
        };

        match result {
            Ok(true) => {
                self.metrics.vessel_reports += 1;
            }
            Ok(false) => {}
            Err(e) => {
                warn!("Database error during vessel status write: {}", e);
                if is_connection_error(e.as_ref()) {
                    if self.db_health_check.force_reconnect(&self.vessel_db, &db_url).is_ok() {
                        let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
                        match self.vessel_status_handler.handle_vessel_status(&db, vessel_status) {
                            Ok(true) => {
                                self.metrics.vessel_reports += 1;
                                info!("Vessel status retry succeeded after reconnection");
                            }
                            Ok(false) => {}
                            Err(e) => warn!("Vessel status retry failed: {}", e),
                        }
                    }
                }
            }
        }
    }

    fn handle_env_status_write(&mut self, now: Instant) {
        let db_url = self.config.database.connection.connection_url();
        let result = {
            let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
            self.environmental_status_handler
                .handle_environment_status(&db, &mut self.env_monitor, now)
        };

        match result {
            Ok(count) => self.metrics.env_reports += count as u64,
            Err(e) => {
                warn!("Database error during environmental write: {}", e);
                if is_connection_error(e.as_ref()) {
                    if self.db_health_check.force_reconnect(&self.vessel_db, &db_url).is_ok() {
                        let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
                        match self.environmental_status_handler
                            .handle_environment_status(&db, &mut self.env_monitor, now)
                        {
                            Ok(count) => {
                                self.metrics.env_reports += count as u64;
                                info!("Environmental status retry succeeded after reconnection");
                            }
                            Err(e) => warn!("Environmental status retry failed: {}", e),
                        }
                    }
                }
            }
        }
    }

    fn is_metrics_enabled(&self) -> bool {
        let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
        db.get_system_status("metrics_enabled").unwrap_or(false)
    }

    fn is_vessel_tracking_enabled(&self) -> bool {
        let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
        db.get_system_status("tracking_enabled").unwrap_or(false)
    }

    fn is_signalk_enabled(&self) -> bool {
        let db = self.vessel_db.read().unwrap_or_else(|e| e.into_inner());
        db.get_system_status("signalk_enabled").unwrap_or(false)
    }

    fn broadcast_mooring_status(&self, vessel_moored_status: bool, now: Instant) {
        use crate::web::signalk_messages::{SignalKDelta, SignalKUpdate, SignalKValue, vessel_context};
        let channels = crate::web::get_signalk_channels();
        let timestamp = crate::utilities::instant_to_rfc3339(now);
        let mooring_value: u8 = if vessel_moored_status { 1 } else { 0 };
        let delta = SignalKDelta {
            context: vessel_context(&self.config.signalk.vessel_uuid),
            updates: vec![SignalKUpdate {
                source: None,
                timestamp,
                values: vec![SignalKValue {
                    path: "vessel.mooringStatus".to_string(),
                    value: serde_json::json!(mooring_value),
                }],
                source_ref: "router.vesselStatus".to_string(),
            }],
        };
        let _ = channels.send(delta);
    }
}
