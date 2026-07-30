//! Service framework for the BPL protocol
//!
//! Defines the service trait and context for implementing feature services.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::{
    pb::*,
    error::{ProtocolError, Result},
    session::Session,
    mux::ChannelManager,
    registry::ServiceRegistry,
};

/// Service context providing access to session, channels, registry, etc.
#[derive(Clone)]
pub struct ServiceContext {
    pub session: Arc<Session>,
    pub channel_manager: Arc<ChannelManager>,
    pub service_registry: Arc<ServiceRegistry>,
    pub config: Arc<RwLock<ServiceConfig>>,
}

/// Service configuration
#[derive(Debug, Clone, Default)]
pub struct ServiceConfig {
    pub settings: HashMap<String, String>,
    pub enabled: bool,
}

/// Service request from protocol
#[derive(Debug, Clone)]
pub struct ServiceRequest {
    pub service_id: String,
    pub channel_id: u32,
    pub request_id: u64,
    pub payload: Bytes,
    pub metadata: HashMap<String, String>,
}

/// Service response to protocol
#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub request_id: u64,
    pub result: ResultCode,
    pub payload: Bytes,
    pub error_message: String,
    pub metadata: HashMap<String, String>,
}

/// Service trait for implementing feature services
#[async_trait]
pub trait Service: Send + Sync {
    /// Service identifier (e.g., "bpl.filesystem")
    fn service_id(&self) -> &str;

    /// Service version
    fn version(&self) -> u32;

    /// Service name
    fn name(&self) -> &str;

    /// Service description
    fn description(&self) -> &str;

    /// Required capability (if this service is required)
    fn required(&self) -> bool {
        false
    }

    /// Initialize the service
    async fn init(&mut self, context: ServiceContext) -> Result<()>;

    /// Start the service
    async fn start(&mut self) -> Result<()>;

    /// Stop the service
    async fn stop(&mut self) -> Result<()>;

    /// Handle incoming request
    async fn handle_request(&mut self, request: ServiceRequest) -> Result<ServiceResponse>;

    /// Handle incoming event
    async fn handle_event(&mut self, event: ServiceEvent) -> Result<()>;

    /// Get service capabilities
    fn capabilities(&self) -> ServiceCapability {
        ServiceCapability {
            service_id: self.service_id().to_string(),
            version: self.version(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            required: self.required(),
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    /// Get service status
    fn status(&self) -> ServiceStatus {
        ServiceStatus {
            service_id: self.service_id().to_string(),
            running: false,
            healthy: true,
            details: String::new(),
        }
    }
}

/// Service status
#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub service_id: String,
    pub running: bool,
    pub healthy: bool,
    pub details: String,
}

/// Service event
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    SessionOpened,
    SessionClosed,
    ChannelOpened(u32),
    ChannelClosed(u32),
    CapabilityNegotiated(Vec<NegotiatedCapability>),
    PeerConfigChanged(String),
}

/// Service manager for lifecycle management
pub struct ServiceManager {
    services: Arc<RwLock<HashMap<String, Box<dyn Service>>>>,
    context: Option<ServiceContext>,
    event_tx: mpsc::Sender<ServiceEvent>,
    event_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<ServiceEvent>>>,
}

impl ServiceManager {
    /// Create a new service manager
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(100);
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            context: None,
            event_tx,
            event_rx: Arc::new(tokio::sync::Mutex::new(event_rx)),
        }
    }

    /// Set service context
    pub fn set_context(&mut self, context: ServiceContext) {
        self.context = Some(context);
    }

    /// Register a service
    pub fn register_service(&self, service: Box<dyn Service>) -> Result<()> {
        let service_id = service.service_id().to_string();

        if self.services.read().contains_key(&service_id) {
            return Err(ProtocolError::ServiceAlreadyRegistered { service_id });
        }

        self.services.write().insert(service_id.clone(), service);
        info!("Registered service: {}", service_id);
        Ok(())
    }

    /// Unregister a service
    pub fn unregister_service(&self, service_id: &str) -> Result<()> {
        if self.services.write().remove(service_id).is_some() {
            info!("Unregistered service: {}", service_id);
            Ok(())
        } else {
            Err(ProtocolError::ServiceNotRegistered { service_id: service_id.to_string() })
        }
    }

    /// Get a service
    pub fn get_service(&self, service_id: &str) -> Option<Box<dyn Service>> {
        // Note: Can't easily return Box<dyn Service> from Arc<RwLock<HashMap>>
        // This would need a different design. For now, use handle_request directly.
        None
    }

    /// Initialize all services
    pub async fn init_all(&mut self) -> Result<()> {
        let context = self.context.clone().ok_or_else(|| {
            ProtocolError::Config("Service context not set".to_string())
        })?;

        for (id, service) in self.services.write().iter_mut() {
            service.init(context.clone()).await
                .map_err(|e| ProtocolError::Config(format!("Failed to init {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Start all services
    pub async fn start_all(&mut self) -> Result<()> {
        for (id, service) in self.services.write().iter_mut() {
            service.start().await
                .map_err(|e| ProtocolError::Config(format!("Failed to start {}: {}", id, e)))?;
        }
        Ok(())
    }

    /// Stop all services
    pub async fn stop_all(&mut self) -> Result<()> {
        for (id, service) in self.services.write().iter_mut() {
            if let Err(e) = service.stop().await {
                tracing::error!("Error stopping {}: {}", id, e);
            }
        }
        Ok(())
    }

    /// Handle request by routing to appropriate service
    pub async fn handle_request(&self, request: ServiceRequest) -> Result<ServiceResponse> {
        if let Some(service) = self.services.read().get(&request.service_id) {
            service.handle_request(request).await
        } else {
            Ok(ServiceResponse {
                request_id: request.request_id,
                result: ResultCode::ErrorNotFound,
                payload: Bytes::new(),
                error_message: format!("Service not found: {}", request.service_id),
                metadata: HashMap::new(),
            })
        }
    }

    /// Broadcast event to all services
    pub async fn broadcast_event(&self, event: ServiceEvent) -> Result<()> {
        for (id, service) in self.services.read().iter() {
            if let Err(e) = service.handle_event(event.clone()).await {
                tracing::error!("Error handling event in {}: {}", id, e);
            }
        }
        Ok(())
    }

    /// Get event sender
    pub fn event_sender(&self) -> mpsc::Sender<ServiceEvent> {
        self.event_tx.clone()
    }

    /// Get list of registered service IDs
    pub fn list_services(&self) -> Vec<String> {
        self.services.read().keys().cloned().collect()
    }

    /// Get service count
    pub fn service_count(&self) -> usize {
        self.services.read().len()
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestService {
        service_id: String,
    }

    #[async_trait]
    impl Service for TestService {
        fn service_id(&self) -> &str { &self.service_id }
        fn version(&self) -> u32 { 1 }
        fn name(&self) -> &str { "Test Service" }
        fn description(&self) -> &str { "Test" }

        async fn init(&mut self, _context: ServiceContext) -> Result<()> { Ok(()) }
        async fn start(&mut self) -> Result<()> { Ok(()) }
        async fn stop(&mut self) -> Result<()> { Ok(()) }

        async fn handle_request(&mut self, request: ServiceRequest) -> Result<ServiceResponse> {
            Ok(ServiceResponse {
                request_id: request.request_id,
                result: ResultCode::Success,
                payload: Bytes::from_static(b"OK"),
                error_message: String::new(),
                metadata: HashMap::new(),
            })
        }

        async fn handle_event(&mut self, _event: ServiceEvent) -> Result<()> { Ok(()) }
    }

    #[tokio::test]
    async fn test_service_manager() {
        let mut manager = ServiceManager::new();
        manager.register_service(Box::new(TestService {
            service_id: "test.service".to_string(),
        })).unwrap();

        assert_eq!(manager.service_count(), 1);
        assert!(manager.list_services().contains(&"test.service".to_string()));
    }
}