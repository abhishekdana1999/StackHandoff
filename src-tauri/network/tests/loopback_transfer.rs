//! Two devices, one machine: a real Noise handshake over a real TCP socket.
//!
//! Everything else in this crate's unit tests stubs something. The handshake is
//! driven by a helper that hands back a session, the chunking is checked against
//! a buffer rather than a socket, the listener is checked for *being* a listener
//! rather than for carrying a workspace. Every one of those is a reasonable way
//! to test a piece. None of them can catch a payload that never arrives because
//! two halves disagree about a frame length, and that is the class of bug most
//! likely to survive to a user's machine, because it only appears when both ends
//! run at once.
//!
//! So this file runs the real thing: two `TransferService`s, each with its own
//! generated X25519 static, one bound to an ephemeral port on the loopback
//! interface, connected over TCP, with a Noise_IK handshake between them and a
//! workspace transferred through it.
//!
//! Loopback rather than a mock socket, deliberately. A mock would have to model
//! the 65535-byte message ceiling, backpressure, and partial reads, and a mock
//! that models them correctly is a second implementation of the transport with
//! its own bugs. The loopback interface gives all of that for free and exactly.
//!
//! These are multi-threaded tests. `#[tokio::test]` defaults to a current-thread
//! runtime, where a spawned `accept` only progresses when the test future itself
//! awaits — which means a test whose main body waits on the *send* would deadlock
//! against a receive that cannot run. Real concurrency between two endpoints is
//! the thing under test, so the runtime has to actually be concurrent.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use workspace_clone_core::device::{DateTimeUtc, DiscoveredDevice};
use workspace_clone_core::Result;
use workspace_clone_crypto::noise::KeyPair;
use workspace_clone_network::transfer::{
    hex_digest, ReceivedTransfer, TransferConfig, TransferService, TransferSession, TransferStatus,
};

/// How long a test waits before declaring the transfer hung.
///
/// Generous, because a loaded machine can be slow and a timeout that fires
/// spuriously reports a failure that does not exist. Long enough that reaching it
/// means something really is stuck.
const PATIENCE: Duration = Duration::from_secs(20);

/// A receiver service, and a sender aimed at it.
///
/// The receiver's key and id are kept here rather than read back off the
/// service, for two reasons. `TransferService` deliberately does not expose its
/// own key material, and it should not: a transfer service has no reason to hand
/// out a private key. And the test needs the *expected* value, computed from the
/// key it generated, so that "the receiver advertised this key" is a comparison
/// against something independent rather than the same field twice.
struct Pair {
    sender: TransferService,
    receiver: TransferService,
    sender_key: KeyPair,
    receiver_key: KeyPair,
    port: u16,
}

impl Pair {
    /// Bring up a receiver on an ephemeral loopback port and a sender aimed at it.
    async fn new() -> Result<Self> {
        let sender_key = KeyPair::generate()?;
        let receiver_key = KeyPair::generate()?;

        // Small chunks on purpose. A large chunk would fit in one transport
        // message and the reassembly path — which is where an off-by-one in a
        // length prefix would hide — would never run. At 97 bytes a 4 KiB
        // payload takes forty-odd frames.
        let config = TransferConfig {
            chunk_size: 97,
            ..TransferConfig::default()
        };

        let mut receiver = TransferService::new(
            receiver_key.clone(),
            device_id_for(&receiver_key)?,
            config.clone(),
        );
        let port = receiver.start(0).await?;

        let sender = TransferService::new(sender_key.clone(), device_id_for(&sender_key)?, config);

        Ok(Self {
            sender,
            receiver,
            sender_key,
            receiver_key,
            port,
        })
    }

    /// What discovery would have found: the receiver, as seen by the sender.
    ///
    /// Built from the keys this test generated, not from the service, so that the
    /// discovery record under test is the one the application would actually
    /// hold after an mDNS browse.
    fn destination(&self) -> Result<DiscoveredDevice> {
        Ok(DiscoveredDevice {
            device_id: device_id_for(&self.receiver_key)?,
            name: "Loopback Receiver".into(),
            os: "linux".into(),
            app_version: "0.1.0".into(),
            protocol_version: 1,
            addresses: vec!["127.0.0.1".into()],
            port: self.port,
            capabilities: Default::default(),
            static_public_key: self.receiver_key.public_key().to_base64(),
            last_seen: DateTimeUtc::from(chrono::Utc::now()),
        })
    }
}

/// A device's id, derived the one way the app derives it: a fingerprint over the
/// connection key.
///
/// Derived here through the application's own function rather than hardcoded, so
/// the test exercises the same rule the application does. A `const "device-1"`
/// would keep passing while the real derivation changed — and the derivation is
/// precisely what `send_workspace`'s peer lookup rests on.
fn device_id_for(key: &KeyPair) -> Result<String> {
    workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&key.public_key().to_base64())
}

/// Put a receiver on its own task and give the result a concrete type.
///
/// The type is named in the return position because `tokio::spawn(async move
/// { .. })` otherwise infers `impl Future`, which cannot be handed to a helper
/// that takes a `JoinHandle`.
///
/// Takes an `Arc` rather than the service itself so that one listener can serve
/// several receives in sequence — which is what the repeated-transfer case does,
/// and what the application does when a user sends a second workspace.
fn spawn_receive(
    receiver: Arc<TransferService>,
) -> tokio::task::JoinHandle<Result<Option<ReceivedTransfer>>> {
    tokio::spawn(async move { receiver.accept_once().await })
}

/// Wait for a spawned receive to finish, with a bound.
///
/// Returns `Ok(None)` for a receive that started and failed, which is what
/// `accept_once` reports. The failure detail is asserted on the *sender's*
/// status, because the sender is the side that has to explain itself to a user.
async fn receive_within(
    task: tokio::task::JoinHandle<Result<Option<ReceivedTransfer>>>,
) -> Result<Option<ReceivedTransfer>> {
    let joined = tokio::time::timeout(PATIENCE, task).await.map_err(|_| {
        workspace_clone_core::NetworkError::Connection(format!(
            "the receiving side was still waiting after {PATIENCE:?}, so a transfer is stuck"
        ))
    })?;

    joined.map_err(|e| {
        workspace_clone_core::NetworkError::Connection(format!("the receive task failed: {e}"))
    })?
}

/// A payload with a known digest, big enough to need many frames.
///
/// Not a run of zeros: real manifest bytes are varied, and a transport bug that
/// only shows on varied bytes would slip past a uniform payload.
fn payload(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i * 31 + (i >> 8) * 17) % 251) as u8)
        .collect()
}

/// Collect every progress value reported, so a test can check the *shape* of the
/// sequence and not just its endpoints.
fn progress_recorder() -> (Arc<dyn Fn(f32) + Send + Sync>, Arc<Mutex<Vec<f32>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let callback: Arc<dyn Fn(f32) + Send + Sync> =
        Arc::new(move |value: f32| sink.lock().expect("progress lock").push(value));
    (callback, seen)
}

// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_workspace_survives_a_real_handshake_and_arrives_byte_for_byte() -> Result<()> {
    let pair = Pair::new().await?;
    let body = payload(4 * 1024);
    let workspace_id = "ws-round-trip";
    let destination = pair.destination()?;

    // The receive goes on its own task because it blocks in `accept` until the
    // send arrives, and the test body is the send. `destination` is built first:
    // it borrows the whole of `pair`, which a later partial move would forbid.
    let task = spawn_receive(Arc::new(pair.receiver));

    let session = pair
        .sender
        .send_workspace(&destination, workspace_id, &body, None)
        .await?;

    let received = receive_within(task)
        .await?
        .expect("a completed send must produce a received transfer");

    assert_eq!(session.status, TransferStatus::Completed);
    assert_eq!(received.workspace_id, workspace_id);
    assert_eq!(
        received.payload, body,
        "the payload must arrive exactly as sent, or the manifest a user receives is not the one they sent"
    );
    assert_eq!(
        received.digest,
        hex_digest(&body),
        "the digest is what proves the bytes matched, and it is recorded on both sides of the transfer"
    );
    assert_eq!(
        received.peer_static,
        pair.sender_key.public_key().to_base64(),
        "the receiver must learn the sender's authenticated key from the handshake, not from a header it was told"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_receiver_learns_the_senders_authenticated_key() -> Result<()> {
    // The identity the pairing rests on. If the handshake were somehow
    // unauthenticated this would still "work" and the safety number would be
    // theatre, so this asserts the recorded key is the one the sender generated
    // and neither a default nor the receiver's own.
    let pair = Pair::new().await?;
    let body = payload(512);
    let destination = pair.destination()?;

    let task = spawn_receive(Arc::new(pair.receiver));
    let _ = pair
        .sender
        .send_workspace(&destination, "ws-keys", &body, None)
        .await?;
    let received = receive_within(task)
        .await?
        .expect("a transfer should have arrived");

    assert_eq!(received.peer_static, pair.sender_key.public_key().to_base64());
    assert_ne!(
        received.peer_static,
        pair.receiver_key.public_key().to_base64(),
        "the receiver must not record its own key as the peer's"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_is_reported_and_only_after_bytes_are_acknowledged() -> Result<()> {
    // The receiver's bar is the only feedback a long transfer gives, and a bar
    // that reaches 100% before the bytes land is worse than none: a user who
    // closes the app on 100% has closed it mid-transfer.
    let pair = Pair::new().await?;
    let body = payload(16 * 1024);
    let destination = pair.destination()?;
    let (on_progress, seen) = progress_recorder();

    let task = spawn_receive(Arc::new(pair.receiver));

    let session = pair
        .sender
        .send_workspace(&destination, "ws-progress", &body, Some(on_progress))
        .await?;

    let received = receive_within(task)
        .await?
        .expect("a transfer should have arrived");
    assert_eq!(received.payload.len(), body.len());

    let values = seen.lock().expect("progress lock").clone();
    assert!(
        !values.is_empty(),
        "a 16 KiB payload spans many frames, so progress must be reported"
    );
    assert!(
        values.iter().all(|v| (0.0..=1.0).contains(v)),
        "progress must stay within 0..=1, got {values:?}"
    );
    assert!(
        values.windows(2).all(|w| w[1] >= w[0]),
        "progress must not go backwards, got {values:?}"
    );
    assert_eq!(
        values.last().copied(),
        Some(1.0),
        "the last reported value covers the whole payload, so it must be 1.0"
    );
    assert_eq!(session.progress, 1.0);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_transfer_stops_it_before_it_finishes() -> Result<()> {
    // A cancel that only stopped the sender would leave the receiver waiting for
    // a payload that is never coming, holding a connection open. This checks the
    // sender half: the send must terminate, report cancelled rather than
    // completed, and say why.
    let pair = Pair::new().await?;
    let body = payload(4 * 1024 * 1024);
    let destination = pair.destination()?;
    let sender = &pair.sender;

    let send = sender.send_workspace(&destination, "ws-cancel", &body, None);
    tokio::pin!(send);
    let trip_after = tokio::time::sleep(Duration::from_millis(2));

    /// Which side of the race the send and the cancel timer reached.
    enum Race {
        Sent(std::result::Result<TransferSession, workspace_clone_core::WorkspaceError>),
        TimerWon,
    }

    tokio::pin!(trip_after);
    let cancelled_at = std::time::Instant::now();
    let race = tokio::select! {
        biased;
        result = &mut send => Race::Sent(result),
        _ = &mut trip_after => Race::TimerWon,
    };

    let session = match race {
        Race::Sent(result) => {
            // A fast machine can push 4 MiB over loopback before the cancel is
            // delivered. That is a race in the test, not a defect, so it is
            // reported as skipped instead of failed — with a note, so that a
            // passing run never quietly stops testing cancellation.
            if matches!(result, Ok(ref s) if s.status == TransferStatus::Completed) {
                eprintln!(
                    "NOTE: the transfer finished before the cancel could be delivered, \
                     so cancellation was not exercised on this run"
                );
                return Ok(());
            }
            result?
        }
        Race::TimerWon => {
            sender.cancel();
            send.await?
        }
    };

    assert_ne!(
        session.status,
        TransferStatus::Completed,
        "the send reported success despite a cancel being requested"
    );
    assert!(
        session.error.is_some(),
        "a cancelled send must say why; an unexplained failure is one the user cannot act on"
    );

    // The latency is the assertion that matters, and it is the one this test was
    // written for. The flag was originally only read at a frame boundary, so a
    // send blocked in a read sat there until the handshake timeout — fifteen
    // seconds of a Cancel button that looked broken. A cancel has to interrupt
    // whatever the transfer is waiting on, so a budget of well under the
    // handshake timeout is a bound the old code could not meet.
    let elapsed = cancelled_at.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "the cancel took {elapsed:?} to take effect; it is being observed only at a \
         frame boundary rather than interrupting the operation in progress"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_device_that_advertised_no_key_is_refused_rather_than_connected_to() -> Result<()> {
    // The fallback this guards against is an unauthenticated session, which would
    // let anything that can answer on a port impersonate a paired machine. The
    // refusal has to name the device, because that is what the user must act on.
    let pair = Pair::new().await?;
    let mut destination = pair.destination()?;
    destination.static_public_key = String::new();

    let session = pair
        .sender
        .send_workspace(&destination, "ws-no-key", b"payload", None)
        .await?;

    assert_eq!(session.status, TransferStatus::Failed);
    let error = session.error.expect("a failed send must explain itself");
    assert!(
        error.contains(&destination.name),
        "the error should name the device the user was trying to reach; got: {error}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_wrong_key_does_not_complete_a_handshake() -> Result<()> {
    // Noise_IK authenticates the responder's static. If the sender believes it
    // is talking to a machine whose key it does not hold, the handshake must fail
    // rather than connect and discover the problem later.
    let pair = Pair::new().await?;
    let mut destination = pair.destination()?;
    // A valid, correctly-encoded key that is not the receiver's.
    destination.static_public_key = KeyPair::generate()?.public_key().to_base64();

    let task = spawn_receive(Arc::new(pair.receiver));

    let session = pair
        .sender
        .send_workspace(
            &destination,
            "ws-wrong-key",
            b"a workspace that must not arrive",
            None,
        )
        .await?;

    assert_ne!(
        session.status,
        TransferStatus::Completed,
        "a handshake against the wrong static key must not report success"
    );

    // The receiver must not be left blocked on a handshake that cannot succeed.
    receive_within(task).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_transfers_in_a_row_both_succeed_with_different_ids() -> Result<()> {
    // A service that works once and not the second time fails on a user's second
    // workspace, which is the case that generates a bug report. Each round gets
    // its own `accept`, so this also proves the listener survives being reused.
    let pair = Pair::new().await?;
    let destination = pair.destination()?;
    let receiver = Arc::new(pair.receiver);
    let mut first_transfer_id = String::new();

    for round in 1..=2u32 {
        let body = payload(2 * 1024 * round as usize);
        let workspace_id = format!("ws-round-{round}");

        let task = spawn_receive(receiver.clone());
        let session = pair
            .sender
            .send_workspace(&destination, &workspace_id, &body, None)
            .await?;
        let received = receive_within(task)
            .await?
            .unwrap_or_else(|| panic!("round {round} produced no transfer"));

        assert_eq!(session.status, TransferStatus::Completed, "round {round}");
        assert_eq!(received.workspace_id, workspace_id, "round {round}");
        assert_eq!(received.payload, body, "round {round}");

        if round == 1 {
            first_transfer_id = received.transfer_id.clone();
            assert!(!first_transfer_id.is_empty(), "a transfer needs an id");
        } else {
            assert_ne!(
                received.transfer_id, first_transfer_id,
                "two transfers must not share an id, or the second overwrites the first in the transfer table"
            );
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_empty_workspace_never_reports_success_without_being_delivered() -> Result<()> {
    // A workspace with nothing in it is unusual but legitimate — a user selects
    // nothing and sends anyway. Zero length is the case most likely to be
    // special-cased wrongly, and "it said it worked but the other machine has
    // nothing" is exactly the failure that would be reported.
    let pair = Pair::new().await?;
    let destination = pair.destination()?;

    let task = spawn_receive(Arc::new(pair.receiver));
    let session = pair
        .sender
        .send_workspace(&destination, "ws-empty", b"", None)
        .await?;
    let received = receive_within(task).await?;

    match received {
        Some(received) => {
            assert_eq!(session.status, TransferStatus::Completed);
            assert!(received.payload.is_empty());
            assert_eq!(received.digest, hex_digest(b""));
        }
        None => {
            // Also acceptable, provided the sender did not claim success. What
            // must never happen is a completed session with nothing delivered.
            assert_ne!(
                session.status,
                TransferStatus::Completed,
                "the sender reported success with nothing received"
            );
        }
    }
    Ok(())
}
