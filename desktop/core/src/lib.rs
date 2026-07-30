//! Core services for the BPL Desktop application
//!
//! Provides configuration management, database layer, service framework,
//! and event bus for inter-service communication.

pub mod config;
pub mod database;
pub mod service;
pub mod events;

pub use config::{ConfigManager, AppConfig};
pub use database::{Database, DatabasePool};
pub use service::{ServiceManager, ServiceContext, Service, ServiceConfig};
pub use events::{EventBus, Event};

use bpl_protocol::{DeviceId, SessionId, ProtocolError, Result};

/// Core application state
pub struct Core {
    pub config: ConfigManager,
    pub database: Database,
    pub services: ServiceManager,
    pub events: EventBus,
    pub device_id: DeviceId,
}

impl Core {
    /// Create new core instance
    pub async fn new() -> Result<Self> {
        let config = ConfigManager::load().await?;
        let database = Database::new(&config.database.path).await?;
        let events = EventBus::new();
        let services = ServiceManager::new(events.clone());

        // Generate device ID if not exists
        let device_id = config.get_or_create_device_id().await?;

        Ok(Self {
            config,
            database,
            services,
            events,
            device_id,
        })
    }

    /// Initialize all services
    pub async fn init(&mut self) -> Result<()> {
        // Run database migrations
        self.database.migrate().await?;

        // Initialize services
        self.services.init_all().await?;

        Ok(())
    }

    /// Start all services
    pub async fn start(&mut self) -> Result<()> {
        self.services.start_all().await
    }

    /// Stop all services
    pub async fn stop(&mut self) -> Result<()> {
        self.services.stop_all().await
    }

    /// Get device ID
    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }
}