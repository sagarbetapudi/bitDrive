//! Session management for the BPL protocol
//!
//! Handles session establishment, capability negotiation, authentication,
//! keepalive, and session teardown.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use parking_lot::RwLock;
use rand::RngCore;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};

use crate::{
    pb::{
        CapabilityNegotiateRequest, CapabilityNegotiateResponse, CapabilitySet, DeviceId,
        KeepAlive, NegotiatedCapability, ProtocolVersion, ResultCode, SessionCloseRequest,
        SessionEventType, SessionId, SessionOpenRequest, SessionOpenResponse,
    },
    capability::CapabilityNegotiator,
    auth::{AuthManager, SessionKeys},
    mux::ChannelManager,
    registry::ServiceRegistry,
    error::{ProtocolError, Result},
    frame::FrameCodec,
    SessionId as CrateSessionId,
    DEFAULT_KEEPALIVE_INTERVAL_MS, DEFAULT_SESSION_TIMEOUT_MS, MAX_CHANNELS,
    CONTROL_CHANNEL_ID,
};

/// Session configuration
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub protocol_version: ProtocolVersion,
    pub local_device_id: DeviceId,
    pub remote_device_id: DeviceId,
    pub session_id: SessionId,
    pub negotiated_capabilities: Vec<NegotiatedCapability>,
    pub capability_versions: HashMap<String, u32>,
    pub max_channels: u32,
    pub max_frame_size: u32,
    pub keepalive_interval: Duration,
    pub session_timeout: Duration,
    pub session_keys: SessionKeys,
    pub established_at: Instant,
    pub last_activity: Instant,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            protocol_version: ProtocolVersion { major: 1, minor: 0, patch: 0 },
            local_device_id: DeviceId { value: vec![] },
            remote_device_id: DeviceId { value: vec![] },
            session_id: SessionId { value: vec![] },
            negotiated_capabilities: Vec::new(),
            capability_versions: HashMap::new(),
            max_channels: MAX_CHANNELS,
            max_frame_size: 16384,
            keepalive_interval: Duration::from_millis(DEFAULT_KEEPALIVE_INTERVAL_MS as u64),
            session_timeout: Duration::from_millis(DEFAULT_SESSION_TIMEOUT_MS as u64),
            session_keys: SessionKeys::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        }
    }
}

/// Session state machine
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Closed,
    Opening,
    NegotiatingCapabilities,
    Authenticating,
    Active,
    Closing,
    Reconnecting,
}

/// Session event for external notification
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub session_id: SessionId,
    pub event_type: SessionEventType,
    pub timestamp: Instant,
    pub message: String,
    pub details: Bytes,
}

/// Session manager handles multiple sessions
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Session>>>>,
    config: SessionManagerConfig,
    event_tx: broadcast::Sender<SessionEvent>,
    codec: Arc<RwLock<FrameCodec>>,
}

/// Session manager configuration
#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    pub max_sessions: usize,
    pub session_timeout: Duration,
    pub keepalive_interval: Duration,
    pub enable_reconnection: bool,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1, // Single trusted device pair
            session_timeout: Duration::from_millis(DEFAULT_SESSION_TIMEOUT_MS as u64),
            keepalive_interval: Duration::from_millis(DEFAULT_KEEPALIVE_INTERVAL_MS as u64),
            enable_reconnection: true,
        }
    }
}

/// Active session
pub struct Session {
    config: SessionConfig,
    state: RwLock<SessionState>,
    channel_manager: Arc<ChannelManager>,
    service_registry: Arc<ServiceRegistry>,
    auth_manager: Arc<AuthManager>,
    capability_negotiator: Arc<CapabilityNegotiator>,
    event_tx: broadcast::Sender<SessionEvent>,
    keepalive_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    rx_sequence: RwLock<u64>,
    tx_sequence: RwLock<u64>,
    last_activity: RwLock<Instant>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: SessionManagerConfig) -> (Self, broadcast::Receiver<SessionEvent>) {
        let (event_tx, event_rx) = broadcast::channel(100);
        let manager = Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
            event_tx,
            codec: Arc::new(RwLock::new(FrameCodec::default())),
        };
        (manager, event_rx)
    }

    /// Create a new outbound session
    pub async fn create_session(
        &self,
        local_device_id: DeviceId,
        remote_device_id: DeviceId,
        local_capabilities: CapabilitySet,
    ) -> Result<Arc<Session>> {
        // Generate session ID
        let mut session_id_bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut session_id_bytes);
        let session_id = SessionId { value: session_id_bytes.to_vec() };

        // Create session config
        let config = SessionConfig {
            protocol_version: ProtocolVersion { major: 1, minor: 0, patch: 0 },
            local_device_id: local_device_id.clone(),
            remote_device_id: remote_device_id.clone(),
            session_id: session_id.clone(),
            negotiated_capabilities: Vec::new(),
            capability_versions: HashMap::new(),
            max_channels: MAX_CHANNELS,
            max_frame_size: 16384,
            keepalive_interval: self.config.keepalive_interval,
            session_timeout: self.config.session_timeout,
            session_keys: SessionKeys::default(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        };

        let session = Arc::new(Session::new(config, self.event_tx.clone()).await?);

        // Store session
        self.sessions.write().insert(session_id.clone(), session.clone());

        // Send session opened event
        let _ = self.event_tx.send(SessionEvent {
            session_id: session_id.clone(),
            event_type: SessionEventType::SessionEventOpened,
            timestamp: Instant::now(),
            message: "Session opened".to_string(),
            details: Bytes::new(),
        });

        Ok(session)
    }

    /// Get existing session
    pub fn get_session(&self, session_id: &SessionId) -> Option<Arc<Session>> {
        self.sessions.read().get(session_id).cloned()
    }

    /// Remove session
    pub fn remove_session(&self, session_id: &SessionId) -> Option<Arc<Session>> {
        self.sessions.write().remove(session_id)
    }

    /// List all sessions
    pub fn list_sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.read().values().cloned().collect()
    }

    /// Get event receiver
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }
}

impl Session {
    /// Create a new session
    async fn new(config: SessionConfig, event_tx: broadcast::Sender<SessionEvent>) -> Result<Self> {
        let channel_manager = Arc::new(ChannelManager::new());
        let service_registry = Arc::new(ServiceRegistry::new());
        let auth_manager = Arc::new(AuthManager::new());
        let capability_negotiator = Arc::new(CapabilityNegotiator::new());

        Ok(Self {
            config,
            state: RwLock::new(SessionState::Opening),
            channel_manager,
            service_registry,
            auth_manager,
            capability_negotiator,
            event_tx,
            keepalive_handle: RwLock::new(None),
            rx_sequence: RwLock::new(0),
            tx_sequence: RwLock::new(0),
            last_activity: RwLock::new(Instant::now()),
        })
    }

    /// Get session configuration
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Get current session state
    pub fn state(&self) -> SessionState {
        *self.state.read()
    }

    /// Set session state
    pub fn set_state(&self, state: SessionState) {
        *self.state.write() = state;
    }

    /// Get channel manager
    pub fn channel_manager(&self) -> &Arc<ChannelManager> {
        &self.channel_manager
    }

    /// Get service registry
    pub fn service_registry(&self) -> &Arc<ServiceRegistry> {
        &self.service_registry
    }

    /// Get auth manager
    pub fn auth_manager(&self) -> &Arc<AuthManager> {
        &self.auth_manager
    }

    /// Get capability negotiator
    pub fn capability_negotiator(&self) -> &Arc<CapabilityNegotiator> {
        &self.capability_negotiator
    }

    /// Get next TX sequence number
    pub fn next_tx_sequence(&self) -> u64 {
        let mut seq = self.tx_sequence.write();
        let current = *seq;
        *seq = seq.wrapping_add(1);
        current
    }

    /// Get current RX sequence number
    pub fn rx_sequence(&self) -> u64 {
        *self.rx_sequence.read()
    }

    /// Set RX sequence number (for reconnection)
    pub fn set_rx_sequence(&self, seq: u64) {
        *self.rx_sequence.write() = seq;
    }

    /// Update last activity timestamp
    pub fn touch(&self) {
        *self.last_activity.write() = Instant::now();
    }

    /// Check if session is alive
    pub fn is_alive(&self) -> bool {
        self.last_activity.read().elapsed() < self.config.session_timeout
    }

    /// Start keepalive task
    pub async fn start_keepalive(self: &Arc<Self>) {
        let session = Arc::clone(self);
        let handle = tokio::spawn(async move {
            session.keepalive_loop().await;
        });
        *self.keepalive_handle.write() = Some(handle);
    }

    /// Stop keepalive task
    pub async fn stop_keepalive(&self) {
        if let Some(handle) = self.keepalive_handle.write().take() {
            handle.abort();
        }
    }

    /// Keepalive loop
    async fn keepalive_loop(&self) {
        let mut interval = interval(self.config.keepalive_interval);
        let mut missed = 0;
        const MAX_MISSED: u32 = 3;

        loop {
            interval.tick().await;

            // Check if session is still active
            if *self.state.read() != SessionState::Active {
                break;
            }

            // Check for timeout
            if !self.is_alive() {
                missed += 1;
                if missed >= MAX_MISSED {
                    warn!("Session {:?} keepalive timeout", self.config.session_id);
                    self.handle_timeout().await;
                    break;
                }
            } else {
                missed = 0;
            }

            // Send keepalive
            if let Err(e) = self.send_keepalive().await {
                error!("Failed to send keepalive: {}", e);
                missed += 1;
                if missed >= MAX_MISSED {
                    self.handle_timeout().await;
                    break;
                }
            }
        }
    }

    /// Send keepalive
    async fn send_keepalive(&self) -> Result<()> {
        let sequence = self.next_tx_sequence();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Build keepalive frame (would be sent via transport)
        debug!("Sending keepalive for session {:?}", self.config.session_id);
        Ok(())
    }

    /// Handle keepalive timeout
    async fn handle_timeout(&self) {
        self.set_state(SessionState::Closing);
        let _ = self.event_tx.send(SessionEvent {
            session_id: self.config.session_id.clone(),
            event_type: SessionEventType::SessionEventKeepaliveTimeout,
            timestamp: Instant::now(),
            message: "Keepalive timeout".to_string(),
            details: Bytes::new(),
        });
    }

    /// Handle incoming keepalive response
    pub fn handle_keepalive_response(&self, rtt_ms: u64) {
        self.touch();
        debug!("Keepalive response received, RTT: {}ms", rtt_ms);
    }

    /// Close session gracefully
    pub async fn close(&self, reason: ResultCode, message: String) {
        self.set_state(SessionState::Closing);
        self.stop_keepalive().await;

        let _ = self.event_tx.send(SessionEvent {
            session_id: self.config.session_id.clone(),
            event_type: SessionEventType::SessionEventClosed,
            timestamp: Instant::now(),
            message,
            details: Bytes::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_manager_creation() {
        let config = SessionManagerConfig::default();
        let (manager, _rx) = SessionManager::new(config);
        assert_eq!(manager.list_sessions().len(), 0);
    }
}