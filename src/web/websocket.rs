use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;
use futures_util::stream::StreamExt;
use futures_util::sink::SinkExt;
use tracing::{debug, warn, error};

// ============================================================================
// MESSAGE-SPECIFIC TYPES - Each NMEA message is serialized independently
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message_type", content = "data")]
pub enum RealtimeMessage {
    /// PGN 129025 - Position (latitude, longitude)
    Position {
        latitude: f64,
        longitude: f64,
        timestamp: i64,
    },
    
    /// PGN 129026 - Course and Speed over Ground
    CourseSpeed {
        cog_deg: f64,
        sog_kn: f64,
        timestamp: i64,
    },
    
    /// PGN 127250 - Vessel Heading
    Heading {
        heading_deg: f64,
        timestamp: i64,
    },
    
    /// PGN 130306 - Wind Data (both true and apparent)
    Wind {
        #[serde(skip_serializing_if = "Option::is_none")]
        true_wind_speed_kn: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        true_wind_angle_deg: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        apparent_wind_speed_kn: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        apparent_wind_angle_deg: Option<f64>,
        timestamp: i64,
    },
    
    /// PGN 130312 - Temperature (water, cabin, etc)
    Temperature {
        temperature_c: f64,
        instance: u8,  // 0=water, 1=cabin, etc.
        timestamp: i64,
    },
    
    /// PGN 130313 - Humidity
    Humidity {
        humidity_percent: f64,
        timestamp: i64,
    },
    
    /// PGN 130314 - Barometric Pressure
    Pressure {
        pressure_pa: f64,
        timestamp: i64,
    },
    
    /// PGN 126992 - System Time and Time Synchronization
    SystemTime {
        time_sync_status: String,  // "synced" or "not_synced"
        time_skew_ms: i64,
        timestamp: i64,
    },
}

// ============================================================================
// BROADCASTER - Simple channel manager
// ============================================================================

#[derive(Clone)]
pub struct BroadcastChannels {
    tx: broadcast::Sender<RealtimeMessage>,
}

impl BroadcastChannels {
    /// Create new broadcast channels with capacity
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(16);
        Self { tx }
    }
    
    /// Send a message to all subscribers
    pub fn send(&self, msg: RealtimeMessage) -> Result<(), broadcast::error::SendError<RealtimeMessage>> {
        self.tx.send(msg).map(|_| ())
    }
    
    /// Subscribe to messages
    pub fn subscribe(&self) -> broadcast::Receiver<RealtimeMessage> {
        self.tx.subscribe()
    }
}

// ============================================================================
// WEBSOCKET HANDLER
// ============================================================================

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<super::api::AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state.broadcast.clone()))
}

/// Handle individual websocket connection
async fn handle_socket(
    socket: WebSocket,
    channels: Arc<BroadcastChannels>,
) {
    let (tx, mut rx) = socket.split();
    let tx = Arc::new(tokio::sync::Mutex::new(tx));

    // Subscribe to message stream
    let mut msg_rx = channels.subscribe();
    debug!("New websocket client connected for real-time updates");

    // Spawn broadcast receiver task
    let tx_clone = tx.clone();
    let broadcast_task = tokio::spawn(async move {
        loop {
            match msg_rx.recv().await {
                Ok(msg) => {
                    // Serialize the message directly
                    if let Ok(json) = serde_json::to_string(&msg) {
                        let mut tx_guard = tx_clone.lock().await;
                        if tx_guard.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                            warn!("WebSocket client disconnected during send");
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    warn!("Websocket client lagged, some messages were dropped");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Broadcast channel closed");
                    break;
                }
            }
        }
    });

    // Main loop to handle incoming messages
    while let Some(msg) = rx.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Close(_)) => {
                debug!("Websocket client sent close frame");
                break;
            }
            Ok(axum::extract::ws::Message::Text(_)) => {
                debug!("Received text message from websocket client (currently ignored)");
            }
            Ok(axum::extract::ws::Message::Binary(_)) => {
                debug!("Received binary message from websocket client (currently ignored)");
            }
            Ok(_) => {
                // Other message types
            }
            Err(e) => {
                error!("Websocket error: {}", e);
                break;
            }
        }
    }

    broadcast_task.abort();
    debug!("Websocket client disconnected");
}
