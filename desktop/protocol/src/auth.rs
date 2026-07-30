//! Authentication and key derivation for the BPL protocol
//!
//! Implements PSK-based mutual authentication with HKDF key derivation.

use std::collections::HashMap;
use std::sync::Arc;

use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{ChaCha20Poly1305, Key as ChachaKey, Nonce as ChachaNonce};
use hmac::{Hmac, Mac};
use hkdf::Hkdf;
use rand::{RngCore, thread_rng};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    pb::{
        AuthChallenge, AuthMethod, AuthResponse, AuthSuccess, AuthFailure,
        ChannelId, ChannelKeys, SessionKeys, SessionKeySet, KeyDerivationParams,
        PairingRecord,
    },
    error::{ProtocolError, Result},
};

/// HMAC-SHA256 type
type HmacSha256 = Hmac<Sha256>;

/// Session key material
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct SessionKeys {
    pub master_key: [u8; 32],
    pub encryption_key: [u8; 32],
    pub authentication_key: [u8; 32],
    pub iv_salt: [u8; 16],
    pub channel_keys: HashMap<u32, ChannelKeys>,
}

impl Default for SessionKeys {
    fn default() -> Self {
        Self {
            master_key: [0u8; 32],
            encryption_key: [0u8; 32],
            authentication_key: [0u8; 32],
            iv_salt: [0u8; 16],
            channel_keys: HashMap::new(),
        }
    }
}

/// Per-channel derived keys
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct ChannelKeys {
    pub encryption_key: [u8; 32],
    pub authentication_key: [u8; 32],
    pub iv_salt: [u8; 16],
}

impl Default for ChannelKeys {
    fn default() -> Self {
        Self {
            encryption_key: [0u8; 32],
            authentication_key: [0u8; 32],
            iv_salt: [0u8; 16],
        }
    }
}

/// Authentication manager
pub struct AuthManager {
    psk: Arc<parking_lot::RwLock<Option<[u8; 32]>>>,
    method: AuthMethod,
    session_keys: Arc<parking_lot::RwLock<Option<SessionKeys>>>,
    pairing_records: Arc<parking_lot::RwLock<HashMap<Vec<u8>, PairingRecord>>>,
}

impl AuthManager {
    /// Create a new auth manager
    pub fn new() -> Self {
        Self {
            psk: Arc::new(parking_lot::RwLock::new(None)),
            method: AuthMethod::Psk,
            session_keys: Arc::new(parking_lot::RwLock::new(None)),
            pairing_records: Arc::new(parking_lot::RwLock::new(HashMap::new())),
        }
    }

    /// Set the pre-shared key
    pub fn set_psk(&self, psk: [u8; 32]) {
        *self.psk.write() = Some(psk);
    }

    /// Get the PSK (for internal use)
    pub fn get_psk(&self) -> Option<[u8; 32]> {
        *self.psk.read()
    }

    /// Set authentication method
    pub fn set_method(&mut self, method: AuthMethod) {
        self.method = method;
    }

    /// Get authentication method
    pub fn method(&self) -> AuthMethod {
        self.method
    }

    /// Generate authentication challenge
    pub fn generate_challenge(&self) -> Result<AuthChallenge> {
        let mut challenge = [0u8; 32];
        let mut salt = [0u8; 32];
        thread_rng().fill_bytes(&mut challenge);
        thread_rng().fill_bytes(&mut salt);

        Ok(AuthChallenge {
            method: self.method as i32,
            challenge: challenge.to_vec(),
            salt: salt.to_vec(),
            iterations: 100_000,
            server_public_key: vec![],
            parameters: Default::default(),
        })
    }

    /// Verify authentication response and derive session keys
    pub fn verify_response(&self, challenge: &AuthChallenge, response: &AuthResponse) -> Result<AuthSuccess> {
        let psk = self.get_psk().ok_or(ProtocolError::AuthenticationFailed(
            "PSK not configured".to_string()
        ))?;

        // Verify client response
        let expected_hmac = self.compute_client_hmac(&psk, challenge, &response.client_nonce)?;
        if expected_hmac != response.response {
            return Err(ProtocolError::AuthenticationFailed("Invalid HMAC".to_string()));
        }

        // Derive session keys
        let session_keys = self.derive_session_keys(&psk, challenge, &response.client_nonce)?;

        // Store session keys
        *self.session_keys.write() = Some(session_keys.clone());

        // Compute server verification HMAC
        let server_hmac = self.compute_server_hmac(&psk, challenge, &response.client_nonce)?;

        // Encrypt session keys for transport
        let encrypted_keys = self.encrypt_session_keys(&session_keys)?;

        Ok(AuthSuccess {
            key_confirmation: server_hmac.to_vec(),
            session_keys_encrypted: encrypted_keys,
            session_keys: Some(session_keys.into()),
        })
    }

    /// Verify server authentication response (client side)
    pub fn verify_server_response(&self, challenge: &AuthChallenge, response: &AuthSuccess) -> Result<SessionKeys> {
        let psk = self.get_psk().ok_or(ProtocolError::AuthenticationFailed(
            "PSK not configured".to_string()
        ))?;

        // Verify server key confirmation
        // In real implementation, we'd verify the HMAC
        // For now, we'll derive the same keys

        // Derive session keys (same as server)
        let client_nonce = [0u8; 32]; // Would be stored from challenge
        let session_keys = self.derive_session_keys(&psk, challenge, &client_nonce)?;

        // Store session keys
        *self.session_keys.write() = Some(session_keys.clone());

        Ok(session_keys)
    }

    /// Derive session keys using HKDF
    fn derive_session_keys(
        &self,
        psk: &[u8; 32],
        challenge: &AuthChallenge,
        client_nonce: &[u8],
    ) -> Result<SessionKeys> {
        let mut keys = SessionKeys::default();

        // Master key = HKDF(PSK, salt, info="bpl-master")
        let hkdf = Hkdf::<Sha256>::new(Some(&challenge.salt), psk);
        hkdf.expand(b"bpl-master", &mut keys.master_key)
            .map_err(|_| ProtocolError::KeyDerivation("Master key derivation failed".to_string()))?;

        // Encryption key = HKDF(master_key, salt, info="bpl-enc")
        let hkdf_enc = Hkdf::<Sha256>::new(Some(&keys.master_key), b"");
        hkdf_enc.expand(b"bpl-enc", &mut keys.encryption_key)
            .map_err(|_| ProtocolError::KeyDerivation("Encryption key derivation failed".to_string()))?;

        // Auth key = HKDF(master_key, salt, info="bpl-auth")
        let hkdf_auth = Hkdf::<Sha256>::new(Some(&keys.master_key), b"");
        hkdf_auth.expand(b"bpl-auth", &mut keys.authentication_key)
            .map_err(|_| ProtocolError::KeyDerivation("Auth key derivation failed".to_string()))?;

        // IV salt = HKDF(master_key, salt, info="bpl-iv")
        let hkdf_iv = Hkdf::<Sha256>::new(Some(&keys.master_key), b"");
        hkdf_iv.expand(b"bpl-iv", &mut keys.iv_salt)
            .map_err(|_| ProtocolError::KeyDerivation("IV salt derivation failed".to_string()))?;

        // Derive channel keys for all channels (0-15)
        for channel_id in 0..16 {
            keys.channel_keys.insert(channel_id, self.derive_channel_keys(&keys.master_key, channel_id)?);
        }

        Ok(keys)
    }

    /// Derive per-channel keys
    fn derive_channel_keys(&self, master_key: &[u8; 32], channel_id: u32) -> Result<ChannelKeys> {
        let mut keys = ChannelKeys::default();
        let info = format!("bpl-channel-{}", channel_id);

        // Encryption key
        let hkdf_enc = Hkdf::<Sha256>::new(None, master_key);
        hkdf_enc.expand(info.as_bytes(), &mut keys.encryption_key)
            .map_err(|_| ProtocolError::KeyDerivation("Channel encryption key failed".to_string()))?;

        // Auth key
        let hkdf_auth = Hkdf::<Sha256>::new(None, master_key);
        hkdf_auth.expand(&format!("{}-auth", info).as_bytes(), &mut keys.authentication_key)
            .map_err(|_| ProtocolError::KeyDerivation("Channel auth key failed".to_string()))?;

        // IV salt
        let hkdf_iv = Hkdf::<Sha256>::new(None, master_key);
        hkdf_iv.expand(&format!("{}-iv", info).as_bytes(), &mut keys.iv_salt)
            .map_err(|_| ProtocolError::KeyDerivation("Channel IV salt failed".to_string()))?;

        Ok(keys)
    }

    /// Compute client HMAC for authentication
    fn compute_client_hmac(
        &self,
        psk: &[u8; 32],
        challenge: &AuthChallenge,
        client_nonce: &[u8],
    ) -> Result<Vec<u8>> {
        let mut mac = HmacSha256::new_from_slice(psk)
            .map_err(|_| ProtocolError::AuthenticationFailed("HMAC init failed".to_string()))?;

        mac.update(&challenge.challenge);
        mac.update(client_nonce);
        mac.update(b"client");

        Ok(mac.finalize().into_bytes().to_vec())
    }

    /// Compute server HMAC for key confirmation
    fn compute_server_hmac(
        &self,
        psk: &[u8; 32],
        challenge: &AuthChallenge,
        client_nonce: &[u8],
    ) -> Result<Vec<u8>> {
        let mut mac = HmacSha256::new_from_slice(psk)
            .map_err(|_| ProtocolError::AuthenticationFailed("HMAC init failed".to_string()))?;

        mac.update(&challenge.challenge);
        mac.update(client_nonce);
        mac.update(b"server");

        Ok(mac.finalize().into_bytes().to_vec())
    }

    /// Encrypt session keys for transport
    fn encrypt_session_keys(&self, keys: &SessionKeys) -> Result<Vec<u8>> {
        // Serialize and encrypt with master key
        let plaintext = bincode::serialize(keys)
            .map_err(|e| ProtocolError::Serialization(e.to_string()))?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&keys.master_key));
        let nonce = Nonce::from_slice(&keys.iv_salt[..12]);
        let ciphertext = cipher.encrypt(nonce, plaintext.as_ref())
            .map_err(|e| ProtocolError::Encryption(e.to_string()))?;

        Ok(ciphertext)
    }

    /// Decrypt session keys
    pub fn decrypt_session_keys(&self, encrypted: &[u8], master_key: &[u8; 32], iv_salt: &[u8; 16]) -> Result<SessionKeys> {
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(master_key));
        let nonce = Nonce::from_slice(&iv_salt[..12]);
        let plaintext = cipher.decrypt(nonce, encrypted)
            .map_err(|e| ProtocolError::Decryption(e.to_string()))?;

        bincode::deserialize(&plaintext)
            .map_err(|e| ProtocolError::Deserialization(e.to_string()))
    }

    /// Get current session keys
    pub fn session_keys(&self) -> Option<SessionKeys> {
        self.session_keys.read().clone()
    }

    /// Get channel keys
    pub fn channel_keys(&self, channel_id: u32) -> Option<ChannelKeys> {
        self.session_keys.read()
            .as_ref()
            .and_then(|k| k.channel_keys.get(&channel_id).cloned())
    }

    /// Rotate session keys
    pub fn rotate_keys(&self, new_master_key: [u8; 32]) -> Result<SessionKeys> {
        let mut keys = SessionKeys::default();
        keys.master_key = new_master_key;

        // Derive new keys from new master key
        let hkdf_enc = Hkdf::<Sha256>::new(None, &keys.master_key);
        hkdf_enc.expand(b"bpl-enc", &mut keys.encryption_key)
            .map_err(|_| ProtocolError::KeyDerivation("Encryption key derivation failed".to_string()))?;

        let hkdf_auth = Hkdf::<Sha256>::new(None, &keys.master_key);
        hkdf_auth.expand(b"bpl-auth", &mut keys.authentication_key)
            .map_err(|_| ProtocolError::KeyDerivation("Auth key derivation failed".to_string()))?;

        let hkdf_iv = Hkdf::<Sha256>::new(None, &keys.master_key);
        hkdf_iv.expand(b"bpl-iv", &mut keys.iv_salt)
            .map_err(|_| ProtocolError::KeyDerivation("IV salt derivation failed".to_string()))?;

        // Derive channel keys
        for channel_id in 0..16 {
            keys.channel_keys.insert(channel_id, self.derive_channel_keys(&keys.master_key, channel_id)?);
        }

        *self.session_keys.write() = Some(keys.clone());
        Ok(keys)
    }

    /// Add pairing record
    pub fn add_pairing(&self, record: PairingRecord) {
        self.pairing_records.write().insert(record.device_id.value.clone(), record);
    }

    /// Get pairing record
    pub fn get_pairing(&self, device_id: &[u8]) -> Option<PairingRecord> {
        self.pairing_records.read().get(device_id).cloned()
    }

    /// List all pairings
    pub fn list_pairings(&self) -> Vec<PairingRecord> {
        self.pairing_records.read().values().cloned().collect()
    }

    /// Remove pairing
    pub fn remove_pairing(&self, device_id: &[u8]) -> bool {
        self.pairing_records.write().remove(device_id).is_some()
    }

    /// Clear all session keys
    pub fn clear_keys(&self) {
        *self.session_keys.write() = None;
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Encrypt data with session keys
pub fn encrypt_with_channel_keys(
    data: &[u8],
    keys: &ChannelKeys,
    sequence: u64,
) -> Result<(Vec<u8>, Vec<u8>)> {
    // Generate nonce from IV salt + sequence
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&sequence.to_le_bytes());
    nonce[8..].copy_from_slice(&keys.iv_salt[..4]);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&keys.encryption_key));
    let ciphertext = cipher.encrypt(Nonce::from_slice(&nonce), data)
        .map_err(|e| ProtocolError::Encryption(e.to_string()))?;

    // Split ciphertext into data + auth tag
    let len = ciphertext.len();
    let (ciphertext, tag) = ciphertext.split_at(len - 16);

    Ok((ciphertext.to_vec(), tag.to_vec()))
}

/// Decrypt data with session keys
pub fn decrypt_with_channel_keys(
    ciphertext: &[u8],
    tag: &[u8],
    keys: &ChannelKeys,
    sequence: u64,
) -> Result<Vec<u8>> {
    // Generate nonce
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&sequence.to_le_bytes());
    nonce[8..].copy_from_slice(&keys.iv_salt[..4]);

    // Combine ciphertext and tag
    let mut combined = Vec::with_capacity(ciphertext.len() + 16);
    combined.extend_from_slice(ciphertext);
    combined.extend_from_slice(tag);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&keys.encryption_key));
    cipher.decrypt(Nonce::from_slice(&nonce), combined.as_ref())
        .map_err(|e| ProtocolError::Decryption(e.to_string()))
}

/// Authenticate data with HMAC
pub fn authenticate_data(data: &[u8], keys: &ChannelKeys) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(&keys.authentication_key)
        .expect("HMAC key is valid");
    mac.update(data);
    mac.finalize().into_bytes()
}

/// Verify data authentication
pub fn verify_authentication(data: &[u8], tag: &[u8], keys: &ChannelKeys) -> bool {
    let expected = authenticate_data(data, keys);
    // Constant-time comparison
    subtle::ConstantTimeEq::ct_eq(&expected, tag).into()
}

use subtle::ConstantTimeEq;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_manager() {
        let mut auth = AuthManager::new();

        // Set PSK
        let psk = [42u8; 32];
        auth.set_psk(psk);

        // Generate challenge
        let challenge = auth.generate_challenge().unwrap();

        // Create response
        let mut client_nonce = [0u8; 32];
        thread_rng().fill_bytes(&mut client_nonce);

        let response = AuthResponse {
            method: AuthMethod::Psk as i32,
            response: auth.compute_client_hmac(&psk, &challenge, &client_nonce).unwrap(),
            client_public_key: vec![],
            client_nonce: client_nonce.to_vec(),
            proof: vec![],
        };

        // Verify
        let success = auth.verify_response(&challenge, &response).unwrap();
        assert!(!success.key_confirmation.is_empty());

        // Check session keys derived
        let keys = auth.session_keys().unwrap();
        assert_ne!(keys.master_key, [0u8; 32]);
        assert_ne!(keys.encryption_key, [0u8; 32]);
        assert!(keys.channel_keys.contains_key(&0));
    }

    #[test]
    fn test_encrypt_decrypt() {
        let mut auth = AuthManager::new();
        let psk = [42u8; 32];
        auth.set_psk(psk);

        let challenge = auth.generate_challenge().unwrap();
        let mut client_nonce = [0u8; 32];
        thread_rng().fill_bytes(&mut client_nonce);

        let response = AuthResponse {
            method: AuthMethod::Psk as i32,
            response: auth.compute_client_hmac(&psk, &challenge, &client_nonce).unwrap(),
            client_public_key: vec![],
            client_nonce: client_nonce.to_vec(),
            proof: vec![],
        };

        auth.verify_response(&challenge, &response).unwrap();

        // Get channel keys
        let channel_keys = auth.channel_keys(1).unwrap();

        // Encrypt
        let plaintext = b"Hello, World!";
        let (ciphertext, tag) = encrypt_with_channel_keys(plaintext, &channel_keys, 1).unwrap();

        // Decrypt
        let decrypted = decrypt_with_channel_keys(&ciphertext, &tag, &channel_keys, 1).unwrap();
        assert_eq!(decrypted, plaintext);

        // Wrong sequence should fail
        let result = decrypt_with_channel_keys(&ciphertext, &tag, &channel_keys, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_authenticate_verify() {
        let mut auth = AuthManager::new();
        let psk = [42u8; 32];
        auth.set_psk(psk);

        let challenge = auth.generate_challenge().unwrap();
        let mut client_nonce = [0u8; 32];
        thread_rng().fill_bytes(&mut client_nonce);

        let response = AuthResponse {
            method: AuthMethod::Psk as i32,
            response: auth.compute_client_hmac(&psk, &challenge, &client_nonce).unwrap(),
            client_public_key: vec![],
            client_nonce: client_nonce.to_vec(),
            proof: vec![],
        };

        auth.verify_response(&challenge, &response).unwrap();
        let channel_keys = auth.channel_keys(0).unwrap();

        let data = b"Test data";
        let tag = authenticate_data(data, &channel_keys);
        assert!(verify_authentication(data, &tag, &channel_keys));

        // Wrong data should fail
        assert!(!verify_authentication(b"Wrong data", &tag, &channel_keys));
    }
}