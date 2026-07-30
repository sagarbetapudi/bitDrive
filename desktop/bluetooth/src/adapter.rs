//! Windows Bluetooth adapter wrapper

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info};
use uuid::Uuid;

use windows::Devices::Bluetooth::{BluetoothAdapter, BluetoothDevice, BluetoothRadioState};
use windows::Devices::Enumeration::{DeviceInformation, DeviceWatcher};
use windows::Foundation::TypedEventHandler;

use crate::{AdapterInfo, BluetoothDevice as BplBluetoothDevice, ConnectionParams, DeviceId};
use bpl_protocol::{ProtocolError, Result};

/// Windows Bluetooth adapter wrapper
pub struct WindowsBluetoothAdapter {
    adapter: BluetoothAdapter,
    device_watcher: Option<DeviceWatcher>,
    discovered_devices: Arc<RwLock<HashMap<String, BplBluetoothDevice>>>,
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

    /// Get adapter ID (Bluetooth address as string)
    pub fn adapter_id(&self) -> String {
        self.adapter.BluetoothAddress().unwrap_or(0).to_string()
    }

    /// Get adapter name
    pub fn name(&self) -> String {
        self.adapter.Name().unwrap_or_default().to_string()
    }

    /// Check if adapter is powered on
    pub fn is_powered(&self) -> bool {
        matches!(self.adapter.GetRadioState().unwrap_or_default(), BluetoothRadioState::On)
    }

    /// Get adapter info
    pub fn info(&self) -> AdapterInfo {
        AdapterInfo {
            id: self.adapter_id(),
            name: self.name(),
            address: self.adapter.BluetoothAddress().unwrap_or(0).to_string(),
            powered: self.is_powered(),
            discoverable: false, // Would need additional API
            pairable: false,
        }
    }

    /// Start device discovery
    pub async fn start_discovery(&mut self) -> Result<()> {
        let selector = BluetoothDevice::GetDeviceSelector();
        let watcher = DeviceInformation::CreateWatcher(selector)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let devices = self.discovered_devices.clone();
        let added_handler = TypedEventHandler::new(move |_, info: Option<&DeviceInformation>| {
            if let Some(info) = info {
                let id = info.Id().unwrap_or_default().to_string();
                let name = info.Name().unwrap_or_default().to_string();

                let mut device = BplBluetoothDevice {
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
                        device.paired = bt_device.DeviceInformation()
                            .and_then(|di| di.Pairing())
                            .and_then(|p| p.IsPaired())
                            .unwrap_or(false);
                    }
                }

                devices.write().insert(id, device);
            }
            Ok(())
        });

        watcher.Added(&added_handler)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        watcher.Start()
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        self.device_watcher = Some(watcher);
        info!("Started Bluetooth device discovery");
        Ok(())
    }

    /// Stop device discovery
    pub fn stop_discovery(&mut self) {
        if let Some(watcher) = self.device_watcher.take() {
            let _ = watcher.Stop();
        }
    }

    /// Get discovered devices
    pub fn get_discovered_devices(&self) -> Vec<BplBluetoothDevice> {
        self.discovered_devices.read().values().cloned().collect()
    }

    /// Pair with device
    pub async fn pair_device(&self, device_id: &DeviceId) -> Result<()> {
        let id_str = String::from_utf8_lossy(&device_id.value).to_string();

        // Find device info
        let device_info = DeviceInformation::CreateFromIdAsync(&id_str)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        if let Some(device_info) = device_info {
            let pairing = device_info.Pairing()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            let result = pairing.PairAsync()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
                .await
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            match result.Status() {
                windows::Devices::Enumeration::DevicePairingResultStatus::Paired |
                windows::Devices::Enumeration::DevicePairingResultStatus::AlreadyPaired => {
                    info!("Successfully paired with device {}", id_str);
                    Ok(())
                }
                _ => Err(ProtocolError::Bluetooth(format!("Pairing failed: {:?}", result.Status()))),
            }
        } else {
            Err(ProtocolError::Bluetooth("Device not found".to_string()))
        }
    }

    /// Unpair device
    pub async fn unpair_device(&self, device_id: &DeviceId) -> Result<()> {
        let id_str = String::from_utf8_lossy(&device_id.value).to_string();

        let device_info = DeviceInformation::CreateFromIdAsync(&id_str)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        if let Some(device_info) = device_info {
            let pairing = device_info.Pairing()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            let result = pairing.UnpairAsync()
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
                .await
                .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

            match result.Status() {
                windows::Devices::Enumeration::DeviceUnpairingResultStatus::Unpaired |
                windows::Devices::Enumeration::DeviceUnpairingResultStatus::AlreadyUnpaired => {
                    info!("Successfully unpaired device {}", id_str);
                    Ok(())
                }
                _ => Err(ProtocolError::Bluetooth(format!("Unpairing failed: {:?}", result.Status()))),
            }
        } else {
            Err(ProtocolError::Bluetooth("Device not found".to_string()))
        }
    }

    /// Get paired devices
    pub async fn get_paired_devices(&self) -> Result<Vec<BplBluetoothDevice>> {
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true);
        let devices = DeviceInformation::FindAllAsync(selector)
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
            .await
            .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

        let mut result = Vec::new();
        for device_info in devices {
            let id = device_info.Id().unwrap_or_default().to_string();
            let name = device_info.Name().unwrap_or_default().to_string();

            result.push(BplBluetoothDevice {
                id: DeviceId { value: id.as_bytes().to_vec() },
                name: Some(name),
                address: id,
                paired: true,
                connected: false,
                rssi: None,
                device_class: None,
                services: Vec::new(),
            });
        }

        Ok(result)
    }
}

/// Get default Windows Bluetooth adapter
pub async fn get_default_adapter() -> Result<WindowsBluetoothAdapter> {
    let adapter = BluetoothAdapter::GetDefaultAsync()
        .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
        .await
        .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

    adapter.ok_or_else(|| ProtocolError::Bluetooth("No Bluetooth adapter found".to_string()))
        .and_then(|a| WindowsBluetoothAdapter::new(a))
}

/// List all Windows Bluetooth adapters
pub async fn list_adapters() -> Result<Vec<AdapterInfo>> {
    let adapters = BluetoothAdapter::GetDeviceSelector();
    let devices = DeviceInformation::FindAllAsync(adapters)
        .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?
        .await
        .map_err(|e| ProtocolError::Bluetooth(e.to_string()))?;

    let mut result = Vec::new();
    for device_info in devices {
        if let Ok(adapter) = BluetoothAdapter::FromIdAsync(&device_info.Id().unwrap()) {
            if let Ok(adapter) = adapter.await {
                let wrapper = WindowsBluetoothAdapter::new(adapter).await?;
                result.push(wrapper.info());
            }
        }
    }

    Ok(result)
}