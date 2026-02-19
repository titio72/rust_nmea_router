// SignalK v1.7.0 Message Types
// Implements delta format as specified in https://signalk.org/specification/1.7.0/doc/
//
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{time::{Instant}};

/// Get the full SignalK vessel context path
pub fn vessel_context(vessel_uuid: &str) -> String {
    format!("vessels.urn:mrn:signalk:uuid:{}", vessel_uuid)
}

/// Get the full vessel URN
pub fn vessel_urn(vessel_uuid: &str) -> String {
    format!("urn:mrn:signalk:uuid:{}", vessel_uuid)
}

/// SignalK Hello message - sent to clients upon connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKHello {
    /// Name of the SignalK server
    pub name: String,
    
    /// Version of the server software
    pub version: String,
    
    /// The vessel's SignalK context identifier
    #[serde(rename = "self")]
    pub vessel_self: String,
    
    /// Server roles (typically ["master", "main"])
    pub roles: Vec<String>,
    
    /// ISO 8601 timestamp when hello was sent
    pub timestamp: String,
}

impl SignalKHello {
    /// Create a new hello message with current timestamp
    pub fn new(vessel_uuid: &str) -> Self {
        
        let timestamp = crate::utilities::instant_to_rfc3339(Instant::now());
        
        SignalKHello {
            name: "nmea-router-signalk-server".to_string(),
            version: "0.1.0".to_string(),
            vessel_self: vessel_context(vessel_uuid),
            roles: vec!["master".to_string(), "main".to_string()],
            timestamp,
        }
    }
}

/// SignalK Delta message - the primary message format for real-time updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKDelta {
    /// Context path (e.g., "vessels.self" for own vessel)
    pub context: String,
    
    /// Array of update objects
    pub updates: Vec<SignalKUpdate>,
}

/// Update object containing source, timestamp, and values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKUpdate {
    /// Source of the data (NMEA2000 metadata)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SignalKSource>,
    
    /// ISO 8601 / RFC 3339 timestamp
    pub timestamp: String,
    
    /// Array of path-value pairs
    pub values: Vec<SignalKValue>,

    #[serde(rename = "$source")]
    pub source_ref: String,
}

/// Path-value pair for delta updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKValue {
    /// SignalK path (e.g., "navigation.position")
    pub path: String,
    
    /// Value (can be number, object, string, etc.)
    pub value: Value,
}

/// Source metadata for NMEA2000 data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalKSource {
    /// Human-readable label for the source
    pub label: String,
    
    /// Type of source (e.g., "NMEA2000")
    #[serde(rename = "type")]
    pub source_type: String,
    
    /// Source address on the bus
    pub src: String,
    
    /// PGN number for NMEA2000
    pub pgn: u32,

    #[serde(rename = "deviceInstance")]
    pub device_instance: u16,
}

impl SignalKSource {
    /// Create a new NMEA2000 source
    pub fn nmea2000(src: u8, pgn: u32) -> Self {
        Self {
            label: format!("N2K-{}", src),
            source_type: "NMEA2000".to_string(),
            src: src.to_string(),
            pgn,
            device_instance: 0,
        }
    }
}
