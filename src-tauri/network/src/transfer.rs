//! Moving a sealed manifest from one device to another.
//!
//! ## How a transfer is protected
//!
//! Noise_IK authenticates the initiator against a key it already knows and
//! authenticates the responder by its static key, which is published in the
//! mDNS TXT record. After the handshake, every frame is encrypted and
//! authenticated by the transport session.
//!
//! That is the *only* encryption applied to the payload in flight, and
//! deliberately so. A previous version of this file also encrypted the payload
//! with a hardcoded 32-byte key, which meant every installation used the same
//! key: real protection came from the transport, and the extra layer only made
//! the file look cautious. A manifest is sealed with a *local* storage key when
//! it is written to disk, and that sealed form is what travels.
//!
//! ## What a successful transfer means
//!
//! The sender computes a SHA-256 over the payload and sends it on the final
//! frame. The receiver computes its own and echoes it back. The sender compares
//! the two before reporting success, so a truncated or altered transfer fails
//! rather than being reported as done.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use tokio::sync::watch;
use if_addrs::{get_if_addrs, Interface};
use socket2::{Domain, Protocol, Socket, Type};
use tracing::{debug, info, warn};
use workspace_clone_core::{device::*, NetworkError, Result};
use workspace_clone_crypto::noise::{
    frame_message, FrameParser, KeyPair, NoiseHandshake, NoiseSession, PublicKey,
};
use workspace_clone_crypto::{MAX_TRANSPORT_CIPHERTEXT, MAX_TRANSPORT_PLAINTEXT};

use crate::wire::{
    chunk_message, ChunkAssembler, MessageHeader, MessageKind, WireMessage,
};

/// The port the transfer listener binds when no port is configured.
pub const DEFAULT_PORT: u16 = 47890;

/// How long a connection may take before it is abandoned.
///
/// Generous enough for a slow Wi-Fi handshake, short enough that a peer that
/// accepts and then says nothing does not hold a thread for ever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Bounds the Noise handshake, separately from the transfer itself.
///
/// A peer that accepts a connection and then sends nothing would otherwise hold
/// the transfer open for the full `FRAME_TIMEOUT`, which is tuned for a large
/// payload in flight. The handshake is two small messages, so it either happens
/// promptly or is not going to happen.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// Bounds a single read during transfer. Generous, because a slow disk on the
/// receiving side can stall a frame for a while and there is no window to slide.
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Sends and receives workspaces.
pub struct TransferService {
    /// Held behind an `Arc` so a [`TransferReceiver`] can be built from it.
    ///
    /// A `TcpListener` is a handle to an OS socket, not ownership of it, so two
    /// handles to the same socket accept from the *same* queue of pending
    /// connections. That is what lets a long-lived background task accept
    /// incoming transfers while a command handler sends one at the same time.
    listener: Option<Arc<TcpListener>>,
    local_static: KeyPair,
    local_device_id: String,
    config: TransferConfig,
    /// Set to true to ask an in-flight transfer to stop.
    cancel: Arc<AtomicBool>,
    /// Publishes the cancel flag so a blocked transfer wakes on a change.
    ///
    /// The flag alone was not enough. A send spends nearly all of its time
    /// blocked in a read, and a flag is only observed at a frame boundary, so a
    /// user who cancelled a transfer that was waiting on a silent peer watched
    /// nothing happen for the length of the handshake timeout — fifteen seconds
    /// — and up to thirty if a frame stalled mid-transfer. The flag is kept as
    /// the source of truth; this is what makes noticing it prompt.
    ///
    /// A `watch` channel rather than a `Notify`, because the flag is resettable
    /// and a transfer can be re-used afterwards. A `Notify` stores a permit when
    /// nobody is listening, and there is no way to take that permit back out, so
    /// a cancel followed by a reset would leave the *next* transfer to see a
    /// stale wake-up and cancel itself. A `watch` value is versioned, so
    /// `reset_cancel` genuinely clears it and every subscriber sees the
    /// difference.
    cancel_tx: watch::Sender<bool>,
    /// Peers the command layer has resolved, by device id.
    known: std::sync::Mutex<HashMap<String, DiscoveredDevice>>,
    /// Test-only: makes the receiver store a payload that does not match the
    /// digest the sender announced, standing in for a corrupted transfer.
    ///
    /// An `Arc<AtomicBool>` rather than a `bool` so a flag set on the service is
    /// visible to a receiver built from it afterwards, which is how every test
    /// that uses it is written.
    #[cfg(test)]
    force_digest_mismatch: Arc<AtomicBool>,
}

impl TransferService {
    pub fn new(
        local_static: KeyPair,
        local_device_id: impl Into<String>,
        config: TransferConfig,
    ) -> Self {
        Self {
            listener: None,
            local_static,
            local_device_id: local_device_id.into(),
            config,
            cancel: Arc::new(AtomicBool::new(false)),
            cancel_tx: watch::channel(false).0,
            known: std::sync::Mutex::new(HashMap::new()),
            #[cfg(test)]
            force_digest_mismatch: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A handle that can stop an in-flight transfer from another task.
    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// Ask any in-flight transfer to stop.
    ///
    /// The flag is only ever set, never cleared, so a caller that cancels twice
    /// does not accidentally re-enable a later transfer sharing the service.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        // The flag is set first, so a waiter that wakes and re-reads the flag
        // sees a consistent value even if it raced with this send.
        let _ = self.cancel_tx.send(true);
    }

    /// Clear the cancel flag so the service can be reused.
    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
        let _ = self.cancel_tx.send(false);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Resolves once a cancel has been requested.
    ///
    /// Observes a request made before this was called as well as one made
    /// after. Subscribing first means a change afterwards wakes this waiter,
    /// and `borrow_and_update` means a change beforehand is already visible in
    /// the value — so there is no window in which a cancel can be missed.
    async fn wait_for_cancel(&self) {
        let mut rx = self.cancel_tx.subscribe();
        if *rx.borrow_and_update() {
            return;
        }
        // Resolves on the next publish, whichever way it goes. A second
        // `reset_cancel` would wake this too, but nothing resets a flag while a
        // transfer is in flight, and treating that as a stop is the safe
        // reading: it can only end a transfer the caller was already tearing
        // down.
        let _ = rx.changed().await;
    }

    /// Start listening for incoming transfers.
    ///
    /// Prefers a dual-stack socket on `[::]` with `IPV6_V6ONLY` switched off,
    /// so a peer that discovered this device by its link-local IPv6 address can
    /// connect over IPv6 exactly like a peer that reached it over IPv4. Falls
    /// back to the long-standing IPv4-only bind when the platform cannot offer
    /// that, so a bind that used to work still does.
    pub async fn start(&mut self, port: u16) -> Result<u16> {
        let listener = bind_listener(port)?;
        let bound = listener
            .local_addr()
            .map_err(|e| NetworkError::Connection(e.to_string()))?
            .port();

        self.listener = Some(Arc::new(listener));
        info!("Transfer listener bound on port {bound}");
        Ok(bound)
    }

    pub fn is_listening(&self) -> bool {
        self.listener.is_some()
    }

    /// Note a peer the user has selected, so a send can be addressed by id.
    ///
    /// The transfer layer deliberately does not talk to the database, so a
    /// command that has resolved a paired device hands it here. A device with no
    /// address is still recorded -- it may be reachable after a rediscovery --
    /// but a send to it will say so rather than hanging.
    pub fn remember_device(&self, device: DiscoveredDevice) {
        self.known
            .lock()
            .expect("the peer map is never poisoned")
            .insert(device.device_id.clone(), device);
    }

    /// The peer for an id, if one is known.
    pub fn discovered_device(&self, device_id: &str) -> Option<DiscoveredDevice> {
        self.known
            .lock()
            .expect("the peer map is never poisoned")
            .get(device_id)
            .cloned()
    }

    /// Every peer this service knows about.
    pub fn known_devices(&self) -> Vec<DiscoveredDevice> {
        self.known
            .lock()
            .expect("the peer map is never poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn local_port(&self) -> Option<u16> {
        self.listener
            .as_ref()
            .and_then(|l| l.local_addr().ok())
            .map(|a| a.port())
    }

    /// A shareable handle for accepting incoming transfers.
    ///
    /// The point of this type is that accepting does not need the send half of
    /// the service. A background task that owns a `TransferReceiver` can sit in
    /// `accept` for the lifetime of the app while a command handler sends a
    /// workspace over the same listener at the same time. Doing this with
    /// `TransferService` itself is not possible: the service is behind a
    /// `tokio::sync::Mutex`, and holding that lock for as long as a transfer
    /// takes would block every send, every discovered-device lookup and every
    /// cancel request for the duration of the transfer.
    pub fn receiver(&self) -> Result<TransferReceiver> {
        let listener = self
            .listener
            .clone()
            .ok_or_else(|| NetworkError::Connection("The transfer listener is not running".into()))?;

        Ok(TransferReceiver {
            listener,
            local_static: self.local_static.clone(),
            #[cfg(test)]
            force_digest_mismatch: self.force_digest_mismatch.clone(),
        })
    }

    /// Accept one connection and receive a workspace.
    ///
    /// Returns `None` when the listener has been stopped, so a caller can tell
    /// "shutting down" from "failed".
    pub async fn accept_once(&self) -> Result<Option<ReceivedTransfer>> {
        self.accept_once_with_progress(None).await
    }

    /// [`Self::accept_once`], reporting progress as frames arrive.
    ///
    /// A transfer can take a while and the receiving device's window is the one
    /// that needs a progress bar: the sender's window closes as soon as it hands
    /// the payload over. Without this the receiving screen has nothing to show but
    /// a spinner for the whole transfer.
    pub async fn accept_once_with_progress(
        &self,
        on_progress: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> Result<Option<ReceivedTransfer>> {
        self.receiver()?.accept_once_with_progress(on_progress).await
    }

    /// Handle one already-accepted connection.
    pub async fn receive_on(&self, stream: TcpStream) -> Result<ReceivedTransfer> {
        self.receive_on_with_progress(stream, None).await
    }

    /// [`Self::receive_on`], reporting progress as frames arrive.
    ///
    /// Progress is only reported when the sender announced a total size, because
    /// a fraction of an unknown whole is a number that lies. A sender that did not
    /// announce one produces no progress callbacks at all rather than a bar that
    /// creeps towards a total nobody agreed to.
    pub async fn receive_on_with_progress(
        &self,
        stream: TcpStream,
        on_progress: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> Result<ReceivedTransfer> {
        self.receiver()?
            .receive_on_with_progress(stream, on_progress)
            .await
    }
}

/// Accepts incoming transfers, and nothing else.
///
/// Split out of [`TransferService`] so a long-lived task can own receiving
/// without holding the lock that guards sending. Everything it needs is a
/// reference to the bound listener and a clone of the local static key, so
/// there is no per-connection mutable state and any number of these can accept
/// concurrently.
///
/// Cloneable for the same reason: a caller that accepts a connection and then
/// handles it on its own task needs a handle on that task, and a
/// [`Self::accept_connection`] to get the stream with.
#[derive(Clone)]
pub struct TransferReceiver {
    listener: Arc<TcpListener>,
    local_static: KeyPair,
    /// Test-only; see [`TransferService::force_digest_mismatch`].
    #[cfg(test)]
    force_digest_mismatch: Arc<AtomicBool>,
}

impl TransferReceiver {
    /// Accept a connection without handling it.
    ///
    /// The caller drives the returned stream, normally on its own task via
    /// [`Self::receive_on_with_progress`].
    ///
    /// Exposed because handling a connection inline serialises the listener
    /// behind it, and handling is bounded by `HANDSHAKE_TIMEOUT`. A single peer
    /// that connects and then says nothing therefore delays *every other peer*
    /// by fifteen seconds -- which is not a slow transfer, it is a machine that
    /// has stopped accepting while it waits on one connection nobody is speaking
    /// on. Accepting first and handling separately is what keeps a bad peer from
    /// being able to make this device unreachable.
    pub async fn accept_connection(&self) -> Result<(TcpStream, SocketAddr)> {
        let (stream, peer) = self
            .listener
            .accept()
            .await
            .map_err(|e| NetworkError::Connection(e.to_string()))?;
        info!("Accepted a connection from {peer}");
        Ok((stream, peer))
    }

    /// Accept one connection and receive a workspace.
    ///
    /// Returns `Ok(None)` when the connection produced no workspace -- a failed
    /// handshake, a cancelled send, a digest mismatch. The distinction matters to
    /// the sender, which is told a real reason, and matters here because a loop of
    /// these has to keep going after a bad connection instead of treating one
    /// refused peer as a reason to stop listening for every later one.
    pub async fn accept_once(&self) -> Result<Option<ReceivedTransfer>> {
        self.accept_once_with_progress(None).await
    }

    /// [`Self::accept_once`], reporting progress as frames arrive.
    pub async fn accept_once_with_progress(
        &self,
        on_progress: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> Result<Option<ReceivedTransfer>> {
        let (stream, peer) = self.accept_connection().await?;

        match self.receive_on_with_progress(stream, on_progress).await {
            Ok(received) => Ok(Some(received)),
            Err(e) => {
                warn!("Incoming transfer from {peer} failed: {e}");
                Ok(None)
            }
        }
    }

    /// Handle one already-accepted connection, returning its real error.
    pub async fn receive_on(&self, stream: TcpStream) -> Result<ReceivedTransfer> {
        self.receive_on_with_progress(stream, None).await
    }

    /// [`Self::receive_on`], reporting progress as frames arrive.
    ///
    /// Progress is only reported when the sender announced a total size, because
    /// a fraction of an unknown whole is a number that lies. A sender that did not
    /// announce one produces no progress callbacks at all rather than a bar that
    /// creeps towards a total nobody agreed to.
    pub async fn receive_on_with_progress(
        &self,
        stream: TcpStream,
        on_progress: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> Result<ReceivedTransfer> {
        let (mut read_half, mut write_half) = stream.into_split();
        let mut session = self.respond(&mut read_half, &mut write_half).await?;

        let peer_static = session.peer_static().to_string();
        debug!("Handshake complete with peer key {peer_static}");

        let transfer_id = uuid::Uuid::new_v4().to_string();
        let mut assembler = ChunkAssembler::new();
        let mut workspace_id = String::new();
        let mut received_bytes: u64 = 0;
        // `None` until a chunk says how big the payload is. Distinguishing "not
        // yet known" from "genuinely zero bytes" matters: a zero-length workspace
        // is legal and produces no chunks at all, so it is reported complete at
        // `Done` rather than sitting at zero for ever.
        let mut announced_total: Option<u64> = None;
        // A claim from the wire, not an authenticated identity. The key that
        // authenticated the handshake is `peer_static`; this only names the device
        // for the user.
        let mut sender_device_id = String::new();
        let started = std::time::Instant::now();

        loop {
            let message = read_frame(&mut read_half, &mut session).await?;

            match message.header.kind() {
                MessageKind::Chunk => {
                    if workspace_id.is_empty() {
                        workspace_id = message.header.workspace_id.clone();
                    }
                    // Every chunk header carries the total, so the first one is
                    // enough. Read from a chunk rather than from a separate
                    // announcement frame, because a fraction of an unknown whole
                    // is a number that lies.
                    announced_total.get_or_insert(message.header.total_bytes);
                    if sender_device_id.is_empty() {
                        sender_device_id = message.header.sender_device_id.clone();
                    }
                    assembler.accept(&message)?;
                    received_bytes += message.body.len() as u64;

                    if let (Some(report), Some(total)) = (&on_progress, announced_total) {
                        if total > 0 {
                            report((received_bytes as f32 / total as f32).clamp(0.0, 1.0));
                        }
                    }

                    // Acknowledge every frame. A sender that waits for each one
                    // is a sender that cannot outrun the receiver's disk, and
                    // this protocol has no window to slide.
                    write_frame(
                        &mut write_half,
                        &mut session,
                        &WireMessage::new(
                            MessageHeader {
                                kind: MessageKind::Ack.into(),
                                transfer_id: transfer_id.clone(),
                                workspace_id: workspace_id.clone(),
                                chunk_index: message.header.chunk_index,
                                ..Default::default()
                            },
                            Vec::new(),
                        ),
                    )
                    .await?;
                }
                MessageKind::Done => {
                    let expected_digest = message.header.digest.clone();
                    // Only the test-only corruption hook below writes to it, so
                    // `mut` is genuinely unneeded in a release build. Suppressing
                    // the warning is better than restructuring the hook to return a
                    // new value, which would obscure the one line that exists to
                    // make a digest mismatch reproducible.
                    #[allow(unused_mut)]
                    let mut payload = assembler.finish()?;
                    #[cfg(test)]
                    if self.force_digest_mismatch.load(Ordering::SeqCst) {
                        payload.push(b'!');
                    }
                    let actual = hex_digest(&payload);

                    if !expected_digest.is_empty() && actual != expected_digest {
                        // Tell the sender plainly rather than closing, so it can
                        // report a real reason instead of a broken pipe.
                        let _ = write_frame(
                            &mut write_half,
                            &mut session,
                            &WireMessage::control(
                                MessageKind::Reject,
                                &transfer_id,
                                "The received payload did not match the digest the sender announced",
                            ),
                        )
                        .await;
                        return Err(NetworkError::Transfer(format!(
                            "Digest mismatch: the sender announced {expected_digest}, the payload hashes to {actual}"
                        ))
                        .into());
                    }

                    write_frame(
                        &mut write_half,
                        &mut session,
                        &WireMessage::new(
                            MessageHeader {
                                kind: MessageKind::Ack.into(),
                                transfer_id: transfer_id.clone(),
                                workspace_id: workspace_id.clone(),
                                received_digest: actual,
                                ..Default::default()
                            },
                            Vec::new(),
                        ),
                    )
                    .await?;

                    if let Some(report) = &on_progress {
                        report(1.0);
                    }

                    return Ok(ReceivedTransfer {
                        transfer_id,
                        workspace_id,
                        payload,
                        peer_static,
                        sender_device_id,
                        digest: expected_digest,
                        duration: started.elapsed(),
                    });
                }
                MessageKind::Cancel => {
                    return Err(NetworkError::Transfer(format!(
                        "The sender cancelled: {}",
                        if message.header.reason.is_empty() {
                            "no reason given"
                        } else {
                            &message.header.reason
                        }
                    ))
                    .into())
                }
                other => {
                    return Err(NetworkError::Protocol(format!(
                        "Unexpected message kind {other:?} while receiving"
                    ))
                    .into())
                }
            }
        }
    }

    /// Complete the responder side of the handshake.
    async fn respond<R, W>(&self, read_half: &mut R, write_half: &mut W) -> Result<NoiseSession>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    {
        let first = read_frame_raw_within(
            read_half,
            HANDSHAKE_TIMEOUT,
            "The other device connected but never began the handshake",
        )
        .await?;
        let mut noise = NoiseHandshake::responder(&self.local_static)?;
        noise.read_message(&first)?;
        let reply = noise.write_message(&[])?;

        write_frame_raw(write_half, &reply).await?;

        Ok(noise.into_transport()?)
    }
}

impl TransferService {
    /// Send a payload to a discovered device.
    ///
    /// `on_progress` is called with a value from 0.0 to 1.0 as frames are
    /// acknowledged, never before the frame is confirmed, so the number a user
    /// sees always matches what has actually landed.
    pub async fn send_workspace(
        &self,
        destination: &DiscoveredDevice,
        workspace_id: &str,
        payload: &[u8],
        on_progress: Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> Result<TransferSession> {
        let transfer_id = uuid::Uuid::new_v4().to_string();
        let mut session = TransferSession::new(transfer_id.clone(), workspace_id);
        let started = std::time::Instant::now();

        let outcome = self
            .send_inner(destination, workspace_id, payload, &transfer_id, &mut session, &on_progress)
            .await;

        match outcome {
            Ok(()) => {
                session.status = TransferStatus::Completed;
                session.progress = 1.0;
                session.completed_at = Some(chrono::Utc::now());
            }
            Err(SendFailure::Cancelled(reason)) => {
                session.status = TransferStatus::Cancelled;
                session.error = Some(reason);
                session.completed_at = Some(chrono::Utc::now());
            }
            Err(SendFailure::Error(e)) => {
                session.status = TransferStatus::Failed;
                session.error = Some(e.to_string());
                session.completed_at = Some(chrono::Utc::now());
            }
        }

        info!(
            "Transfer {transfer_id} finished as {:?} in {:?}",
            session.status,
            started.elapsed()
        );
        Ok(session)
    }

    async fn send_inner(
        &self,
        destination: &DiscoveredDevice,
        workspace_id: &str,
        payload: &[u8],
        transfer_id: &str,
        session: &mut TransferSession,
        on_progress: &Option<Arc<dyn Fn(f32) + Send + Sync>>,
    ) -> std::result::Result<(), SendFailure> {
        if self.is_cancelled() {
            return Err(SendFailure::Cancelled(
                "The transfer was cancelled before it started".into(),
            ));
        }

        let remote_static = public_key_of(destination)?;

        // Every advertised address is a candidate, IPv4 first. An mDNS record
        // frequently leads with a link-local IPv6 address like
        // `fe80::f727:abc9:2280:4f3:54108`, and connecting to that as written
        // fails on macOS with "No route to host": the address names the *link*,
        // and the kernel refuses to guess which of your interfaces the peer is
        // on. `candidate_socket_addrs` also scopes such addresses to the local
        // interfaces that can host them, and the race below takes the first
        // candidate that answers, so a peer reached over IPv4 connects even
        // when its record lists IPv6 first.
        let port = if destination.port == 0 {
            DEFAULT_PORT
        } else {
            destination.port
        };
        let candidates = candidate_socket_addrs(
            &destination.addresses,
            port,
            &get_if_addrs().unwrap_or_default(),
        );
        if candidates.is_empty() {
            return Err(SendFailure::Error(
                NetworkError::Connection(format!(
                    "{} advertised no address to connect to",
                    destination.name
                ))
                .into(),
            ));
        }

        session.status = TransferStatus::Connecting;

        let mut stream = tokio::select! {
            // A cancel that arrives while connecting is honoured here rather than
            // after the connect budget expires, which for an address that will
            // never answer is ten seconds of a button that looks broken.
            biased;
            _ = self.wait_for_cancel() => {
                return Err(SendFailure::Cancelled(
                    "The transfer was cancelled before the connection was made".into(),
                ));
            }
            connected = connect_any(&candidates, CONNECT_TIMEOUT) => {
                match connected {
                    Ok(stream) => stream,
                    Err(ConnectFailure::TimedOut { first, budget }) => {
                        return Err(SendFailure::Error(
                            NetworkError::Connection(format!(
                                "Connecting to {first} timed out after {} seconds",
                                budget.as_secs()
                            ))
                            .into(),
                        ));
                    }
                    Err(ConnectFailure::Refused { address, error }) => {
                        return Err(SendFailure::Error(
                            NetworkError::Connection(format!(
                                "Could not reach {address}: {error}"
                            ))
                            .into(),
                        ));
                    }
                }
            }
        };

        let (mut read_half, write_half) = stream.split();
        session.status = TransferStatus::Handshaking;

        let mut noise = NoiseHandshake::initiator(&self.local_static, &remote_static)
            .map_err(|e| SendFailure::Error(NetworkError::Handshake(e.to_string()).into()))?;

        let first = noise
            .write_message(&[])
            .map_err(|e| SendFailure::Error(NetworkError::Handshake(e.to_string()).into()))?;
        let mut write_half = write_half;
        write_frame_raw(&mut write_half, &first)
            .await
            .map_err(SendFailure::from)?;

        let reply = tokio::select! {
            biased;
            _ = self.wait_for_cancel() => {
                return Err(SendFailure::Cancelled(
                    "The transfer was cancelled while waiting for the other device to answer".into(),
                ));
            }
            reply = read_frame_raw_within(
                &mut read_half,
                HANDSHAKE_TIMEOUT,
                "The other device accepted the connection but never answered the handshake",
            ) => reply.map_err(SendFailure::from)?,
        };
        noise
            .read_message(&reply)
            .map_err(|e| SendFailure::Error(NetworkError::Handshake(e.to_string()).into()))?;

        let mut noise = noise
            .into_transport()
            .map_err(|e| SendFailure::Error(NetworkError::Handshake(e.to_string()).into()))?;

        session.status = TransferStatus::Transferring;

        let messages = chunk_message(
            transfer_id,
            workspace_id,
            payload,
            self.config.clamped_chunk_size(),
        )
        .map_err(SendFailure::from)?;

        // Stamped on every frame rather than the first: a receiver that is handed
        // an out-of-order frame still knows who sent it, and the id is not worth a
        // round trip of its own.
        let messages: Vec<WireMessage> = messages
            .into_iter()
            .map(|mut message| {
                message.header.sender_device_id = self.local_device_id.clone();
                message
            })
            .collect();
        let total = messages.len().max(1) as u64;
        let mut sent_frames: u64 = 0;
        let mut sent_bytes: u64 = 0;

        for message in &messages {
            if self.is_cancelled() {
                // Tell the peer rather than dropping the connection, so it does
                // not sit waiting for a frame that will never arrive.
                let _ = write_frame(
                    &mut write_half,
                    &mut noise,
                    &WireMessage::control(
                        MessageKind::Cancel,
                        transfer_id,
                        "The user cancelled the transfer",
                    ),
                )
                .await;
                return Err(SendFailure::Cancelled(
                    "The transfer was cancelled after part of the workspace had been sent".into(),
                ));
            }

            write_frame(&mut write_half, &mut noise, message)
                .await
                .map_err(SendFailure::from)?;

            // Wait for the acknowledgement. Reporting progress before the frame
            // is confirmed would let the bar reach 100% for bytes the receiver
            // never stored.
            //
            // The wait is raced against cancellation, because this is where a
            // send spends its time: a peer that has gone quiet mid-transfer
            // would otherwise hold the cancel until `FRAME_TIMEOUT` expired.
            // The connection is simply dropped on that path. The peer sees the
            // socket close and ends, which is the same outcome the in-band
            // cancel frame below achieves, minus a write to a peer that has
            // already stopped answering.
            let ack = tokio::select! {
                biased;
                _ = self.wait_for_cancel() => {
                    return Err(SendFailure::Cancelled(
                        "The transfer was cancelled after part of the workspace had been sent".into(),
                    ));
                }
                ack = read_frame(&mut read_half, &mut noise) => ack.map_err(SendFailure::from)?,
            };
            match ack.header.kind() {
                MessageKind::Ack => {}
                MessageKind::Reject => {
                    return Err(SendFailure::Error(
                        NetworkError::Transfer(format!(
                            "The other device refused the transfer: {}",
                            if ack.header.reason.is_empty() {
                                "no reason given"
                            } else {
                                &ack.header.reason
                            }
                        ))
                        .into(),
                    ))
                }
                other => {
                    return Err(SendFailure::Error(
                        NetworkError::Protocol(format!(
                            "Expected an acknowledgement, got {other:?}"
                        ))
                        .into(),
                    ))
                }
            }

            sent_frames += 1;
            sent_bytes += message.body.len() as u64;
            session.bytes_transferred = sent_bytes;
            session.progress = (sent_frames as f32 / total as f32).clamp(0.0, 1.0);
            if let Some(callback) = on_progress {
                callback(session.progress);
            }
        }

        session.status = TransferStatus::Verifying;
        let digest = hex_digest(payload);
        session.total_bytes = payload.len() as u64;

        write_frame(
            &mut write_half,
            &mut noise,
            &WireMessage::new(
                MessageHeader {
                    kind: MessageKind::Done.into(),
                    transfer_id: transfer_id.to_string(),
                    workspace_id: workspace_id.to_string(),
                    chunk_count: messages.len() as u32,
                    total_bytes: payload.len() as u64,
                    digest: digest.clone(),
                    ..Default::default()
                },
                Vec::new(),
            ),
        )
        .await
        .map_err(SendFailure::from)?;

        let confirmation = tokio::select! {
            biased;
            _ = self.wait_for_cancel() => {
                return Err(SendFailure::Cancelled(
                    "The transfer was cancelled before the other device confirmed it".into(),
                ));
            }
            confirmation = read_frame(&mut read_half, &mut noise) => {
                confirmation.map_err(SendFailure::from)?
            }
        };

        match confirmation.header.kind() {
            MessageKind::Ack => {}
            MessageKind::Reject => {
                return Err(SendFailure::Error(
                    NetworkError::Transfer(format!(
                        "The other device rejected the workspace: {}",
                        if confirmation.header.reason.is_empty() {
                            "no reason given"
                        } else {
                            &confirmation.header.reason
                        }
                    ))
                    .into(),
                ))
            }
            other => {
                return Err(SendFailure::Error(
                    NetworkError::Protocol(format!(
                        "Expected a final acknowledgement, got {other:?}"
                    ))
                    .into(),
                ))
            }
        }

        // The receiver hashed what it stored. Compare, rather than trusting that
        // reaching this point means the bytes are intact.
        let received = confirmation.header.received_digest.clone();
        if received.is_empty() {
            return Err(SendFailure::Error(
                NetworkError::Transfer(
                    "The other device did not report a digest, so the transfer cannot be verified"
                        .into(),
                )
                .into(),
            ));
        }
        if received != digest {
            return Err(SendFailure::Error(
                NetworkError::Transfer(format!(
                    "Digest mismatch: sent {digest}, the other device stored {received}"
                ))
                .into(),
            ));
        }

        Ok(())
    }
}

/// Why a send did not finish.
enum SendFailure {
    /// The user asked to stop. Not an error.
    Cancelled(String),
    Error(workspace_clone_core::WorkspaceError),
}

impl From<workspace_clone_core::WorkspaceError> for SendFailure {
    fn from(e: workspace_clone_core::WorkspaceError) -> Self {
        Self::Error(e)
    }
}

impl From<std::io::Error> for SendFailure {
    fn from(e: std::io::Error) -> Self {
        Self::Error(NetworkError::Connection(e.to_string()).into())
    }
}

/// Read the peer's advertised Noise key.
///
/// A device with no advertised key cannot be connected to. Falling back to an
/// unauthenticated session here would let anything on the network impersonate a
/// paired device, so this is a hard error.
fn public_key_of(device: &DiscoveredDevice) -> Result<PublicKey> {
    if device.static_public_key.is_empty() {
        return Err(NetworkError::Authentication(format!(
            "{} did not advertise a key, so it cannot be authenticated. Pair it again, or use a build that publishes one.",
            device.name
        ))
        .into());
    }

    PublicKey::from_base64(&device.static_public_key).map_err(|e| {
        NetworkError::Authentication(format!(
            "{} advertised a key this build cannot read: {e}",
            device.name
        ))
        .into()
    })
}

/// Why a set of connect attempts produced no connection.
///
/// Split into these two shapes so each caller can say what happened in its own
/// words: an address that refuses a connection immediately is a different
/// diagnosis from an address that never answers at all.
#[derive(Debug)]
pub(crate) enum ConnectFailure {
    /// The single candidate that answered said no, named with its error.
    ///
    /// `select_ok` returns the first error as soon as every candidate has
    /// failed, so there is at most one of these.
    Refused { address: String, error: String },
    /// The whole budget ran out before any candidate answered.
    TimedOut { first: String, budget: Duration },
}

/// Try several candidate addresses, racing them against one budget.
///
/// The candidates are plural because a single advertised address may be
/// unusable: a stale entry errors immediately while the address that actually
/// reaches the peer is another row in the same list. Racing them means the
/// budget covers all of them once, the first candidate that connects wins, and
/// a peer that answers over any of its addresses is reached. The caller races
/// this future against cancellation, so a user cancel drops the pending
/// connects with everything else.
pub(crate) async fn connect_any(
    candidates: &[SocketAddr],
    budget: Duration,
) -> std::result::Result<TcpStream, ConnectFailure> {
    let Some(first) = candidates.first() else {
        return Err(ConnectFailure::Refused {
            address: "(no address)".to_string(),
            error: "the device advertised nothing usable to connect to".to_string(),
        });
    };
    let first = first.to_string();

    let attempts = futures::future::select_ok(candidates.iter().map(|&candidate| {
        let candidate = candidate;
        Box::pin(async move {
            match TcpStream::connect(candidate).await {
                Ok(stream) => Ok(stream),
                Err(e) => Err((candidate.to_string(), e)),
            }
        })
    }));

    match tokio::time::timeout(budget, attempts).await {
        Ok(Ok((stream, _rest))) => Ok(stream),
        Ok(Err((address, error))) => Err(ConnectFailure::Refused {
            address,
            error: error.to_string(),
        }),
        Err(_) => Err(ConnectFailure::TimedOut { first, budget }),
    }
}

/// Turn every advertised address into something a TCP connect can use.
///
/// An mDNS record may advertise a link-local IPv6 address such as
/// `fe80::f727:abc9:2280:4f3:54108`, and a connect to that address as written
/// fails on macOS with "No route to host" (ENETUNREACH): the address names the
/// *link*, and the kernel refuses to guess which of your interfaces the peer
/// is on. A plain IPv4 address has no such problem, which is why IPv4
/// candidates come first and why a device discovered by mDNS can almost always
/// be reached over one: both ends proved they share a link when the discovery
/// answer arrived.
///
/// For link-local IPv6 the address is therefore expanded into one candidate
/// per local interface carrying a link-local address, each with that
/// interface's scope id, so `fe80::x` means "x on my interface" instead of "x
/// everywhere and nowhere". A `%zone` suffix the user typed by hand is trusted
/// as given (`fe80::1%en0`) and kept as a single candidate, and every other
/// address passes through unchanged. Duplicates are dropped, so a record that
/// lists the same address twice does not get connected to twice.
pub(crate) fn candidate_socket_addrs(
    advertised: &[String],
    port: u16,
    interfaces: &[Interface],
) -> Vec<SocketAddr> {
    let mut v4: Vec<SocketAddr> = Vec::new();
    let mut v6: Vec<SocketAddr> = Vec::new();
    let mut v6_scoped: Vec<SocketAddr> = Vec::new();
    let mut seen: HashSet<SocketAddr> = HashSet::new();

    for raw in advertised {
        let (ip, zone) = match parse_scoped_ip(raw, interfaces) {
            Some(parsed) => parsed,
            None => continue,
        };

        match ip {
            IpAddr::V4(ip) => {
                let socket = SocketAddr::new(ip.into(), port);
                if seen.insert(socket) {
                    v4.push(socket);
                }
            }
            IpAddr::V6(ip) if ip.is_unicast_link_local() && zone.is_none() => {
                // Scope the peer to each local interface that can host a
                // link-local address. The pair was discovered on one of these
                // links, and a scoped connect either reaches it or fails fast
                // with a real route error -- never with the confusing
                // unscoped "no route to host".
                let mut scoped: Vec<SocketAddr> = interfaces
                    .iter()
                    .filter_map(|iface| match iface.ip() {
                        IpAddr::V6(local) if local.is_unicast_link_local() => {
                            iface.index.map(|index| {
                                SocketAddr::V6(SocketAddrV6::new(ip, port, 0, index))
                            })
                        }
                        _ => None,
                    })
                    .collect();
                if scoped.is_empty() {
                    // No local interface can scope the address. Keep it as-is
                    // so the attempt still reports a real error instead of
                    // pretending the peer cannot be reached.
                    scoped.push(SocketAddr::V6(SocketAddrV6::new(ip, port, 0, 0)));
                }
                for socket in scoped {
                    if seen.insert(socket) {
                        v6_scoped.push(socket);
                    }
                }
            }
            IpAddr::V6(ip) => {
                let socket = SocketAddr::V6(SocketAddrV6::new(ip, port, 0, zone.unwrap_or(0)));
                if seen.insert(socket) {
                    v6.push(socket);
                }
            }
        }
    }

    v4.extend(v6);
    v4.extend(v6_scoped);
    v4
}

/// Parse an address string that may carry the RFC 6874 `%zone` suffix used for
/// IPv6 link-local addresses (`fe80::1%en0`).
///
/// `std`'s IP parser rejects the suffix, so it is split off by hand. The zone
/// may be a number or an interface name; a name is resolved against the local
/// interface list. When the zone cannot be resolved the address is returned
/// without one and the caller applies its own scoping rules.
fn parse_scoped_ip(raw: &str, interfaces: &[Interface]) -> Option<(IpAddr, Option<u32>)> {
    if let Some((address, zone)) = raw.rsplit_once('%') {
        let ip: IpAddr = address.parse().ok()?;
        let scope = zone
            .parse()
            .ok()
            .or_else(|| interfaces.iter().find(|i| i.name == zone).and_then(|i| i.index));
        return Some((ip, scope));
    }
    raw.parse().ok().map(|ip| (ip, None))
}

/// Bind the transfer listener, preferring a dual-stack socket.
///
/// `[::]` with `IPV6_V6ONLY` off accepts IPv4 and IPv6 on one socket, which is
/// what lets a peer discovered by its link-local IPv6 address connect over
/// IPv6 at all. The fallback keeps the IPv4-only bind the listener has always
/// used, so a platform that cannot offer dual-stack behaves exactly as before.
fn bind_listener(port: u16) -> Result<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))
        .ok()
        .and_then(|socket| {
            socket.set_only_v6(false).ok()?;
            socket
                .bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())
                .ok()?;
            socket.listen(1024).ok()?;
            socket.set_nonblocking(true).ok()?;
            Some(socket)
        });
    if let Some(socket) = socket {
        let listener =
            TcpListener::from_std(socket.into()).map_err(|e| NetworkError::Connection(e.to_string()))?;
        return Ok(listener);
    }

    let listener = std::net::TcpListener::bind(("0.0.0.0", port))
        .and_then(|listener| {
            listener.set_nonblocking(true)?;
            Ok(listener)
        })
        .map_err(|e| NetworkError::Connection(format!("Could not bind port {port}: {e}")))?;
    TcpListener::from_std(listener).map_err(|e| NetworkError::Connection(e.to_string()).into())
}

/// Read one transport frame.
async fn read_frame<R>(reader: &mut R, session: &mut NoiseSession) -> Result<WireMessage>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let ciphertext = read_frame_raw(reader).await?;
    let plaintext = session
        .decrypt(&ciphertext)
        .map_err(|e| NetworkError::Transfer(format!("Could not decrypt a frame: {e}")))?;
    let (message, _) = WireMessage::decode(&plaintext)?;
    Ok(message)
}

/// Write one transport frame.
async fn write_frame<W>(writer: &mut W, session: &mut NoiseSession, message: &WireMessage) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let frame = message.encode()?;
    let ciphertext = session
        .encrypt(&frame)
        .map_err(|e| NetworkError::Transfer(format!("Could not encrypt a frame: {e}")))?;
    write_frame_raw(writer, &ciphertext).await
}

/// Read one length-prefixed message.
async fn read_frame_raw<R>(reader: &mut R) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    read_frame_raw_within(reader, FRAME_TIMEOUT, "The other device stopped responding").await
}

/// [`read_frame_raw`], with the deadline and the message it produces on expiry
/// supplied by the caller.
///
/// The handshake and the transfer need different budgets, and saying which one
/// ran out is the difference between "this device is not answering" and "the
/// transfer stalled".
async fn read_frame_raw_within<R>(
    reader: &mut R,
    within: Duration,
    on_timeout: &'static str,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut parser = FrameParser::new();
    let mut buffer = vec![0u8; 16 * 1024];

    loop {
        let read = tokio::time::timeout(within, reader.read(&mut buffer))
            .await
            .map_err(|_| NetworkError::Connection(on_timeout.to_string()))?
            .map_err(|e| NetworkError::Connection(format!("Read failed: {e}")))?;

        if read == 0 {
            return Err(NetworkError::Connection(
                "The other device closed the connection".into(),
            )
            .into());
        }

        if let Some(message) = parser.feed(&buffer[..read])?.into_iter().next() {
            return Ok(message);
        }
    }
}

/// Write one length-prefixed message and flush it.
async fn write_frame_raw<W>(writer: &mut W, message: &[u8]) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    if message.len() > MAX_TRANSPORT_CIPHERTEXT {
        return Err(NetworkError::Transfer(format!(
            "Refusing to frame a {} byte message, over the {MAX_TRANSPORT_CIPHERTEXT} byte limit",
            message.len()
        ))
        .into());
    }

    writer
        .write_all(&frame_message(message))
        .await
        .map_err(|e| NetworkError::Connection(format!("Write failed: {e}")))?;
    writer
        .flush()
        .await
        .map_err(|e| NetworkError::Connection(format!("Flush failed: {e}")))?;
    Ok(())
}

/// Lowercase hex SHA-256.
pub fn hex_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Tuning for a transfer.
#[derive(Debug, Clone)]
pub struct TransferConfig {
    pub chunk_size: usize,
    pub timeout_secs: u64,
    pub max_retries: u32,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            // Comfortably under the transport limit, so a chunk plus its framing
            // always fits. The default was previously 64 KiB, which is *over*
            // the limit: a chunk plus a header is 65536 bytes of plaintext, and
            // the transport ceiling is 65519. Every transfer of more than one
            // chunk therefore failed on the second write with an opaque cipher
            // error.
            chunk_size: 60 * 1024,
            timeout_secs: 30,
            max_retries: 3,
        }
    }
}

impl TransferConfig {
    /// A chunk size that can never produce an unframable message.
    ///
    /// A chunk becomes a frame, and a frame is encrypted whole, so the budget is
    /// the transport's plaintext ceiling less the length prefix and less a
    /// header with room to spare. The header allowance is generous on purpose: it
    /// carries the transfer id, the workspace id and two digests, and a digest
    /// alone is 64 characters.
    pub fn clamped_chunk_size(&self) -> usize {
        const LENGTH_PREFIX: usize = 4;
        // Digests on both sides, a reason string, and generous slack for
        // escaping. Four times the hard header ceiling is still far below the
        // transport limit.
        const HEADER_ALLOWANCE: usize = 4096;

        let ceiling = MAX_TRANSPORT_PLAINTEXT
            .saturating_sub(LENGTH_PREFIX)
            .saturating_sub(HEADER_ALLOWANCE);

        self.chunk_size.clamp(1024, ceiling.max(1024))
    }

    /// Whether a payload can be sent with this configuration.
    ///
    /// Always true once the size is clamped, which is the point: the clamp is
    /// what makes it true.
    pub fn can_send(&self, payload_len: usize) -> bool {
        payload_len == 0 || self.clamped_chunk_size() > 0
    }
}

/// The record of one transfer, as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferSession {
    pub id: String,
    pub workspace_id: String,
    pub source_device_id: String,
    pub destination_device_id: String,
    pub status: TransferStatus,
    pub progress: f32,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
}

impl TransferSession {
    pub fn new(id: String, workspace_id: &str) -> Self {
        Self {
            id,
            workspace_id: workspace_id.to_string(),
            source_device_id: String::new(),
            destination_device_id: String::new(),
            status: TransferStatus::Connecting,
            progress: 0.0,
            bytes_transferred: 0,
            total_bytes: 0,
            started_at: chrono::Utc::now(),
            completed_at: None,
            error: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
        )
    }
}

/// Where a transfer has got to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferStatus {
    Connecting,
    Handshaking,
    Transferring,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

/// A workspace that arrived.
#[derive(Debug, Clone)]
pub struct ReceivedTransfer {
    pub transfer_id: String,
    pub workspace_id: String,
    /// The bytes, as sent.
    pub payload: Vec<u8>,
    /// The peer's Noise static key, base64. This is what the safety number is
    /// derived from, and it is the only value a user can meaningfully compare.
    pub peer_static: String,
    /// The device id the sender claimed, for display.
    ///
    /// Not an authenticated identity -- `peer_static` is. This is the name a
    /// receiving screen shows so a person recognises which machine sent the
    /// workspace; a sender that lied here would produce a misleading label, and
    /// the handshake would still have to complete against `peer_static`.
    pub sender_device_id: String,
    pub digest: String,
    pub duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> TransferService {
        TransferService::new(
            KeyPair::generate().unwrap(),
            "local-device",
            TransferConfig::default(),
        )
    }

    fn device(port: u16, address: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            device_id: "remote".into(),
            name: "Remote".into(),
            os: "linux".into(),
            app_version: "0.1.0".into(),
            protocol_version: 1,
            addresses: vec![address.to_string()],
            port,
            capabilities: DeviceCapabilities::default(),
            static_public_key: String::new(),
            last_seen: chrono::Utc::now().into(),
        }
    }

    use if_addrs::{IfAddr, Ifv4Addr, Ifv6Addr};
    use std::net::Ipv4Addr;

    fn fake_ifaddr(ip: IpAddr) -> IfAddr {
        match ip {
            IpAddr::V4(ip) => IfAddr::V4(Ifv4Addr {
                ip,
                netmask: Ipv4Addr::UNSPECIFIED,
                prefixlen: 0,
                broadcast: None,
            }),
            IpAddr::V6(ip) => IfAddr::V6(Ifv6Addr {
                ip,
                netmask: Ipv6Addr::UNSPECIFIED,
                prefixlen: 0,
                broadcast: None,
            }),
        }
    }

    /// A stand-in local interface, so tests do not depend on the machine's
    /// real network layout.
    #[cfg(windows)]
    fn fake_interface(name: &str, ip: IpAddr, index: u32) -> Interface {
        Interface {
            name: name.to_string(),
            addr: fake_ifaddr(ip),
            index: Some(index),
            adapter_name: String::new(),
        }
    }

    #[cfg(not(windows))]
    fn fake_interface(name: &str, ip: IpAddr, index: u32) -> Interface {
        Interface {
            name: name.to_string(),
            addr: fake_ifaddr(ip),
            index: Some(index),
        }
    }

    #[test]
    fn advertised_ipv4_addresses_come_first_however_the_record_is_ordered() {
        // A record that leads with a link-local IPv6 address is exactly why the
        // connect failed with "No route to host": IPv4 must win the ordering
        // regardless of how mDNS happened to list the peer.
        let advertised = vec![
            "fe80::f727:abc9:2280:4f3:5410:8".to_string(),
            "192.168.1.20".to_string(),
            "fe80::4f3:5410:8".to_string(),
            "192.168.1.21".to_string(),
        ];
        let candidates = candidate_socket_addrs(
            &advertised,
            9000,
            &[fake_interface("en0", "fe80::1".parse().unwrap(), 5)],
        );

        assert_eq!(candidates[0], "192.168.1.20:9000".parse().unwrap());
        assert_eq!(candidates[1], "192.168.1.21:9000".parse().unwrap());
        // The link-local addresses are scoped to the local link-local
        // interface instead of being sent out unscoped.
        assert_eq!(
            candidates[2],
            SocketAddr::V6(SocketAddrV6::new(
                "fe80::f727:abc9:2280:4f3:5410:8".parse().unwrap(),
                9000,
                0,
                5,
            ))
        );
        assert_eq!(
            candidates[3],
            SocketAddr::V6(SocketAddrV6::new("fe80::4f3:5410:8".parse().unwrap(), 9000, 0, 5))
        );
    }

    #[test]
    fn a_link_local_address_is_scoped_to_every_local_link_interface() {
        let links = [
            fake_interface("en0", "fe80::1".parse().unwrap(), 5),
            fake_interface("en1", "fe80::2".parse().unwrap(), 7),
            fake_interface("utun0", "fe80::3".parse().unwrap(), 9),
        ];
        let candidates = candidate_socket_addrs(&["fe80::dead".to_string()], 9000, &links);

        assert_eq!(candidates.len(), 3);
        let scopes: HashSet<u32> = candidates
            .iter()
            .map(|address| match address {
                SocketAddr::V6(v6) => v6.scope_id(),
                SocketAddr::V4(_) => panic!("a link-local address must never become IPv4"),
            })
            .collect();
        assert_eq!(scopes, HashSet::from([5, 7, 9]));
    }

    #[test]
    fn a_link_local_address_with_no_local_link_network_stays_bare() {
        // No local interface carries a link-local address (only IPv4 links are
        // faked), so there is nothing to scope to and the address survives
        // rather than vanishing silently.
        let links = [fake_interface("en0", "192.168.1.5".parse().unwrap(), 5)];
        let candidates = candidate_socket_addrs(&["fe80::dead".to_string()], 9000, &links);
        assert_eq!(
            candidates,
            vec![SocketAddr::V6(SocketAddrV6::new(
                "fe80::dead".parse().unwrap(),
                9000,
                0,
                0,
            ))]
        );
    }

    #[test]
    fn a_hand_typed_zone_is_trusted_and_not_expanded() {
        let links = [fake_interface("en0", "fe80::1".parse().unwrap(), 5)];
        let candidates = candidate_socket_addrs(&["fe80::dead%en0".to_string()], 9000, &links);
        // Exactly one candidate: the user's zone, not one per interface.
        assert_eq!(
            candidates,
            vec![SocketAddr::V6(SocketAddrV6::new(
                "fe80::dead".parse().unwrap(),
                9000,
                0,
                5,
            ))]
        );
    }

    #[test]
    fn a_numeric_zone_is_kept_as_written() {
        let candidates = candidate_socket_addrs(&["fe80::dead%12".to_string()], 9000, &[]);
        assert_eq!(
            candidates,
            vec![SocketAddr::V6(SocketAddrV6::new(
                "fe80::dead".parse().unwrap(),
                9000,
                0,
                12,
            ))]
        );
    }

    #[test]
    fn a_global_ipv6_address_passes_through_without_scoping() {
        let candidates = candidate_socket_addrs(&["2001:db8::1".to_string()], 9000, &[]);
        assert_eq!(
            candidates,
            vec![SocketAddr::V6(SocketAddrV6::new(
                "2001:db8::1".parse().unwrap(),
                9000,
                0,
                0,
            ))]
        );
    }

    #[test]
    fn duplicate_advertised_addresses_are_connected_to_once() {
        let advertised = vec![
            "192.168.1.20".to_string(),
            "192.168.1.20".to_string(),
            "fe80::dead".to_string(),
            "fe80::dead".to_string(),
        ];
        let links = [fake_interface("en0", "fe80::1".parse().unwrap(), 5)];
        let candidates = candidate_socket_addrs(&advertised, 9000, &links);
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn an_unreadable_advertised_address_is_skipped() {
        let candidates = candidate_socket_addrs(&["not-an-address".to_string()], 9000, &[]);
        assert!(candidates.is_empty());
    }

    /// A dead candidate does not stand in the way of a live one: the connect
    /// budget races everything, so the address behind the one that answers
    /// wins.
    #[tokio::test]
    async fn connect_any_reaches_a_later_candidate_when_the_first_one_is_dead() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let dead = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let dead_port = dead.local_addr().unwrap().port();
        drop(dead);

        let accept = tokio::spawn(async move { listener.accept().await.unwrap() });

        let candidates = vec![
            SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), dead_port)),
            SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), port)),
        ];
        let stream = connect_any(&candidates, Duration::from_secs(10))
            .await
            .expect("the live candidate must be reached");
        drop(stream);

        let (_, peer) = tokio::time::timeout(Duration::from_secs(2), accept)
            .await
            .expect("the listener accepts within the deadline")
            .unwrap();
        assert!(
            peer.ip().is_loopback(),
            "the connection that landed must be a loopback client, got {peer}"
        );
    }

    /// A refusal that happens to be the only outcome still names the address,
    /// so the error is a diagnosis instead of a shrug.
    #[tokio::test]
    async fn connect_any_names_the_address_when_every_candidate_refuses() {
        let dead = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = dead.local_addr().unwrap().port();
        drop(dead);

        let candidates = vec![SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), port))];
        match connect_any(&candidates, Duration::from_secs(10)).await {
            Err(ConnectFailure::Refused { address, .. }) => {
                assert!(address.contains("127.0.0.1"), "got: {address}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The listener regains its old reachability whatever it binds with: if
    /// the platform allows a dual-stack socket, the peer arrives through it as
    /// a v4-mapped IPv6 address; if not, the IPv4 fallback still serves.
    #[tokio::test]
    async fn the_listener_serves_ipv4_through_either_family_of_bind() {
        let listener = bind_listener(0).unwrap();
        let addr = listener.local_addr().unwrap();

        let accept = tokio::spawn(async move { listener.accept().await.unwrap() });
        let port = addr.port();
        let conn = tokio::net::TcpStream::connect(("127.0.0.1", port)).await;
        assert!(conn.is_ok(), "IPv4 loopback must always reach the listener: {conn:?}");
        drop(conn);

        let (_, peer) = tokio::time::timeout(Duration::from_secs(2), accept)
            .await
            .expect("an IPv4 connection must be served")
            .unwrap();
        if addr.is_ipv6() {
            assert!(peer.ip().is_ipv6(), "dual-stack surfaces IPv4 peers as v4-mapped, got {peer}");
        } else {
            assert!(peer.ip().is_ipv4(), "fallback bind stays IPv4-only, got {peer}");
        }
    }

    /// The key a service will answer a handshake with.
    ///
    /// Discovery publishes exactly this, which is what lets a sender name a peer
    /// it has never spoken to before.
    fn advertised_key(service: &TransferService) -> String {
        PublicKey::from_bytes(service.local_static.public_bytes())
            .unwrap()
            .to_base64()
    }

    /// Run a receiver to completion and report whether a workspace arrived.
    ///
    /// `accept_once` answers `Ok(None)` when a connection produced no workspace,
    /// which is the ordinary outcome for a send that failed or was cancelled --
    /// so the task's own error is unwrapped here rather than at every call site.
    async fn accept(
        spawned: tokio::task::JoinHandle<Result<Option<ReceivedTransfer>>>,
    ) -> Option<ReceivedTransfer> {
        spawned
            .await
            .expect("the receiver task must not panic")
            .expect("accepting a connection must not fail")
    }

    /// Start a receiver on a loopback port and return a device that points at it.
    async fn receiver_on_random_port(
        device_id: &str,
    ) -> (TransferService, u16, DiscoveredDevice) {
        let mut receiver =
            TransferService::new(KeyPair::generate().unwrap(), device_id, Default::default());
        let port = receiver.start(0).await.unwrap();
        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&receiver);
        (receiver, port, destination)
    }

    /// The receiving device's progress bar is fed by these callbacks, so the
    /// contract matters: monotonically non-decreasing, never above 1.0, and
    /// ending at exactly 1.0 once the workspace is in hand.
    #[tokio::test]
    async fn the_receiver_reports_progress_that_only_ever_moves_forward() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;

        let seen = Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
        let sink = Arc::clone(&seen);
        let handle = tokio::spawn(async move {
            receiver
                .accept_once_with_progress(Some(Arc::new(move |value: f32| {
                    sink.lock().unwrap().push(value);
                })))
                .await
        });

        // Larger than one chunk, so there is more than the final 1.0 to report.
        let payload = vec![7u8; 300_000];
        let expected = payload.len();
        let session = service()
            .send_workspace(&destination, "ws-progress", &payload, None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);
        assert_eq!(received.payload.len(), expected);

        let reported = seen.lock().unwrap().clone();
        assert!(!reported.is_empty(), "progress was never reported");
        assert!(
            reported.windows(2).all(|w| w[0] <= w[1]),
            "progress went backwards: {reported:?}"
        );
        assert!(
            reported.iter().all(|v| (0.0..=1.0).contains(v)),
            "progress left the 0..1 range: {reported:?}"
        );
        assert_eq!(
            reported.last().copied(),
            Some(1.0),
            "a completed transfer must end at 1.0, got {reported:?}"
        );
    }

    /// A zero-byte workspace is legal -- an empty selection still produces a
    /// manifest. If "no size announced" were confused with "zero bytes", the bar
    /// would sit at 0% and then jump, which reads as a hang.
    #[tokio::test]
    async fn an_empty_workspace_still_reports_completion() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;

        let seen = Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
        let sink = Arc::clone(&seen);
        let handle = tokio::spawn(async move {
            receiver
                .accept_once_with_progress(Some(Arc::new(move |value: f32| {
                    sink.lock().unwrap().push(value);
                })))
                .await
        });

        service()
            .send_workspace(&destination, "ws-empty", b"", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("an empty workspace must arrive");

        assert!(received.payload.is_empty());
        assert_eq!(
            seen.lock().unwrap().last().copied(),
            Some(1.0),
            "an empty payload is complete, not stalled"
        );
    }

    /// A transfer with no progress callback must behave identically, and must not
    /// be refused because the callback is absent.
    #[tokio::test]
    async fn a_receiver_with_no_progress_callback_still_works() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let payload = vec![3u8; 200_000];
        service()
            .send_workspace(&destination, "ws-quiet", &payload, None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(received.payload, payload);
    }

    /// The device id on the wire is a label, and the Noise key is the identity.
    /// Keeping them separate is what lets a receiving screen name a device
    /// without the name being load-bearing.
    #[tokio::test]
    async fn the_receiver_learns_which_device_sent_a_workspace() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let sender = service();
        let sender_id = sender.local_device_id.clone();
        sender
            .send_workspace(&destination, "ws-19", b"payload", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(
            received.sender_device_id, sender_id,
            "the claimed id is what the sender said"
        );
        // The identity is still the key the handshake authenticated, and the two
        // are not interchangeable.
        assert_eq!(received.peer_static, advertised_key(&sender));
        assert_ne!(
            received.sender_device_id, received.peer_static,
            "a display label and an authenticated key must not be the same value"
        );
    }

    /// A sender that omits the label must not fail the transfer. The label is
    /// decoration; refusing a payload over a missing caption would be absurd, and
    /// an older build legitimately sends no label at all.
    #[tokio::test]
    async fn a_sender_with_no_device_id_still_transfers() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let handle = tokio::spawn(async move { receiver.accept_once().await });

        // Strip the label the way an older build would have sent it.
        let mut sender = service();
        sender.local_device_id = String::new();

        let session = sender
            .send_workspace(&destination, "ws-20", b"payload", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);
        assert!(received.sender_device_id.is_empty());
        assert_eq!(received.payload, b"payload");
    }

    #[tokio::test]
    async fn a_workspace_arrives_intact() {
        let (receiver, _port, destination) = receiver_on_random_port("remote-device").await;
        let payload = b"{\"schema_version\":1,\"workspace\":{\"name\":\"demo\"}}".to_vec();
        let sender = service();

        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let progress = Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
        let recorder = progress.clone();
        let session = sender
            .send_workspace(
                &destination,
                "ws-1",
                &payload,
                Some(Arc::new(move |p| {
                    recorder.lock().unwrap().push(p);
                })),
            )
            .await
            .unwrap();

        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(
            session.status,
            TransferStatus::Completed,
            "error: {:?}",
            session.error
        );
        assert_eq!(session.workspace_id, "ws-1");
        assert_eq!(session.total_bytes, payload.len() as u64);
        assert_eq!(session.bytes_transferred, payload.len() as u64);
        assert_eq!(session.progress, 1.0);

        assert_eq!(received.payload, payload);
        assert_eq!(received.workspace_id, "ws-1");
        assert_eq!(received.digest, hex_digest(&payload));
        // The peer key is what the safety number comes from, so it must be
        // reported rather than discarded.
        assert!(!received.peer_static.is_empty());

        // Progress must be monotonic and finish at 1.0.
        let seen = progress.lock().unwrap().clone();
        assert_eq!(*seen.last().unwrap(), 1.0);
        assert!(seen.windows(2).all(|w| w[0] <= w[1]), "{seen:?}");
    }

    #[tokio::test]
    async fn a_large_payload_is_split_and_reassembled() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;

        // Three chunks' worth at the default 64 KiB, so the chunking path is
        // genuinely exercised rather than a single frame.
        let payload: Vec<u8> = (0..(64 * 1024 * 3 + 17))
            .map(|i| (i % 253) as u8)
            .collect();

        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let session = service()
            .send_workspace(&destination, "ws-2", &payload, None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(
            session.status,
            TransferStatus::Completed,
            "error: {:?}",
            session.error
        );
        assert_eq!(received.payload.len(), payload.len());
        assert_eq!(received.payload, payload);
        assert_eq!(session.total_bytes, payload.len() as u64);
        assert_eq!(session.bytes_transferred, payload.len() as u64);
    }

    #[tokio::test]
    async fn an_empty_payload_still_verifies() {
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let session = service()
            .send_workspace(&destination, "ws-empty", b"", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);
        assert!(received.payload.is_empty());
        assert_eq!(received.digest, hex_digest(b""));
    }

    #[tokio::test]
    async fn a_cancelled_transfer_is_reported_as_cancelled_not_failed() {
        // Nothing is started on the receiving side: a transfer cancelled before
        // it begins must not open a connection at all, so there is nothing to
        // wait for.
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let payload = vec![7u8; 64 * 1024 * 4];
        let sender = service();
        sender.cancel();

        let session = sender
            .send_workspace(&destination, "ws-3", &payload, None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Cancelled);
        assert!(session.is_finished());
        let error = session.error.unwrap();
        assert!(error.contains("cancelled"), "got: {error}");
        assert_eq!(session.progress, 0.0, "nothing was sent, so nothing is done");
        assert_eq!(session.bytes_transferred, 0);
        drop(receiver);
    }

    #[tokio::test]
    async fn a_cancellation_mid_transfer_tells_the_receiver() {
        // The helper's own port is unused: this test binds its own listener so it
        // can drive `receive_on` directly and inspect the receiver's error, which
        // `accept_once` deliberately collapses to `None`.
        let (receiver, _port, _destination) = receiver_on_random_port("remote").await;
        let payload = vec![7u8; 64 * 1024 * 8];
        let sender = service();

        // `receive_on` is driven directly here so the receiver's own error can
        // be inspected. `accept_once` reports a failed connection as `None`,
        // which is right for the app but would hide the reason.
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&receiver);

        let handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            receiver.receive_on(stream).await
        });

        // The cancel flag is shared, so setting it from the progress callback is
        // the same as calling cancel(). The callback fires after the first
        // acknowledgement, which is where a user pressing stop would land.
        let flag = sender.cancellation_handle();
        let session = sender
            .send_workspace(
                &destination,
                "ws-4",
                &payload,
                Some(Arc::new(move |_| {
                    flag.store(true, Ordering::SeqCst);
                })),
            )
            .await
            .unwrap();

        assert_eq!(
            session.status,
            TransferStatus::Cancelled,
            "error: {:?}",
            session.error
        );
        // The receiver must be told, not left waiting for the rest of a payload
        // that is never coming.
        let error = handle
            .await
            .expect("the receiver task must not panic")
            .expect_err("a cancelled transfer must not be stored")
            .to_string();
        assert!(error.contains("cancelled"), "got: {error}");
    }

    #[tokio::test]
    async fn resetting_the_cancel_flag_allows_another_transfer() {
        // A cancel that permanently poisoned the service would make the second
        // attempt in a session fail for no reason the user can see.
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let sender = service();
        sender.cancel();
        assert!(sender.is_cancelled());

        let first = sender
            .send_workspace(&destination, "ws-a", b"first", None)
            .await
            .unwrap();
        assert_eq!(first.status, TransferStatus::Cancelled);

        sender.reset_cancel();
        assert!(!sender.is_cancelled());

        let handle = tokio::spawn(async move { receiver.accept_once().await });
        let second = sender
            .send_workspace(&destination, "ws-b", b"second", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(second.status, TransferStatus::Completed, "{:?}", second.error);
        assert_eq!(received.payload, b"second");
    }

    #[tokio::test]
    async fn a_device_with_no_advertised_key_cannot_be_contacted() {
        let mut destination = device(1, "127.0.0.1");
        destination.static_public_key = String::new();

        let session = service()
            .send_workspace(&destination, "ws-5", b"payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        assert!(
            session.error.unwrap().contains("did not advertise a key"),
            "falling back to an unauthenticated session is not acceptable"
        );
    }

    #[tokio::test]
    async fn a_device_with_an_unreadable_key_is_refused_before_connecting() {
        let mut destination = device(1, "127.0.0.1");
        destination.static_public_key = "not base64 at all!!".into();

        let session = service()
            .send_workspace(&destination, "ws-6", b"payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        let error = session.error.unwrap();
        assert!(error.contains("cannot read"), "got: {error}");
        // The message must name the peer, so a user knows which device to
        // re-pair rather than guessing.
        assert!(error.contains("Remote"), "got: {error}");
    }

    #[tokio::test]
    async fn a_device_with_no_address_is_reported() {
        let mut destination = device(DEFAULT_PORT, "127.0.0.1");
        destination.addresses.clear();
        destination.static_public_key = advertised_key(&service());

        let session = service()
            .send_workspace(&destination, "ws-6b", b"payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        assert!(session.error.unwrap().contains("no address"));
    }

    #[tokio::test]
    async fn connecting_to_a_dead_port_fails_with_an_address() {
        // Bind then drop, so the port is almost certainly closed.
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&service());

        let session = service()
            .send_workspace(&destination, "ws-7", b"payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        let error = session.error.unwrap();
        assert!(
            error.contains("Could not reach") || error.contains("timed out"),
            "got: {error}"
        );
        // The message must name the address, so the user can tell which device
        // could not be reached.
        assert!(error.contains("127.0.0.1"), "got: {error}");
    }

    #[tokio::test]
    async fn a_receiver_refuses_a_payload_that_does_not_match_its_digest() {
        // A responder that stores a modified payload, standing in for a
        // corrupted or tampered transfer.
        let mut fake = TransferService::new(KeyPair::generate().unwrap(), "fake", Default::default());
        fake.force_digest_mismatch.store(true, Ordering::SeqCst);
        let port = fake.start(0).await.unwrap();

        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&fake);

        let handle = tokio::spawn(async move { fake.accept_once().await });

        let session = service()
            .send_workspace(&destination, "ws-8", b"the real payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        let error = session.error.unwrap();
        // The receiver's own reason must reach the user, not just the fact that
        // something went wrong.
        assert!(error.contains("rejected"), "got: {error}");
        assert!(
            error.contains("did not match the digest"),
            "the reason must survive the trip: {error}"
        );
        // The receiver itself reports the mismatch as the reason.
        assert!(accept(handle).await.is_none());
    }

    #[test]
    fn digests_are_stable_and_distinct() {
        assert_eq!(hex_digest(b"abc"), hex_digest(b"abc"));
        assert_ne!(hex_digest(b"abc"), hex_digest(b"abd"));
        // 64 hex characters, the SHA-256 width.
        assert_eq!(hex_digest(b"").len(), 64);
    }

    #[test]
    fn a_chunk_size_that_would_overflow_a_frame_is_clamped() {
        for size in [0, 1, 64 * 1024, 1024 * 1024, usize::MAX] {
            let config = TransferConfig {
                chunk_size: size,
                ..Default::default()
            };
            let clamped = config.clamped_chunk_size();

            assert!(clamped > 0, "a zero chunk size would send nothing");
            assert!(
                clamped + 4096 < MAX_TRANSPORT_PLAINTEXT,
                "size {size} clamped to {clamped}, which still cannot be framed and encrypted"
            );
        }
    }

    #[test]
    fn the_default_chunk_size_actually_fits_the_transport() {
        // The defect this replaces: a 64 KiB default chunk meant every transfer
        // of more than one chunk failed on its second write.
        let config = TransferConfig::default();
        let chunk = config.clamped_chunk_size();

        // A real frame, built by the real encoder, must be encryptable.
        let frame = chunk_message("t-12345678", "ws-12345678", &vec![0u8; chunk], chunk)
            .unwrap()
            .remove(0)
            .encode()
            .unwrap();

        assert!(
            frame.len() <= MAX_TRANSPORT_PLAINTEXT,
            "a default chunk frames to {} bytes, over the {} byte limit",
            frame.len(),
            MAX_TRANSPORT_PLAINTEXT
        );
        // And the ciphertext it becomes must be framable.
        assert!(frame.len() + 16 <= MAX_TRANSPORT_CIPHERTEXT);
    }

    #[tokio::test]
    async fn a_chunk_size_over_the_transport_limit_still_transfers() {
        // A misconfigured chunk size must be clamped, not fail the transfer.
        let mut receiver =
            TransferService::new(KeyPair::generate().unwrap(), "remote", Default::default());
        let port = receiver.start(0).await.unwrap();
        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&receiver);

        let sender = TransferService::new(
            KeyPair::generate().unwrap(),
            "local",
            TransferConfig {
                chunk_size: 1024 * 1024,
                ..Default::default()
            },
        );
        let payload = vec![3u8; 300_000];

        let handle = tokio::spawn(async move { receiver.accept_once().await });
        let session = sender
            .send_workspace(&destination, "ws-big", &payload, None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);
        assert_eq!(received.payload, payload);
    }

    #[test]
    fn the_transport_limits_leave_room_for_a_tag() {
        // These two must stay in step with the Noise library, and the difference
        // must be exactly the Poly1305 tag.
        assert_eq!(MAX_TRANSPORT_CIPHERTEXT - MAX_TRANSPORT_PLAINTEXT, 16);
    }

    #[tokio::test]
    async fn the_service_binds_and_reports_its_port() {
        let mut service = service();
        assert!(!service.is_listening());
        assert!(service.local_port().is_none());

        let port = service.start(0).await.unwrap();

        assert!(service.is_listening());
        assert_eq!(service.local_port(), Some(port));
    }

    #[tokio::test]
    async fn accepting_before_listening_is_an_error() {
        // A caller must not be told a transfer failed when the real problem is
        // that the listener never started.
        let error = service().accept_once().await.unwrap_err().to_string();
        assert!(error.contains("not running"), "got: {error}");
    }

    #[tokio::test]
    async fn a_mismatched_key_is_refused_by_the_handshake() {
        // The receiver holds a different key from the one advertised, which is
        // what an impersonating device looks like from the sender's side.
        let mut receiver =
            TransferService::new(KeyPair::generate().unwrap(), "real", Default::default());
        let port = receiver.start(0).await.unwrap();

        // Advertise somebody else's key.
        let mut destination = device(port, "127.0.0.1");
        destination.static_public_key = advertised_key(&service());

        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let session = service()
            .send_workspace(&destination, "ws-10", b"payload", None)
            .await
            .unwrap();

        assert_eq!(session.status, TransferStatus::Failed);
        let error = session.error.unwrap();
        assert!(
            error.to_lowercase().contains("handshake")
                || error.to_lowercase().contains("decrypt")
                || error.to_lowercase().contains("closed the connection"),
            "got: {error}"
        );
        assert!(accept(handle).await.is_none());
    }

    #[tokio::test]
    async fn the_receiver_learns_the_senders_key_from_the_handshake() {
        // Noise_IK authenticates both sides. The receiver learns the initiator's
        // static key, which is what a user is shown as a safety number when the
        // other device initiates, so it must be the key that actually
        // authenticated the connection rather than anything advertised.
        let (receiver, _port, destination) = receiver_on_random_port("remote").await;
        let handle = tokio::spawn(async move { receiver.accept_once().await });

        let sender = service();
        let sender_key = advertised_key(&sender);
        let session = sender
            .send_workspace(&destination, "ws-11", b"payload", None)
            .await
            .unwrap();
        let received = accept(handle).await.expect("the workspace must arrive");

        assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);
        assert_eq!(
            received.peer_static, sender_key,
            "the peer must be identified by the key that authenticated the handshake"
        );
    }
}
