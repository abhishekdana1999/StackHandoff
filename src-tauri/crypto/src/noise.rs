//! Noise protocol implementation for authenticated encryption.
//!
//! Pattern: `Noise_IK_25519_ChaChaPoly_BLAKE2s`.
//!
//! In the IK pattern the initiator already knows the responder's static public
//! key (it came from a completed pairing), so a single round trip is enough:
//!
//! ```text
//!   -> e, es, s, ss     message 1 (initiator -> responder)
//!   <- e, ee, se        message 2 (responder -> initiator)
//! ```
//!
//! The handshake is inherently stateful: neither side may enter transport mode
//! until it has read the peer's message. [`NoiseHandshake`] models that
//! explicitly so callers cannot skip a step.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use bytes::{Buf, BytesMut};
use rand::rngs::OsRng;
use snow::{params::NoiseParams, Builder, HandshakeState, TransportState};
use workspace_clone_core::{CryptoError, Result, WorkspaceError};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

/// Noise protocol parameters for StackHandoff.
pub const NOISE_PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

/// Upper bound on a single handshake message, used to pre-size buffers and to
/// reject absurd inputs before they reach the Noise state machine.
const MAX_HANDSHAKE_MESSAGE: usize = 1024;

/// Upper bound on a single transport frame as it appears on the wire.
///
/// This was previously 16 MiB, which was simply wrong: the Noise library caps
/// one transport message at 65535 bytes including the Poly1305 tag, so the
/// promise was 256 times larger than the implementation could deliver. The
/// consequence was a `write_message` failure deep inside the cipher, reported as
/// an opaque "input error" rather than as the chunk size that caused it.
///
/// The limits are spelled out separately because the plaintext and ciphertext
/// ceilings differ by the tag.
pub const MAX_TRANSPORT_CIPHERTEXT: usize = 65_535;

/// Largest plaintext one transport message can carry.
///
/// The ciphertext is 16 bytes longer, so this is the ceiling `encrypt` accepts
/// and the ceiling the frame parser must be compared against *after* decrypting.
pub const MAX_TRANSPORT_PLAINTEXT: usize = MAX_TRANSPORT_CIPHERTEXT - 16;

fn noise_params() -> Result<NoiseParams> {
    NOISE_PARAMS
        .parse::<NoiseParams>()
        .map_err(|e| WorkspaceError::Crypto(CryptoError::NoiseProtocol(e.to_string())))
}

fn noise_err<E: std::fmt::Display>(e: E) -> WorkspaceError {
    WorkspaceError::Crypto(CryptoError::NoiseProtocol(e.to_string()))
}

/// Key pair for Noise using a reconstructible static secret.
#[derive(Clone)]
pub struct KeyPair {
    pub public: PublicKey,
    pub private: PrivateKey,
}

impl KeyPair {
    pub fn generate() -> Result<Self> {
        let mut rng = OsRng;
        let private_key = StaticSecret::random_from_rng(&mut rng);
        let public_key = X25519PublicKey::from(&private_key);
        Ok(Self {
            public: PublicKey(public_key.to_bytes()),
            private: PrivateKey(private_key.to_bytes()),
        })
    }

    pub fn from_private(private: &[u8; 32]) -> Result<Self> {
        let private_key = StaticSecret::from(*private);
        let public_key = X25519PublicKey::from(&private_key);
        Ok(Self {
            public: PublicKey(public_key.to_bytes()),
            private: PrivateKey(*private),
        })
    }

    pub fn public_bytes(&self) -> &[u8] {
        &self.public.0
    }

    pub fn private_bytes(&self) -> &[u8] {
        &self.private.0
    }

    /// This key's public half, base64.
    ///
    /// The form used everywhere a key is named outside this crate: the mDNS TXT
    /// record, a pairing invitation, and a `DiscoveredDevice`. Having one
    /// conversion here means a caller cannot accidentally publish a key in a
    /// form the peer will fail to parse.
    pub fn public_key_b64(&self) -> String {
        self.public.to_base64()
    }

    /// This key's public half.
    ///
    /// For the places that need the key itself rather than its text form, such
    /// as deriving a safety number.
    pub fn public_key(&self) -> PublicKey {
        self.public.clone()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(
                "Public key must be 32 bytes".to_string(),
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_base64(&self) -> String {
        BASE64.encode(self.0)
    }

    pub fn from_base64(s: &str) -> Result<Self> {
        let bytes = BASE64
            .decode(s)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::InvalidKeyFormat(e.to_string())))?;
        Self::from_bytes(&bytes)
    }
}

#[derive(Clone, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct PrivateKey([u8; 32]);

impl PrivateKey {
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

/// Which side of the exchange we are, tracked so the state machine refuses
/// out-of-order calls instead of silently producing garbage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Initiator,
    Responder,
}

/// A Noise handshake in progress.
///
/// Correct usage for IK:
///
/// ```ignore
/// // initiator
/// let mut hs = NoiseHandshake::initiator(&local, &remote_static)?;
/// let msg1 = hs.write_message(&[])?;
/// let msg2 = read_from_peer().await?;
/// hs.read_message(&msg2)?;
/// let mut session = hs.into_transport()?;
///
/// // responder
/// let mut hs = NoiseHandshake::responder(&local)?;
/// let msg1 = read_from_peer().await?;
/// hs.read_message(&msg1)?;
/// let msg2 = hs.write_message(&[])?;
///
/// let session = hs.into_transport()?;
/// ```
pub struct NoiseHandshake {
    state: HandshakeState,
    role: Role,
}

impl NoiseHandshake {
    /// Begin as the initiator. `remote_static` must be the responder's static
    /// key obtained from a previously completed pairing.
    pub fn initiator(local_static: &KeyPair, remote_static: &PublicKey) -> Result<Self> {
        let state = Builder::new(noise_params()?)
            .local_private_key(local_static.private_bytes())
            .remote_public_key(remote_static.as_bytes())
            .build_initiator()
            .map_err(noise_err)?;

        Ok(Self {
            state,
            role: Role::Initiator,
        })
    }

    /// Begin as the responder. The responder learns the initiator's static key
    /// from message 1, so no remote key is supplied up front.
    pub fn responder(local_static: &KeyPair) -> Result<Self> {
        let state = Builder::new(noise_params()?)
            .local_private_key(local_static.private_bytes())
            .build_responder()
            .map_err(noise_err)?;

        Ok(Self {
            state,
            role: Role::Responder,
        })
    }

    /// Which side of the exchange this handshake is driving.
    pub fn role(&self) -> Role {
        self.role
    }

    /// True once both handshake messages have been exchanged.
    pub fn is_complete(&self) -> bool {
        self.state.is_handshake_finished()
    }

    /// Write the next handshake message.
    ///
    /// snow enforces the pattern's message ordering, so calling this out of
    /// turn fails rather than producing a bad transcript.
    pub fn write_message(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        if self.state.is_handshake_finished() {
            return Err(WorkspaceError::Crypto(CryptoError::NoiseProtocol(
                "handshake already complete; no further messages may be written".to_string(),
            )));
        }
        let mut out = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let len = self
            .state
            .write_message(payload, &mut out)
            .map_err(noise_err)?;
        out.truncate(len);
        Ok(out)
    }

    /// Read the peer's handshake message.
    pub fn read_message(&mut self, message: &[u8]) -> Result<Vec<u8>> {
        if message.len() > MAX_HANDSHAKE_MESSAGE {
            return Err(WorkspaceError::Crypto(CryptoError::NoiseProtocol(
                "Handshake message exceeds maximum size".to_string(),
            )));
        }
        if self.state.is_handshake_finished() {
            return Err(WorkspaceError::Crypto(CryptoError::NoiseProtocol(
                "handshake already complete; no further messages may be read".to_string(),
            )));
        }

        let mut out = vec![0u8; MAX_HANDSHAKE_MESSAGE];
        let len = self
            .state
            .read_message(message, &mut out)
            .map_err(noise_err)?;
        out.truncate(len);
        Ok(out)
    }

    /// Consume the handshake and produce a transport session.
    ///
    /// Fails if the handshake has not actually completed, which is what stops
    /// the original bug of entering transport mode straight after writing
    /// message 1 and never reading the peer's reply.
    pub fn into_transport(self) -> Result<NoiseSession> {
        if !self.state.is_handshake_finished() {
            return Err(WorkspaceError::Crypto(CryptoError::NoiseProtocol(
                "Handshake is incomplete; cannot enter transport mode".to_string(),
            )));
        }

        // Only trustworthy because the handshake completed: the pattern
        // authenticates the responder's static key to the initiator and the
        // initiator's to the responder.
        let peer_static = self
            .state
            .get_remote_static()
            .ok_or_else(|| {
                WorkspaceError::Crypto(CryptoError::NoiseProtocol(
                    "Handshake completed without authenticating a remote static key".to_string(),
                ))
            })
            .and_then(PublicKey::from_bytes)?
            .to_base64();

        let transport = self.state.into_transport_mode().map_err(noise_err)?;

        Ok(NoiseSession {
            transport,
            peer_static,
        })
    }
}

/// An established Noise transport. Encrypts and authenticates every message
/// with forward secrecy, and keeps its own nonce counter so callers cannot
/// reuse one.
pub struct NoiseSession {
    transport: TransportState,
    /// Base64 of the authenticated peer static key, captured at handshake time.
    peer_static: String,
}

impl std::fmt::Debug for NoiseSession {
    /// Deliberately omits the transport keys. `TransportState` is not `Debug`
    /// anyway, and printing key material would be a hazard.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseSession")
            .field("peer_static", &self.peer_static)
            .finish_non_exhaustive()
    }
}

impl NoiseSession {
    /// The peer's static public key, as authenticated by the handshake.
    ///
    /// This is the value both endpoints must turn into a safety number: it is
    /// only trustworthy because the handshake completed.
    pub fn peer_static(&self) -> &str {
        &self.peer_static
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        if plaintext.len() > MAX_TRANSPORT_PLAINTEXT {
            return Err(WorkspaceError::Crypto(CryptoError::Encryption(format!(
                "A {}-byte message exceeds the {MAX_TRANSPORT_PLAINTEXT}-byte transport limit",
                plaintext.len()
            ))));
        }
        // ChaChaPoly appends a 16-byte tag.
        let mut out = vec![0u8; plaintext.len() + 16];
        let len = self
            .transport
            .write_message(plaintext, &mut out)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::Encryption(e.to_string())))?;
        out.truncate(len);
        Ok(out)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() > MAX_TRANSPORT_CIPHERTEXT {
            return Err(WorkspaceError::Crypto(CryptoError::Decryption(format!(
                "A {}-byte ciphertext exceeds the {MAX_TRANSPORT_CIPHERTEXT}-byte transport limit",
                ciphertext.len()
            ))));
        }
        if ciphertext.len() < 16 {
            return Err(WorkspaceError::Crypto(CryptoError::Decryption(
                "Ciphertext is too short to contain an authentication tag".to_string(),
            )));
        }
        let mut out = vec![0u8; ciphertext.len()];
        let len = self
            .transport
            .read_message(ciphertext, &mut out)
            .map_err(|e| WorkspaceError::Crypto(CryptoError::Decryption(e.to_string())))?;
        out.truncate(len);
        Ok(out)
    }
}

/// Derive the pairing safety number from the two authenticated static keys.
///
/// Both endpoints must call this with the same two keys in the same order to
/// see the same string. Keys are ordered by their bytes so that A and B agree
/// regardless of which side initiates.
pub fn safety_number_from_static_keys(a: &PublicKey, b: &PublicKey) -> String {
    use sha2::{Digest, Sha256};

    let (first, second) = if a.as_bytes() <= b.as_bytes() {
        (a, b)
    } else {
        (b, a)
    };

    let mut hasher = Sha256::new();
    hasher.update(b"stackhandoff/safety-number/v1");
    hasher.update(first.as_bytes());
    hasher.update(second.as_bytes());
    let digest = hasher.finalize();

    // Nine groups of two bytes rendered as five digits, the same shape Signal
    // uses, which is well proven for out-of-band comparison.
    let mut groups = Vec::with_capacity(9);
    for chunk in digest[..18].chunks(2) {
        let value = (u16::from(chunk[0]) << 8) | u16::from(chunk[1]);
        groups.push(format!("{:05}", u32::from(value) % 100_000));
    }
    groups.join(" ")
}

/// Frame a message with a big-endian u32 length prefix for stream transport.
pub fn frame_message(msg: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(4 + msg.len());
    framed.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    framed.extend_from_slice(msg);
    framed
}

/// Incremental parser for length-prefixed frames.
///
/// A frame larger than [`MAX_TRANSPORT_CIPHERTEXT`] is rejected rather than
/// buffered, so a hostile peer cannot exhaust memory by announcing a huge
/// length and then trickling bytes.
pub struct FrameParser {
    buffer: BytesMut,
}

impl FrameParser {
    pub fn new() -> Self {
        Self {
            buffer: BytesMut::new(),
        }
    }

    /// Feed raw bytes, returning every complete frame that is now available.
    pub fn feed(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>> {
        self.buffer.extend_from_slice(data);
        let mut messages = Vec::new();

        while self.buffer.len() >= 4 {
            let len = u32::from_be_bytes([
                self.buffer[0],
                self.buffer[1],
                self.buffer[2],
                self.buffer[3],
            ]) as usize;

            if len > MAX_TRANSPORT_CIPHERTEXT {
                return Err(WorkspaceError::Crypto(CryptoError::Decryption(
                    format!("Announced frame size {} exceeds the maximum", len),
                )));
            }

            if self.buffer.len() < 4 + len {
                break;
            }
            self.buffer.advance(4);
            let msg = self.buffer.split_to(len).to_vec();
            messages.push(msg);
        }

        Ok(messages)
    }
}

impl Default for FrameParser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive a full IK handshake and return both transport sessions.
    fn complete_handshake() -> (NoiseSession, NoiseSession, KeyPair, KeyPair) {
        let initiator_key = KeyPair::generate().unwrap();
        let responder_key = KeyPair::generate().unwrap();

        let mut initiator = NoiseHandshake::initiator(&initiator_key, &responder_key.public).unwrap();
        let msg1 = initiator.write_message(&[]).unwrap();

        let mut responder = NoiseHandshake::responder(&responder_key).unwrap();
        responder.read_message(&msg1).unwrap();
        let msg2 = responder.write_message(&[]).unwrap();

        initiator.read_message(&msg2).unwrap();

        let initiator_session = initiator.into_transport().unwrap();
        let responder_session = responder.into_transport().unwrap();

        (
            initiator_session,
            responder_session,
            initiator_key,
            responder_key,
        )
    }

    #[test]
    fn restoring_private_bytes_reproduces_the_key_pair() {
        let generated = KeyPair::generate().unwrap();
        let private_bytes = generated.private.to_bytes();
        let restored = KeyPair::from_private(&private_bytes).unwrap();

        assert_eq!(restored.private.to_bytes(), private_bytes);
        assert_eq!(restored.public.as_bytes(), generated.public.as_bytes());
    }

    #[test]
    fn initiator_cannot_enter_transport_before_reading_responder() {
        let initiator_key = KeyPair::generate().unwrap();
        let responder_key = KeyPair::generate().unwrap();

        let mut initiator =
            NoiseHandshake::initiator(&initiator_key, &responder_key.public).unwrap();
        let _msg1 = initiator.write_message(&[]).unwrap();

        // This is the exact bug that made the original transport unusable:
        // writing message 1 is not enough, the peer's reply must be read.
        let err = initiator.into_transport().unwrap_err();
        assert!(
            err.to_string().contains("incomplete"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn initiator_cannot_write_a_second_message() {
        let initiator_key = KeyPair::generate().unwrap();
        let responder_key = KeyPair::generate().unwrap();
        let mut initiator =
            NoiseHandshake::initiator(&initiator_key, &responder_key.public).unwrap();
        let _msg1 = initiator.write_message(&[]).unwrap();

        // IK gives the initiator exactly one write; snow rejects the second.
        assert!(initiator.write_message(&[]).is_err());
    }

    #[test]
    fn handshake_authenticates_both_static_keys() {
        let (initiator, responder, initiator_key, responder_key) = complete_handshake();

        // Each side learns the other's static key from the handshake, and it
        // must match the key we started with.
        assert_eq!(initiator.peer_static(), responder_key.public.to_base64());
        assert_eq!(responder.peer_static(), initiator_key.public.to_base64());
    }

    #[test]
    fn transport_round_trips_in_both_directions() {
        let (mut initiator, mut responder, _, _) = complete_handshake();

        let request = b"workspace manifest bytes";
        let encrypted = initiator.encrypt(request).unwrap();
        let decrypted = responder.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, request);

        let reply = b"ack";
        let encrypted = responder.encrypt(reply).unwrap();
        let decrypted = initiator.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, reply);
    }

    #[test]
    fn tampering_is_detected() {
        let (mut initiator, mut responder, _, _) = complete_handshake();
        let mut encrypted = initiator.encrypt(b"payload").unwrap();

        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xff;

        assert!(responder.decrypt(&encrypted).is_err());
    }

    #[test]
    fn sessions_from_different_handshakes_cannot_talk() {
        let (mut a1, _b1, _, _) = complete_handshake();
        let (mut a2, _b2, _, _) = complete_handshake();

        let encrypted = a1.encrypt(b"secret").unwrap();
        assert!(a2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn safety_number_agrees_regardless_of_initiator() {
        let a = KeyPair::generate().unwrap();
        let b = KeyPair::generate().unwrap();

        let from_a = safety_number_from_static_keys(&a.public, &b.public);
        let from_b = safety_number_from_static_keys(&b.public, &a.public);

        assert_eq!(from_a, from_b);
        assert_eq!(from_a.split(' ').count(), 9);
    }

    #[test]
    fn safety_number_differs_for_different_keys() {
        let a = KeyPair::generate().unwrap();
        let b = KeyPair::generate().unwrap();
        let c = KeyPair::generate().unwrap();

        assert_ne!(
            safety_number_from_static_keys(&a.public, &b.public),
            safety_number_from_static_keys(&a.public, &c.public)
        );
    }

    #[test]
    fn frame_parser_reassembles_split_frames() {
        let mut parser = FrameParser::new();
        let framed = frame_message(b"hello world");

        // Deliver one byte at a time to exercise the partial-frame path.
        let mut collected = Vec::new();
        for byte in &framed {
            collected.extend(parser.feed(&[*byte]).unwrap());
        }

        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0], b"hello world");
    }

    #[test]
    fn frame_parser_handles_multiple_frames_in_one_read() {
        let mut parser = FrameParser::new();
        let mut data = frame_message(b"one");
        data.extend(frame_message(b"two"));

        let messages = parser.feed(&data).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], b"one");
        assert_eq!(messages[1], b"two");
    }

    #[test]
    fn frame_parser_rejects_oversized_length_prefix() {
        let mut parser = FrameParser::new();
        // Announce a 4 GiB frame without sending any of it.
        let hostile = (u32::MAX).to_be_bytes();

        let err = parser.feed(&hostile).unwrap_err();
        assert!(err.to_string().contains("exceeds the maximum"), "got: {err}");
    }

    #[test]
    fn transport_rejects_undersized_ciphertext() {
        let (mut _initiator, mut responder, _, _) = complete_handshake();
        assert!(responder.decrypt(b"short").is_err());
    }

    #[test]
    fn a_message_at_the_plaintext_limit_is_sent_and_received() {
        // The stated limit has to be one the implementation can actually reach.
        // It previously claimed 16 MiB while the Noise library caps a message at
        // 65535 bytes, so every multi-chunk transfer failed at the second write
        // with an opaque cipher error. This test is the guard against that
        // coming back.
        let (mut initiator, mut responder, _, _) = complete_handshake();

        let payload = vec![7u8; MAX_TRANSPORT_PLAINTEXT];
        let ciphertext = initiator.encrypt(&payload).expect("the limit must be sendable");
        assert_eq!(ciphertext.len(), MAX_TRANSPORT_CIPHERTEXT);
        assert_eq!(responder.decrypt(&ciphertext).unwrap(), payload);
    }

    #[test]
    fn one_byte_over_the_plaintext_limit_is_refused_with_a_usable_message() {
        let (mut initiator, _responder, _, _) = complete_handshake();

        let error = initiator
            .encrypt(&vec![0u8; MAX_TRANSPORT_PLAINTEXT + 1])
            .unwrap_err()
            .to_string();

        // An opaque "input error" sends the reader looking for a cipher problem
        // rather than the chunk size that caused it.
        assert!(error.contains(&MAX_TRANSPORT_PLAINTEXT.to_string()), "got: {error}");
        assert!(error.contains("transport limit"), "got: {error}");
    }

    #[test]
    fn a_ciphertext_over_the_limit_is_refused() {
        let (_initiator, mut responder, _, _) = complete_handshake();

        let error = responder
            .decrypt(&vec![0u8; MAX_TRANSPORT_CIPHERTEXT + 1])
            .unwrap_err()
            .to_string();

        assert!(error.contains(&MAX_TRANSPORT_CIPHERTEXT.to_string()), "got: {error}");
    }

    #[test]
    fn a_frame_at_the_ciphertext_limit_round_trips_through_the_parser() {
        // The parser's ceiling and the cipher's ceiling must agree, or a legal
        // message is rejected by the framing layer.
        let (mut initiator, mut responder, _, _) = complete_handshake();
        let payload = vec![1u8; MAX_TRANSPORT_PLAINTEXT];
        let ciphertext = initiator.encrypt(&payload).unwrap();

        let mut parser = FrameParser::new();
        let frames = parser.feed(&frame_message(&ciphertext)).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(responder.decrypt(&frames[0]).unwrap().len(), payload.len());
    }
}
