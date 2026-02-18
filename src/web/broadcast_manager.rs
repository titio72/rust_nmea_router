use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::broadcast;
use super::websocket::BroadcastChannels;
use super::signalk_messages::SignalKDelta;

/// Global broadcast channels - initialized once at startup
static BROADCAST_CHANNELS: Lazy<Arc<BroadcastChannels>> =
    Lazy::new(|| Arc::new(BroadcastChannels::new()));

/// Global SignalK broadcast channels
static SIGNALK_CHANNELS: Lazy<Arc<SignalKBroadcastChannels>> =
    Lazy::new(|| Arc::new(SignalKBroadcastChannels::new()));

/// Get the global broadcast channels
pub fn get_broadcast_channels() -> Arc<BroadcastChannels> {
    BROADCAST_CHANNELS.clone()
}

/// Get the global SignalK broadcast channels
pub fn get_signalk_channels() -> Arc<SignalKBroadcastChannels> {
    SIGNALK_CHANNELS.clone()
}

/// SignalK delta message broadcast channels
#[derive(Clone)]
pub struct SignalKBroadcastChannels {
    tx: broadcast::Sender<SignalKDelta>,
}

impl SignalKBroadcastChannels {
    /// Create new SignalK broadcast channels with capacity
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(16);
        Self { tx }
    }
    
    /// Send a SignalK delta message to all subscribers
    pub fn send(&self, msg: SignalKDelta) -> Result<(), broadcast::error::SendError<SignalKDelta>> {
        self.tx.send(msg).map(|_| ())
    }
    
    /// Subscribe to SignalK delta messages
    pub fn subscribe(&self) -> broadcast::Receiver<SignalKDelta> {
        self.tx.subscribe()
    }
}
