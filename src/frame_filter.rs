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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SourceFilterConfig;
    use std::collections::HashMap;
    use socketcan::ExtendedId;

    /// Helper function to create a test config with source filter
    fn create_test_config(pgn_source_map: HashMap<u32, u8>) -> Config {
        let mut config = Config::default();
        config.source_filter.pgn_source_map = pgn_source_map;
        config
    }

    /// Helper function to create an Identifier with specific PGN and source
    /// CAN ID format: (priority << 26) | (pgn << 8) | source
    fn create_identifier(pgn: u32, source: u8) -> Identifier {
        // Create a 29-bit CAN ID
        let priority = 6u32;
        let can_id = (priority << 26) | (pgn << 8) | (source as u32);
        
        // Create ExtendedId and convert to Identifier
        let extended_id = ExtendedId::new(can_id).expect("Valid extended ID");
        Identifier::from_can_id(extended_id)
    }

    #[test]
    fn test_should_process_n2k_message_always_true() {
        // The placeholder implementation always returns true
        let config = Config::default();
        let n2k_message = N2kMessage::from_pgn(129025, &[0x00; 8]);
        
        assert!(should_process_n2k_message(&config, &n2k_message));
    }

    #[test]
    fn test_should_process_frame_by_id_no_filter() {
        // With no PGN filters configured, all frames should be accepted
        let config = Config::default();
        
        let id1 = create_identifier(129025, 10);
        let id2 = create_identifier(127488, 5);
        let id3 = create_identifier(128259, 0);
        
        assert!(should_process_frame_by_id(&config, id1));
        assert!(should_process_frame_by_id(&config, id2));
        assert!(should_process_frame_by_id(&config, id3));
    }

    #[test]
    fn test_should_process_frame_by_id_with_specific_filter() {
        // With specific PGN and source filters
        let mut pgn_source_map = HashMap::new();
        pgn_source_map.insert(129025, 22);
        pgn_source_map.insert(127488, 5);
        
        let config = create_test_config(pgn_source_map);
        
        // Matching PGN and source - should accept
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 22)));
        assert!(should_process_frame_by_id(&config, create_identifier(127488, 5)));
        
        // Matching PGN but different source - should reject
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 10)));
        assert!(!should_process_frame_by_id(&config, create_identifier(127488, 10)));
        
        // Different PGN not in map - should accept (no filter for this PGN)
        assert!(should_process_frame_by_id(&config, create_identifier(128259, 0)));
    }

    #[test]
    fn test_should_process_frame_by_id_multiple_sources_same_pgn() {
        // Test filtering with only PGN-level filtering using source 255 (or any specific source for that PGN)
        let mut pgn_source_map = HashMap::new();
        pgn_source_map.insert(129025, 22);  // Accept only source 22 for this PGN
        
        let config = create_test_config(pgn_source_map);
        
        // Only source 22 for PGN 129025 should be accepted
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 22)));
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 0)));
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 10)));
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 254)));
        
        // Different PGN not in map should be accepted (no filter for it)
        assert!(should_process_frame_by_id(&config, create_identifier(127488, 0)));
    }

    #[test]
    fn test_should_process_frame_by_id_empty_pgn_source_map() {
        // With empty PGN map, all frames should be accepted (no filters configured)
        let pgn_source_map = HashMap::new();
        let config = create_test_config(pgn_source_map);
        
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 10)));
        assert!(should_process_frame_by_id(&config, create_identifier(127488, 5)));
        assert!(should_process_frame_by_id(&config, create_identifier(128259, 0)));
    }

    #[test]
    fn test_should_process_frame_by_id_various_pgn_values() {
        // Test with various valid PGN values
        let mut pgn_source_map = HashMap::new();
        pgn_source_map.insert(60928, 0);   // System Time
        pgn_source_map.insert(129025, 1);  // Position Rapid Update
        pgn_source_map.insert(130306, 2);  // Wind Data
        pgn_source_map.insert(127245, 3);  // Rudder
        
        let config = create_test_config(pgn_source_map);
        
        // All configured PGNs with correct sources should be accepted
        assert!(should_process_frame_by_id(&config, create_identifier(60928, 0)));
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 1)));
        assert!(should_process_frame_by_id(&config, create_identifier(130306, 2)));
        assert!(should_process_frame_by_id(&config, create_identifier(127245, 3)));
        
        // Same PGNs but wrong sources should be rejected
        assert!(!should_process_frame_by_id(&config, create_identifier(60928, 1)));
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 0)));
        assert!(!should_process_frame_by_id(&config, create_identifier(130306, 10)));
        assert!(!should_process_frame_by_id(&config, create_identifier(127245, 5)));
    }

    #[test]
    fn test_should_process_frame_by_id_boundary_sources() {
        // Test with boundary source values (0 and 254)
        let mut pgn_source_map = HashMap::new();
        pgn_source_map.insert(129025, 0);    // Source 0
        pgn_source_map.insert(127488, 254);  // Source 254 (near max)
        
        let config = create_test_config(pgn_source_map);
        
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 0)));
        assert!(should_process_frame_by_id(&config, create_identifier(127488, 254)));
        
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 254)));
        assert!(!should_process_frame_by_id(&config, create_identifier(127488, 0)));
    }

    #[test]
    fn test_should_process_frame_by_id_source_filter_config() {
        // Test using SourceFilterConfig methods directly
        let mut filter = SourceFilterConfig::default();
        filter.pgn_source_map.insert(129025, 22);
        filter.pgn_source_map.insert(127488, 5);
        
        let config = create_test_config(filter.pgn_source_map);
        
        // Verify filtering works correctly with configured sources
        assert!(should_process_frame_by_id(&config, create_identifier(129025, 22)));
        assert!(should_process_frame_by_id(&config, create_identifier(127488, 5)));
        assert!(!should_process_frame_by_id(&config, create_identifier(129025, 10)));
        assert!(!should_process_frame_by_id(&config, create_identifier(127488, 10)));
    }
}
