// Database module - provides data access abstraction layer
// Organized into focused submodules for maintainability

pub mod types;
pub mod connection;
pub mod operations;

// Test helpers - only available in test builds
#[cfg(test)]
pub mod test_helpers;

#[cfg(test)]
mod test_examples;

// Re-export main types and the connection manager
pub use types::{
    VesselDatabase, VesselStatusOperation, TripOperation, HealthCheckManager,
    is_connection_error,
    // Response types
    TripSummary, TrackPoint, WebMetricData, MultiMetricData, SpeedDistributionData, WindStatisticsData,
    TripLegsData, HeatmapData, TrackAnalytics, MonthlyStatistics,
};

// Operations are available through the operations module if needed
