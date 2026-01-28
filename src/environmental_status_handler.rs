use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use crate::config::EnvironmentalConfig;
use crate::db::VesselDatabase;
use crate::environmental_monitor::{EnvironmentalMonitor, MetricId};

/// State for tracking environmental metric persistence
struct EnvironmentalStatusState {
    timing: HashMap<MetricId, Instant>,
    config: EnvironmentalConfig,
}

fn get_period(config: &EnvironmentalConfig, metric: MetricId) -> Duration {
    match metric {
        MetricId::WindSpeed => config.wind_speed_interval(),
        MetricId::WindDir => config.wind_direction_interval(),
        MetricId::Roll => config.roll_interval(),
        MetricId::Pressure => config.pressure_interval(),
        MetricId::CabinTemp => config.cabin_temp_interval(),
        MetricId::WaterTemp => config.water_temp_interval(),
        MetricId::Humidity => config.humidity_interval(),
    }
}

impl EnvironmentalStatusState {
    /// Create a new EnvironmentalStatusState with initial timing based on config
    fn new(environmental_config: &EnvironmentalConfig) -> Self {
        let mut x = Self {
            timing: HashMap::new(),
            config: environmental_config.clone(),
        };
        let now = Instant::now();
        x.timing.insert(
            MetricId::WindSpeed,
            now.checked_sub(get_period(&environmental_config, MetricId::WindSpeed)).unwrap(),
        );
        x.timing.insert(
            MetricId::WindDir,
            now.checked_sub(get_period(&environmental_config, MetricId::WindDir)).unwrap(),
        );
        x.timing.insert(
            MetricId::Roll,
            now.checked_sub(get_period(&environmental_config, MetricId::Roll)).unwrap(),
        );
        x.timing.insert(
            MetricId::Pressure,
            now.checked_sub(get_period(&environmental_config, MetricId::Pressure)).unwrap(),    
        );
        x.timing.insert(
            MetricId::CabinTemp,
            now.checked_sub(get_period(&environmental_config, MetricId::CabinTemp)).unwrap(),
        );
        x.timing.insert(
            MetricId::WaterTemp,
            now.checked_sub(get_period(&environmental_config, MetricId::WaterTemp)).unwrap(),
        );
        x.timing.insert(
            MetricId::Humidity,
            now.checked_sub(get_period(&environmental_config, MetricId::Humidity)).unwrap(),
        );
        x
    }

    /// Get the list of metrics that should be persisted to the database now
    fn get_metrics_to_persist(&self, env_monitor: &EnvironmentalMonitor, now: Instant) -> Vec<MetricId> {
        let mut metrics_to_persist = Vec::new();
        
        for metricid in MetricId::ALL_METRICS.iter() {
            let last_persist = self.timing.get(metricid).unwrap();
            if now.duration_since(*last_persist) >= get_period(&self.config, *metricid) && env_monitor.has_samples(*metricid) {
                metrics_to_persist.push(*metricid);
            }
        }
        metrics_to_persist
    }

    /// Mark specific metrics as persisted to the database
    fn mark_metric_persisted(&mut self, metric: MetricId, now: Instant) {
        *self.timing.get_mut(&metric).unwrap() = now;
    }
}

/// Handler for environmental status reporting and persistence
pub struct EnvironmentalStatusHandler {
    state: EnvironmentalStatusState,
}

impl EnvironmentalStatusHandler {
    pub fn new(environmental_config: &EnvironmentalConfig) -> Self {
        Self {
            state: EnvironmentalStatusState::new(environmental_config),
        }
    }

    /// Handle environmental status reporting and persistence
    /// Returns Ok(count) with the number of environmental metrics written to the database
    /// Returns Err if there was a database error
    pub fn handle_environment_status(
        &mut self,
        vessel_db: &Option<VesselDatabase>,
        env_monitor: &mut EnvironmentalMonitor,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        handle_environment_status(vessel_db, env_monitor, &mut self.state)
    }
}

/// Handles environmental status persistence to the database
/// 
/// This function processes environmental metrics and writes them to the database
/// when conditions are met (database connected, time synchronized, metrics ready).
fn handle_environment_status(
    vessel_db: &Option<VesselDatabase>,
    env_monitor: &mut EnvironmentalMonitor,
    state: &mut EnvironmentalStatusState,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut written_count = 0;
    // Write to database if connected, time to persist, and time is synchronized
    if let Some(ref db) = *vessel_db {
        let now = Instant::now();
        let metrics_to_persist = state.get_metrics_to_persist(env_monitor, now);
        if !metrics_to_persist.is_empty() {
            for metricid in metrics_to_persist.iter() {
                debug!("Persisting environmental metric: {}", metricid.name());
                let data = env_monitor.calculate_metric_data(*metricid);
                if let Some(metric_data) = data {
                    debug!("Metric Data for {}: avg={:?}, max={:?}, min={:?}, count={:?}", 
                        metricid.name(), 
                        metric_data.avg, 
                        metric_data.max, 
                        metric_data.min,
                        metric_data.count);
                    if let Err(e) = db.insert_environmental_metrics(&metric_data, *metricid) {
                        warn!("Error writing {} data to database: {}", metricid.name(), e);
                        return Err(e);
                    } else {
                        state.mark_metric_persisted(*metricid, now);
                        env_monitor.cleanup_all_samples(*metricid);
                        debug!("Environmental metric {} written to database", metricid.name());
                        written_count += 1;
                    }
                } else {
                    debug!("No data available for metric: {}", metricid.name());
                }
            }
        }
    }
    Ok(written_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environmental_monitor::{EnvironmentalMonitor, MetricId, Sample};
    use crate::config::EnvironmentalConfig;
    use std::time::Instant;

    #[test]
    fn test_mark_metric_persisted() {
        let db_periods = EnvironmentalConfig::default();
        let mut state = EnvironmentalStatusState::new(db_periods);
        
        let now = Instant::now();   
        state.mark_metric_persisted(MetricId::Pressure, now);
        state.mark_metric_persisted(MetricId::CabinTemp, now);
        
        assert!(*state.timing.get(&MetricId::Pressure).unwrap()==now);
        assert!(*state.timing.get(&MetricId::CabinTemp).unwrap()==now);
        assert!(*state.timing.get(&MetricId::WindSpeed).unwrap()<now);
    }

    #[test]
    fn test_get_metrics_to_persist_initial() {
        let config = EnvironmentalConfig::default();
        let monitor = EnvironmentalMonitor::new();
        let state = EnvironmentalStatusState::new(config);
        
        // Initially, no metrics have data, so nothing to persist
        let metrics = state.get_metrics_to_persist(&monitor, Instant::now());
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn test_get_metrics_to_persist_with_data() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new();
        let state = EnvironmentalStatusState::new(config);

        // Add dummy data for all metrics
        let now = Instant::now();
        for samples in monitor.data_samples.iter_mut() {
             samples.push_back(Sample { value: 10.0, timestamp: now });
        }
        
        // Now all 7 should be ready as they have data and haven't been persisted
        let metrics = state.get_metrics_to_persist(&monitor, now.checked_add(Duration::from_secs(600)).unwrap());
        assert_eq!(metrics.len(), 7);
    }
}
