//! Capability negotiation for the BPL protocol
//!
//! Handles advertising and negotiating supported services and features
//! between peers.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::{
    pb::{
        CapabilityAdvertisement, CapabilitySet, NegotiatedCapability, ResultCode,
        ServiceCapability, VersionCompatibility,
    },
    error::{ProtocolError, Result},
    service_ids,
};

/// Capability negotiator
pub struct CapabilityNegotiator {
    local_capabilities: RwLock<CapabilitySet>,
    negotiated: RwLock<Vec<NegotiatedCapability>>,
}

impl CapabilityNegotiator {
    /// Create a new capability negotiator
    pub fn new() -> Self {
        Self {
            local_capabilities: RwLock::new(CapabilitySet::default()),
            negotiated: RwLock::new(Vec::new()),
        }
    }

    /// Set local capabilities
    pub fn set_local_capabilities(&self, capabilities: CapabilitySet) {
        *self.local_capabilities.write() = capabilities;
    }

    /// Get local capabilities
    pub fn local_capabilities(&self) -> CapabilitySet {
        self.local_capabilities.read().clone()
    }

    /// Negotiate capabilities with remote peer
    pub fn negotiate(&self, remote_capabilities: &CapabilitySet) -> Result<Vec<NegotiatedCapability>> {
        let local = self.local_capabilities.read();
        let mut negotiated = Vec::new();

        for remote_cap in &remote_capabilities.capabilities {
            // Find matching local capability
            let local_cap = local.capabilities.iter()
                .find(|c| c.service_id == remote_cap.service_id);

            let negotiated_cap = match local_cap {
                Some(local) => self.negotiate_single(local, remote_cap),
                None => {
                    if remote_cap.required {
                        return Err(ProtocolError::RequiredCapabilityMissing {
                            service_id: remote_cap.service_id.clone(),
                        });
                    }
                    NegotiatedCapability {
                        service_id: remote_cap.service_id.clone(),
                        negotiated_version: 0,
                        negotiated_features: HashMap::new(),
                        available: false,
                        unavailable_reason: "Not supported locally".to_string(),
                    }
                }
            };

            negotiated.push(negotiated_cap);
        }

        // Check for required capabilities that weren't negotiated
        for local_cap in &local.capabilities {
            if local_cap.required {
                let found = negotiated.iter().any(|n| n.service_id == local_cap.service_id && n.available);
                if !found {
                    return Err(ProtocolError::RequiredCapabilityMissing {
                        service_id: local_cap.service_id.clone(),
                    });
                }
            }
        }

        *self.negotiated.write() = negotiated.clone();
        info!("Negotiated {} capabilities", negotiated.len());
        Ok(negotiated)
    }

    /// Negotiate a single capability
    fn negotiate_single(
        &self,
        local: &ServiceCapability,
        remote: &ServiceCapability,
    ) -> NegotiatedCapability {
        let negotiated_version = local.version.min(remote.version);

        if negotiated_version == 0 {
            warn!(
                "Version mismatch for {}: local {}, remote {}",
                local.service_id, local.version, remote.version
            );
            return NegotiatedCapability {
                service_id: local.service_id.clone(),
                negotiated_version: 0,
                negotiated_features: HashMap::new(),
                available: false,
                unavailable_reason: "Version mismatch".to_string(),
            };
        }

        let mut negotiated_features = HashMap::new();
        for (k, v) in &local.metadata {
            if remote.metadata.contains_key(k) {
                negotiated_features.insert(k.clone(), v.clone());
            }
        }

        NegotiatedCapability {
            service_id: local.service_id.clone(),
            negotiated_version,
            negotiated_features,
            available: true,
            unavailable_reason: String::new(),
        }
    }

    /// Check if a service capability is supported and negotiated
    pub fn is_capability_supported(&self, service_id: &str, required_version: u32) -> bool {
        self.negotiated.read()
            .iter()
            .any(|c| c.service_id == service_id && c.available && c.negotiated_version >= required_version)
    }

    /// Get negotiated capabilities
    pub fn negotiated(&self) -> Vec<NegotiatedCapability> {
        self.negotiated.read().clone()
    }

    /// Check if a capability is available
    pub fn is_available(&self, service_id: &str) -> bool {
        self.negotiated.read()
            .iter()
            .any(|c| c.service_id == service_id && c.available)
    }

    /// Get negotiated version for a service
    pub fn get_version(&self, service_id: &str) -> Option<u32> {
        self.negotiated.read()
            .iter()
            .find(|c| c.service_id == service_id && c.available)
            .map(|c| c.negotiated_version)
    }

    /// Get version compatibility info
    pub fn check_compatibility(&self, remote: &CapabilitySet) -> Vec<VersionCompatibility> {
        let local = self.local_capabilities.read();
        let mut results = Vec::new();

        for local_cap in &local.capabilities {
            let remote_cap = remote.capabilities.iter()
                .find(|c| c.service_id == local_cap.service_id);

            let compatible = remote_cap.map_or(false, |r| r.version == local_cap.version);

            results.push(VersionCompatibility {
                service_id: local_cap.service_id.clone(),
                local_version: local_cap.version,
                remote_version: remote_cap.map(|r| r.version).unwrap_or(0),
                compatible,
                incompatibility_reason: if compatible {
                    String::new()
                } else {
                    "Version mismatch".to_string()
                },
            });
        }

        results
    }
}

impl Default for CapabilityNegotiator {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper module for building default capability sets
pub mod defaults {
    use super::*;

    /// Get default capability descriptor for filesystem service
    pub fn filesystem() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.filesystem".to_string(),
            version: 1,
            name: "Filesystem Service".to_string(),
            description: "Remote filesystem access and manipulation".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for sync service
    pub fn sync() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.sync".to_string(),
            version: 1,
            name: "File Sync Service".to_string(),
            description: "Bidirectional file synchronization".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for config service
    pub fn config() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.config".to_string(),
            version: 1,
            name: "Configuration Service".to_string(),
            description: "Device configuration management".to_string(),
            required: true,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for photo backup service
    pub fn photo_backup() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.photo_backup".to_string(),
            version: 1,
            name: "Photo Backup Service".to_string(),
            description: "Automatic photo and video backup".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for shell service
    pub fn shell() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.shell".to_string(),
            version: 1,
            name: "Remote Shell Service".to_string(),
            description: "Remote command execution and shell access".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for media control service
    pub fn media_control() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.media_control".to_string(),
            version: 1,
            name: "Media Control Service".to_string(),
            description: "Remote media playback control".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for phone FS service
    pub fn phone_fs() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.phone_fs".to_string(),
            version: 1,
            name: "Phone Filesystem Service".to_string(),
            description: "Access to Android SAF document trees".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for proximity service
    pub fn proximity() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.proximity".to_string(),
            version: 1,
            name: "Proximity Service".to_string(),
            description: "Proximity detection and RSSI monitoring".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for file stream service
    pub fn file_stream() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.file_stream".to_string(),
            version: 1,
            name: "File Stream Service".to_string(),
            description: "High-speed streaming file transfers".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get default capability descriptor for app launcher service
    pub fn app_launcher() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.app_launcher".to_string(),
            version: 1,
            name: "App Launcher Service".to_string(),
            description: "Remote application launching".to_string(),
            required: false,
            metadata: Default::default(),
            feature_flags: 0,
        }
    }

    /// Get all default capabilities for Desktop
    pub fn desktop_default() -> CapabilitySet {
        CapabilitySet {
            capabilities: vec![
                filesystem(),
                sync(),
                config(),
                photo_backup(),
                shell(),
                media_control(),
                phone_fs(),
                proximity(),
                file_stream(),
                app_launcher(),
            ],
            protocol_version: Some(crate::pb::ProtocolVersion { major: 1, minor: 0, patch: 0 }),
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            device_name: "BPL Desktop".to_string(),
            device_id: Some(crate::pb::DeviceId { value: vec![] }),
        }
    }

    /// Get all default capabilities for Android
    pub fn android_default() -> CapabilitySet {
        CapabilitySet {
            capabilities: vec![
                filesystem(),
                sync(),
                config(),
                photo_backup(),
                shell(),
                media_control(),
                phone_fs(),
                proximity(),
                file_stream(),
                app_launcher(),
            ],
            protocol_version: Some(crate::pb::ProtocolVersion { major: 1, minor: 0, patch: 0 }),
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            device_name: "BPL Android".to_string(),
            device_id: Some(crate::pb::DeviceId { value: vec![] }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_negotiation() {
        let negotiator = CapabilityNegotiator::new();
        negotiator.set_local_capabilities(defaults::desktop_default());

        let remote = defaults::android_default();
        let result = negotiator.negotiate(&remote).unwrap();

        assert_eq!(result.len(), 10);
        for cap in result {
            assert!(cap.available);
            assert_eq!(cap.negotiated_version, 1);
        }
    }

    #[test]
    fn test_missing_required() {
        let mut local = defaults::desktop_default();
        local.capabilities.retain(|c| c.service_id != "bpl.config");

        let negotiator = CapabilityNegotiator::new();
        negotiator.set_local_capabilities(local);

        let remote = defaults::android_default();
        let result = negotiator.negotiate(&remote);

        assert!(result.is_err());
    }
}