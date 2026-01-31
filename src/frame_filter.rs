use nmea2k::{Identifier, N2kMessage};
use crate::config::Config;

/// Filters NMEA2000 frames based on application configuration
/// Use this filter to implement filtering logic depending on the values in the full NMEA2000 message.
/// # Arguments
/// * `config` - Application configuration containing filter rules
/// * `n2k_message` - The NMEA2000 message to filter
/// 
/// # Returns
/// true if frame should be processed, false if it should be skipped
pub fn should_process_n2k_message(_config: &Config, _n2k_message: &N2kMessage) -> bool {
    true // Placeholder: implement message-specific filtering logic as needed
}

/// Filters NMEA2000 frames based on application configuration using only the Identifier
/// Use this filter early in the processing pipeline before full message assembly.
/// # Arguments
/// * `config` - Application configuration containing filter rules
/// * `id` - The NMEA2000 Identifier to filter
/// # Returns
/// true if frame should be processed, false if it should be skipped
pub fn should_process_frame_by_id(config: &Config, id: Identifier) -> bool {
    // Apply PGN filter - skip messages that don't match the configured PGNs
    config.source_filter.should_accept(id.pgn(), id.source())
}
