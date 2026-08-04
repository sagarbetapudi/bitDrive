//! Service framework for BPL Desktop

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tracing::{error, info};

use bpl_protocol::{
        DeviceId, SessionId, ChannelId, NegotiatedCapability,
        ResultCode, ProtocolError, Result, ServiceCapability as ProtoServiceCapability,
};

use crate::{ConfigManager, Database, events::EventBus};

/// Service configuration
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub service_id: String,
    pub enabled: bool,
    pub settings: serde_json::Value,
}

/// Service context providing access to core resources
#[derive(Clone)]
pub struct ServiceContext {
    pub config: ConfigManager,
    pub database: Database,
    pub events: EventBus,
    pub device_id: DeviceId,
    pub session_id: Option<SessionId>,
    pub channel_id: Option<ChannelId>,
    pub negotiated_capabilities: Vec<NegotiatedCapability>,
}

impl ServiceContext {
    /// Create new service context
    pub fn new(
        config: ConfigManager,
        database: Database,
        events: EventBus,
        device_id: DeviceId,
    ) -> Self {
        Self {
            config,
            database,
            events,
            device_id,
            session_id: None,
            channel_id: None,
            negotiated_capabilities: Vec::new(),
        }
    }

    /// Set session ID
    pub fn with_session(mut self, session_id: SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set channel ID
    pub fn with_channel(mut self, channel_id: ChannelId) -> Self {
        self.channel_id = Some(channel_id);
        self
    }

    /// Set negotiated capabilities
    pub fn with_capabilities(mut self, capabilities: Vec<NegotiatedCapability>) -> Self {
        self.negotiated_capabilities = capabilities;
        self
    }

    /// Check if capability is available
    pub fn has_capability(&self, service_id: &str) -> bool {
        self.negotiated_capabilities.iter()
            .any(|c| c.service_id == service_id && c.available)
    }

    /// Get negotiated version for service
    pub fn get_capability_version(&self, service_id: &str) -> Option<u32> {
        self.negotiated_capabilities.iter()
            .find(|c| c.service_id == service_id && c.available)
            .map(|c| c.negotiated_version)
    }
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

    /// Whether this service is required
    fn required(&self) -> bool {
        false
    }

    /// Initialize the service
    async fn init(&mut self, context: ServiceContext) -> Result<()>;

    /// Start the service
    async fn start(&mut self) -> Result<()>;

    /// Stop the service
    async fn stop(&mut self) -> Result<()>;

    /// Handle incoming request from protocol
    async fn handle_request(&mut self, request: ServiceRequest) -> Result<ServiceResponse>;

    /// Handle event from event bus
    async fn handle_event(&mut self, event: ServiceEvent) -> Result<()>;

    /// Get service capabilities for negotiation
    fn capabilities(&self) -> ProtoServiceCapability {
        ProtoServiceCapability {
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

/// Service request from protocol
#[derive(Debug, Clone)]
pub struct ServiceRequest {
    pub service_id: String,
    pub channel_id: u32,
    pub request_id: u64,
    pub payload: Vec<u8>,
    pub metadata: HashMap<String, String>,
}

/// Service response to protocol
#[derive(Debug, Clone)]
pub struct ServiceResponse {
    pub request_id: u64,
    pub result: i32,
    pub payload: Vec<u8>,
    pub error_message: String,
    pub metadata: HashMap<String, String>,
}

impl ServiceResponse {
    /// Create success response
    pub fn success(request_id: u64, payload: Vec<u8>) -> Self {
        Self {
            request_id,
            result: 0, // SUCCESS
            payload,
            error_message: String::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create error response
    pub fn error(request_id: u64, code: i32, message: String) -> Self {
        Self {
            request_id,
            result: code,
            payload: Vec::new(),
            error_message: message,
            metadata: HashMap::new(),
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

/// Service events
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    SessionOpened(SessionId),
    SessionClosed(SessionId),
    CapabilitiesNegotiated(Vec<NegotiatedCapability>),
    ChannelOpened(ChannelId),
    ChannelClosed(ChannelId),
    PeerConfigChanged(String),
    DeviceConnected(DeviceId),
    DeviceDisconnected(DeviceId),
}

/// Service manager for lifecycle management
pub struct ServiceManager {
    services: Arc<RwLock<HashMap<String, Box<dyn Service>>>>,
    context: Option<ServiceContext>,
    events: EventBus,
    running: Arc<RwLock<bool>>,
}

impl ServiceManager {
    /// Create new service manager
    pub fn new(events: EventBus) -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            context: None,
            events,
            running: Arc::new(RwLock::new(false)),
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

    /// Get service by ID
    pub fn get_service(&self, _service_id: &str) -> Option<Box<dyn Service>> {
        // Can't easily return Box<dyn Service> from Arc<RwLock<HashMap>>
        // This would need a different design. For now, use handle_request directly.
        None
    }

    /// Initialize all services
    pub async fn init_all(&mut self) -> Result<()> {
        let context = self.context.as_ref()
            .ok_or_else(|| ProtocolError::Config("Service context not set".to_string()))?
            .clone();

        let service_ids: Vec<String> = self.services.read().keys().cloned().collect();

        for id in service_ids {
            if let Some(mut service) = self.services.write().remove(&id) {
                service.init(context.clone()).await
                    .map_err(|e| ProtocolError::Config(format!("Failed to init {}: {}", id, e)))?;
                self.services.write().insert(id, service);
            }
        }

        Ok(())
    }

    /// Start all services
    pub async fn start_all(&mut self) -> Result<()> {
        *self.running.write() = true;

        let service_ids: Vec<String> = self.services.read().keys().cloned().collect();

        for id in service_ids {
            if let Some(mut service) = self.services.write().remove(&id) {
                service.start().await
                    .map_err(|e| ProtocolError::Config(format!("Failed to start {}: {}", id, e)))?;
                self.services.write().insert(id, service);
            }
        }

        info!("All services started");
        Ok(())
    }

    /// Stop all services
    pub async fn stop_all(&mut self) -> Result<()> {
        *self.running.write() = false;

        let service_ids: Vec<String> = self.services.read().keys().cloned().collect();

        for id in service_ids {
            if let Some(mut service) = self.services.write().remove(&id) {
                if let Err(e) = service.stop().await {
                    error!("Error stopping {}: {}", id, e);
                }
            }
        }

        info!("All services stopped");
        Ok(())
    }

    /// Handle request by routing to appropriate service.
    ///
    /// The service is temporarily removed from the map so a parking_lot lock
    /// is never held across an `.await`.
    pub async fn handle_request(&self, request: ServiceRequest) -> Result<ServiceResponse> {
        let service_id = request.service_id.clone();
        let request_id = request.request_id;

        let service = self.services.write().remove(&service_id);

        if let Some(mut service) = service {
            let result = service.handle_request(request).await;
            self.services.write().insert(service_id, service);
            result
        } else {
            Ok(ServiceResponse::error(
                request_id,
                ResultCode::ErrorNotFound as i32,
                format!("Service not found: {}", service_id),
            ))
        }
    }

    /// Broadcast event to all services.
    ///
    /// Services are temporarily removed so no synchronous lock is held across `.await`.
    pub async fn broadcast_event(&self, event: ServiceEvent) -> Result<()> {
    let service_ids: Vec<String> =
        self.services.read().keys().cloned().collect();

    for id in service_ids {
        let service = self.services.write().remove(&id);

        if let Some(mut service) = service {
            if let Err(e) = service.handle_event(event.clone()).await {
                error!("Error handling event in {}: {}", id, e);
            }

            // Always put the service back
            self.services.write().insert(id, service);
        }
    }

        Ok(())
    }

    /// List all registered services
    pub fn list_services(&self) -> Vec<String> {
        self.services.read().keys().cloned().collect()
    }

    /// Get service count
    pub fn service_count(&self) -> usize {
        self.services.read().len()
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Get event bus
    pub fn events(&self) -> &EventBus {
        &self.events
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
            Ok(ServiceResponse::success(request.request_id, b"OK".to_vec()))
        }
        async fn handle_event(&mut self, _event: ServiceEvent) -> Result<()> { Ok(()) }
    }

    #[tokio::test]
    async fn test_service_manager() {
        let events = EventBus::new();
        let mut manager = ServiceManager::new(events);

        manager.register_service(Box::new(TestService {
            service_id: "test.service".to_string(),
        })).unwrap();

        assert_eq!(manager.service_count(), 1);
        assert!(manager.list_services().contains(&"test.service".to_string()));
    }
}