use nmea2k::{Identifier, N2kFrame};
use crate::config::Config;

/// Filters NMEA2000 frames based on application configuration
/// 
/// # Arguments
/// * `config` - Application configuration containing filter rules
/// * `n2k_frame` - The NMEA2000 frame to filter
/// 
/// # Returns
/// true if frame should be processed, false if it should be skipped
pub fn should_process_frame(config: &Config, n2k_frame: &N2kFrame) -> bool {
    let pgn = n2k_frame.identifier.pgn();
    let source = n2k_frame.identifier.source();
                    
    // Apply source filter - skip messages that don't match the configured source
    config.source_filter.should_accept(pgn, source)
}

pub fn should_process_frame_by_id(config: &Config, id: Identifier) -> bool {
    // Apply PGN filter - skip messages that don't match the configured PGNs
    config.source_filter.should_accept(id.pgn(), id.source())
}
