use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use crate::db::VesselDatabase;
use crate::environmental_monitor::{EnvironmentalMonitor, MetricId};

/// State for tracking environmental metric persistence
struct EnvironmentalStatusState {
    last_db_persist: HashMap<MetricId, Instant>,
    db_periods: [Duration; 7],
}

impl EnvironmentalStatusState {
    fn new(db_periods: [Duration; 7]) -> Self {
        Self {
            last_db_persist: HashMap::new(),
            db_periods,
        }
    }

    /// Get the list of metrics that should be persisted to the database now
    fn get_metrics_to_persist(&self, env_monitor: &EnvironmentalMonitor) -> Vec<MetricId> {
        let now = Instant::now();
        let mut metrics_to_persist = Vec::new();
        
        for metricid in MetricId::ALL_METRICS.iter() {
            if env_monitor.has_samples(*metricid) {
                let interval = self.db_periods[metricid.as_index()];
                if let Some(last_persist) = self.last_db_persist.get(&metricid) {
                    if now.duration_since(*last_persist) >= interval {
                        metrics_to_persist.push(*metricid);
                    }
                } else {
                    // Never persisted before, should persist now
                    metrics_to_persist.push(*metricid);
                }
            }
        }
        
        metrics_to_persist
    }

    /// Mark specific metrics as persisted to the database
    fn mark_metric_persisted(&mut self, metric: MetricId) {
        let now = Instant::now();
        self.last_db_persist.insert(metric, now);
    }
}

/// Handler for environmental status reporting and persistence
pub struct EnvironmentalStatusHandler {
    state: EnvironmentalStatusState,
}

impl EnvironmentalStatusHandler {
    pub fn new(db_periods: [Duration; 7]) -> Self {
        Self {
            state: EnvironmentalStatusState::new(db_periods),
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
        let metrics_to_persist = state.get_metrics_to_persist(env_monitor);
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
                        state.mark_metric_persisted(*metricid);
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
        let periods = [
            db_periods.wind_speed_interval(),
            db_periods.wind_direction_interval(),
            db_periods.roll_interval(),
            db_periods.pressure_interval(),
            db_periods.cabin_temp_interval(),
            db_periods.water_temp_interval(),
            db_periods.humidity_interval(),
        ];
        let mut state = EnvironmentalStatusState::new(periods);
        
        state.mark_metric_persisted(MetricId::Pressure);
        state.mark_metric_persisted(MetricId::CabinTemp);
        
        assert!(state.last_db_persist.contains_key(&MetricId::Pressure));
        assert!(state.last_db_persist.contains_key(&MetricId::CabinTemp));
        assert!(!state.last_db_persist.contains_key(&MetricId::WindSpeed));
    }

    #[test]
    fn test_get_metrics_to_persist_initial() {
        let config = EnvironmentalConfig::default();
        let monitor = EnvironmentalMonitor::new(config.clone());
        let periods = monitor.db_periods();
        let state = EnvironmentalStatusState::new(periods);
        
        // Initially, no metrics have data, so nothing to persist
        let metrics = state.get_metrics_to_persist(&monitor);
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn test_get_metrics_to_persist_with_data() {
        let config = EnvironmentalConfig::default();
        let mut monitor = EnvironmentalMonitor::new(config.clone());
        let periods = monitor.db_periods();
        let state = EnvironmentalStatusState::new(periods);

        // Add dummy data for all metrics
        let now = Instant::now();
        for samples in monitor.data_samples.iter_mut() {
             samples.push_back(Sample { value: 10.0, timestamp: now });
        }
        
        // Now all 7 should be ready as they have data and haven't been persisted
        let metrics = state.get_metrics_to_persist(&monitor);
        assert_eq!(metrics.len(), 7);
    }
}
