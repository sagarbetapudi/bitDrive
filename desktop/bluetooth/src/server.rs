//! RFCOMM server implementation for Windows

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use windows::Devices::Bluetooth::Rfcomm::RfcommServiceProvider;
use windows::Devices::Bluetooth::BluetoothServiceCapabilities;
use windows::Networking::Sockets::{StreamSocketListener, StreamSocket};
use windows::Storage::Streams::{DataReader, DataWriter, IInputStream, IOutputStream};
use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
use windows::Security::Cryptography::CryptographicBuffer;

use crate::{ServerConfig, RfcommStream, StreamConfig, ConnectionParams};
use bpl_protocol::{DeviceId, ProtocolError, Result};

/// RFCOMM server for accepting incoming connections
pub struct RfcommServer {
    provider: Option<RfcommServiceProvider>,
    listener: Option<StreamSocketListener>,
    config: ServerConfig,
    running: Arc<RwLock<bool>>,
    connection_tx: mpsc::Sender<RfcommStream>,
    connection_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<RfcommStream>>>,
    service_uuid: Uuid,
}

impl RfcommServer {
    /// Create a new RFCOMM server
    pub async fn new(config: ServerConfig) -> Result<Self> {
        let (connection_tx, connection_rx) = mpsc::channel(100);

        let service_uuid = config.service_uuid
            .parse::<Uuid>()
            .map_err(|e| ProtocolError::Config(format!("Invalid service UUID: {}", e)))?;

        Ok(Self {
            provider: None,
            listener: None,
            config,
            running: Arc::new(RwLock::new(false)),
            connection_tx,
            connection_rx: Arc::new(tokio::sync::Mutex::new(connection_rx)),
            service_uuid,
        })
    }

    /// Start the RFCOMM server
    pub async fn start(&mut self) -> Result<()> {
        if *self.running.read() {
            return Err(ProtocolError::Config("Server already running".to_string()));
        }

        let guid = windows::core::GUID::from_u128(self.service_uuid.as_u128());
        let service_id = windows::Devices::Bluetooth::Rfcomm::RfcommServiceId::FromUuid(guid)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Create RFCOMM service provider
        let provider = RfcommServiceProvider::CreateAsync(&service_id)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .get()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Create socket listener
        let listener = StreamSocketListener::new()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Set up connection received handler
        let connection_tx = self.connection_tx.clone();
        let handler = TypedEventHandler::new(move |_, args: &Option<windows::Networking::Sockets::StreamSocketListenerConnectionReceivedEventArgs>| {
            if let Some(args) = args {
                if let Ok(socket) = args.Socket() {
                    let stream = RfcommStream::from_socket(socket);
                    let _ = connection_tx.try_send(stream);
                }
            }
            Ok(())
        });

        listener.ConnectionReceived(&handler)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Bind to service provider
        let service_id_str = provider.ServiceId()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .AsString()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        listener.BindServiceNameAsync(&service_id_str)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .get()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        // Start advertising
        provider.StartAdvertising(&listener)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        self.provider = Some(provider);
        self.listener = Some(listener);
        *self.running.write() = true;

        info!("RFCOMM server started on service {}", self.service_uuid);
        Ok(())
    }

    /// Stop the RFCOMM server
    pub async fn stop(&mut self) -> Result<()> {
        if !*self.running.read() {
            return Ok(());
        }

        if let Some(provider) = self.provider.take() {
            provider.StopAdvertising()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;
        }

        if let Some(listener) = self.listener.take() {
            // StreamSocketListener doesn't have explicit close, just drop
            drop(listener);
        }

        *self.running.write() = false;
        info!("RFCOMM server stopped");
        Ok(())
    }

    /// Accept incoming connection
    pub async fn accept(&self) -> Result<RfcommStream> {
        let mut rx = self.connection_rx.lock().await;
        rx.recv().await
            .ok_or_else(|| ProtocolError::Bluetooth("Server stopped".to_string()))
    }

    /// Check if server is running
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Get service UUID
    pub fn service_uuid(&self) -> Uuid {
        self.service_uuid
    }

    /// Get server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }
}

/// Create RFCOMM server with default config
pub async fn create_server(config: ServerConfig) -> Result<RfcommServer> {
    RfcommServer::new(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_creation() {
        let config = ServerConfig::default();
        let server = RfcommServer::new(config).await;
        assert!(server.is_ok());
    }
}