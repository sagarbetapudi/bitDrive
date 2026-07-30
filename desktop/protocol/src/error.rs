//! Protocol error types

use thiserror::Error;

/// Protocol result type
pub type Result<T> = std::result::Result<T, ProtocolError>;

/// Protocol errors
#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("Invalid frame header: {0}")]
    InvalidHeader(String),

    #[error("Checksum mismatch: expected {expected:#010x}, got {actual:#010x}")]
    ChecksumMismatch { expected: u32, actual: u32 },

    #[error("Frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: usize, max: usize },

    #[error("Payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: usize, max: usize },

    #[error("Invalid magic number: expected {expected:#010x}, got {actual:#010x}")]
    InvalidMagic { expected: u32, actual: u32 },

    #[error("Unsupported protocol version: {version:#010x}")]
    UnsupportedVersion { version: u32 },

    #[error("Unknown frame type: {type:?}")]
    UnknownFrameType { type: u8 },

    #[error("Sequence number mismatch: expected {expected}, got {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },

    #[error("Replay attack detected: sequence {sequence} already processed")]
    ReplayDetected { sequence: u64 },

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Authentication timeout")]
    AuthTimeout,

    #[error("Session not found: {session_id:?}")]
    SessionNotFound { session_id: Vec<u8> },

    #[error("Session already exists: {session_id:?}")]
    SessionExists { session_id: Vec<u8> },

    #[error("Session closed: {reason}")]
    SessionClosed { reason: String },

    #[error("Session state error: expected {expected:?}, got {actual:?}")]
    SessionStateError { expected: String, actual: String },

    #[error("Channel not found: {channel_id}")]
    ChannelNotFound { channel_id: u32 },

    #[error("Channel closed: {channel_id}")]
    ChannelClosed { channel_id: u32 },

    #[error("Channel window exhausted: {channel_id}")]
    ChannelWindowExhausted { channel_id: u32 },

    #[error("Flow control error: {0}")]
    FlowControl(String),

    #[error("Capability negotiation failed: {0}")]
    CapabilityNegotiation(String),

    #[error("Capability not supported: {service_id}")]
    CapabilityNotSupported { service_id: String },

    #[error("Required capability missing: {service_id}")]
    RequiredCapabilityMissing { service_id: String },

    #[error("Service not found: {service_id}")]
    ServiceNotFound { service_id: String },

    #[error("Service already registered: {service_id}")]
    ServiceAlreadyRegistered { service_id: String },

    #[error("Service not registered: {service_id}")]
    ServiceNotRegistered { service_id: String },

    #[error("Invalid service configuration: {0}")]
    InvalidServiceConfig(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivation(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Invalid key length: expected {expected}, got {actual}")]
    InvalidKeyLength { expected: usize, actual: usize },

    #[error("Invalid nonce length: expected {expected}, got {actual}")]
    InvalidNonceLength { expected: usize, actual: usize },

    #[error("Invalid auth tag length: expected {expected}, got {actual}")]
    InvalidAuthTagLength { expected: usize, actual: usize },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] prost::EncodeError),

    #[error("Deserialization error: {0}")]
    Deserialization(#[from] prost::DecodeError),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("Not connected")]
    NotConnected,

    #[error("Already connected")]
    AlreadyConnected,

    #[error("Peer disconnected")]
    PeerDisconnected,

    #[error("Buffer overflow")]
    BufferOverflow,

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl ProtocolError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProtocolError::Timeout(_)
                | ProtocolError::Io(_)
                | ProtocolError::ChannelWindowExhausted { .. }
                | ProtocolError::FlowControl(_)
        )
    }

    /// Check if error is fatal (requires session termination)
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            ProtocolError::AuthenticationFailed(_)
                | ProtocolError::DecryptionFailed(_)
                | ProtocolError::ReplayDetected { .. }
                | ProtocolError::SequenceMismatch { .. }
                | ProtocolError::ChecksumMismatch { .. }
                | ProtocolError::InvalidMagic { .. }
                | ProtocolError::KeyDerivation(_)
                | ProtocolError::KeyDerivation(_)
        )
    }
}