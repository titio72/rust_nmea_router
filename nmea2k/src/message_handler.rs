use crate::N2kFrame;


/// Trait for components that handle NMEA2000 messages
/// 
/// This trait allows monitors to receive all messages and decide internally
/// which ones they're interested in, reducing coupling between the main loop
/// and individual monitors.
pub trait MessageHandler {
    /// Process an incoming NMEA2000 message
    /// 
    /// Implementations should check the message type and handle only the
    /// messages they're interested in, ignoring others.
    fn handle_message(&mut self, frame_and_message: &N2kFrame, timestamp: std::time::Instant);
}
