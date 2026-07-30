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
                None => NegotiatedCapability {
                    service_id: remote_cap.service_id.clone(),
                    negotiated_version: 0,
                    negotiated_features: HashMap::new(),
                    available: false,
                    unavailable_reason: "Not supported locally".to_string(),
                },
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
        local: &CapabilityAdvertisement,
        remote: &CapabilityAdvertisement,
    ) -> NegotiatedCapability {
        // Check version compatibility
        let min_version = local.min_version.max(remote.min_version);
        let max_version = local.max_version.min(remote.max_version);

        if min_version > max_version {
            warn!(
                "Version mismatch for {}: local {}-{}, remote {}-{}",
                local.service_id, local.min_version, local.max_version,
                remote.min_version, remote.max_version
            );
            return NegotiatedCapability {
                service_id: local.service_id.clone(),
                negotiated_version: 0,
                negotiated_features: HashMap::new(),
                available: false,
                unavailable_reason: "Version mismatch".to_string(),
            };
        }

        // Use preferred version if in range, otherwise max compatible
        let negotiated_version = if local.preferred_version >= min_version
            && local.preferred_version <= max_version {
            local.preferred_version
        } else if remote.preferred_version >= min_version
            && remote.preferred_version <= max_version {
            remote.preferred_version
        } else {
            max_version
        };

        // Merge features
        let mut negotiated_features = HashMap::new();
        for (k, v) in &local.features {
            if remote.features.contains_key(k) {
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

            let compatible = remote_cap.map_or(false, |r| {
                let min = local_cap.min_version.max(r.min_version);
                let max = local_cap.max_version.min(r.max_version);
                min <= max
            });

            results.push(VersionCompatibility {
                service_id: local_cap.service_id.clone(),
                local_version: local_cap.preferred_version,
                remote_version: remote_cap.map(|r| r.preferred_version).unwrap_or(0),
                compatible,
                incompatibility_reason: if compatible {
                    String::new()
                } else {
                    "Version range incompatible".to_string()
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

/// Built-in capability definitions
pub mod capabilities {
    use super::*;

    pub fn filesystem() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.filesystem".to_string(),
            version: 1,
            name: "Filesystem".to_string(),
            description: "File and directory operations".to_string(),
            required: true,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn sync() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.sync".to_string(),
            version: 1,
            name: "Sync".to_string(),
            description: "Directory synchronization".to_string(),
            required: true,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn config() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.config".to_string(),
            version: 1,
            name: "Config".to_string(),
            description: "Configuration management".to_string(),
            required: true,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn photo_backup() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.photo_backup".to_string(),
            version: 1,
            name: "Photo Backup".to_string(),
            description: "Automatic photo backup".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn shell() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.shell".to_string(),
            version: 1,
            name: "Remote Shell".to_string(),
            description: "Command execution".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn media_control() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.media_control".to_string(),
            version: 1,
            name: "Media Control".to_string(),
            description: "Media playback control".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn phone_fs() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.phone_fs".to_string(),
            version: 1,
            name: "Phone Filesystem".to_string(),
            description: "Access phone storage".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn proximity() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.proximity".to_string(),
            version: 1,
            name: "Proximity".to_string(),
            description: "Proximity detection".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn file_stream() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.file_stream".to_string(),
            version: 1,
            name: "File Streaming".to_string(),
            description: "On-demand file streaming".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    pub fn app_launcher() -> ServiceCapability {
        ServiceCapability {
            service_id: "bpl.app_launcher".to_string(),
            version: 1,
            name: "App Launcher".to_string(),
            description: "Launch applications".to_string(),
            required: false,
            metadata: HashMap::new(),
            feature_flags: 0,
        }
    }

    /// Get all default capabilities for desktop
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
            protocol_version: crate::pb::ProtocolVersion { major: 1, minor: 0, patch: 0 },
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            device_name: "BPL Desktop".to_string(),
            device_id: crate::pb::DeviceId { value: vec![] },
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
            protocol_version: crate::pb::ProtocolVersion { major: 1, minor: 0, patch: 0 },
            software_version: env!("CARGO_PKG_VERSION").to_string(),
            device_name: "BPL Android".to_string(),
            device_id: crate::pb::DeviceId { value: vec![] },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_negotiation() {
        let negotiator = CapabilityNegotiator::new();
        negotiator.set_local_capabilities(capabilities::desktop_default());

        let remote = capabilities::android_default();
        let result = negotiator.negotiate(&remote).unwrap();

        assert_eq!(result.len(), 10);
        for cap in result {
            assert!(cap.available);
            assert_eq!(cap.negotiated_version, 1);
        }
    }

    #[test]
    fn test_missing_required() {
        let mut local = capabilities::desktop_default();
        local.capabilities.retain(|c| c.service_id != "bpl.filesystem");

        let negotiator = CapabilityNegotiator::new();
        negotiator.set_local_capabilities(local);

        let remote = capabilities::android_default();
        let result = negotiator.negotiate(&remote);

        assert!(result.is_err());
    }
}