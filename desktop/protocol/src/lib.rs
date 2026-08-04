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
    include!("pb/bpl.protocol.rs");
}
pub use pb::*;

// Re-export commonly used types
pub use error::{ProtocolError, Result};
pub use frame::{Frame, FrameCodec};
pub use session::{Session, SessionConfig, SessionState, SessionEvent, SessionManager};
pub use capability::CapabilityNegotiator;
pub use auth::{AuthManager, SessionKeys, ChannelKeys};
pub use mux::{ChannelManager, Channel, ChannelEvent, ChannelStats};
pub use registry::ServiceRegistry;
pub use service::{Service, ServiceContext, ServiceRequest, ServiceResponse, ServiceManager, ServiceStatus};

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

/// Maximum frame payload size (16KB)
pub const MAX_FRAME_PAYLOAD_SIZE: usize = 16384;

/// Service identifiers and versions (moved from .proto files for Proto3 compliance)
pub mod service_ids {
    pub const FILESYSTEM: &str = "bpl.filesystem";
    pub const FILESYSTEM_VERSION: u32 = 1;

    pub const SYNC: &str = "bpl.sync";
    pub const SYNC_VERSION: u32 = 1;

    pub const PHOTO_BACKUP: &str = "bpl.photo_backup";
    pub const PHOTO_BACKUP_VERSION: u32 = 1;

    pub const SHELL: &str = "bpl.shell";
    pub const SHELL_VERSION: u32 = 1;

    pub const MEDIA_CONTROL: &str = "bpl.media_control";
    pub const MEDIA_CONTROL_VERSION: u32 = 1;

    pub const PHONE_FS: &str = "bpl.phone_fs";
    pub const PHONE_FS_VERSION: u32 = 1;

    pub const PROXIMITY: &str = "bpl.proximity";
    pub const PROXIMITY_VERSION: u32 = 1;

    pub const FILE_STREAM: &str = "bpl.file_stream";
    pub const FILE_STREAM_VERSION: u32 = 1;

    pub const APP_LAUNCHER: &str = "bpl.app_launcher";
    pub const APP_LAUNCHER_VERSION: u32 = 1;

    pub const CONFIG: &str = "bpl.config";
    pub const CONFIG_VERSION: u32 = 1;
}