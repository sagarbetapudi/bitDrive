//! RFCOMM client implementation for Windows

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use windows::Devices::Bluetooth::Rfcomm::{RfcommDeviceService, RfcommServiceId};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Networking::Sockets::StreamSocket;
use windows::Storage::Streams::{DataReader, DataWriter, IInputStream, IOutputStream};
use windows::Foundation::{IAsyncOperation, TimeSpan};

use crate::{ClientConfig, RfcommStream, ConnectionParams};
use bpl_protocol::{DeviceId, ProtocolError, Result};

/// RFCOMM client for connecting to remote devices
pub struct RfcommClient {
    config: ClientConfig,
    socket: Option<StreamSocket>,
    connected: Arc<RwLock<bool>>,
    remote_device_id: Option<DeviceId>,
}

impl RfcommClient {
    /// Create a new RFCOMM client
    pub async fn new(config: ClientConfig) -> Result<Self> {
        Ok(Self {
            config,
            socket: None,
            connected: Arc::new(RwLock::new(false)),
            remote_device_id: None,
        })
    }

    /// Connect to a remote device
    pub async fn connect(&mut self, device_id: &DeviceId) -> Result<RfcommStream> {
        if *self.connected.read() {
            return Err(ProtocolError::Config("Already connected".to_string()));
        }

        let id_str = String::from_utf8_lossy(&device_id.value).to_string();

        // Find RFCOMM service on device
        let selector = RfcommDeviceService::GetDeviceSelector(RfcommServiceId::from_uuid(
            self.config.service_uuid.parse()
                .map_err(|e| ProtocolError::Config(format!("Invalid UUID: {}", e)))?
        ));

        let services = DeviceInformation::FindAllAsync(selector)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Find service for our device
        let mut target_service = None;
        for service in services {
            if service.Id().unwrap_or_default().to_string().contains(&id_str) {
                target_service = Some(service);
                break;
            }
        }

        let service_info = target_service.ok_or_else(|| {
            ProtocolError::Bluetooth("RFCOMM service not found on device".to_string())
        })?;

        // Get RFCOMM device service
        let rfcomm_service = RfcommDeviceService::FromIdAsync(&service_info.Id().unwrap())
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Create socket and connect
        let socket = StreamSocket::new()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Set connection timeout
        let connect_op = socket.ConnectAsync(
            rfcomm_service.ConnectionHostName().unwrap(),
            rfcomm_service.ConnectionServiceName().unwrap(),
        ).map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Wait with timeout
        timeout(self.config.connection_params.timeout_ms as u64, connect_op)
            .await
            .map_err(|_| ProtocolError::Timeout("Connection timeout".to_string()))?
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        self.socket = Some(socket);
        self.remote_device_id = Some(device_id.clone());
        *self.connected.write() = true;

        info!("Connected to device {}", id_str);

        Ok(RfcommStream::from_socket(self.socket.as_ref().unwrap().clone()))
    }

    /// Disconnect from remote device
    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(socket) = self.socket.take() {
            // Close socket by dropping
            drop(socket);
        }
        *self.connected.write() = false;
        self.remote_device_id = None;
        info!("Disconnected from remote device");
        Ok(())
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        *self.connected.read()
    }

    /// Get remote device ID
    pub fn remote_device_id(&self) -> Option<&DeviceId> {
        self.remote_device_id.as_ref()
    }

    /// Get connection parameters
    pub fn connection_params(&self) -> &ConnectionParams {
        &self.config.connection_params
    }
}

/// Create RFCOMM client with default config
pub async fn create_client(config: ClientConfig) -> Result<RfcommClient> {
    RfcommClient::new(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let config = ClientConfig::default();
        let client = RfcommClient::new(config).await;
        assert!(client.is_ok());
    }
}