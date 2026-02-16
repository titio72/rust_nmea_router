use once_cell::sync::Lazy;
use std::sync::Arc;
use super::websocket::BroadcastChannels;

/// Global broadcast channels - initialized once at startup
static BROADCAST_CHANNELS: Lazy<Arc<BroadcastChannels>> =
    Lazy::new(|| Arc::new(BroadcastChannels::new()));

/// Get the global broadcast channels
pub fn get_broadcast_channels() -> Arc<BroadcastChannels> {
    BROADCAST_CHANNELS.clone()
}
