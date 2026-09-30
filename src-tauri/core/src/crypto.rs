//! Cryptographic primitives for StackHandoff - Minimal Implementation

use crate::error::{CryptoError, Result, WorkspaceError};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, Key, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey};

/// Device identity key pair (Ed25519 for signing)
#[derive(Debug, Clone)]
pub struct DeviceIdentityKey {
    pub public_key: VerifyingKey,
    private_key: [u8; 32],
}

impl DeviceIdentityKey {
    /// Generate a new device identity key pair
    pub fn generate() -> Result<Self> {
        let mut csprng = OsRng;
        let mut seed = [0u8; 32];
        csprng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            public_key: verifying_key,
            private_key: seed,
        })
    }

    /// Get the public key as base64
    pub fn public_key_b64(&self) -> String {
        BASE64.encode(self.public_key.to_bytes())
    }

    /// Get the private key as base64 (for secure storage only)
    pub fn private_key_b64(&self) -> String {
        BASE64.encode(self.private_key)
    }

    /// Load from base64 encoded private key
    pub fn from_private_key_b64(b64: &str) -> Result<Self> {
        let bytes = BASE64
            .decode(b64)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))?;
        if bytes.len() != 32 {
            return Err(WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(
                "Invalid private key length".to_string(),
            )));
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            public_key: verifying_key,
            private_key: key_bytes,
        })
    }

    /// Sign data
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        let signing_key = SigningKey::from_bytes(&self.private_key);
        let signature = signing_key.sign(data);
        signature.to_bytes().to_vec()
    }

    /// Verify signature
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<()> {
        let sig =
            Signature::from_bytes(signature.try_into().map_err(|_| {
                CryptoError::InvalidKeyFormat("Invalid signature length".to_string())
            })?);
        self.public_key
            .verify(data, &sig)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))
    }

    /// Get fingerprint (first 16 chars of public key hash)
    pub fn fingerprint(&self) -> String {
        fingerprint_from_public_key(&self.public_key)
    }
}

/// Generate a display fingerprint from a base64-encoded Ed25519 public key.
pub fn fingerprint_from_public_key_b64(public_key_b64: &str) -> Result<String> {
    let bytes = BASE64
        .decode(public_key_b64)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))?;
    let key_bytes: [u8; 32] = bytes.try_into().map_err(|_| {
        WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(
            "Public key must be 32 bytes".to_string(),
        ))
    })?;
    let public_key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))?;
    Ok(fingerprint_from_public_key(&public_key))
}

fn fingerprint_from_public_key(public_key: &VerifyingKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(public_key.to_bytes());
    let hash = hasher.finalize();
    BASE64.encode(&hash[..16])
}

/// A display fingerprint derived from a connection (X25519) key.
///
/// Separate from [`fingerprint_from_public_key_b64`] on purpose, and this is the
/// one a paired *peer* is identified by. A peer's Ed25519 key is only ever
/// advertised, never verified by anything this build performs, so deriving a
/// fingerprint from it and showing it next to a device name would assert a
/// binding that was never established. The Noise static key, by contrast, is what
/// actually authenticates a connection, so a fingerprint over it means something.
///
/// The two are not interchangeable, and a caller that mixes them up will show a
/// user two devices with the same name and different fingerprints, or one device
/// whose fingerprint changes after re-pairing.
pub fn fingerprint_from_connection_key_b64(key_b64: &str) -> Result<String> {
    let bytes = BASE64
        .decode(key_b64.trim())
        .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))?;
    if bytes.len() != 32 {
        return Err(WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(
            "A connection key must be 32 bytes".to_string(),
        )));
    }

    let mut hasher = Sha256::new();
    // Domain-separated from the Ed25519 fingerprint above, so the same 32 bytes
    // can never produce the same displayed value through the two functions.
    hasher.update(b"stackhandoff/connection-fingerprint/v1");
    hasher.update(&bytes);
    Ok(BASE64.encode(&hasher.finalize()[..16]))
}

/// Ephemeral session key pair (X25519 for key exchange)
pub struct SessionKey {
    pub public_key: PublicKey,
    private_key: EphemeralSecret,
}

impl SessionKey {
    /// Generate a new session key pair
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let private_key = EphemeralSecret::random_from_rng(&mut csprng);
        let public_key = PublicKey::from(&private_key);
        Self {
            public_key,
            private_key,
        }
    }

    /// Perform Diffie-Hellman key exchange (consumes self)
    pub fn diffie_hellman(self, peer_public: &PublicKey) -> [u8; 32] {
        let shared = self.private_key.diffie_hellman(peer_public);
        *shared.as_bytes()
    }

    /// Get public key as base64
    pub fn public_key_b64(&self) -> String {
        BASE64.encode(self.public_key.as_bytes())
    }
}

/// Encrypted payload structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub nonce: String,
    pub ciphertext: String,
    pub tag: String,
}

/// Encryption key wrapper
#[derive(Clone)]
pub struct EncryptionKey {
    key: [u8; 32],
}

impl EncryptionKey {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(
                "Key must be 32 bytes".to_string(),
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        Ok(Self { key })
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }
}

/// Encrypt data with AES-GCM
pub fn encrypt(
    key: &EncryptionKey,
    plaintext: &[u8],
    associated_data: &[u8],
) -> Result<EncryptedPayload> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad: associated_data,
            },
        )
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Encryption(e.to_string())))?;

    let tag_len = 16;
    let (ct, tag) = ciphertext.split_at(ciphertext.len() - tag_len);

    Ok(EncryptedPayload {
        nonce: BASE64.encode(nonce_bytes),
        ciphertext: BASE64.encode(ct),
        tag: BASE64.encode(tag),
    })
}

/// Decrypt data with AES-GCM
pub fn decrypt(
    key: &EncryptionKey,
    payload: &EncryptedPayload,
    associated_data: &[u8],
) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_bytes()));
    let nonce = BASE64
        .decode(&payload.nonce)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;
    let ciphertext = BASE64
        .decode(&payload.ciphertext)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;
    let tag = BASE64
        .decode(&payload.tag)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;

    if nonce.len() != 12 {
        return Err(WorkspaceError::Crypto(CryptoError::Decryption(
            "Invalid nonce length".to_string(),
        )));
    }
    if tag.len() != 16 {
        return Err(WorkspaceError::Crypto(CryptoError::Decryption(
            "Invalid tag length".to_string(),
        )));
    }

    let mut combined = ciphertext;
    combined.extend_from_slice(&tag);

    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            aes_gcm::aead::Payload {
                msg: &combined,
                aad: associated_data,
            },
        )
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;

    Ok(plaintext)
}

/// Derive encryption key from shared secret using HKDF
pub fn derive_key(shared_secret: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(Some(salt), shared_secret);
    let mut key = [0u8; 32];
    hk.expand(info, &mut key)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::KeyGeneration(e.to_string())))?;
    Ok(key)
}

/// Generate a random salt
pub fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Encrypt a JSON value
pub fn encrypt_json<T: serde::Serialize>(key_bytes: &[u8; 32], value: &T) -> Result<String> {
    let key = EncryptionKey::new(*key_bytes);
    let json = serde_json::to_vec(value)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Encryption(e.to_string())))?;
    let payload = encrypt(&key, &json, b"stackhandoff-manifest")?;
    Ok(serde_json::to_string(&payload)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Encryption(e.to_string())))?)
}

/// Decrypt a JSON value
pub fn decrypt_json<T: serde::de::DeserializeOwned>(
    key_bytes: &[u8; 32],
    encrypted: &str,
) -> Result<T> {
    let key = EncryptionKey::new(*key_bytes);
    let payload: EncryptedPayload = serde_json::from_str(encrypted)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;
    let plaintext = decrypt(&key, &payload, b"stackhandoff-manifest")?;
    serde_json::from_slice(&plaintext)
        .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))
}

/// Pairing safety numbers are derived from the Noise static keys that the
/// handshake authenticates, so they live in the `crypto` crate alongside the
/// handshake rather than here. This Ed25519 identity key is used for signing
/// and for the user-visible device fingerprint.
#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_and_distinguishing() {
        let a = DeviceIdentityKey::generate().unwrap();
        let b = DeviceIdentityKey::generate().unwrap();

        assert_eq!(a.fingerprint(), a.fingerprint());
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn fingerprint_from_b64_round_trips() {
        let key = DeviceIdentityKey::generate().unwrap();
        let from_b64 = fingerprint_from_public_key_b64(&key.public_key_b64()).unwrap();
        assert_eq!(from_b64, key.fingerprint());
    }

    #[test]
    fn signature_does_not_verify_against_another_key() {
        let signer = DeviceIdentityKey::generate().unwrap();
        let other = DeviceIdentityKey::generate().unwrap();
        let data = b"transfer request";
        let sig = signer.sign(data);

        assert!(signer.verify(data, &sig).is_ok());
        assert!(other.verify(data, &sig).is_err());
    }

    #[test]
    fn signature_does_not_verify_over_different_data() {
        let key = DeviceIdentityKey::generate().unwrap();
        let sig = key.sign(b"original");
        assert!(key.verify(b"tampered", &sig).is_err());
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn session_key_diffie_hellman_agrees() {
        let key1 = SessionKey::generate();
        let key2 = SessionKey::generate();

        let key1_public = key1.public_key.clone();
        let key2_public = key2.public_key.clone();
        let shared1 = key1.diffie_hellman(&key2_public);
        let shared2 = key2.diffie_hellman(&key1_public);

        assert_eq!(shared1, shared2);
    }
}

#[cfg(test)]
mod encryption_tests {
    use super::*;

    #[test]
    fn aes_gcm_round_trips() {
        let key = EncryptionKey::new([42u8; 32]);
        let plaintext = b"Hello, StackHandoff!";
        let aad = b"associated data";

        let encrypted = encrypt(&key, plaintext, aad).unwrap();
        let decrypted = decrypt(&key, &encrypted, aad).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn aes_gcm_rejects_wrong_associated_data() {
        let key = EncryptionKey::new([42u8; 32]);
        let encrypted = encrypt(&key, b"payload", b"aad-one").unwrap();

        assert!(decrypt(&key, &encrypted, b"aad-two").is_err());
    }

    #[test]
    fn derived_keys_differ_by_info() {
        let shared = [7u8; 32];
        let salt = generate_salt();
        let a = derive_key(&shared, &salt, b"context-a").unwrap();
        let b = derive_key(&shared, &salt, b"context-b").unwrap();
        assert_ne!(a, b);
    }
}
