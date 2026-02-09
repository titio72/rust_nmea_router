// Database module - provides data access abstraction layer
// Organized into focused submodules for maintainability

pub mod types;
pub mod connection;
pub mod operations;

// Re-export main types and the connection manager
pub use types::{
    VesselDatabase, VesselStatusOperation, TripOperation, HealthCheckManager,
    // Response types
    TripSummary, TrackPoint, WebMetricData, SpeedDistributionData, WindStatisticsData,
    TripLegsData, HeatmapData, TrackAnalytics, MonthlyStatistics,
};

// Operations are available through the operations module if needed
