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

        let uuid = Uuid::parse_str(&self.config.service_uuid)
            .map_err(|e| ProtocolError::Config(format!("Invalid UUID: {}", e)))?;
        let guid = windows::core::GUID::from_u128(uuid.as_u128());
        let service_id = RfcommServiceId::FromUuid(guid)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        let selector = RfcommDeviceService::GetDeviceSelector(&service_id)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let services = DeviceInformation::FindAllAsyncAqsFilter(&selector)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .get()
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
        let service_id_hstring = service_info.Id()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        let rfcomm_service = RfcommDeviceService::FromIdAsync(&service_id_hstring)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .get()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Create socket and connect
        let socket = StreamSocket::new()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let host_name = rfcomm_service.ConnectionHostName()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        let service_name = rfcomm_service.ConnectionServiceName()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        socket.ConnectAsync(&host_name, &service_name)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .get()
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