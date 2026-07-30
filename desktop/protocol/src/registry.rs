//! Service registry for the BPL protocol
//!
//! Handles service registration, discovery, and health monitoring.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::{
    pb::{
        ServiceCapability, ServiceDiscoverRequest, ServiceDiscoverResponse,
        ServiceHeartbeat, ServiceHeartbeatResponse, ServiceInfo, ServiceRegisterRequest,
        ServiceRegisterResponse, ServiceUnregisterRequest, ServiceUnregisterResponse,
    },
    error::{ProtocolError, Result},
};

/// Service registry
pub struct ServiceRegistry {
    services: Arc<RwLock<HashMap<String, Arc<RwLock<RegisteredService>>>>>,
    event_tx: Option<mpsc::Sender<ServiceEvent>>,
}

/// Registered service
#[derive(Debug, Clone)]
pub struct RegisteredService {
    pub info: ServiceInfo,
    pub capability: ServiceCapability,
    pub channel_id: u32,
    pub last_heartbeat: Instant,
    pub healthy: bool,
}

/// Service events
#[derive(Debug, Clone)]
pub enum ServiceEvent {
    Registered(String),
    Unregistered(String),
    HealthChanged(String, bool),
    HeartbeatMissed(String),
}

impl ServiceRegistry {
    /// Create a new service registry
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            event_tx: None,
        }
    }

    /// Set event sender
    pub fn set_event_sender(&mut self, tx: mpsc::Sender<ServiceEvent>) {
        self.event_tx = Some(tx);
    }

    /// Register a service
    pub fn register_service(
        &self,
        request: ServiceRegisterRequest,
    ) -> Result<ServiceRegisterResponse> {
        let capability = request.capability.ok_or_else(|| {
            ProtocolError::InvalidServiceConfig("Missing capability".to_string())
        })?;

        let channel_config = request.channel_config.ok_or_else(|| {
            ProtocolError::InvalidServiceConfig("Missing channel config".to_string())
        })?;

        let service_id = capability.service_id.clone();

        // Check if already registered
        if self.services.read().contains_key(&service_id) {
            return Ok(ServiceRegisterResponse {
                result: crate::pb::ResultCode::ErrorGeneric as i32,
                assigned_channel: crate::pb::ChannelId { value: 0 },
                error_message: "Service already registered".to_string(),
            });
        }

        let service = Arc::new(RwLock::new(RegisteredService {
            info: ServiceInfo {
                service_id: service_id.clone(),
                version: capability.version,
                name: capability.name,
                description: capability.description,
                channel_id: crate::pb::ChannelId { value: channel_config.channel_id.value },
                channel_type: crate::pb::ChannelType::try_from(channel_config.r#type)
                    .unwrap_or(crate::pb::ChannelType::Data),
                active: true,
                healthy: true,
                registered_at: Some(crate::pb::Timestamp {
                    value: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                }),
                last_heartbeat: Some(crate::pb::Timestamp {
                    value: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                }),
                metadata: capability.metadata.clone(),
                feature_flags: capability.feature_flags,
            },
            capability,
            channel_id: channel_config.channel_id.value,
            last_heartbeat: Instant::now(),
            healthy: true,
        }));

        self.services.write().insert(service_id.clone(), service);

        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(ServiceEvent::Registered(service_id.clone()));
        }

        info!("Registered service: {}", service_id);

        Ok(ServiceRegisterResponse {
            result: crate::pb::ResultCode::Success as i32,
            assigned_channel: crate::pb::ChannelId { value: channel_config.channel_id.value },
            error_message: String::new(),
        })
    }

    /// Unregister a service
    pub fn unregister_service(
        &self,
        request: ServiceUnregisterRequest,
    ) -> Result<ServiceUnregisterResponse> {
        let removed = self.services.write().remove(&request.service_id).is_some();

        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(ServiceEvent::Unregistered(request.service_id.clone()));
        }

        if removed {
            info!("Unregistered service: {}", request.service_id);
            Ok(ServiceUnregisterResponse {
                result: crate::pb::ResultCode::Success as i32,
                error_message: String::new(),
            })
        } else {
            Ok(ServiceUnregisterResponse {
                result: crate::pb::ResultCode::ErrorNotFound as i32,
                error_message: "Service not found".to_string(),
            })
        }
    }

    /// Discover services
    pub fn discover_services(
        &self,
        request: ServiceDiscoverRequest,
    ) -> Result<ServiceDiscoverResponse> {
        let services = self.services.read();
        let mut results = Vec::new();

        for (_, service) in services.iter() {
            let service = service.read();
            let info = &service.info;

            // Filter by service_id if specified
            if !request.service_id.is_empty() && info.service_id != request.service_id {
                continue;
            }

            // Filter by version if specified
            if request.version > 0 && info.version != request.version {
                continue;
            }

            // Skip inactive unless requested
            if !request.include_inactive && !info.active {
                continue;
            }

            results.push(info.clone());
        }

        Ok(ServiceDiscoverResponse {
            result: crate::pb::ResultCode::Success as i32,
            services: results,
            error_message: String::new(),
        })
    }

    /// Get service info
    pub fn get_service(&self, service_id: &str) -> Option<ServiceInfo> {
        self.services.read()
            .get(service_id)
            .map(|s| s.read().info.clone())
    }

    /// Get service capability
    pub fn get_capability(&self, service_id: &str) -> Option<ServiceCapability> {
        self.services.read()
            .get(service_id)
            .map(|s| s.read().capability.clone())
    }

    /// Get service channel ID
    pub fn get_channel_id(&self, service_id: &str) -> Option<u32> {
        self.services.read()
            .get(service_id)
            .map(|s| s.read().channel_id)
    }

    /// Handle service heartbeat
    pub fn handle_heartbeat(&self, heartbeat: ServiceHeartbeat) -> Result<ServiceHeartbeatResponse> {
        if let Some(service) = self.services.read().get(&heartbeat.service_id) {
            let mut service = service.write();
            service.last_heartbeat = Instant::now();
            service.healthy = heartbeat.healthy;
            service.info.healthy = heartbeat.healthy;
            service.info.last_heartbeat = Some(crate::pb::Timestamp {
                value: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
            });

            if !heartbeat.healthy {
                if let Some(tx) = &self.event_tx {
                    let _ = tx.try_send(ServiceEvent::HealthChanged(
                        heartbeat.service_id.clone(),
                        false,
                    ));
                }
            }

            Ok(ServiceHeartbeatResponse {
                result: crate::pb::ResultCode::Success as i32,
                acknowledge: true,
            })
        } else {
            Ok(ServiceHeartbeatResponse {
                result: crate::pb::ResultCode::ErrorNotFound as i32,
                acknowledge: false,
            })
        }
    }

    /// Check service health (call periodically)
    pub fn check_health(&self, timeout: Duration) -> Vec<String> {
        let now = Instant::now();
        let mut unhealthy = Vec::new();

        for (id, service) in self.services.read().iter() {
            let service = service.read();
            if service.healthy && now.duration_since(service.last_heartbeat) > timeout {
                unhealthy.push(id.clone());
            }
        }

        // Mark as unhealthy
        for id in &unhealthy {
            if let Some(service) = self.services.read().get(id) {
                service.write().healthy = false;
                service.write().info.healthy = false;

                if let Some(tx) = &self.event_tx {
                    let _ = tx.try_send(ServiceEvent::HealthChanged(id.clone(), false));
                }
                warn!("Service {} marked unhealthy (heartbeat timeout)", id);
            }
        }

        unhealthy
    }

    /// List all registered services
    pub fn list_services(&self) -> Vec<ServiceInfo> {
        self.services.read()
            .values()
            .map(|s| s.read().info.clone())
            .collect()
    }

    /// Get service count
    pub fn service_count(&self) -> usize {
        self.services.read().len()
    }
}

impl Default for ServiceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_registry() {
        let registry = ServiceRegistry::new();

        let capability = ServiceCapability {
            service_id: "test.service".to_string(),
            version: 1,
            name: "Test Service".to_string(),
            description: "Test".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        };

        let channel_config = crate::pb::ChannelConfig {
            channel_id: crate::pb::ChannelId { value: 1 },
            r#type: crate::pb::ChannelType::Data as i32,
            priority: crate::pb::ChannelPriority::Normal as i32,
            send_window: 65536,
            receive_window: 65536,
            max_frame_size: 16384,
            service_id: "test.service".to_string(),
            metadata: Default::default(),
        };

        let req = ServiceRegisterRequest {
            capability: Some(capability),
            channel_config: Some(channel_config),
            service_state: vec![],
        };

        let resp = registry.register_service(req).unwrap();
        assert_eq!(resp.result, crate::pb::ResultCode::Success as i32);

        let service = registry.get_service("test.service").unwrap();
        assert_eq!(service.service_id, "test.service");

        let services = registry.list_services();
        assert_eq!(services.len(), 1);

        let unreg = registry.unregister_service(ServiceUnregisterRequest {
            service_id: "test.service".to_string(),
            channel_id: 1,
        }).unwrap();
        assert_eq!(unreg.result, crate::pb::ResultCode::Success as i32);

        assert_eq!(registry.service_count(), 0);
    }
}