// SignalK WebSocket Stream Handler
// Implements /signalk/v1/stream endpoint per SignalK v1.7.0 specification
//
use axum::{
    extract::ws::{WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use std::sync::Arc;
use tokio::sync::broadcast;
use futures_util::stream::StreamExt;
use futures_util::sink::SinkExt;
use tracing::{debug, warn, error};
use serde_json;

use super::api::AppState;
use super::signalk_messages::{SignalKHello, SignalKDelta, SignalKUpdate, SignalKValue, vessel_context, vessel_urn, instant_to_rfc3339};
use std::time::Instant;

/// SignalK WebSocket handler for /signalk/v1/stream
pub async fn signalk_stream(
    ws: WebSocketUpgrade,
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_signalk_socket(socket, state.signalk_broadcast.clone(), state.config.clone()))
}

/// Handle individual SignalK WebSocket connection
async fn handle_signalk_socket(
    socket: WebSocket,
    channels: Arc<super::broadcast_manager::SignalKBroadcastChannels>,
    config: std::sync::Arc<crate::config::Config>,
) {
    let (tx, mut rx) = socket.split();
    let tx = Arc::new(tokio::sync::Mutex::new(tx));

    // Subscribe to SignalK delta stream
    let mut msg_rx = channels.subscribe();
    debug!("New SignalK WebSocket client connected at /signalk/v1/stream");

    // Send hello message to the client
    let hello = SignalKHello::new(&config.signalk.vessel_uuid);
    if let Ok(hello_json) = serde_json::to_string(&hello) {
        let mut tx_guard = tx.lock().await;
        if let Err(e) = tx_guard.send(axum::extract::ws::Message::Text(hello_json)).await {
            warn!("Failed to send SignalK hello message: {}", e);
            return; // Connection already closed, exit early
        }
        debug!("Sent SignalK hello message to client");
    }
    
    // Send initialization delta messages with vessel info
    let timestamp = instant_to_rfc3339(Instant::now());
    let init_delta = SignalKDelta {
        context: vessel_context(&config.signalk.vessel_uuid),
        updates: vec![SignalKUpdate {
            source: None,
            timestamp: timestamp.clone(),
            values: vec![SignalKValue {
                path: "".to_string(),
                value: serde_json::json!({
                    "uuid": vessel_urn(&config.signalk.vessel_uuid),
                    "name": &config.signalk.vessel_name
                }),
            }],
            source_ref: "defaults".to_string(),
        }],
    };
    
    // Send the initialization message twice as requested
    for _ in 0..2 {
        if let Ok(init_json) = serde_json::to_string(&init_delta) {
            let mut tx_guard = tx.lock().await;
            if let Err(e) = tx_guard.send(axum::extract::ws::Message::Text(init_json)).await {
                warn!("Failed to send SignalK initialization message: {}", e);
                return;
            }
        }
    }
    debug!("Sent SignalK initialization messages to client");

    // Spawn broadcast receiver task
    let tx_clone = tx.clone();
    let broadcast_task = tokio::spawn(async move {
        loop {
            match msg_rx.recv().await {
                Ok(delta) => {
                    // Serialize SignalK delta message to JSON
                    if let Ok(json) = serde_json::to_string(&delta) {
                        let mut tx_guard = tx_clone.lock().await;
                        if tx_guard.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                            warn!("SignalK WebSocket client disconnected during send");
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SignalK WebSocket client lagged, {} messages were dropped", skipped);
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("SignalK broadcast channel closed");
                    break;
                }
            }
        }
    });

    // Main loop to handle incoming messages (for future subscription protocol)
    while let Some(msg) = rx.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Close(_)) => {
                debug!("SignalK WebSocket client sent close frame");
                break;
            }
            Ok(axum::extract::ws::Message::Text(text)) => {
                debug!("Received text message from SignalK client: {} (subscription not yet implemented)", text);
                // Future: implement SignalK subscription protocol here
            }
            Ok(axum::extract::ws::Message::Binary(_)) => {
                debug!("Received binary message from SignalK client (currently ignored)");
            }
            Ok(_) => {
                // Other message types (ping, pong, etc.)
            }
            Err(e) => {
                error!("SignalK WebSocket error: {}", e);
                break;
            }
        }
    }

    broadcast_task.abort();
    debug!("SignalK WebSocket client disconnected");
}
