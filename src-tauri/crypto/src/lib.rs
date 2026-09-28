//! Cryptography: identity keys, authenticated encryption, and the Noise
//! handshake used for device pairing and workspace transfer.
//!
//! Re-exports are listed explicitly rather than globbed, because `encryption`
//! and `keys` both surface the primitives owned by `workspace_clone_core` and
//! globbing made every such name ambiguous at the call site.

pub mod encryption;
pub mod keys;
pub mod noise;

pub use encryption::{open_json, seal_json};
pub use keys::{KeyStorage, TrustStore, TrustedDevice};
pub use noise::{
    frame_message, safety_number_from_static_keys, FrameParser, KeyPair, NoiseHandshake,
    NoiseSession, PrivateKey, PublicKey, Role, MAX_TRANSPORT_CIPHERTEXT,
    MAX_TRANSPORT_PLAINTEXT, NOISE_PARAMS,
};
