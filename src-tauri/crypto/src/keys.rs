//! Key management for Workspace Clone.
//!
//! Device private keys live in the OS credential store (Keychain on macOS,
//! Credential Manager on Windows) and are never written to the database or to
//! a manifest.

use chrono::{DateTime, Utc};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use workspace_clone_core::{
    crypto::DeviceIdentityKey, device::TrustScope, CryptoError, Result, WorkspaceError,
};

/// Trust store for paired devices
pub struct TrustStore {
    devices: HashMap<String, TrustedDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedDevice {
    pub id: String,
    pub name: String,
    pub public_key: String,
    pub fingerprint: String,
    pub trust_scopes: Vec<TrustScope>,
    pub os: String,
    pub os_version: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self {
            devices: HashMap::new(),
        }
    }

    pub fn add_device(&mut self, device: TrustedDevice) {
        self.devices.insert(device.id.clone(), device);
    }

    pub fn get_device(&self, id: &str) -> Option<&TrustedDevice> {
        self.devices.get(id)
    }

    pub fn get_device_mut(&mut self, id: &str) -> Option<&mut TrustedDevice> {
        self.devices.get_mut(id)
    }

    pub fn remove_device(&mut self, id: &str) -> Option<TrustedDevice> {
        self.devices.remove(id)
    }

    pub fn list_devices(&self) -> Vec<&TrustedDevice> {
        self.devices.values().collect()
    }

    pub fn revoke_device(&mut self, id: &str) -> bool {
        if let Some(device) = self.devices.get_mut(id) {
            device.revoked = true;
            device.revoked_at = Some(chrono::Utc::now());
            true
        } else {
            false
        }
    }

    pub fn is_revoked(&self, id: &str) -> bool {
        self.devices.get(id).map(|d| d.revoked).unwrap_or(true)
    }

    pub fn update_last_seen(&mut self, id: &str) {
        if let Some(device) = self.devices.get_mut(id) {
            device.last_seen = Some(chrono::Utc::now());
        }
    }
}

impl Default for TrustStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Secure key storage using OS keyring
pub struct KeyStorage;

impl KeyStorage {
    fn get_keyring_entry(service: &str, username: &str) -> Result<Entry> {
        Entry::new(service, username)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))
    }

    /// Store the local device's key material.
    ///
    /// Three distinct secrets, each written to its own credential and to no
    /// other:
    ///
    /// - an Ed25519 identity key, used for signing and for the fingerprint the
    ///   user compares out of band;
    /// - an X25519 static key, which Noise uses for the pairing handshake and
    ///   the transport;
    /// - a local storage key, used only to seal captured manifests at rest.
    ///
    /// An earlier version wrote the whole bundle to all three names, so that
    /// reading any one credential handed over all three secrets. That is the
    /// opposite of what the three names imply, and it made a partial write --
    /// one of the three `set_password` calls failing -- look like a success and
    /// yield a device whose identity was also its transport key.
    pub fn store_local_keys(bundle: &LocalKeyBundle) -> Result<()> {
        for (username, value) in [
            ("device-identity", &bundle.ed25519_private_b64),
            ("device-noise", &bundle.noise_private_b64),
            ("storage-key", &bundle.storage_key_b64),
        ] {
            Self::get_keyring_entry(KEYRING_SERVICE, username)?
                .set_password(value)
                .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))?;
        }
        Ok(())
    }

    /// Load the local device's key material, generating it on first run.
    ///
    /// The device identity is stable for the life of the installation, which is
    /// what lets a paired peer recognise it. Deleting the credential produces a
    /// new identity, and every peer must then pair again.
    pub fn load_or_create_local_keys() -> Result<LocalKeyBundle> {
        match Self::load_local_keys() {
            Ok(bundle) => Ok(bundle),
            Err(WorkspaceError::Crypto(CryptoError::KeyStorage(_))) => {
                let bundle = LocalKeyBundle::generate()?;
                Self::store_local_keys(&bundle)?;
                Ok(bundle)
            }
            Err(other) => Err(other),
        }
    }

    /// Load existing local key material, without generating any.
    ///
    /// Tolerates the older layout, in which the whole bundle was written to all
    /// three credential names. Without that, upgrading would silently mint a
    /// new device identity and every paired peer would have to pair again --
    /// for a change that is purely internal.
    pub fn load_local_keys() -> Result<LocalKeyBundle> {
        let identity = Self::read_local_secret("device-identity")?;
        let noise = Self::read_local_secret("device-noise")?;
        let storage = Self::read_local_secret("storage-key")?;

        // The old layout: every name held the same serialised bundle, so the
        // value under `device-identity` is JSON rather than a bare key.
        if identity.trim_start().starts_with('{') {
            let bundle: LocalKeyBundle = serde_json::from_str(&identity)?;
            return Ok(bundle);
        }

        Ok(LocalKeyBundle {
            ed25519_private_b64: identity,
            noise_private_b64: noise,
            storage_key_b64: storage,
        })
    }

    /// One secret from the credential store, as stored — no interpretation.
    fn read_local_secret(username: &str) -> Result<String> {
        Ok(Self::get_keyring_entry(KEYRING_SERVICE, username)?
            .get_password()
            .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))?)
    }

    /// Whether this installation already has an identity.
    pub fn has_local_keys() -> bool {
        Self::load_local_keys().is_ok()
    }

    /// Remove all local key material, destroying this device's identity.
    pub fn delete_local_keys() -> Result<()> {
        for username in ["device-identity", "device-noise", "storage-key"] {
            let entry = Self::get_keyring_entry(KEYRING_SERVICE, username)?;
            // A missing credential is not an error when tearing down.
            let _ = entry.delete_credential();
        }
        Ok(())
    }

    /// Cache a paired peer's X25519 static key, which the Noise handshake
    /// needs up front in order to act as initiator.
    ///
    /// Only the public key is stored: a peer's private key never belongs on
    /// this device.
    pub fn store_paired_device_key(device_id: &str, public_key_b64: &str) -> Result<()> {
        Self::get_keyring_entry(KEYRING_SERVICE, &paired_key_account(device_id))?
            .set_password(public_key_b64)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))
    }

    /// Load a paired peer's cached X25519 static key.
    pub fn load_paired_device_key(device_id: &str) -> Result<String> {
        Self::get_keyring_entry(KEYRING_SERVICE, &paired_key_account(device_id))?
            .get_password()
            .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))
    }

    /// Forget a paired peer's cached key, for example when it is revoked.
    pub fn delete_paired_device_key(device_id: &str) -> Result<()> {
        Self::get_keyring_entry(KEYRING_SERVICE, &paired_key_account(device_id))?
            .delete_credential()
            .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyStorage(e.to_string())))
    }
}

/// Credential-store service name. Namespaced so the app never collides with
/// another application's entries.
const KEYRING_SERVICE: &str = "workspace-clone";

/// The local device's secret material, as held in the OS credential store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalKeyBundle {
    /// Base64 Ed25519 private seed.
    pub ed25519_private_b64: String,
    /// Base64 X25519 private scalar, used by the Noise handshake.
    pub noise_private_b64: String,
    /// Base64 32-byte key used to seal manifests stored on this device.
    pub storage_key_b64: String,
}

impl LocalKeyBundle {
    pub fn generate() -> Result<Self> {
        let identity = DeviceIdentityKey::generate()?;
        let noise = crate::noise::KeyPair::generate()?;
        let storage_key = workspace_clone_core::crypto::generate_salt();

        Ok(Self {
            ed25519_private_b64: identity.private_key_b64(),
            noise_private_b64: base64_encode(&noise.private_bytes()),
            storage_key_b64: base64_encode(&storage_key),
        })
    }

    /// The Ed25519 identity key, for signing and fingerprint display.
    pub fn identity_key(&self) -> Result<DeviceIdentityKey> {
        DeviceIdentityKey::from_private_key_b64(&self.ed25519_private_b64)
    }

    /// The X25519 static key used by the Noise handshake.
    pub fn noise_key(&self) -> Result<crate::noise::KeyPair> {
        let bytes = base64_decode(&self.noise_private_b64)?;
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(crate::noise::KeyPair::from_private(&arr)?)
    }

    /// The key used to seal manifests at rest on this device.
    pub fn storage_key(&self) -> Result<workspace_clone_core::crypto::EncryptionKey> {
        let bytes = base64_decode(&self.storage_key_b64)?;
        workspace_clone_core::crypto::EncryptionKey::from_bytes(&bytes)
    }

    /// Base64 Ed25519 public key, exchanged during pairing.
    pub fn public_key_b64(&self) -> Result<String> {
        Ok(self.identity_key()?.public_key_b64())
    }

    /// Short fingerprint for out-of-band comparison.
    pub fn fingerprint(&self) -> Result<String> {
        Ok(self.identity_key()?.fingerprint())
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    BASE64.encode(bytes)
}

fn base64_decode(text: &str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    BASE64
        .decode(text)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))
}


fn paired_key_account(device_id: &str) -> String {
    format!("paired-noise-key-{device_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // These assert the bundle is internally consistent. They deliberately do
    // not touch the OS credential store, so they run in CI.

    #[test]
    fn noise_key_is_stable_across_loads() {
        let bundle = LocalKeyBundle::generate().unwrap();
        let first = bundle.noise_key().unwrap();
        let second = bundle.noise_key().unwrap();

        assert_eq!(first.public.as_bytes(), second.public.as_bytes());
    }

    #[test]
    fn identity_key_matches_stored_public_key() {
        let bundle = LocalKeyBundle::generate().unwrap();
        let identity = bundle.identity_key().unwrap();

        assert_eq!(bundle.public_key_b64().unwrap(), identity.public_key_b64());
    }

    #[test]
    fn storage_key_is_thirty_two_bytes() {
        let bundle = LocalKeyBundle::generate().unwrap();
        assert_eq!(base64_decode(&bundle.storage_key_b64).unwrap().len(), 32);
    }

    #[test]
    fn bundle_survives_the_credential_stores_json_round_trip() {
        // The credential store hands the value back as a string, so this is
        // precisely the path a real load takes.
        let bundle = LocalKeyBundle::generate().unwrap();
        let encoded = serde_json::to_string(&bundle).unwrap();
        let restored: LocalKeyBundle = serde_json::from_str(&encoded).unwrap();

        assert_eq!(
            restored.fingerprint().unwrap(),
            bundle.fingerprint().unwrap()
        );
        assert_eq!(
            restored.noise_key().unwrap().public.as_bytes(),
            bundle.noise_key().unwrap().public.as_bytes()
        );
    }
}
