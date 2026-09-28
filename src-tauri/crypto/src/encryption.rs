//! Encryption primitives.
//!
//! The implementation lives in `workspace_clone_core::crypto` so that every
//! crate in the workspace shares exactly one definition. This module re-exports
//! it under the historical path so existing `use` sites keep working, and holds
//! the higher-level helpers that build on top of those primitives.

pub use workspace_clone_core::crypto::{
    decrypt, decrypt_json, derive_key, encrypt, encrypt_json, generate_salt, EncryptedPayload,
    EncryptionKey,
};

use workspace_clone_core::{Result, WorkspaceError};

/// Encrypt a JSON value under a derived key and return a storable string.
pub fn seal_json<T: serde::Serialize>(key: &EncryptionKey, value: &T) -> Result<String> {
    let json = serde_json::to_vec(value)?;
    let payload = encrypt(key, &json, SEALED_MANIFEST_AAD)?;
    Ok(serde_json::to_string(&payload)?)
}

/// Reverse of [`seal_json`].
pub fn open_json<T: serde::de::DeserializeOwned>(key: &EncryptionKey, sealed: &str) -> Result<T> {
    let payload: EncryptedPayload = serde_json::from_str(sealed)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;
    let plaintext = decrypt(key, &payload, SEALED_MANIFEST_AAD)?;
    Ok(serde_json::from_slice(&plaintext)?)
}

/// Associated data bound to every sealed manifest, so a ciphertext cannot be
/// replayed into a different context.
const SEALED_MANIFEST_AAD: &[u8] = b"workspace-clone-manifest/v1";

use workspace_clone_core::CryptoError;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_json_round_trips() {
        let key = EncryptionKey::new([9u8; 32]);
        let value = serde_json::json!({
            "workspace": { "id": "ws-1", "name": "test" },
            "projects": ["a", "b"]
        });

        let sealed = seal_json(&key, &value).unwrap();
        assert!(!sealed.contains("ws-1"), "plaintext must not be recoverable");

        let opened: serde_json::Value = open_json(&key, &sealed).unwrap();
        assert_eq!(opened, value);
    }

    #[test]
    fn sealed_json_rejects_a_different_key() {
        let key = EncryptionKey::new([9u8; 32]);
        let other = EncryptionKey::new([10u8; 32]);
        let sealed = seal_json(&key, &serde_json::json!({ "a": 1 })).unwrap();

        assert!(open_json::<serde_json::Value>(&other, &sealed).is_err());
    }
}
