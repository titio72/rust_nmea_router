// SignalK Delta Broadcaster
// Converts NMEA2000 messages to SignalK v1.7.0 delta format and broadcasts via WebSocket
// For architectural guidance and coding conventions, see: AGENTS.md
//
use std::time::{Instant, Duration};
use nmea2k::{MessageHandler, N2kFrame};
use nmea2k::pgns::{N2kMessage, HeadingReference};
use crate::position_utils::Position;
use crate::web::signalk_messages::{
    SignalKDelta, SignalKUpdate, SignalKValue, SignalKSource,
    vessel_context
};
use crate::web::get_signalk_channels;
use crate::utilities::calculate_true_wind;
use serde_json::json;

/// Broadcasts NMEA2000 messages as SignalK delta messages via WebSocket
/// Converts native NMEA2000 units (already SI in most cases) to SignalK paths
/// Rate-limits each SignalK path independently
pub struct SignalKBroadcaster {
    /// Vessel UUID for context path
    vessel_uuid: String,
    
    /// Cached SOG in m/s for true wind calculations
    last_sog_ms: Option<f64>,
    last_sog_timestamp: Option<Instant>,

    /// Cached time sync status string ("synced" or "not_synced")
    last_time_sync_status: Option<String>,
    
    /// Cached time skew in milliseconds
    last_time_skew_ms: Option<i64>,
    
    /// Rate limiting interval in milliseconds
    rate_limit_ms: u64,
    
    // Rate limiting: track last broadcast time for each SignalK path
    last_depth_broadcast: Option<Instant>,
    last_position_broadcast: Option<Instant>,
    last_sog_broadcast: Option<Instant>,
    last_stw_broadcast: Option<Instant>,
    last_cog_broadcast: Option<Instant>,
    last_heading_broadcast: Option<Instant>,
    last_wind_speed_true_broadcast: Option<Instant>,
    last_wind_angle_true_broadcast: Option<Instant>,
    last_wind_speed_apparent_broadcast: Option<Instant>,
    last_wind_angle_apparent_broadcast: Option<Instant>,
    last_temperature_broadcast: Option<Instant>,
    last_humidity_broadcast: Option<Instant>,
    last_pressure_broadcast: Option<Instant>,
    last_datetime_broadcast: Option<Instant>,
    last_timesync_broadcast: Option<Instant>,
    last_roll_broadcast: Option<Instant>,

    last_position: Option<Position>,
}

impl SignalKBroadcaster {
    pub fn new(rate_limit_ms: u64, vessel_uuid: String) -> Self {
        Self {
            vessel_uuid,
            last_sog_ms: None,
            last_sog_timestamp: None,
            last_time_sync_status: None,
            last_time_skew_ms: None,
            rate_limit_ms,
            last_depth_broadcast: None,
            last_position_broadcast: None,
            last_sog_broadcast: None,
            last_stw_broadcast: None,
            last_cog_broadcast: None,
            last_heading_broadcast: None,
            last_wind_speed_true_broadcast: None,
            last_wind_angle_true_broadcast: None,
            last_wind_speed_apparent_broadcast: None,
            last_wind_angle_apparent_broadcast: None,
            last_temperature_broadcast: None,
            last_humidity_broadcast: None,
            last_pressure_broadcast: None,
            last_datetime_broadcast: None,
            last_timesync_broadcast: None,
            last_roll_broadcast: None,
            last_position: None,
        }
    }
    
    /// Update the cached time sync status and skew
    /// This should be called from the main loop with the latest values from TimeMonitor
    pub fn update_time_sync_status(&mut self, status: String, skew_ms: i64) {
        self.last_time_sync_status = Some(status);
        self.last_time_skew_ms = Some(skew_ms);
    }
    
    /// Check if enough time has passed to broadcast this path
    fn should_broadcast(&self, last_broadcast: Option<Instant>, now: Instant) -> bool {
        match last_broadcast {
            None => true,
            Some(last) => now.duration_since(last) >= Duration::from_millis(self.rate_limit_ms),
        }
    }
    
    /// Send a delta message with multiple values
    fn send_delta(&self, source: SignalKSource, timestamp: String, values: Vec<SignalKValue>) {
        let channels = get_signalk_channels();
        
        let source_ref = format!("{}.{}", &source.label, &source.src);
        
        let delta = SignalKDelta {
            context: vessel_context(&self.vessel_uuid),
            updates: vec![SignalKUpdate {
                source: Some(source),
                timestamp,
                values,
                source_ref,
            }],
        };
        
        // Silently ignore send errors - these occur when there are no subscribers,
        // which is normal during operation when no clients are connected
        let _ = channels.send(delta);
    }

    /// Send a delta message with a custom context (for AIS targets)
    /// AIS targets use context: "vessels.urn:mrn:imo:mmsi:<MMSI>"
    fn send_ais_delta(&self, mmsi: u32, source: SignalKSource, timestamp: String, values: Vec<SignalKValue>) {
        let channels = get_signalk_channels();
        
        let source_ref = format!("{}.{}", &source.label, &source.src);
        let ais_context = format!("vessels.urn:mrn:imo:mmsi:{}", mmsi);
        
        let delta = SignalKDelta {
            context: ais_context,
            updates: vec![SignalKUpdate {
                source: Some(source),
                timestamp,
                values,
                source_ref,
            }],
        };
        
        // Silently ignore send errors
        let _ = channels.send(delta);
    }
}

impl MessageHandler for SignalKBroadcaster {
    fn handle_message(&mut self, frame: &N2kFrame, now: std::time::Instant) {
        let timestamp = crate::utilities::instant_to_rfc3339(now);
        let source = SignalKSource::nmea2000(frame.identifier.source(), frame.identifier.pgn());
        
        match &frame.message {
            // Position data - PGN 129025
            // SignalK: navigation.position with {latitude, longitude} in decimal degrees
            N2kMessage::PositionRapidUpdate(pos) => {
                self.last_position = Some(Position {
                    latitude: pos.latitude,
                    longitude: pos.longitude,
                });
                if self.should_broadcast(self.last_position_broadcast, now) {
                    let values = vec![SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": pos.latitude,
                            "longitude": pos.longitude,
                        }),
                    }];
                    
                    self.send_delta(source, timestamp, values);
                    self.last_position_broadcast = Some(now);
                }
            },
            
            // COG/SOG data - PGN 129026
            // SignalK: navigation.speedOverGround (m/s), navigation.courseOverGroundTrue (radians)
            N2kMessage::CogSogRapidUpdate(cog_sog) => {
                // Cache SOG for true wind calculations (store in m/s)
                self.last_sog_ms = Some(cog_sog.sog);
                self.last_sog_timestamp = Some(now);
                
                let mut values = Vec::new();
                
                // Speed over ground - use raw field (m/s)
                if self.should_broadcast(self.last_sog_broadcast, now) {
                    values.push(SignalKValue {
                        path: "navigation.speedOverGround".to_string(),
                        value: json!(cog_sog.sog),
                    });
                    self.last_sog_broadcast = Some(now);
                }
                
                // Course over ground - use raw field (radians)
                if self.should_broadcast(self.last_cog_broadcast, now) {
                    values.push(SignalKValue {
                        path: "navigation.courseOverGroundTrue".to_string(),
                        value: json!(cog_sog.cog),
                    });
                    self.last_cog_broadcast = Some(now);
                }
                
                if !values.is_empty() {
                    self.send_delta(source, timestamp, values);
                }
            },
            
            // Heading - PGN 127250
            // SignalK: navigation.headingTrue (radians)
            N2kMessage::VesselHeading(heading) => {
                // Only process magnetic heading and convert to true
                if heading.reference == HeadingReference::Magnetic {
                    if self.should_broadcast(self.last_heading_broadcast, now) {
                        // Heading is stored in radians in the struct (already SI)
                        let values = vec![SignalKValue {
                            path: "navigation.headingMagnetic".to_string(),
                            value: json!(heading.heading),
                        }];
                        
                        self.send_delta(source.clone(), timestamp.clone(), values);
                        self.last_heading_broadcast = Some(now);


                        if let Some(pos) = self.last_position {
                            let var = match crate::utilities::get_variation_deg(pos.latitude, pos.longitude, chrono::Utc::now()) {
                                Ok(v) => v,
                                Err(_) => 0.0, // Unable to get variation, revert to magnetic - better than nothing
                            };
                            let heading_true = heading.heading + var.to_radians(); // heading is in radians, var is in degrees, convert var to radians
                            // println!("Calculated true heading: {:.2}° + {:.2}° = {:.2}°", heading.heading.to_degrees(), var, heading_true.to_degrees());
                            let values = vec![SignalKValue {
                                path: "navigation.headingTrue".to_string(),
                                value: json!(heading_true),
                            }];
                            
                            self.send_delta(source, timestamp, values);
                        }
                    }
                }
            },
            
            // Wind data - PGN 130306
            // SignalK: environment.wind.speedOverGround, environment.wind.angleTrueGround (true wind)
            //          environment.wind.speedApparent, environment.wind.angleApparent
            N2kMessage::WindData(wind) => {
                let is_apparent = matches!(
                    wind.reference,
                    nmea2k::pgns::pgn130306::WindReference::Apparent
                );
                
                if is_apparent {
                    // Apparent wind - use raw fields (m/s and radians)
                    let aws_ms = wind.speed;
                    let awa_rad = wind.angle;
                    

                    if let Some(sog_time) = self.last_sog_timestamp {
                        // Invalidate cached SOG if it's older than 1 second (the two measure must be contemporaneous for true wind calculation to be accurate)
                        if now.duration_since(sog_time) > Duration::from_secs(1) {
                            self.last_sog_ms = None;
                        }
                    }

                    // Calculate true wind if we have cached SOG
                    let (tws_ms, twa_rad) = if let Some(sog_ms) = self.last_sog_ms {
                        // Convert to knots for calculation, then back to m/s
                        let aws_kn = aws_ms * 1.94384;
                        let awa_deg = awa_rad.to_degrees();
                        let sog_kn = sog_ms * 1.94384;
                        
                        let (tws_kn, twa_deg) = calculate_true_wind(aws_kn, awa_deg, sog_kn);
                        
                        // Convert back to SI units
                        (tws_kn / 1.94384, twa_deg.to_radians())
                    } else {
                        // No SOG available, use apparent as true
                        (aws_ms, awa_rad)
                    };
                    
                    let mut values = Vec::new();
                    
                    // True wind speed
                    if self.should_broadcast(self.last_wind_speed_true_broadcast, now) {
                        values.push(SignalKValue {
                            path: "environment.wind.speedOverGround".to_string(),
                            value: json!(tws_ms),
                        });
                        self.last_wind_speed_true_broadcast = Some(now);
                    }
                    
                    // True wind angle
                    if self.should_broadcast(self.last_wind_angle_true_broadcast, now) {
                        values.push(SignalKValue {
                            path: "environment.wind.angleTrueGround".to_string(),
                            value: json!(twa_rad),
                        });
                        self.last_wind_angle_true_broadcast = Some(now);
                    }
                    
                    // Apparent wind speed
                    if self.should_broadcast(self.last_wind_speed_apparent_broadcast, now) {
                        values.push(SignalKValue {
                            path: "environment.wind.speedApparent".to_string(),
                            value: json!(aws_ms),
                        });
                        self.last_wind_speed_apparent_broadcast = Some(now);
                    }
                    
                    // Apparent wind angle
                    if self.should_broadcast(self.last_wind_angle_apparent_broadcast, now) {
                        values.push(SignalKValue {
                            path: "environment.wind.angleApparent".to_string(),
                            value: json!(awa_rad),
                        });
                        self.last_wind_angle_apparent_broadcast = Some(now);
                    }
                    
                    if !values.is_empty() {
                        self.send_delta(source, timestamp, values);
                    }
                }
            },
            
            // Temperature sensors - PGN 130312
            // SignalK: environment.outside.temperature or environment.water.temperature (Kelvin)
            N2kMessage::Temperature(temp) => {
                if self.should_broadcast(self.last_temperature_broadcast, now) {
                    // Temperature is stored in Kelvin in the struct (already SI)
                    // Source 0 = water temperature, 4 = inside cabin, others = outside cabin
                    let path = match temp.source {
                        0 => "environment.water.temperature",
                        3 => "environment.air.inside.engineroom.temperature",
                        4 => "environment.air.inside.maincabin.temperature",
                        _ => "environment.air.outside.temperature",
                    };
                    
                    let values = vec![SignalKValue {
                        path: path.to_string(),
                        value: json!(temp.temperature),
                    }];
                    
                    self.send_delta(source, timestamp, values);
                    self.last_temperature_broadcast = Some(now);
                }
            },
            
            // Humidity sensor - PGN 130313
            // SignalK: environment.outside.relativeHumidity (ratio 0-1)
            N2kMessage::Humidity(humidity) => {
                if self.should_broadcast(self.last_humidity_broadcast, now) {
                    // actual_humidity is stored as percentage (0-100), convert to ratio
                    let humidity_ratio = humidity.actual_humidity / 100.0;
                    
                    let values = vec![SignalKValue {
                        path: "environment.outside.relativeHumidity".to_string(),
                        value: json!(humidity_ratio),
                    }];
                    
                    self.send_delta(source, timestamp, values);
                    self.last_humidity_broadcast = Some(now);
                }
            },
            
            // Pressure sensor - PGN 130314
            // SignalK: environment.outside.pressure (Pa)
            N2kMessage::ActualPressure(pressure) => {
                if self.should_broadcast(self.last_pressure_broadcast, now) {
                    // Pressure is already in Pascals (SI unit)
                    let values = vec![SignalKValue {
                        path: "environment.outside.pressure".to_string(),
                        value: json!(pressure.pressure),
                    }];
                    
                    self.send_delta(source, timestamp, values);
                    self.last_pressure_broadcast = Some(now);
                }
            },
            
            // System Time - PGN 126992
            // SignalK: navigation.datetime (ISO 8601 string) + time sync status
            N2kMessage::NMEASystemTime(system_time) => {
                if self.should_broadcast(self.last_datetime_broadcast, now) || self.should_broadcast(self.last_timesync_broadcast, now) {
                    // Convert NMEA datetime to ISO 8601
                    let gnss_secs = system_time.date_time.to_unix_timestamp();
                    let gnss_millis = system_time.date_time.milliseconds();
                    
                    let datetime = chrono::DateTime::from_timestamp(gnss_secs, gnss_millis * 1_000_000)
                        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
                    
                    let iso_timestamp = format!("{}.{:03}Z", datetime.format("%Y-%m-%dT%H:%M:%S"), gnss_millis);
                    
                    let mut values = vec![SignalKValue {
                        path: "navigation.datetime".to_string(),
                        value: json!(iso_timestamp),
                    }];
                    
                    // Include time sync status if available (set via update_time_sync_status)
                    if let Some(ref status) = self.last_time_sync_status {
                        values.push(SignalKValue {
                            path: "vessel.timeSyncStatus".to_string(),
                            value: json!(status),
                        });
                    }
                    if let Some(skew) = self.last_time_skew_ms {
                        values.push(SignalKValue {
                            path: "vessel.timeSkewMs".to_string(),
                            value: json!(skew),
                        });
                    }
                    
                    self.send_delta(source, timestamp, values);
                    if self.should_broadcast(self.last_datetime_broadcast, now) {
                        self.last_datetime_broadcast = Some(now);
                    }
                    if self.should_broadcast(self.last_timesync_broadcast, now) {
                        self.last_timesync_broadcast = Some(now);

                    }
                }
            },
            
            N2kMessage::WaterDepth(depth) => {
                // PGN 128267 - Water Depth below transducer
                // SignalK: environment.depth.belowTransducer (meters)
                if self.should_broadcast(self.last_depth_broadcast, now) {
                    let values = vec![SignalKValue {
                        path: "environment.depth.belowTransducer".to_string(),
                        value: json!(depth.depth),
                    }];
                    self.send_delta(source.clone(), timestamp.clone(), values);
                    self.last_depth_broadcast = Some(now);
                }
            },

            N2kMessage::SpeedWaterReferenced(speed) => {
                // PGN 128259 - Speed Water Referenced
                // SignalK: navigation.speedThroughWater (m/s)
                if self.should_broadcast(self.last_stw_broadcast, now) {
                    let values = vec![SignalKValue {
                        path: "navigation.speedThroughWater".to_string(),
                        value: json!(speed.speed),
                    }];
                    self.send_delta(source.clone(), timestamp.clone(), values);
                    self.last_stw_broadcast = Some(now);
                }
            },

            N2kMessage::AisClassAPositionReport(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": ais.get_latitude_degrees(),
                            "longitude": ais.get_longitude_degrees(),
                        }),
                    },
                    SignalKValue {
                        path: "navigation.speedOverGround".to_string(),
                        value: json!(ais.get_sog_ms()),
                    },
                    SignalKValue {
                        path: "navigation.courseOverGroundTrue".to_string(),
                        value: json!(ais.get_cog_radians()),
                    },
                    SignalKValue {
                        path: "navigation.headingTrue".to_string(),
                        value: json!(ais.get_heading_radians()),
                    },
                    SignalKValue {
                        path: "navigation.state".to_string(),
                        value: json!(ais.nav_status.to_string()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("A"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisClassBPositionReport(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": ais.get_latitude_degrees(),
                            "longitude": ais.get_longitude_degrees(),
                        }),
                    },
                    SignalKValue {
                        path: "navigation.speedOverGround".to_string(),
                        value: json!(ais.get_sog_ms()),
                    },
                    SignalKValue {
                        path: "navigation.courseOverGroundTrue".to_string(),
                        value: json!(ais.get_cog_radians()),
                    },
                    SignalKValue {
                        path: "navigation.headingTrue".to_string(),
                        value: json!(ais.get_heading_radians()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("B"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisClassBExtPositionReport(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": ais.get_latitude_degrees(),
                            "longitude": ais.get_longitude_degrees(),
                        }),
                    },
                    SignalKValue {
                        path: "navigation.speedOverGround".to_string(),
                        value: json!(ais.get_sog_ms()),
                    },
                    SignalKValue {
                        path: "navigation.courseOverGroundTrue".to_string(),
                        value: json!(ais.get_cog_radians()),
                    },
                    SignalKValue {
                        path: "navigation.headingTrue".to_string(),
                        value: json!(ais.get_heading_radians()),
                    },
                    SignalKValue {
                        path: "design.length".to_string(),
                        value: json!(ais.get_length_meters()),
                    },
                    SignalKValue {
                        path: "design.beam".to_string(),
                        value: json!(ais.get_beam_meters()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("B"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisAtonReport(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": ais.get_latitude_degrees(),
                            "longitude": ais.get_longitude_degrees(),
                        }),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("AtoN"),
                    },
                    SignalKValue {
                        path: "navigation.atonType".to_string(),
                        value: json!(ais.aton_type.to_string()),
                    },
                    SignalKValue {
                        path: "name".to_string(),
                        value: json!(ais.name.trim()),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisUtcDateReport(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "navigation.position".to_string(),
                        value: json!({
                            "latitude": ais.get_latitude_degrees(),
                            "longitude": ais.get_longitude_degrees(),
                        }),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisClassAStaticData(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "name".to_string(),
                        value: json!(ais.name.trim()),
                    },
                    SignalKValue {
                        path: "communication.callsignVhf".to_string(),
                        value: json!(ais.callsign.trim()),
                    },
                    SignalKValue {
                        path: "design.aisShipType".to_string(),
                        value: json!(ais.type_of_ship),
                    },
                    SignalKValue {
                        path: "design.length".to_string(),
                        value: json!(ais.get_length_meters()),
                    },
                    SignalKValue {
                        path: "design.beam".to_string(),
                        value: json!(ais.get_beam_meters()),
                    },
                    SignalKValue {
                        path: "design.draft".to_string(),
                        value: json!(ais.get_draft_meters()),
                    },
                    SignalKValue {
                        path: "navigation.destination".to_string(),
                        value: json!(ais.destination.trim()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("A"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisClassBStaticDataPartA(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "name".to_string(),
                        value: json!(ais.name.trim()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("B"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            N2kMessage::AisClassBStaticDataPartB(ais) => {
                let values = vec![
                    SignalKValue {
                        path: "design.aisShipType".to_string(),
                        value: json!(ais.type_of_ship),
                    },
                    SignalKValue {
                        path: "communication.callsignVhf".to_string(),
                        value: json!(ais.callsign.trim()),
                    },
                    SignalKValue {
                        path: "design.length".to_string(),
                        value: json!(ais.get_length_meters()),
                    },
                    SignalKValue {
                        path: "design.beam".to_string(),
                        value: json!(ais.get_beam_meters()),
                    },
                    SignalKValue {
                        path: "sensors.ais.class".to_string(),
                        value: json!("B"),
                    },
                ];
                
                self.send_ais_delta(ais.mmsi, source, timestamp, values);
            },

            // Attitude data - PGN 127257
            // SignalK: navigation.roll (radians)
            N2kMessage::Attitude(attitude) => {
                if self.should_broadcast(self.last_roll_broadcast, now) {
                    let values = vec![SignalKValue {
                        path: "navigation.roll".to_string(),
                        value: json!(attitude.roll),
                    }];
                    
                    self.send_delta(source, timestamp, values);
                    self.last_roll_broadcast = Some(now);
                }
            },

            // All other messages - ignore
            _ => {}
        }
    }
}
