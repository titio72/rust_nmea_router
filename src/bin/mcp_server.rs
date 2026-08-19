#[path = "../utilities.rs"]
pub mod utilities;
#[path = "../position_utils.rs"]
pub mod position_utils;
#[path = "../config.rs"]
pub mod config;
#[path = "../trip.rs"]
pub mod trip;
#[path = "../environmental_monitor.rs"]
pub mod environmental_monitor;
#[path = "../db/mod.rs"]
pub mod db;
#[path = "../error.rs"]
pub mod error;

use chrono::{DateTime, Utc};
use db::VesselDatabase;
use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};

// ── Parameter structs ─────────────────────────────────────────────────────────

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ListTripsParams {
    year: Option<i32>,
    last_months: Option<u32>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TripIdParams {
    id: u32,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TripUuidParams {
    uuid: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct TimeRangeParams {
    trip_id: Option<u32>,
    #[schemars(description = "ISO-8601 UTC datetime, e.g. \"2024-06-01T00:00:00Z\"")]
    start: Option<String>,
    #[schemars(description = "ISO-8601 UTC datetime")]
    end: Option<String>,
    max_points: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetMetricsParams {
    #[schemars(description = "Metric name: wind_speed, wind_dir, roll, pressure, cabin_temp, water_temp, humidity")]
    metric: String,
    trip_id: Option<u32>,
    #[schemars(description = "ISO-8601 UTC datetime")]
    start: Option<String>,
    #[schemars(description = "ISO-8601 UTC datetime")]
    end: Option<String>,
    max_points: Option<usize>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct MonthlyStatsParams {
    year: Option<i32>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UpdateDescriptionParams {
    id: u32,
    description: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct SetNavWindowParams {
    trip_id: u32,
    leg_number: u32,
    #[schemars(description = "ISO-8601 UTC datetime for nav start; null to clear override")]
    nav_start: Option<String>,
    #[schemars(description = "ISO-8601 UTC datetime for nav end; null to clear override")]
    nav_end: Option<String>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct NavAnalysisParams {
    #[schemars(description = "Filter to a specific trip ID; omit to analyze all cached trips")]
    trip_id: Option<u32>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct FixMooringParams {
    #[schemars(description = "ISO-8601 UTC datetime, e.g. \"2024-06-01T22:20:00Z\"")]
    start: String,
    #[schemars(description = "ISO-8601 UTC datetime")]
    end: String,
    #[schemars(description = "true = mark the range moored (and resample it down to the moored reporting cadence); false = mark it underway")]
    is_moored: bool,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_dt(s: &str) -> Result<DateTime<Utc>, Box<dyn std::error::Error>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
        .map_err(|e| e.into())
}

fn parse_opt_dt(
    s: &Option<String>,
) -> Result<Option<DateTime<Utc>>, Box<dyn std::error::Error>> {
    s.as_ref().map(|s| parse_dt(s)).transpose()
}

fn to_json(data: impl serde::Serialize) -> Result<CallToolResult, McpError> {
    match serde_json::to_string(&data) {
        Ok(json) => Ok(CallToolResult::success(vec![Content::text(json)])),
        Err(e) => Err(McpError::internal_error(e.to_string(), None)),
    }
}

fn db_err(e: impl std::fmt::Display) -> McpError {
    McpError::internal_error(e.to_string(), None)
}

// ── Server ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct NmeaRouterMcp {
    db: VesselDatabase,
    moored_interval_secs: u64,
    #[allow(dead_code)]
    tool_router: ToolRouter<NmeaRouterMcp>,
}

#[tool_router]
impl NmeaRouterMcp {
    fn new(db: VesselDatabase, moored_interval_secs: u64) -> Self {
        Self {
            db,
            moored_interval_secs,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List trips, optionally filtered by year or last N months")]
    async fn list_trips(
        &self,
        Parameters(ListTripsParams { year, last_months }): Parameters<ListTripsParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_trips(year, last_months).map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get a single trip by numeric ID")]
    async fn get_trip(
        &self,
        Parameters(TripIdParams { id }): Parameters<TripIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || db.fetch_trip(id).map_err(|e| e.to_string()))
            .await
            .map_err(db_err)?
            .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get a single trip by its UUID")]
    async fn get_trip_by_uuid(
        &self,
        Parameters(TripUuidParams { uuid }): Parameters<TripUuidParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_trip_by_uuid(&uuid).map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get leg breakdown for a trip. Cached for closed trips (ended >24h ago). Legs shorter than 0.5 nm are excluded.")]
    async fn get_trip_legs(
        &self,
        Parameters(TripIdParams { id }): Parameters<TripIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let data =
            tokio::task::spawn_blocking(move || db.fetch_trip_legs(id).map_err(|e| e.to_string()))
                .await
                .map_err(db_err)?
                .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get vessel track points for a trip or time range. max_points downsamples the result.")]
    async fn get_track(
        &self,
        Parameters(TimeRangeParams {
            trip_id,
            start,
            end,
            max_points,
        }): Parameters<TimeRangeParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_opt_dt(&start).map_err(db_err)?;
        let end = parse_opt_dt(&end).map_err(db_err)?;
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_track(trip_id, start, end, max_points)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get environmental time-series. metric: wind_speed, wind_dir, roll, pressure, cabin_temp, water_temp, humidity")]
    async fn get_metrics(
        &self,
        Parameters(GetMetricsParams {
            metric,
            trip_id,
            start,
            end,
            max_points,
        }): Parameters<GetMetricsParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_opt_dt(&start).map_err(db_err)?;
        let end = parse_opt_dt(&end).map_err(db_err)?;
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_metrics(&metric, trip_id, start, end, max_points)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get speed distribution histogram split by sailing vs motoring mode")]
    async fn get_speed_distribution(
        &self,
        Parameters(TimeRangeParams {
            trip_id,
            start,
            end,
            ..
        }): Parameters<TimeRangeParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_opt_dt(&start).map_err(db_err)?;
        let end = parse_opt_dt(&end).map_err(db_err)?;
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_speed_distribution(trip_id, start, end)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get wind rose statistics (72 direction buckets, 5° each) for a trip or time range")]
    async fn get_wind_statistics(
        &self,
        Parameters(TimeRangeParams {
            trip_id,
            start,
            end,
            ..
        }): Parameters<TimeRangeParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_opt_dt(&start).map_err(db_err)?;
        let end = parse_opt_dt(&end).map_err(db_err)?;
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_wind_statistics(trip_id, start, end)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Get monthly sailing and motoring distance summary, optionally filtered to a specific year")]
    async fn get_monthly_statistics(
        &self,
        Parameters(MonthlyStatsParams { year }): Parameters<MonthlyStatsParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let mut data =
            tokio::task::spawn_blocking(move || db.fetch_monthly_statistics().map_err(|e| e.to_string()))
                .await
                .map_err(db_err)?
                .map_err(db_err)?;
        if let Some(y) = year {
            data.months.retain(|m| m.year == y);
        }
        to_json(data)
    }

    #[tool(description = "Trim a trip: removes moored padding at start/end, keeps a 1-hour buffer, recalculates aggregates, and invalidates all caches atomically")]
    async fn trim_trip(
        &self,
        Parameters(TripIdParams { id }): Parameters<TripIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || db.trim_trip(id).map_err(|e| e.to_string()))
            .await
            .map_err(db_err)?
            .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Delete a trip and all associated vessel_status, environmental_data, and cache rows atomically")]
    async fn delete_trip(
        &self,
        Parameters(TripIdParams { id }): Parameters<TripIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || db.delete_trip(id).map_err(|e| e.to_string()))
            .await
            .map_err(db_err)?
            .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Update the description (name) of a trip")]
    async fn update_trip_description(
        &self,
        Parameters(UpdateDescriptionParams { id, description }): Parameters<UpdateDescriptionParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            db.update_trip_description(id as i64, &description)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Invalidate the legs cache for a trip, forcing recomputation on the next fetch")]
    async fn invalidate_trip_legs(
        &self,
        Parameters(TripIdParams { id }): Parameters<TripIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            db.invalidate_trip_legs_cache(id)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Override the automatically detected navigation window for a specific leg. The algorithm's original detection is preserved for calibration. Pass null for both nav_start and nav_end to clear a previous override.")]
    async fn set_nav_window(
        &self,
        Parameters(SetNavWindowParams {
            trip_id,
            leg_number,
            nav_start,
            nav_end,
        }): Parameters<SetNavWindowParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let current = tokio::task::spawn_blocking(move || {
            db.fetch_trip_legs(trip_id).map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;

        let leg = current.legs.iter().find(|l| l.leg_number == leg_number);
        let auto_start = leg.and_then(|l| l.nav_start_timestamp.clone());
        let auto_end = leg.and_then(|l| l.nav_end_timestamp.clone());

        let db2 = self.db.clone();
        tokio::task::spawn_blocking(move || {
            db2.set_nav_override(
                trip_id,
                leg_number,
                nav_start.as_deref(),
                nav_end.as_deref(),
                auto_start.as_deref(),
                auto_end.as_deref(),
            )
            .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(description = "Analyze nav window detection quality across cached trip legs. Returns per-leg windows, trimmed marina durations, and detection method. Omit trip_id to analyze all cached trips.")]
    async fn get_nav_analysis(
        &self,
        Parameters(NavAnalysisParams { trip_id }): Parameters<NavAnalysisParams>,
    ) -> Result<CallToolResult, McpError> {
        let db = self.db.clone();
        let data = tokio::task::spawn_blocking(move || {
            db.fetch_nav_analysis(trip_id).map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }

    #[tool(description = "Fix a mislabeled mooring period: set is_moored for every vessel_status row in [start, end] to the given value. When set to true, also resamples the (now dense, underway-cadence) rows in that window down to the moored reporting cadence, recomputes the covering trip's aggregate totals, and invalidates its trip_legs_cache/heatmap_cache entries. Errors if no trip covers the window or no rows are found in range.")]
    async fn fix_mooring_status(
        &self,
        Parameters(FixMooringParams { start, end, is_moored }): Parameters<FixMooringParams>,
    ) -> Result<CallToolResult, McpError> {
        let start = parse_dt(&start).map_err(db_err)?;
        let end = parse_dt(&end).map_err(db_err)?;
        let db = self.db.clone();
        let moored_interval_secs = self.moored_interval_secs;
        let data = tokio::task::spawn_blocking(move || {
            db.fix_mooring_status(start, end, is_moored, moored_interval_secs)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(db_err)?
        .map_err(db_err)?;
        to_json(data)
    }
}

#[tool_handler]
impl ServerHandler for NmeaRouterMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr) // stdout is reserved for the MCP JSON-RPC wire
        .with_max_level(tracing::Level::WARN)
        .init();

    let config_path = parse_config_arg();
    let cfg = config::Config::load_for_context(config_path.as_deref())
        .unwrap_or_else(|e| {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        });

    let conn_url = cfg.database.connection.connection_url();
    let db = VesselDatabase::new(
        &conn_url,
        cfg.database.connection.pool_min,
        cfg.database.connection.pool_max,
    )
    .unwrap_or_else(|e| {
        eprintln!("DB error: {e}");
        std::process::exit(1);
    });

    let moored_interval_secs = cfg.database.vessel_status.interval_moored_seconds;
    let transport = rmcp::transport::stdio();
    NmeaRouterMcp::new(db, moored_interval_secs)
        .serve(transport)
        .await
        .expect("MCP serve failed")
        .waiting()
        .await
        .expect("MCP server run failed");
}

fn parse_config_arg() -> Option<String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i + 1 < args.len() {
        if args[i] == "--config" {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}
