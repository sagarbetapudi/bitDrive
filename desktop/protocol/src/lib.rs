//! Bluetooth Personal Link Protocol Library
//!
//! This crate implements the custom binary protocol for communication between
//! the desktop and Android components over Bluetooth RFCOMM.
//!
//! The protocol provides:
//! - Frame-level framing with CRC32C checksums
//! - Session management with capability negotiation
//! - Mutual authentication with PSK
//! - Multiplexed logical channels with flow control
//! - Service registry for dynamic service discovery

pub mod auth;
pub mod capability;
pub mod codec;
pub mod error;
pub mod frame;
pub mod mux;
pub mod registry;
pub mod session;
pub mod service;

// Re-export generated protobuf types
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/bpl.protocol.rs"));
}

// Re-export commonly used types
pub use error::{ProtocolError, Result};
pub use frame::{Frame, FrameHeader, FrameType, FrameFlags, FrameCodec, MAX_FRAME_PAYLOAD_SIZE};
pub use session::{Session, SessionConfig, SessionState, SessionEvent, SessionManager};
pub use capability::{CapabilitySet, CapabilityNegotiator, NegotiatedCapability, ServiceCapability};
pub use auth::{AuthManager, AuthMethod, SessionKeys, ChannelKeys};
pub use mux::{ChannelManager, Channel, ChannelConfig, ChannelType, ChannelPriority, FlowControl};
pub use registry::{ServiceRegistry, ServiceInfo, ServiceCapability as RegistryServiceCapability};
pub use service::{Service, ServiceContext, ServiceRequest, ServiceResponse};

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 0x00010000; // 1.0.0

/// Magic number for frame identification (0x42504C01 = "BPL\x01")
pub const MAGIC_NUMBER: u32 = 0x42504C01;

/// Maximum frame size (64KB)
pub const MAX_FRAME_SIZE: usize = 65535;

/// Control channel ID (always 0)
pub const CONTROL_CHANNEL_ID: u32 = 0;

/// Default keepalive interval (30 seconds)
pub const DEFAULT_KEEPALIVE_INTERVAL_MS: u32 = 30_000;

/// Default session timeout (90 seconds)
pub const DEFAULT_SESSION_TIMEOUT_MS: u32 = 90_000;

/// Maximum number of logical channels
pub const MAX_CHANNELS: u32 = 16;

/// Default flow control window size (64KB)
pub const DEFAULT_WINDOW_SIZE: u32 = 65536;

/// Default maximum frame payload size (16KB)
pub const DEFAULT_MAX_FRAME_SIZE: u32 = 16384;