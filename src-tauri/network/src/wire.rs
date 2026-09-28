//! The message format that travels inside the Noise transport.
//!
//! Everything here runs *after* the Noise handshake, so a message is already
//! confidential and authenticated by the time it reaches this module. The codec
//! therefore only has to be unambiguous and bounded; it must not invent its own
//! security.
//!
//! Frame layout:
//!
//! ```text
//! [4 bytes: header length, big endian]
//! [header length bytes: JSON header, which states the body length]
//! [body length bytes: body]
//! ```
//!
//! The header repeats the body length rather than leaving it implied. The
//! transport authenticates the whole frame, so a live peer cannot make the two
//! disagree; the only way they can differ is a bug in one of the two sides, and
//! having `decode` insist that the parts add up means that bug is a refused
//! frame instead of a silently truncated payload.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use workspace_clone_core::{NetworkError, Result};

/// Largest header this module will accept.
///
/// Headers carry routing metadata, not payload, so anything bigger than this is
/// a peer trying to make the receiver allocate. Refusing is better than
/// buffering.
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// The protocol version this build speaks.
///
/// Advertised in the mDNS TXT record and in the by-address probe, so a peer can
/// tell "this build is too old to talk to" from "this device is not there".
/// Bumped only for a change that makes two builds genuinely unable to
/// interoperate -- a new optional frame kind does not need it, and bumping on that
/// would strand a user who has one app slightly behind.
///
/// This constant used to live in a `protocol` module alongside a second,
/// entirely separate set of message types. Nothing ever constructed or read
/// those, and they shadowed the real ones through a glob re-export, so
/// `network::MessageHeader` meant two different things depending on which one a
/// reader had in scope. The module is gone; `wire` is the protocol.
pub const PROTOCOL_VERSION: u32 = 1;

/// What a frame is for.
///
/// Every kind here travels *inside* the Noise transport, so a peer is
/// authenticated before it can say anything here. There is deliberately no
/// identity-probe kind: pairing by address uses a separate, unauthenticated probe
/// (`discovery::connect_manual`), because a probe that ran inside this transport
/// would have nothing to authenticate against yet. An earlier revision declared a
/// `Hello` kind here that nothing sent and nothing handled, with a doc comment
/// describing the other mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// One piece of the payload.
    Chunk,
    /// The sender believes it is finished and is asking the receiver to confirm
    /// the digest it computed.
    Done,
    /// Receipt of one frame.
    Ack,
    /// The receiver refused the payload, with a reason.
    Reject,
    /// One side gave up. Always sent, never forced.
    Cancel,
}

impl MessageKind {
    /// Whether this kind carries payload bytes.
    pub fn carries_payload(self) -> bool {
        matches!(self, MessageKind::Chunk)
    }
}

/// The routing metadata for one frame.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MessageHeader {
    pub kind: MessageKindWrapper,
    /// The transfer this frame belongs to.
    #[serde(default)]
    pub transfer_id: String,
    /// The workspace being moved, so a receiver with several transfers in
    /// flight can tell them apart.
    #[serde(default)]
    pub workspace_id: String,
    /// The sender's device id.
    ///
    /// A claim, like everything on the wire before the handshake authenticates
    /// the peer. It is here so a receiving screen can name the device that sent a
    /// workspace instead of showing a key, and so a log can be read. The identity
    /// that is actually trusted is `peer_static`, learned from the handshake --
    /// a sender that put someone else's id here would produce a wrong name and
    /// still fail to complete the handshake.
    #[serde(default)]
    pub sender_device_id: String,
    /// Zero-based position of a payload frame.
    #[serde(default)]
    pub chunk_index: u32,
    /// How many payload frames there are in total.
    #[serde(default)]
    pub chunk_count: u32,
    /// Total payload size in bytes, so the receiver knows when to stop.
    #[serde(default)]
    pub total_bytes: u64,
    /// Lowercase hex SHA-256 of the full payload. Sent on the final frame and
    /// echoed back in the acknowledgement, so a truncated or altered transfer
    /// is detected rather than reported as success.
    #[serde(default)]
    pub digest: String,
    /// The digest the receiver computed, echoed in the acknowledgement.
    #[serde(default)]
    pub received_digest: String,
    /// A human-readable explanation, used only for a rejection or a
    /// cancellation. Never interpreted.
    #[serde(default)]
    pub reason: String,
    /// How many body bytes follow the header.
    #[serde(default)]
    pub body_len: u64,
}

/// Newtype so `kind` can default when a header omits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageKindWrapper(pub MessageKind);

impl Default for MessageKindWrapper {
    fn default() -> Self {
        Self(MessageKind::Ack)
    }
}

impl From<MessageKind> for MessageKindWrapper {
    fn from(kind: MessageKind) -> Self {
        Self(kind)
    }
}

impl MessageHeader {
    pub fn kind(&self) -> MessageKind {
        self.kind.0
    }
}

/// A decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireMessage {
    pub header: MessageHeader,
    pub body: Vec<u8>,
}

impl WireMessage {
    pub fn new(header: MessageHeader, body: Vec<u8>) -> Self {
        Self { header, body }
    }

    /// A frame that only says something, with no payload.
    pub fn control(kind: MessageKind, transfer_id: &str, reason: &str) -> Self {
        Self {
            header: MessageHeader {
                kind: kind.into(),
                transfer_id: transfer_id.to_string(),
                reason: reason.to_string(),
                ..Default::default()
            },
            body: Vec::new(),
        }
    }

    /// Serialise into the frame layout.
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut header = self.header.clone();
        // Written from the body, not trusted from the caller's header, so the
        // two can never be out of step.
        header.body_len = self.body.len() as u64;

        let header_json = serde_json::to_vec(&header)?;
        if header_json.len() > MAX_HEADER_BYTES {
            return Err(NetworkError::Protocol(format!(
                "Header of {} bytes exceeds the {MAX_HEADER_BYTES} byte limit",
                header_json.len()
            ))
            .into());
        }

        let mut frame = Vec::with_capacity(4 + header_json.len() + self.body.len());
        frame.extend_from_slice(&(header_json.len() as u32).to_be_bytes());
        frame.extend_from_slice(&header_json);
        frame.extend_from_slice(&self.body);
        Ok(frame)
    }

    /// Parse one frame from the start of `bytes`.
    ///
    /// Returns the message and how many bytes it consumed, so a caller reading
    /// from a stream can keep the remainder. A frame whose header announces more
    /// bytes than were supplied is an error rather than a partial message: the
    /// transport frames are length-delimited, so this only happens on a
    /// corrupted or hostile stream.
    pub fn decode(bytes: &[u8]) -> Result<(Self, usize)> {
        if bytes.len() < 4 {
            return Err(NetworkError::Protocol("Truncated frame header".into()).into());
        }

        let header_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        if header_len > MAX_HEADER_BYTES {
            return Err(NetworkError::Protocol(format!(
                "Announced header of {header_len} bytes exceeds the {MAX_HEADER_BYTES} byte limit"
            ))
            .into());
        }
        if bytes.len() < 4 + header_len {
            return Err(NetworkError::Protocol(format!(
                "Frame claims a {header_len} byte header but only {} bytes are present",
                bytes.len().saturating_sub(4)
            ))
            .into());
        }

        let header: MessageHeader = serde_json::from_slice(&bytes[4..4 + header_len])
            .map_err(|e| NetworkError::Protocol(format!("Unreadable frame header: {e}")))?;

        let available = bytes.len() - (4 + header_len);
        if header.body_len as usize != available {
            return Err(NetworkError::Protocol(format!(
                "Frame header claims a {} byte body but {available} bytes follow it",
                header.body_len
            ))
            .into());
        }

        let body = bytes[4 + header_len..].to_vec();
        let consumed = 4 + header_len + body.len();
        Ok((Self { header, body }, consumed))
    }
}

/// How a payload is split into frames.
///
/// Chunks are cut on a byte boundary with no attempt to align them, because the
/// payload is an opaque sealed manifest: splitting it anywhere is fine, and
/// padding it to make alignment look tidy would only add bytes to send.
pub fn plan_chunks(payload: &[u8], chunk_size: usize) -> Result<Vec<(u32, &[u8])>> {
    if chunk_size == 0 {
        // Returning an empty plan would silently drop the payload, and returning
        // a single empty frame would too. Neither is an honest answer, so the
        // caller is told the configuration is unusable.
        return Err(NetworkError::Protocol(
            "A chunk size of zero cannot send a payload".into(),
        )
        .into());
    }
    if payload.is_empty() {
        return Ok(Vec::new());
    }

    Ok(payload
        .chunks(chunk_size)
        .enumerate()
        .map(|(index, chunk)| (index as u32, chunk))
        .collect())
}

/// Build the payload frames for a transfer.
pub fn chunk_message(
    transfer_id: &str,
    workspace_id: &str,
    payload: &[u8],
    chunk_size: usize,
) -> Result<Vec<WireMessage>> {
    let chunks = plan_chunks(payload, chunk_size)?;
    let chunk_count = chunks.len() as u32;

    Ok(chunks
        .into_iter()
        .map(|(index, bytes)| {
            WireMessage::new(
                MessageHeader {
                    kind: MessageKind::Chunk.into(),
                    transfer_id: transfer_id.to_string(),
                    workspace_id: workspace_id.to_string(),
                    chunk_index: index,
                    chunk_count,
                    total_bytes: payload.len() as u64,
                    ..Default::default()
                },
                bytes.to_vec(),
            )
        })
        .collect())
}

/// Collects payload frames back into the original bytes.
///
/// Chunks are placed by index rather than appended, so a peer that delivers
/// frames out of order still reconstructs the payload correctly. A missing or
/// repeated index is an error: silently accepting a gap is how a truncated
/// transfer would be reported as a success.
#[derive(Debug, Default)]
pub struct ChunkAssembler {
    expected: u32,
    total_bytes: Option<u64>,
    slots: BTreeMap<u32, Vec<u8>>,
}

impl ChunkAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one payload frame.
    pub fn accept(&mut self, message: &WireMessage) -> Result<()> {
        if message.header.kind() != MessageKind::Chunk {
            return Err(NetworkError::Protocol(format!(
                "Expected a payload frame, got {:?}",
                message.header.kind()
            ))
            .into());
        }

        // The shape of the transfer is checked first. Without this, a frame
        // that both repeats an index *and* disagrees about the count would be
        // reported as a replay, sending the reader looking for a duplicate
        // frame that is not the actual problem.
        if self.total_bytes.is_none() {
            self.total_bytes = Some(message.header.total_bytes);
        } else if self.total_bytes != Some(message.header.total_bytes) {
            return Err(NetworkError::Protocol(format!(
                "Payload frames disagree about the total size: {} then {}",
                self.total_bytes.unwrap_or_default(),
                message.header.total_bytes
            ))
            .into());
        }

        if self.expected == 0 {
            self.expected = message.header.chunk_count;
        } else if self.expected != message.header.chunk_count {
            return Err(NetworkError::Protocol(format!(
                "Payload frames disagree about the count: {} then {}",
                self.expected, message.header.chunk_count
            ))
            .into());
        }

        let index = message.header.chunk_index;
        if index >= self.expected {
            return Err(NetworkError::Protocol(format!(
                "Payload frame {index} is outside the announced count of {}",
                self.expected
            ))
            .into());
        }

        if self.slots.insert(index, message.body.clone()).is_some() {
            return Err(NetworkError::Protocol(format!(
                "Payload frame {index} arrived twice"
            ))
            .into());
        }

        Ok(())
    }

    /// Whether every announced frame has arrived.
    ///
    /// Zero announced frames counts as complete, which is what lets an empty
    /// payload transfer. The digest on the final frame is what confirms that the
    /// empty payload is the one the sender meant to send.
    pub fn is_complete(&self) -> bool {
        self.slots.len() == self.expected as usize
    }

    pub fn frame_count(&self) -> u32 {
        self.expected
    }

    /// Reassemble the payload.
    pub fn finish(self) -> Result<Vec<u8>> {
        if !self.is_complete() {
            return Err(NetworkError::Protocol(format!(
                "Only {} of {} payload frames arrived",
                self.slots.len(),
                self.expected
            ))
            .into());
        }

        let mut payload = Vec::with_capacity(self.total_bytes.unwrap_or(0) as usize);
        for bytes in self.slots.values() {
            payload.extend_from_slice(bytes);
        }

        if let Some(total) = self.total_bytes {
            if payload.len() as u64 != total {
                return Err(NetworkError::Protocol(format!(
                    "Reassembled {} bytes but the sender announced {total}",
                    payload.len()
                ))
                .into());
            }
        }

        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(message: &WireMessage) -> WireMessage {
        let encoded = message.encode().unwrap();
        let (decoded, consumed) = WireMessage::decode(&encoded).unwrap();
        assert_eq!(consumed, encoded.len(), "the frame length must be exact");
        decoded
    }

    #[test]
    fn a_chunk_survives_encoding() {
        let message = chunk_message("t1", "ws-1", b"hello workspace", 1024).unwrap().remove(0);

        let decoded = roundtrip(&message);

        assert_eq!(decoded.header.kind(), MessageKind::Chunk);
        assert_eq!(decoded.header.transfer_id, "t1");
        assert_eq!(decoded.header.workspace_id, "ws-1");
        assert_eq!(decoded.body, b"hello workspace");
    }

    #[test]
    fn a_large_body_survives_encoding() {
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let message = chunk_message("t1", "ws-1", &payload, 100_000).unwrap();
        let decoded: Vec<u8> = message
            .iter()
            .flat_map(|m| roundtrip(m).body)
            .collect();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn a_truncated_frame_is_rejected() {
        // Every prefix of a valid frame is invalid. Without the body length in
        // the header, cutting only the body would have decoded happily into a
        // shorter payload.
        let encoded = chunk_message("t1", "ws-1", b"payload", 1024)
            .unwrap()
            .remove(0)
            .encode()
            .unwrap();

        for cut in 0..encoded.len() {
            let result = WireMessage::decode(&encoded[..cut]);
            assert!(result.is_err(), "a {cut} byte prefix was accepted");
        }
    }

    #[test]
    fn an_oversized_header_length_is_refused_without_allocating() {
        // A peer announcing a 2 GiB header must not cause a 2 GiB reservation,
        // and neither must one announcing one byte over the limit.
        for announced in [0x7FFF_FFFFu32, MAX_HEADER_BYTES as u32 + 1] {
            let mut frame = Vec::from(announced.to_be_bytes());
            frame.resize(4 + 16, 0);

            let error = WireMessage::decode(&frame).unwrap_err().to_string();
            assert!(error.contains("exceeds"), "announced {announced}: {error}");
        }
    }

    #[test]
    fn a_header_that_understates_the_body_is_refused() {
        // The lengths disagreeing means one of the two sides has a bug, and
        // guessing which one would mean guessing at the payload.
        let encoded = chunk_message("t1", "ws-1", b"payload", 1024)
            .unwrap()
            .remove(0)
            .encode()
            .unwrap();
        let header_len = u32::from_be_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]) as usize;

        let mut header: MessageHeader =
            serde_json::from_slice(&encoded[4..4 + header_len]).unwrap();
        header.body_len = 0;
        let shorter = serde_json::to_vec(&header).unwrap();

        let mut frame = Vec::from((shorter.len() as u32).to_be_bytes());
        frame.extend_from_slice(&shorter);
        frame.extend_from_slice(&encoded[4 + header_len..]);

        let error = WireMessage::decode(&frame).unwrap_err().to_string();
        assert!(error.contains("claims a 0 byte body"), "got: {error}");
    }

    #[test]
    fn the_header_records_the_body_it_was_built_with() {
        // The body is authoritative, so a caller's stale count cannot travel.
        let mut message = WireMessage::new(MessageHeader::default(), vec![1, 2, 3]);
        message.header.body_len = 99;

        let (decoded, _) = WireMessage::decode(&message.encode().unwrap()).unwrap();

        assert_eq!(decoded.header.body_len, 3);
        assert_eq!(decoded.body, vec![1, 2, 3]);
    }

    #[test]
    fn an_unreadable_header_is_reported_clearly() {
        let mut frame = 5u32.to_be_bytes().to_vec();
        frame.extend_from_slice(b"{{{{{");

        let error = WireMessage::decode(&frame).unwrap_err().to_string();
        assert!(error.contains("Unreadable frame header"), "got: {error}");
    }

    #[test]
    fn an_empty_payload_produces_no_chunks() {
        assert!(chunk_message("t1", "ws-1", b"", 1024).unwrap().is_empty());
    }

    #[test]
    fn a_zero_chunk_size_is_refused_rather_than_dropping_the_payload() {
        // Either an empty plan or a single empty frame would report success
        // while sending nothing.
        let error = plan_chunks(&[0u8; 10], 0).unwrap_err().to_string();
        assert!(error.contains("cannot send a payload"), "got: {error}");
        assert!(chunk_message("t1", "ws-1", b"payload", 0).is_err());
    }

    #[test]
    fn chunks_reassemble_in_order() {
        let payload = b"the quick brown fox jumps over the lazy dog";
        let messages = chunk_message("t1", "ws-1", payload, 8).unwrap();

        let mut assembler = ChunkAssembler::new();
        for message in &messages {
            assembler.accept(message).unwrap();
        }
        assert!(assembler.is_complete());
        assert_eq!(assembler.finish().unwrap(), payload);
    }

    #[test]
    fn out_of_order_chunks_still_reassemble() {
        let payload: Vec<u8> = (0..100u8).collect();
        let mut messages = chunk_message("t1", "ws-1", &payload, 10).unwrap();
        messages.reverse();

        let mut assembler = ChunkAssembler::new();
        for message in &messages {
            assembler.accept(message).unwrap();
        }
        assert_eq!(assembler.finish().unwrap(), payload);
    }

    #[test]
    fn a_missing_chunk_is_refused() {
        let payload: Vec<u8> = (0..100u8).collect();
        let messages = chunk_message("t1", "ws-1", &payload, 10).unwrap();

        let mut assembler = ChunkAssembler::new();
        for message in messages.iter().skip(1) {
            assembler.accept(message).unwrap();
        }

        assert!(!assembler.is_complete());
        let error = assembler.finish().unwrap_err().to_string();
        assert!(error.contains("Only 9 of 10"), "got: {error}");
    }

    #[test]
    fn a_repeated_chunk_is_refused() {
        // A replayed frame would otherwise double a chunk and be caught only by
        // the digest, long after the bytes had been buffered.
        let messages = chunk_message("t1", "ws-1", b"payload", 1024).unwrap();

        let mut assembler = ChunkAssembler::new();
        assembler.accept(&messages[0]).unwrap();
        let error = assembler.accept(&messages[0]).unwrap_err().to_string();

        assert!(error.contains("twice"), "got: {error}");
    }

    #[test]
    fn a_chunk_with_the_wrong_count_is_refused() {
        // Two frames that disagree about how many there are. Accepting either
        // would mean the sender and receiver do not share an understanding of
        // the transfer, and the payload could not be trusted.
        let first = chunk_message("t1", "ws-1", b"aaaa", 4).unwrap().remove(0);
        let mut second = chunk_message("t1", "ws-1", b"bbbb", 4).unwrap().remove(0);
        second.header.chunk_count = 7;

        let mut assembler = ChunkAssembler::new();
        assembler.accept(&first).unwrap();
        let error = assembler.accept(&second).unwrap_err().to_string();
        assert!(error.contains("disagree"), "got: {error}");
    }

    #[test]
    fn a_chunk_with_the_wrong_total_is_refused() {
        let first = chunk_message("t1", "ws-1", b"aaaa", 4).unwrap().remove(0);
        let mut second = chunk_message("t1", "ws-1", b"bbbb", 4).unwrap().remove(0);
        second.header.total_bytes = 999;

        let mut assembler = ChunkAssembler::new();
        assembler.accept(&first).unwrap();
        let error = assembler.accept(&second).unwrap_err().to_string();
        assert!(error.contains("disagree"), "got: {error}");
    }

    #[test]
    fn a_control_message_is_refused_by_the_assembler() {
        let mut assembler = ChunkAssembler::new();
        let control = WireMessage::control(MessageKind::Ack, "t1", "");

        let error = assembler.accept(&control).unwrap_err().to_string();
        assert!(error.contains("Expected a payload frame"), "got: {error}");
    }

    #[test]
    fn an_oversized_reassembly_is_refused() {
        // The sender announces more bytes than it sends, so the reassembled
        // payload is short. Reporting that as a transfer would store a manifest
        // that never existed.
        let mut short = chunk_message("t1", "ws-1", b"short", 1024).unwrap().remove(0);
        short.header.total_bytes = 5000;

        let mut assembler = ChunkAssembler::new();
        assembler.accept(&short).unwrap();

        let error = assembler.finish().unwrap_err().to_string();
        assert!(error.contains("announced 5000"), "got: {error}");
    }

    #[test]
    fn an_empty_payload_assembles_to_nothing_without_error() {
        // A workspace with no content still has a digest to verify, so the
        // empty case must not be treated as a broken transfer.
        let mut assembler = ChunkAssembler::new();

        assert!(assembler.is_complete());
        assert_eq!(assembler.finish().unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn a_frame_beyond_the_announced_count_is_refused() {
        let messages = chunk_message("t1", "ws-1", b"abcdef", 2).unwrap();
        assert_eq!(messages.len(), 3);

        let mut assembler = ChunkAssembler::new();
        assembler.accept(&messages[0]).unwrap();
        let mut out_of_range = messages[1].clone();
        out_of_range.header.chunk_index = 99;

        let error = assembler.accept(&out_of_range).unwrap_err().to_string();
        assert!(error.contains("outside the announced count"), "got: {error}");
    }

    #[test]
    fn a_cancellation_carries_its_reason() {
        let cancel = WireMessage::control(MessageKind::Cancel, "t1", "user pressed stop");

        let decoded = roundtrip(&cancel);

        assert_eq!(decoded.header.kind(), MessageKind::Cancel);
        assert_eq!(decoded.header.reason, "user pressed stop");
        assert!(decoded.body.is_empty());
    }

    #[test]
    fn only_chunks_are_declared_to_carry_payload() {
        assert!(MessageKind::Chunk.carries_payload());
        for kind in [
            MessageKind::Done,
            MessageKind::Ack,
            MessageKind::Reject,
            MessageKind::Cancel,
        ] {
            assert!(!kind.carries_payload(), "{kind:?} must not carry payload");
        }
    }

    #[test]
    fn a_control_frame_sends_no_body() {
        let frame = WireMessage::control(MessageKind::Done, "t1", "").encode().unwrap();
        let (decoded, consumed) = WireMessage::decode(&frame).unwrap();
        assert_eq!(consumed, frame.len());
        assert!(decoded.body.is_empty());
    }
}
