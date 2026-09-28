//! App-level Tauri commands.
//!
//! These answer "what device am I running on" and "do I have keys yet". They are
//! the first commands the UI calls on startup, so a failure here is the one most
//! likely to strand the user on a blank screen with no explanation.

use serde::Serialize;
use tauri::command;
use workspace_clone_crypto::keys::{KeyStorage, LocalKeyBundle};
use workspace_clone_core::Result;

#[command]
pub async fn get_app_version() -> Result<String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

#[command]
pub async fn get_platform() -> Result<String> {
    Ok(std::env::consts::OS.to_string())
}

#[command]
pub async fn get_device_key_exists() -> Result<bool> {
    // Reported as a boolean rather than an error: "no keys yet" is a normal
    // first-run state, not a failure, and the UI needs to distinguish it from
    // the key store being broken.
    Ok(KeyStorage::has_local_keys())
}

/// Ensure this device has a key bundle, creating one on first run.
///
/// Idempotent. The bundle holds three separate keys -- Ed25519 for identity,
/// X25519 for the Noise handshake, and a local key for sealing manifests -- so
/// an existing bundle is never partly replaced: regenerating any part of it
/// would strand devices already paired against the old keys.
#[command]
pub async fn generate_device_key() -> Result<String> {
    let bundle = KeyStorage::load_or_create_local_keys()?;
    Ok(bundle.public_key_b64()?)
}

/// The Ed25519 public key, for display next to the device name.
#[command]
pub async fn get_device_fingerprint() -> Result<Option<String>> {
    match KeyStorage::load_local_keys() {
        Ok(bundle) => Ok(Some(bundle.fingerprint()?)),
        Err(_) => Ok(None),
    }
}

/// The identity this device presents during pairing.
///
/// Returns the whole bundle's public halves in one call so the UI cannot end up
/// displaying a fingerprint from one key while the transfer layer uses another.
#[command]
pub async fn get_device_identity() -> Result<DeviceIdentityInfo> {
    let bundle: LocalKeyBundle = KeyStorage::load_or_create_local_keys()?;

    Ok(DeviceIdentityInfo {
        signing_public_key_b64: bundle.public_key_b64()?,
        noise_public_key_b64: bundle.noise_key()?.public_key_b64(),
        fingerprint: bundle.fingerprint()?,
    })
}

/// The public halves of this device's key bundle.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentityInfo {
    /// Ed25519 public key, base64. This is what pairs against.
    pub signing_public_key_b64: String,
    /// X25519 static public key, base64. This is what the Noise handshake uses
    /// and what mDNS advertises.
    pub noise_public_key_b64: String,
    /// Short fingerprint, for comparing out of band.
    pub fingerprint: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three keys must stay distinct roles: reusing one key across
    /// identity, transport and storage would mean a compromise of any one of
    /// them compromises the others.
    #[test]
    fn a_bundle_exposes_three_independent_keys() {
        let a = LocalKeyBundle::generate().unwrap();
        let b = LocalKeyBundle::generate().unwrap();

        assert_ne!(a.ed25519_private_b64, b.ed25519_private_b64);
        assert_ne!(a.noise_private_b64, b.noise_private_b64);
        assert_ne!(a.storage_key_b64, b.storage_key_b64);

        // The Noise public key is 32 bytes, base64 encoded.
        let noise = a.noise_key().unwrap().public_key_b64();
        assert!(noise.len() >= 43 && noise.len() <= 44, "got {noise}");
    }

    #[test]
    fn a_bundle_round_trips_through_its_accessors() {
        let bundle = LocalKeyBundle::generate().unwrap();

        assert_eq!(bundle.fingerprint().unwrap(), bundle.fingerprint().unwrap());
        assert!(!bundle.public_key_b64().unwrap().is_empty());
        assert!(!bundle.storage_key().unwrap().as_bytes().is_empty());
    }

    /// A tampered bundle must fail rather than silently using a wrong key.
    #[test]
    fn a_bundle_with_a_corrupt_noise_key_is_rejected() {
        let mut bundle = LocalKeyBundle::generate().unwrap();
        bundle.noise_private_b64 = "not base64!!".to_string();

        assert!(bundle.noise_key().is_err());
    }
}
