//! Configuration management for BPL Desktop core/src/config.rs

use std::path::PathBuf;
use std::sync::Arc;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use config::{Config, File, FileFormat, Environment};
use parking_lot::RwLock;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::info;

use bpl_protocol::{DeviceId, ProtocolError, Result};

/// Configuration manager
#[derive(Clone)]
pub struct ConfigManager {
    config: Arc<RwLock<AppConfig>>,
    config_path: PathBuf,
}

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub bluetooth: BluetoothConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    pub security: SecurityConfig,
    pub services: ServicesConfig,
    pub device: DeviceConfig,
}

/// Bluetooth configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BluetoothConfig {
    pub adapter_id: Option<String>,
    pub service_uuid: String,
    pub device_name: String,
    pub auto_connect: bool,
    pub reconnect_interval_sec: u64,
    pub max_reconnect_attempts: u32,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
    pub max_connections: u32,
    pub backup_enabled: bool,
    pub backup_interval_hours: u32,
    pub backup_dir: Option<String>,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: String, // "json", "text", "compact"
    pub output: String, // "stdout", "file", "both"
    pub file_path: Option<String>,
    pub max_file_size_mb: u64,
    pub max_files: u32,
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub psk: Option<String>, // Base64 encoded PSK
    pub require_authentication: bool,
    pub session_timeout_sec: u64,
    pub max_failed_attempts: u32,
    pub lockout_duration_sec: u64,
}

/// Services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicesConfig {
    pub filesystem: ServiceSettings,
    pub sync: ServiceSettings,
    pub photo_backup: ServiceSettings,
    pub shell: ServiceSettings,
    pub media_control: ServiceSettings,
    pub phone_fs: ServiceSettings,
    pub proximity: ServiceSettings,
    pub file_stream: ServiceSettings,
    pub app_launcher: ServiceSettings,
    pub config: ServiceSettings,
}

/// Individual service settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSettings {
    pub enabled: bool,
    pub settings: serde_json::Value,
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_id: Option<String>, // Base64 encoded
    pub trusted_devices: Vec<TrustedDevice>,
}

/// Trusted device entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub device_id: String, // Base64 encoded
    pub name: String,
    pub paired_at: String, // ISO 8601
    pub last_seen: String, // ISO 8601
    pub trusted: bool,
    pub psk: Option<String>, // Base64 encoded (optional, for device-specific PSK)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bluetooth: BluetoothConfig::default(),
            database: DatabaseConfig::default(),
            logging: LoggingConfig::default(),
            security: SecurityConfig::default(),
            services: ServicesConfig::default(),
            device: DeviceConfig::default(),
        }
    }
}

impl Default for BluetoothConfig {
    fn default() -> Self {
        Self {
            adapter_id: None,
            service_uuid: "B7E5E0F0-1A2B-4C3D-8E9F-A0B1C2D3E4F5".to_string(),
            device_name: "BPL Desktop".to_string(),
            auto_connect: true,
            reconnect_interval_sec: 30,
            max_reconnect_attempts: 10,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "bpl.db".to_string(),
            max_connections: 5,
            backup_enabled: true,
            backup_interval_hours: 24,
            backup_dir: None,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "json".to_string(),
            output: "stdout".to_string(),
            file_path: None,
            max_file_size_mb: 100,
            max_files: 10,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            psk: None,
            require_authentication: true,
            session_timeout_sec: 3600,
            max_failed_attempts: 5,
            lockout_duration_sec: 300,
        }
    }
}

impl Default for ServicesConfig {
    fn default() -> Self {
        Self {
            filesystem: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            sync: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            photo_backup: ServiceSettings { enabled: false, settings: serde_json::json!({}) },
            shell: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            media_control: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            phone_fs: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            proximity: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            file_stream: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            app_launcher: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
            config: ServiceSettings { enabled: true, settings: serde_json::json!({}) },
        }
    }
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            trusted_devices: Vec::new(),
        }
    }
}

impl ConfigManager {
    /// Load configuration from default location
    pub async fn load() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");
        Self::load_from_path(&config_path).await
    }

    /// Load configuration from specific path
    pub async fn load_from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let config_path = path.as_ref().to_path_buf();

        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ProtocolError::Config(e.to_string()))?;
        }

        let mut builder = Config::builder();

        if config_path.exists() {
        builder = builder.add_source(
            File::new(
                config_path.to_string_lossy().as_ref(),
                FileFormat::Toml,
            )
        );
    }

        builder = builder.add_source(
        Environment::with_prefix("BPL").separator("__")
    );

        let config = builder
        .build()
        .map_err(|e| ProtocolError::Config(e.to_string()))?;

        let app_config: AppConfig = config
        .try_deserialize()
        .map_err(|e| ProtocolError::Config(e.to_string()))?;

        let manager = Self {
        config: Arc::new(RwLock::new(app_config)),
        config_path,
    };

        if !manager.config_path.exists() {
        manager.save().await?;
    }

        Ok(manager)
    }

    /// Get config directory
    pub fn config_dir() -> Result<PathBuf> {
        if let Some(config_dir) = dirs::config_dir() {
            Ok(config_dir.join("bluetooth-personal-link"))
        } else {
            Err(ProtocolError::Config("Could not determine config directory".to_string()))
        }
    }

    /// Get data directory
    pub fn data_dir() -> Result<PathBuf> {
        if let Some(data_dir) = dirs::data_dir() {
            Ok(data_dir.join("bluetooth-personal-link"))
        } else {
            Err(ProtocolError::Config("Could not determine data directory".to_string()))
        }
    }

    /// Get config snapshot
    pub fn get(&self) -> AppConfig {
        self.config.read().clone()
    }

    /// Get mutable config reference
    pub fn get_mut(&self) -> parking_lot::RwLockWriteGuard<AppConfig> {
        self.config.write()
    }

    /// Update config
    pub fn update<F>(&self, f: F) where F: FnOnce(&mut AppConfig) {
        f(&mut self.config.write());
    }

    /// Save configuration to file
    pub async fn save(&self) -> Result<()> {
        let config = self.config.read();
        let toml = toml::to_string_pretty(&*config)
            .map_err(|e| ProtocolError::Config(e.to_string()))?;

        fs::write(&self.config_path, toml).await
            .map_err(|e| ProtocolError::Config(e.to_string()))?;

        info!("Configuration saved to {:?}", self.config_path);
        Ok(())
    }

    /// Get or create device ID
    pub async fn get_or_create_device_id(&self) -> Result<DeviceId> {
        let mut config = self.config.write();

        if let Some(device_id_str) = &config.device.device_id {
            // Decode existing device ID
            let bytes = STANDARD
            .decode(device_id_str)
            .map_err(|e| ProtocolError::Config(format!("Invalid device ID: {}", e)))?;
            if bytes.len() == 16 {
                return Ok(DeviceId { value: bytes });
            }
        }

        // Generate new device ID
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        let device_id = DeviceId { value: bytes.to_vec() };

        // Save as base64
        config.device.device_id = Some(STANDARD.encode(bytes));
        drop(config);
        self.save().await?;

        Ok(device_id)
    }

    /// Get PSK
    pub fn get_psk(&self) -> Option<Vec<u8>> {
        self.config
            .read()
            .security
            .psk
            .as_ref()
            .and_then(|s| STANDARD.decode(s).ok())
    }

    /// Set PSK
    pub async fn set_psk(&self, psk: Vec<u8>) -> Result<()> {
        let mut config = self.config.write();
        config.security.psk = Some(STANDARD.encode(&psk));
        drop(config);
        self.save().await
    }

    /// Add trusted device
    pub async fn add_trusted_device(&self, device: TrustedDevice) -> Result<()> {
        let mut config = self.config.write();

        // Remove existing entry if present
        config.device.trusted_devices.retain(|d| d.device_id != device.device_id);

        // Add new entry
        config.device.trusted_devices.push(device);
        drop(config);
        self.save().await
    }

    /// Remove trusted device
    pub async fn remove_trusted_device(&self, device_id: &str) -> Result<()> {
        let mut config = self.config.write();
        config.device.trusted_devices.retain(|d| d.device_id != device_id);
        drop(config);
        self.save().await
    }

    /// Get trusted devices
    pub fn get_trusted_devices(&self) -> Vec<TrustedDevice> {
        self.config.read().device.trusted_devices.clone()
    }

    /// Get trusted device by ID
    pub fn get_trusted_device(&self, device_id: &str) -> Option<TrustedDevice> {
        self.config.read().device.trusted_devices.iter()
            .find(|d| d.device_id == device_id)
            .cloned()
    }

    /// Update device last seen
    pub async fn update_device_last_seen(&self, device_id: &str) -> Result<()> {
        let mut config = self.config.write();
        if let Some(device) = config.device.trusted_devices.iter_mut()
            .find(|d| d.device_id == device_id) {
            device.last_seen = chrono::Utc::now().to_rfc3339();
        }
        drop(config);
        self.save().await
    }

    /// Get service settings
    pub fn get_service_settings(&self, service: &str) -> Option<ServiceSettings> {
        let config = self.config.read();
        let services = &config.services;

        match service {
            "filesystem" => Some(services.filesystem.clone()),
            "sync" => Some(services.sync.clone()),
            "photo_backup" => Some(services.photo_backup.clone()),
            "shell" => Some(services.shell.clone()),
            "media_control" => Some(services.media_control.clone()),
            "phone_fs" => Some(services.phone_fs.clone()),
            "proximity" => Some(services.proximity.clone()),
            "file_stream" => Some(services.file_stream.clone()),
            "app_launcher" => Some(services.app_launcher.clone()),
            "config" => Some(services.config.clone()),
            _ => None,
        }
    }

    /// Update service settings
    pub async fn update_service_settings(&self, service: &str, settings: ServiceSettings) -> Result<()> {
        let mut config = self.config.write();

        match service {
            "filesystem" => config.services.filesystem = settings,
            "sync" => config.services.sync = settings,
            "photo_backup" => config.services.photo_backup = settings,
            "shell" => config.services.shell = settings,
            "media_control" => config.services.media_control = settings,
            "phone_fs" => config.services.phone_fs = settings,
            "proximity" => config.services.proximity = settings,
            "file_stream" => config.services.file_stream = settings,
            "app_launcher" => config.services.app_launcher = settings,
            "config" => config.services.config = settings,
            _ => return Err(ProtocolError::Config(format!("Unknown service: {}", service))),
        }

        drop(config);
        self.save().await
    }
}

impl Default for ConfigManager {
    fn default() -> Self {
        // This shouldn't be used directly, use load() instead
        Self {
            config: Arc::new(RwLock::new(AppConfig::default())),
            config_path: PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_config_load() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        // Create minimal config
        let config = AppConfig::default();
        let toml = toml::to_string_pretty(&config).unwrap();
        fs::write(&config_path, toml).await.unwrap();

        // Load config
        let manager = ConfigManager::load_from_path(&config_path)
        .await
        .unwrap();
        let loaded = manager.get();

        assert_eq!(loaded.bluetooth.device_name, "BPL Desktop");
        assert!(loaded.services.filesystem.enabled);
    }

    #[tokio::test]
    async fn test_device_id() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");

        let manager = ConfigManager::load_from_path(&config_path)
            .await
            .unwrap();

        let device_id = manager
            .get_or_create_device_id()
            .await
            .unwrap();

        assert_eq!(device_id.value.len(), 16);

        let device_id2 = manager
            .get_or_create_device_id()
            .await
            .unwrap();

        assert_eq!(device_id.value, device_id2.value);
    }
}