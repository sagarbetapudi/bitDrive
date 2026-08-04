//! Windows Bluetooth RFCOMM implementation using Windows.Devices.Bluetooth APIs

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use windows::Devices::Bluetooth::{
    BluetoothAdapter, BluetoothDevice, BluetoothConnectionStatus, BluetoothPairingResultStatus,
};
use windows::Devices::Bluetooth::Rfcomm::{RfcommServiceProvider, RfcommServiceProviderStatus, RfcommDeviceService};
use windows::Devices::Enumeration::{DeviceInformation, DeviceWatcher, DeviceWatcherStatus};
use windows::Foundation::{EventRegistrationToken, IAsyncOperation, TypedEventHandler};
use windows::Storage::Streams::{DataReader, DataWriter, IInputStream, IOutputStream};
use windows::Networking::Sockets::{StreamSocket, StreamSocketListener};
use windows::Security::Authentication::Identity::Core::GetForCurrentProcess;
use windows::Foundation::Collections::IVectorView;

use crate::{AdapterInfo, BluetoothAdapter, BluetoothDevice, ClientConfig, ConnectionParams, RfcommClient, RfcommServer, ServerConfig, DeviceId};
use bpl_protocol::{ProtocolError, Result};

/// Windows Bluetooth adapter wrapper
pub struct WindowsBluetoothAdapter {
    adapter: BluetoothAdapter,
    device_watcher: Option<DeviceWatcher>,
    discovered_devices: Arc<RwLock<HashMap<String, BluetoothDevice>>>,
}

impl WindowsBluetoothAdapter {
    /// Create from Windows BluetoothAdapter
    pub async fn new(adapter: BluetoothAdapter) -> Result<Self> {
        Ok(Self {
            adapter,
            device_watcher: None,
            discovered_devices: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Get adapter ID
    pub fn adapter_id(&self) -> String {
        self.adapter.BluetoothAddress().unwrap_or(0).to_string()
    }

    /// Get adapter name
    pub fn name(&self) -> String {
        self.adapter.Name().unwrap_or_default().to_string()
    }

    /// Check if adapter is powered on
    pub fn is_powered(&self) -> bool {
        matches!(self.adapter.GetRadioState().unwrap_or_default(), windows::Devices::Bluetooth::BluetoothRadioState::On)
    }

    /// Start device discovery
    pub async fn start_discovery(&mut self) -> Result<()> {
        let selector = BluetoothDevice::GetDeviceSelector();
        let watcher = DeviceInformation::CreateWatcher(selector)?;

        let devices = self.discovered_devices.clone();
        let added_handler = TypedEventHandler::new(move |_, info: Option<&DeviceInformation>| {
            if let Some(info) = info {
                let id = info.Id().unwrap_or_default().to_string();
                let name = info.Name().unwrap_or_default().to_string();
                let mut device = BluetoothDevice {
                    id: DeviceId { value: id.as_bytes().to_vec() },
                    name: Some(name),
                    address: id.clone(),
                    paired: false,
                    connected: false,
                    rssi: None,
                    device_class: None,
                    services: Vec::new(),
                };

                // Check if paired
                if let Ok(bt_device) = BluetoothDevice::FromIdAsync(&info.Id().unwrap()) {
                    if let Ok(bt_device) = bt_device.await {
                        device.paired = bt_device.DeviceInformation().unwrap().Pairing().unwrap().IsPaired().unwrap_or(false);
                        device.connected = matches!(bt_device.ConnectionStatus().unwrap_or_default(), BluetoothConnectionStatus::Connected);
                    }
                }

                devices.write().insert(id, device);
            }
            Ok(())
        })?;

        let removed_handler = TypedEventHandler::new(move |_, info: Option<&DeviceInformation>| {
            if let Some(info) = info {
                let id = info.Id().unwrap_or_default().to_string();
                devices.write().remove(&id);
            }
            Ok(())
        })?;

        watcher.Added(&added_handler)?;
        watcher.Removed(&removed_handler)?;
        watcher.Start()?;

        self.device_watcher = Some(watcher);
        Ok(())
    }

    /// Stop device discovery
    pub fn stop_discovery(&mut self) {
        if let Some(watcher) = self.device_watcher.take() {
            let _ = watcher.Stop();
        }
    }

    /// Get discovered devices
    pub fn get_discovered_devices(&self) -> Vec<BluetoothDevice> {
        self.discovered_devices.read().values().cloned().collect()
    }

    /// Get paired devices
    pub async fn get_paired_devices(&self) -> Result<Vec<BluetoothDevice>> {
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true);
        let devices = DeviceInformation::FindAllAsync(selector)?.await?;

        let mut result = Vec::new();
        for info in devices {
            let id = info.Id().unwrap_or_default().to_string();
            let name = info.Name().unwrap_or_default().to_string();

            let mut device = BluetoothDevice {
                id: DeviceId { value: id.as_bytes().to_vec() },
                name: Some(name),
                address: id.clone(),
                paired: true,
                connected: false,
                rssi: None,
                device_class: None,
                services: Vec::new(),
            };

            // Check connection status
            if let Ok(bt_device) = BluetoothDevice::FromIdAsync(&info.Id().unwrap()) {
                if let Ok(bt_device) = bt_device.await {
                    device.connected = matches!(bt_device.ConnectionStatus().unwrap_or_default(), BluetoothConnectionStatus::Connected);
                }
            }

            result.push(device);
        }

        Ok(result)
    }

    /// Pair with device
    pub async fn pair_device(&self, device_id: &DeviceId) -> Result<()> {
        let id_str = String::from_utf8_lossy(&device_id.value);
        let bt_device = BluetoothDevice::FromIdAsync(&id_str)?.await?;

        let pairing = bt_device.DeviceInformation()?.Pairing()?;
        let result = pairing.PairAsync()?.await?;

        match result.Status() {
            BluetoothPairingResultStatus::Paired | BluetoothPairingResultStatus::AlreadyPaired => Ok(()),
            status => Err(ProtocolError::Protocol(format!("Pairing failed: {:?}", status))),
        }
    }

    /// Unpair device
    pub async fn unpair_device(&self, device_id: &DeviceId) -> Result<()> {
        let id_str = String::from_utf8_lossy(&device_id.value);
        let bt_device = BluetoothDevice::FromIdAsync(&id_str)?.await?;

        let pairing = bt_device.DeviceInformation()?.Pairing()?;
        let result = pairing.UnpairAsync()?.await?;

        if result.Status() == BluetoothPairingResultStatus::Unpaired {
            Ok(())
        } else {
            Err(ProtocolError::Protocol("Unpairing failed".to_string()))
        }
    }
}

/// Windows RFCOMM server
pub struct WindowsRfcommServer {
    provider: RfcommServiceProvider,
    listener: StreamSocketListener,
    service_id: Uuid,
    connection_tx: mpsc::Sender<RfcommClient>,
    connection_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<RfcommClient>>>,
}

impl WindowsRfcommServer {
    /// Create new RFCOMM server
    pub async fn new(config: ServerConfig) -> Result<Self> {
        let service_id = Uuid::parse_str(&config.service_uuid)
            .map_err(|_| ProtocolError::InvalidArgument("Invalid service UUID".to_string()))?;

        // Create RFCOMM service provider
        let provider = RfcommServiceProvider::CreateAsync(service_id)?.await?;

        if provider.Status() != RfcommServiceProviderStatus::Success {
            return Err(ProtocolError::Protocol(format!(
                "Failed to create RFCOMM service: {:?}",
                provider.Status()
            )));
        }

        // Create socket listener
        let listener = StreamSocketListener::new()?;
        listener.Control().SetQualityOfService(windows::Networking::Sockets::SocketQualityOfService::Normal)?;

        // Set up connection handler
        let (tx, rx) = mpsc::channel(32);

        let connection_handler = TypedEventHandler::new(move |_, args: Option<&windows::Networking::Sockets::StreamSocketListenerConnectionReceivedEventArgs>| {
            if let Some(args) = args {
                let socket = args.Socket()?;
                let client = WindowsRfcommClient::from_socket(socket);
                let _ = tx.try_send(client);
            }
            Ok(())
        })?;

        listener.ConnectionReceived(&connection_handler)?;

        // Bind to service
        listener.BindServiceNameAsync(provider.ServiceId()?.Name()?)?.await?;

        // Start advertising
        provider.StartAdvertising(
            provider.ServiceId()?.Name()?,
            windows::Devices::Bluetooth::Rfcomm::RfcommServiceProviderAdvertisementMode::Standard,
        )?;

        info!("RFCOMM server started on service ID: {}", service_id);

        Ok(Self {
            provider,
            listener,
            service_id,
            connection_tx: tx,
            connection_rx: Arc::new(tokio::sync::Mutex::new(rx)),
        })
    }

    /// Accept incoming connection
    pub async fn accept(&self) -> Result<RfcommClient> {
        let mut rx = self.connection_rx.lock().await;
        rx.recv().await.ok_or(ProtocolError::NotConnected)
    }

    /// Get service ID
    pub fn service_id(&self) -> Uuid {
        self.service_id
    }

    /// Stop server
    pub async fn stop(&self) -> Result<()> {
        self.provider.StopAdvertising()?;
        Ok(())
    }
}

/// Windows RFCOMM client
pub struct WindowsRfcommClient {
    socket: StreamSocket,
    reader: DataReader,
    writer: DataWriter,
    remote_address: String,
    remote_service_id: Uuid,
}

impl WindowsRfcommClient {
    /// Create from connected socket
    pub fn from_socket(socket: StreamSocket) -> Self {
        let reader = DataReader::CreateDataReader(socket.InputStream()?)?;
        let writer = DataWriter::CreateDataWriter(socket.OutputStream()?)?;

        let remote_address = socket.Information()?.RemoteAddress()?.ToString()?.to_string();
        let remote_service_id = socket.Information()?.RemoteServiceName()?.to_string()
            .parse().unwrap_or(Uuid::nil());

        Self {
            socket,
            reader,
            writer,
            remote_address,
            remote_service_id,
        }
    }

    /// Connect to remote device
    pub async fn connect(config: ClientConfig) -> Result<Self> {
        let device_id = String::from_utf8_lossy(&config.device_id.value);
        let service_id = Uuid::parse_str(&config.service_uuid)
            .map_err(|_| ProtocolError::InvalidArgument("Invalid service UUID".to_string()))?;

        // Get device service
        let bt_device = BluetoothDevice::FromIdAsync(&device_id)?.await?;
        let services = bt_device.GetRfcommServicesAsync()?.await?;

        let service = services
            .iter()
            .find(|s| s.ServiceId().unwrap() == service_id)
            .ok_or(ProtocolError::ServiceNotFound {
                service_id: config.service_uuid.clone(),
            })?;

        // Connect socket
        let socket = StreamSocket::new()?;
        socket.Control().SetQualityOfService(windows::Networking::Sockets::SocketQualityOfService::Normal)?;

        socket.ConnectAsync(service.ConnectionHostName()?, service.ConnectionServiceName()?)?.await?;

        Ok(Self::from_socket(socket))
    }

    /// Read data
    pub async fn read(&mut self, buffer: &mut [u8]) -> Result<usize> {
        self.reader.LoadAsync(buffer.len() as u32)?.await?;
        let count = self.reader.UnconsumedBufferLength()? as usize;
        let data = self.reader.ReadBytes(count)?;
        buffer[..count].copy_from_slice(&data);
        Ok(count)
    }

    /// Write data
    pub async fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.writer.WriteBytes(data)?;
        self.writer.StoreAsync()?.await?;
        Ok(data.len())
    }

    /// Flush write buffer
    pub async fn flush(&mut self) -> Result<()> {
        self.writer.FlushAsync()?.await?;
        Ok(())
    }

    /// Close connection
    pub async fn close(&mut self) -> Result<()> {
        self.socket.Close()?;
        Ok(())
    }

    /// Get remote address
    pub fn remote_address(&self) -> &str {
        &self.remote_address
    }

    /// Get remote service ID
    pub fn remote_service_id(&self) -> Uuid {
        self.remote_service_id
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.socket.Information().map(|i| i.RemoteAddress().is_ok()).unwrap_or(false)
    }
}

/// Get default Windows Bluetooth adapter
pub async fn get_default_adapter() -> Result<BluetoothAdapter> {
    let adapter = BluetoothAdapter::GetDefaultAsync()?.await?;
    Ok(BluetoothAdapter::Windows(WindowsBluetoothAdapter::new(adapter).await?))
}

/// List all Windows Bluetooth adapters
pub async fn list_adapters() -> Result<Vec<AdapterInfo>> {
    let adapters = DeviceInformation::FindAllAsync(BluetoothAdapter::GetDeviceSelector())?.await?;

    let mut result = Vec::new();
    for info in adapters {
        if let Ok(adapter) = BluetoothAdapter::FromIdAsync(&info.Id()?) {
            if let Ok(adapter) = adapter.await() {
                result.push(AdapterInfo {
                    id: adapter.BluetoothAddress().unwrap_or(0).to_string(),
                    name: adapter.Name().unwrap_or_default().to_string(),
                    address: format!("{:012X}", adapter.BluetoothAddress().unwrap_or(0)),
                    powered: matches!(adapter.GetRadioState().unwrap_or_default(), windows::Devices::Bluetooth::BluetoothRadioState::On),
                    discoverable: false, // Would need additional API
                    pairable: false,
                });
            }
        }
    }

    Ok(result)
}

/// Scan for Bluetooth devices
pub async fn scan_devices(adapter_id: &str, duration_sec: u32) -> Result<Vec<BluetoothDevice>> {
    let adapter = BluetoothAdapter::FromIdAsync(&adapter_id)?.await?;
    let mut wrapper = WindowsBluetoothAdapter::new(adapter).await?;

    wrapper.start_discovery().await?;

    // Wait for discovery duration
    tokio::time::sleep(Duration::from_secs(duration_sec as u64)).await;

    let devices = wrapper.get_discovered_devices();
    wrapper.stop_discovery();

    Ok(devices)
}

/// Pair with device
pub async fn pair_device(adapter_id: &str, device_id: &DeviceId) -> Result<()> {
    let adapter = BluetoothAdapter::FromIdAsync(&adapter_id)?.await?;
    let wrapper = WindowsBluetoothAdapter::new(adapter).await?;
    wrapper.pair_device(device_id).await
}

/// Unpair device
pub async fn unpair_device(adapter_id: &str, device_id: &DeviceId) -> Result<()> {
    let adapter = BluetoothAdapter::FromIdAsync(&adapter_id)?.await?;
    let wrapper = WindowsBluetoothAdapter::new(adapter).await?;
    wrapper.unpair_device(device_id).await
}

/// Get paired devices
pub async fn get_paired_devices(adapter_id: &str) -> Result<Vec<BluetoothDevice>> {
    let adapter = BluetoothAdapter::FromIdAsync(&adapter_id)?.await?;
    let wrapper = WindowsBluetoothAdapter::new(adapter).await?;
    wrapper.get_paired_devices().await
}