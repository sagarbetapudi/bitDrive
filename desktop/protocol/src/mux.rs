//! Multiplexing layer for the BPL protocol
//!
//! Handles logical channels over a single Bluetooth connection with
//! flow control and prioritization.

use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::{
    pb::{ChannelConfig, ChannelPriority, ChannelType, FlowControlUpdate},
    error::{ProtocolError, Result},
    DEFAULT_WINDOW_SIZE, MAX_CHANNELS,
};

/// Channel state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    Closed,
    Opening,
    Open,
    Closing,
    Error,
}

/// Logical channel
#[derive(Debug, Clone)]
pub struct Channel {
    pub config: ChannelConfig,
    pub state: ChannelState,
    pub send_window: u32,
    pub recv_window: u32,
    pub send_window_used: u32,
    pub recv_window_used: u32,
    pub frames_sent: u64,
    pub frames_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub retransmissions: u64,
    pub errors: u64,
    pub tx_sender: Option<mpsc::Sender<Bytes>>,
}

impl Channel {
    /// Create a new channel
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            send_window: config.send_window,
            recv_window: config.receive_window,
            send_window_used: 0,
            recv_window_used: 0,
            frames_sent: 0,
            frames_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            retransmissions: 0,
            errors: 0,
            tx_sender: None,
            config,
            state: ChannelState::Closed,
        }
    }

    /// Check if we can send a frame
    pub fn can_send(&self, frame_size: u32) -> bool {
        self.state == ChannelState::Open
            && self.send_window_used + frame_size <= self.send_window
    }

    /// Reserve send window
    pub fn reserve_window(&mut self, size: u32) -> Result<()> {
        if self.send_window_used + size > self.send_window {
            return Err(ProtocolError::ChannelWindowExhausted {
                channel_id: self.config.channel_id.value,
            });
        }
        self.send_window_used += size;
        Ok(())
    }

    /// Release send window
    pub fn release_window(&mut self, size: u32) {
        self.send_window_used = self.send_window_used.saturating_sub(size);
    }

    /// Update send window (flow control)
    pub fn update_send_window(&mut self, increment: u32) {
        self.send_window = self.send_window.saturating_add(increment);
    }

    /// Update receive window
    pub fn update_recv_window(&mut self, increment: u32) {
        self.recv_window = self.recv_window.saturating_add(increment);
    }

    /// Record frame sent
    pub fn record_sent(&mut self, size: u32) {
        self.frames_sent += 1;
        self.bytes_sent += size as u64;
    }

    /// Record frame received
    pub fn record_received(&mut self, size: u32) {
        self.frames_received += 1;
        self.bytes_received += size as u64;
    }

    /// Record retransmission
    pub fn record_retransmission(&mut self) {
        self.retransmissions += 1;
    }

    /// Record error
    pub fn record_error(&mut self) {
        self.errors += 1;
    }
}

/// Channel manager for multiplexing
pub struct ChannelManager {
    channels: Arc<RwLock<HashMap<u32, Arc<RwLock<Channel>>>>>,
    next_channel_id: Arc<RwLock<u32>>,
    event_tx: Option<mpsc::Sender<ChannelEvent>>,
}

/// Channel events
#[derive(Debug, Clone)]
pub enum ChannelEvent {
    Opened(u32),
    Closed(u32),
    FlowControl(u32, u32), // channel_id, new_window
    Error(u32, String),
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            next_channel_id: Arc::new(RwLock::new(1)), // 0 is control channel
            event_tx: None,
        }
    }

    /// Set event sender
    pub fn set_event_sender(&mut self, tx: mpsc::Sender<ChannelEvent>) {
        self.event_tx = Some(tx);
    }

    /// Open a new channel
    pub fn open_channel(&self, mut config: ChannelConfig) -> Result<Arc<RwLock<Channel>>> {
        let channel_id = if config.channel_id.value == 0 {
            *self.next_channel_id.write()
        } else {
            config.channel_id.value
        };

        if channel_id >= MAX_CHANNELS {
            return Err(ProtocolError::InvalidArgument(
                "Maximum channels reached".to_string(),
            ));
        }

        if self.channels.read().contains_key(&channel_id) {
            return Err(ProtocolError::InvalidArgument(
                format!("Channel {} already exists", channel_id),
            ));
        }

        config.channel_id.value = channel_id;
        let channel = Arc::new(RwLock::new(Channel::new(config)));
        self.channels.write().insert(channel_id, channel.clone());

        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(ChannelEvent::Opened(channel_id));
        }

        *self.next_channel_id.write() = (channel_id + 1).min(MAX_CHANNELS);
        debug!("Opened channel {}", channel_id);
        Ok(channel)
    }

    /// Get channel by ID
    pub fn get_channel(&self, channel_id: u32) -> Option<Arc<RwLock<Channel>>> {
        self.channels.read().get(&channel_id).cloned()
    }

    /// Close a channel
    pub fn close_channel(&self, channel_id: u32, reason: String) -> Result<()> {
        if let Some(channel) = self.channels.write().remove(&channel_id) {
            channel.write().state = ChannelState::Closed;

            if let Some(tx) = &self.event_tx {
                let _ = tx.try_send(ChannelEvent::Closed(channel_id));
            }

            debug!("Closed channel {}: {}", channel_id, reason);
            Ok(())
        } else {
            Err(ProtocolError::ChannelNotFound { channel_id })
        }
    }

    /// Handle flow control update
    pub fn handle_flow_control(&self, update: FlowControlUpdate) -> Result<()> {
        if let Some(channel) = self.get_channel(update.channel_id.value) {
            channel.write().update_send_window(update.window_increment);

            if let Some(tx) = &self.event_tx {
                let _ = tx.try_send(ChannelEvent::FlowControl(
                    update.channel_id.value,
                    channel.read().send_window,
                ));
            }
            Ok(())
        } else {
            Err(ProtocolError::ChannelNotFound {
                channel_id: update.channel_id.value,
            })
        }
    }

    /// List all channels
    pub fn list_channels(&self) -> Vec<Arc<RwLock<Channel>>> {
        self.channels.read().values().cloned().collect()
    }

    /// Get channel stats
    pub fn get_stats(&self) -> Vec<ChannelStats> {
        self.channels.read()
            .iter()
            .map(|(id, ch)| {
                let ch = ch.read();
                ChannelStats {
                    channel_id: *id,
                    channel_type: ch.config.r#type,
                    priority: ch.config.priority,
                    state: ch.state,
                    send_window: ch.send_window,
                    send_window_used: ch.send_window_used,
                    recv_window: ch.recv_window,
                    recv_window_used: ch.recv_window_used,
                    frames_sent: ch.frames_sent,
                    frames_received: ch.frames_received,
                    bytes_sent: ch.bytes_sent,
                    bytes_received: ch.bytes_received,
                    retransmissions: ch.retransmissions,
                    errors: ch.errors,
                    service_id: ch.config.service_id.clone(),
                }
            })
            .collect()
    }

    /// Get total frames sent across all channels
    pub fn total_frames_sent(&self) -> u64 {
        self.channels.read().values().map(|c| c.read().frames_sent).sum()
    }

    /// Get total frames received across all channels
    pub fn total_frames_received(&self) -> u64 {
        self.channels.read().values().map(|c| c.read().frames_received).sum()
    }

    /// Get total bytes sent across all channels
    pub fn total_bytes_sent(&self) -> u64 {
        self.channels.read().values().map(|c| c.read().bytes_sent).sum()
    }

    /// Get total bytes received across all channels
    pub fn total_bytes_received(&self) -> u64 {
        self.channels.read().values().map(|c| c.read().bytes_received).sum()
    }
}

/// Channel statistics
#[derive(Debug, Clone)]
pub struct ChannelStats {
    pub channel_id: u32,
    pub channel_type: ChannelType,
    pub priority: ChannelPriority,
    pub state: ChannelState,
    pub send_window: u32,
    pub send_window_used: u32,
    pub recv_window: u32,
    pub recv_window_used: u32,
    pub frames_sent: u64,
    pub frames_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub retransmissions: u64,
    pub errors: u64,
    pub service_id: String,
}

/// Build default channel config for a service
pub fn default_channel_config(
    service_id: &str,
    channel_type: ChannelType,
    priority: ChannelPriority,
) -> ChannelConfig {
    ChannelConfig {
        channel_id: crate::pb::ChannelId { value: 0 },
        r#type: channel_type as i32,
        priority: priority as i32,
        send_window: DEFAULT_WINDOW_SIZE,
        receive_window: DEFAULT_WINDOW_SIZE,
        max_frame_size: 16384,
        service_id: service_id.to_string(),
        metadata: Default::default(),
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_manager() {
        let manager = ChannelManager::new();

        let config = default_channel_config("test.service", ChannelType::Data, ChannelPriority::Normal);
        let channel = manager.open_channel(config).unwrap();

        assert_eq!(manager.list_channels().len(), 1);

        channel.read().config.service_id, "test.service");

        manager.close_channel(1, "test".to_string()).unwrap();
        assert_eq!(manager.list_channels().len(), 0);
    }

    #[test]
    fn test_flow_control() {
        let manager = ChannelManager::new();

        let config = default_channel_config("test.service", ChannelType::Data, ChannelPriority::Normal);
        let channel = manager.open_channel(config).unwrap();

        // Reserve window
        channel.write().reserve_window(1000).unwrap();
        assert_eq!(channel.read().send_window_used, 1000);

        // Release window
        channel.write().release_window(500);
        assert_eq!(channel.read().send_window_used, 500);

        // Flow control update
        let update = FlowControlUpdate {
            channel_id: crate::pb::ChannelId { value: 1 },
            window_increment: 2000,
        };
        manager.handle_flow_control(update).unwrap();
        assert_eq!(channel.read().send_window, DEFAULT_WINDOW_SIZE + 2000);
    }
}