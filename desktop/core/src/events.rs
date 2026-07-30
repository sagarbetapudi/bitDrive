//! Event bus for inter-service communication

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, trace};

use bpl_protocol::{DeviceId, SessionId, ChannelId, NegotiatedCapability, ProtocolError, Result};

/// Event types for the system
#[derive(Debug, Clone)]
pub enum Event {
    // Bluetooth events
    BluetoothAdapterAdded(AdapterInfo),
    BluetoothAdapterRemoved(String),
    BluetoothDeviceFound(DeviceInfo),
    BluetoothDeviceLost(DeviceId),
    BluetoothDevicePaired(DeviceId),
    BluetoothDeviceUnpaired(DeviceId),
    BluetoothDeviceConnected(DeviceId),
    BluetoothDeviceDisconnected(DeviceId),

    // Session events
    SessionOpening(SessionId),
    SessionOpened(SessionId),
    SessionNegotiatingCapabilities(SessionId),
    SessionAuthenticating(SessionId),
    SessionActive(SessionId),
    SessionClosing(SessionId),
    SessionClosed(SessionId),
    SessionError(SessionId, String),

    // Capability negotiation events
    CapabilitiesReceived(DeviceId, Vec<ServiceCapability>),
    CapabilitiesNegotiated(SessionId, Vec<NegotiatedCapability>),

    // Authentication events
    AuthChallengeSent(SessionId),
    AuthChallengeReceived(SessionId),
    AuthResponseSent(SessionId),
    AuthResponseReceived(SessionId),
    AuthSuccess(SessionId),
    AuthFailure(SessionId, String),

    // Channel events
    ChannelOpened(SessionId, ChannelId, String), // channel_id, service_id
    ChannelClosed(SessionId, ChannelId),
    ChannelError(SessionId, ChannelId, String),

    // Service events
    ServiceRegistered(String), // service_id
    ServiceUnregistered(String),
    ServiceStarted(String),
    ServiceStopped(String),
    ServiceHealthChanged(String, bool),

    // Filesystem events
    FileCreated(String, FileMetadata), // path, metadata
    FileModified(String, FileMetadata),
    FileDeleted(String),
    FileMoved(String, String), // from, to
    DirectoryCreated(String),
    DirectoryDeleted(String),

    // Sync events
    SyncJobStarted(String), // job_id
    SyncJobCompleted(String, SyncStats),
    SyncJobFailed(String, String),
    SyncConflictDetected(String, SyncConflict), // job_id, conflict
    SyncConflictResolved(String, String), // conflict_id, strategy
    SyncProgress(String, u64, u64), // job_id, current, total

    // Photo backup events
    PhotoBackupStarted(String), // session_id
    PhotoBackupProgress(String, u64, u64, String), // session_id, current, total, current_file
    PhotoBackupCompleted(String, PhotoBackupStats),
    PhotoBackupFailed(String, String),

    // Shell events
    ShellSessionCreated(String, DeviceId), // session_id, device_id
    ShellCommandExecuted(String, String, i32), // session_id, command, exit_code
    ShellOutput(String, String, String), // session_id, stdout, stderr
    ShellSessionClosed(String),

    // Media control events
    MediaStateChanged(MediaState),
    MediaMetadataChanged(MediaMetadata),
    MediaVolumeChanged(f32),

    // Proximity events
    ProximityStateChanged(ProximityState),
    ProximityRssiUpdated(i16),

    // File streaming events
    StreamOpened(String, StreamInfo), // stream_id, info
    StreamData(String, Vec<u8>), // stream_id, data
    StreamClosed(String),
    StreamError(String, String),

    // App launcher events
    AppLaunched(String, String), // package_name, activity
    AppLaunchFailed(String, String),

    // Config events
    ConfigChanged(String, String, String), // key, old_value, new_value

    // System events
    ShutdownRequested,
    ConfigReloaded,
}

/// Adapter info
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub id: String,
    pub name: String,
    pub address: String,
    pub powered: bool,
    pub discoverable: bool,
    pub pairable: bool,
}

/// Device info
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub name: Option<String>,
    pub address: String,
    pub paired: bool,
    pub connected: bool,
    pub rssi: Option<i16>,
    pub device_class: Option<u32>,
    pub services: Vec<String>,
}

/// Service capability
#[derive(Debug, Clone)]
pub struct ServiceCapability {
    pub service_id: String,
    pub version: u32,
    pub name: String,
    pub description: String,
    pub required: bool,
    pub metadata: HashMap<String, String>,
    pub feature_flags: u64,
}

/// File metadata
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
    pub modified_time: u64,
    pub created_time: u64,
    pub mime_type: Option<String>,
    pub hash: Option<String>,
    pub permissions: u32,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
}

/// Sync stats
#[derive(Debug, Clone)]
pub struct SyncStats {
    pub files_total: u64,
    pub files_synced: u64,
    pub files_skipped: u64,
    pub files_conflicted: u64,
    pub bytes_total: u64,
    pub bytes_synced: u64,
    pub errors: u64,
}

/// Sync conflict
#[derive(Debug, Clone)]
pub struct SyncConflict {
    pub id: String,
    pub job_id: String,
    pub local_path: String,
    pub remote_path: String,
    pub local_metadata: String,
    pub remote_metadata: String,
    pub local_hash: String,
    pub remote_hash: String,
}

/// Photo backup stats
#[derive(Debug, Clone)]
pub struct PhotoBackupStats {
    pub total_photos: u64,
    pub backed_up: u64,
    pub skipped: u64,
    pub errors: u64,
    pub bytes_total: u64,
    pub bytes_transferred: u64,
}

/// Media state
#[derive(Debug, Clone)]
pub struct MediaState {
    pub state: String, // playing, paused, stopped, etc.
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: String,
}

/// Media metadata
#[derive(Debug, Clone)]
pub struct MediaMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub track_number: u32,
    pub total_tracks: u32,
    pub year: u32,
    pub duration_ms: u64,
    pub artwork_uri: Option<String>,
    pub artwork_data: Option<Vec<u8>>,
}

/// Proximity state
#[derive(Debug, Clone)]
pub struct ProximityState {
    pub state: String, // near, far, out_of_range
    pub rssi: i16,
    pub distance_estimate: Option<f32>,
    pub confidence: f32,
}

/// Stream info
#[derive(Debug, Clone)]
pub struct StreamInfo {
    pub stream_id: String,
    pub path: String,
    pub file_size: u64,
    pub mime_type: String,
    pub seekable: bool,
    pub supports_range: bool,
    pub recommended_chunk_size: u32,
}

/// Event bus for publishing and subscribing to events
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    event_history: Arc<RwLock<Vec<Event>>>,
    max_history: usize,
}

impl EventBus {
    /// Create a new event bus
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1000);
        Self {
            sender,
            event_history: Arc::new(RwLock::new(Vec::new())),
            max_history: 1000,
        }
    }

    /// Publish an event
    pub fn publish(&self, event: Event) -> Result<()> {
        // Add to history
        {
            let mut history = self.event_history.write();
            history.push(event.clone());
            if history.len() > self.max_history {
                history.drain(0..history.len() - self.max_history);
            }
        }

        // Broadcast to subscribers
        let _ = self.sender.send(event);
        Ok(())
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Get event history
    pub fn history(&self) -> Vec<Event> {
        self.event_history.read().clone()
    }

    /// Clear event history
    pub fn clear_history(&self) {
        self.event_history.write().clear();
    }

    /// Get history size
    pub fn history_size(&self) -> usize {
        self.event_history.read().len()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            event_history: self.event_history.clone(),
            max_history: self.max_history,
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience macros for publishing events
#[macro_export]
macro_rules! publish_event {
    ($bus:expr, $event:expr) => {
        if let Err(e) = $bus.publish($event) {
            tracing::warn!("Failed to publish event: {}", e);
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_bus() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(Event::BluetoothDeviceConnected(bpl_protocol::DeviceId { value: vec![1,2,3] })).unwrap();

        let event = rx.recv().await.unwrap();
        match event {
            Event::BluetoothDeviceConnected(id) => assert_eq!(id.value, vec![1,2,3]),
            _ => panic!("Wrong event type"),
        }

        assert_eq!(bus.history_size(), 1);
    }
}