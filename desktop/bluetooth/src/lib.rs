//! Bluetooth RFCOMM abstraction layer
//!
//! Provides platform-agnostic Bluetooth RFCOMM functionality with
//! Windows implementation using Windows.Devices.Bluetooth.Rfcomm APIs.

pub mod platform;
pub mod adapter;
pub mod server;
pub mod client;
pub mod stream;

pub use adapter::{BluetoothAdapter, AdapterInfo, AdapterState};
pub use server::{RfcommServer, ServerConfig};
pub use client::{RfcommClient, ClientConfig};
pub use stream::{RfcommStream, StreamConfig};

use crate::platform::PlatformBluetooth;
use bpl_protocol::{DeviceId, ProtocolError, Result};

/// Bluetooth manager - main entry point for Bluetooth operations
pub struct BluetoothManager {
    platform: Box<dyn PlatformBluetooth>,
    adapter: Option<BluetoothAdapter>,
}

impl BluetoothManager {
    /// Create a new Bluetooth manager
    pub async fn new() -> Result<Self> {
        let platform = PlatformBluetooth::new().await?;
        Ok(Self {
            platform,
            adapter: None,
        })
    }

    /// Get the default Bluetooth adapter
    pub async fn get_default_adapter(&mut self) -> Result<BluetoothAdapter> {
        if let Some(adapter) = &self.adapter {
            return Ok(adapter.clone());
        }

        let adapter = self.platform.get_default_adapter().await?;
        self.adapter = Some(adapter.clone());
        Ok(adapter)
    }

    /// Get adapter by ID
    pub async fn get_adapter(&mut self, adapter_id: &str) -> Result<BluetoothAdapter> {
        let adapter = self.platform.get_adapter(adapter_id).await?;
        self.adapter = Some(adapter.clone());
        Ok(adapter)
    }

    /// List all available adapters
    pub async fn list_adapters(&self) -> Result<Vec<AdapterInfo>> {
        self.platform.list_adapters().await
    }

    /// Create RFCOMM server
    pub async fn create_server(&self, config: ServerConfig) -> Result<RfcommServer> {
        self.platform.create_server(config).await
    }

    /// Create RFCOMM client
    pub async fn create_client(&self, config: ClientConfig) -> Result<RfcommClient> {
        self.platform.create_client(config).await
    }

    /// Get platform info
    pub fn platform_name(&self) -> &'static str {
        self.platform.platform_name()
    }
}

/// Bluetooth device information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BluetoothDevice {
    pub id: DeviceId,
    pub name: Option<String>,
    pub address: String,
    pub paired: bool,
    pub connected: bool,
    pub rssi: Option<i16>,
    pub device_class: Option<u32>,
    pub services: Vec<String>,
}

/// RFCOMM service UUID for BPL
pub const BPL_RFCOMM_SERVICE_UUID: &str = "00001101-0000-1000-8000-00805F9B34FB"; // Serial Port Profile

/// Custom BPL service UUID (for dedicated service)
pub const BPL_CUSTOM_SERVICE_UUID: &str = "B7E5E0F0-1A2B-4C3D-8E9F-A0B1C2D3E4F5";

/// Connection parameters
#[derive(Debug, Clone)]
pub struct ConnectionParams {
    pub mtu: u16,
    pub timeout_ms: u32,
    pub retry_count: u32,
    pub retry_delay_ms: u32,
}

impl Default for ConnectionParams {
    fn default() -> Self {
        Self {
            mtu: 1024,
            timeout_ms: 10000,
            retry_count: 3,
            retry_delay_ms: 1000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_params_default() {
        let params = ConnectionParams::default();
        assert_eq!(params.mtu, 1024);
        assert_eq!(params.timeout_ms, 10000);
    }
}