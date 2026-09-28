//! Receiving a workspace from a paired device.
//!
//! ## Why this module exists
//!
//! The transfer listener was bound and its port advertised over mDNS, and no
//! task ever called `accept_once`. A peer connected, the connection sat in the
//! kernel's accept queue, and nothing was ever received: no payload, no
//! workspace row, no indication on the receiving machine that anything had
//! arrived. The sender, meanwhile, waited out its frame timeout and reported a
//! failure. Sending a workspace from one machine to another did not work at all.
//!
//! ## What arriving means
//!
//! Receiving is not "write the bytes to a file". The blueprint's first restore
//! stage ("Validate the manifest schema, sender signature, device authorization,
//! size limits, and supported features") is the receiver's job, and it runs
//! before anything is stored:
//!
//! 1. **Authorization.** The peer must be paired on *this* device, not revoked,
//!    and trusted with `send`. Pairing is per-device in both directions, so a
//!    machine that has never been paired here cannot push a workspace here, no
//!    matter what it announces about itself. The identity checked is the Noise
//!    static key the handshake authenticated, never the device id in a frame
//!    header -- that is an unauthenticated string, and trusting it would let
//!    anything on the network claim to be a paired device.
//! 2. **Schema and policy.** `WorkspaceManifest::validate` rejects a manifest
//!    from a newer schema, one that claims to contain secret values, and one
//!    that asks for automatic command execution.
//! 3. **Size.** A payload larger than the limit is refused rather than written.
//!
//! ## What is stored, and with which key
//!
//! The payload in flight is the manifest as JSON, not the sealed form. Sealing
//! is protection *at rest*, and the key that does it is this device's own storage
//! key, which no other machine has. Shipping the sealed file would have meant
//! storing bytes on arrival that this device cannot open -- a workspace that
//! appears in the list and then fails to restore. Confidentiality in flight comes
//! from the Noise transport, which is already authenticated and encrypted for
//! the whole session.
//!
//! So the receiver seals the manifest with its *own* key on arrival, and the
//! stored workspace is indistinguishable from one captured locally. That is also
//! why `workspaces.manifest_digest` is a digest of the local sealed bytes and
//! not of what arrived: the two are different documents, and a receive-side check
//! that compared them would fail every time.

use serde::Serialize;
use std::sync::Arc;
use tauri::{command, State};
use tokio::sync::Mutex;
use tracing::{info, warn};
use workspace_clone_core::{
    crypto::EncryptionKey, device::TrustScope, manifest::WorkspaceManifest, Result,
};
use workspace_clone_crypto::keys::KeyStorage;
use workspace_clone_db::{
    models::{TransferSessionRecord, WorkspaceFilesRecord},
    repository::{DeviceRepository, TransferSessionRepository, WorkspaceFilesRepository, WorkspaceRepository},
    DbPool,
};
use workspace_clone_network::transfer::{ReceivedTransfer, TransferReceiver};

use sha2::{Digest, Sha256};

/// The largest payload this build will accept from a peer: the file archive
/// cap (512 MiB of file contents, shared with the sender's snapshot builder)
/// plus headroom for the envelope header and the manifest itself.
///
/// A cap on a *received* payload is what stops a peer from filling this
/// machine's disk. The sender's own limit is the same value (the snapshot
/// builder stops at `TOTAL_MAX_BYTES`), so a workspace that could not have
/// been sent is not one that can arrive either; the check is repeated here
/// because the receiver is the side that pays for it.
pub const MAX_INCOMING_PAYLOAD_BYTES: usize =
    workspace_clone_files::TOTAL_MAX_BYTES as usize + 32 * 1024 * 1024;

/// How many arrivals to keep for the UI to show.
///
/// A transfer that completes while the window is closed is still stored -- the
/// workspace is on disk and in the database either way. This only bounds how
/// many *notifications* are remembered, so a machine that ran for a week
/// unattended does not grow an unbounded list in memory. Dropped arrivals are
/// logged.
const MAX_REMEMBERED_ARRIVALS: usize = 50;

/// Arrivals the user has not dismissed yet.
///
/// Shared with the accept loop, which is a background task, and read by
/// commands, which run on Tauri's own runtime. A `tokio::sync::Mutex` for the
/// same reason the network state uses one: it can be moved into a spawned task
/// and cloned as an `Arc`.
#[derive(Clone, Default)]
pub struct IncomingState {
    pub arrivals: Arc<Mutex<Vec<IncomingTransfer>>>,
}

impl IncomingState {
    pub fn new() -> Self {
        Self::default()
    }

    async fn record(&self, arrival: IncomingTransfer) {
        let mut arrivals = self.arrivals.lock().await;
        if arrivals.len() >= MAX_REMEMBERED_ARRIVALS {
            let dropped = arrivals.remove(0);
            warn!(
                "Not showing the arrival of '{}' any more: more than {MAX_REMEMBERED_ARRIVALS} \
                 are waiting to be dismissed. The workspace is still on disk.",
                dropped.workspace_name
            );
        }
        arrivals.insert(0, arrival);
    }
}

/// Something a peer tried to send, and what this device did with it.
///
/// Both outcomes are represented rather than only the successful one. A machine
/// that silently ignores an unauthorised push is indistinguishable from a broken
/// one, and the first question a user asks when a workspace does not appear is
/// whether it arrived -- so a refusal is reported with the reason it was
/// refused.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingTransfer {
    /// The transfer id the network layer generated. Matches the `id` of the
    /// recorded `transfer_sessions` row, so an arrival can be traced.
    pub transfer_id: String,
    /// The workspace's own id, empty for a payload that was refused before it
    /// could be parsed.
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    /// The workspace's name, for display. Falls back to the claimed source
    /// device when the manifest was refused before its name could be trusted.
    pub workspace_name: String,
    /// The sending device's id, derived from the key the handshake authenticated.
    #[serde(rename = "senderDeviceId")]
    pub sender_device_id: String,
    /// The name this device has for the sender, or the id when it has none --
    /// an unpaired sender has no row and therefore no name.
    pub sender_device_name: String,
    /// Where the sender claims the workspace was captured. Usually the sender.
    pub source_device_id: String,
    /// Whether the manifest is now stored on this device.
    pub accepted: bool,
    /// Why it was refused, or `None` when it was not. Written for a person, not
    /// for a log: it says what to do next.
    pub refusal_reason: Option<String>,
    /// The digest of the payload *as it arrived*.
    ///
    /// A different value from `workspaces.manifest_digest`, which covers the
    /// locally sealed copy. Both are here on purpose -- the first proves what
    /// travelled, the second what is stored.
    pub transfer_digest: String,
    pub bytes_received: u64,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

/// Transfers this device has taken part in, newest first.
///
/// A workspace id filters to one workspace. With no filter, every transfer.
///
/// The table is written by both ends of every transfer. It exists because
/// "did that actually get there?" used to have no answer: the send result lived
/// in the window until it closed, and an arriving transfer left no trace at all.
#[command]
pub async fn get_transfer_history(
    pool: State<'_, DbPool>,
    workspace_id: Option<String>,
) -> Result<Vec<TransferHistoryEntry>> {
    let repo = TransferSessionRepository::new(pool.inner().clone());

    let sessions = match workspace_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => repo.list_for_workspace(id).await?,
        None => repo.list().await?,
    };

    let names = device_names(pool.inner()).await?;

    Ok(sessions
        .into_iter()
        .map(|session| TransferHistoryEntry {
            id: session.id,
            workspace_id: session.workspace_id,
            source_device_id: session.source_device_id.clone(),
            source_device_name: names
                .get(&session.source_device_id)
                .cloned()
                .unwrap_or_else(|| session.source_device_id.clone()),
            destination_device_id: session.destination_device_id.clone(),
            destination_device_name: names
                .get(&session.destination_device_id)
                .cloned()
                .unwrap_or_else(|| session.destination_device_id.clone()),
            status: session.status,
            progress: session.progress,
            started_at: session.started_at,
            completed_at: session.completed_at,
            error: session.error,
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferHistoryEntry {
    pub id: String,
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    #[serde(rename = "sourceDeviceId")]
    pub source_device_id: String,
    #[serde(rename = "sourceDeviceName")]
    pub source_device_name: String,
    #[serde(rename = "destinationDeviceId")]
    pub destination_device_id: String,
    #[serde(rename = "destinationDeviceName")]
    pub destination_device_name: String,
    pub status: String,
    pub progress: f32,
    #[serde(rename = "startedAt")]
    pub started_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub error: Option<String>,
}

/// Device id to display name, for history and arrivals.
///
/// A map rather than a query per row: a transfer history is read as a list, and
/// one query per entry is a query per entry for information the devices table
/// already holds in full.
async fn device_names(pool: &DbPool) -> Result<std::collections::HashMap<String, String>> {
    Ok(DeviceRepository::new(pool.clone())
        .list(true)
        .await?
        .into_iter()
        .map(|device| (device.id, device.name))
        .collect())
}

/// Arrivals this device has not dismissed.
#[command]
pub async fn get_incoming_transfers(state: State<'_, IncomingState>) -> Result<Vec<IncomingTransfer>> {
    Ok(state.arrivals.lock().await.clone())
}

/// Stop showing an arrival.
///
/// The workspace itself is untouched: this only clears the notification, so
/// dismissing an arrival cannot lose a workspace that was already stored.
#[command]
pub async fn dismiss_incoming_transfer(
    state: State<'_, IncomingState>,
    transfer_id: String,
) -> Result<()> {
    let mut arrivals = state.arrivals.lock().await;
    arrivals.retain(|arrival| arrival.transfer_id != transfer_id);
    Ok(())
}

/// Supplies this device's manifest-sealing key, one transfer at a time.
///
/// A closure rather than a key passed in, because the key must not be fetched
/// once and held for the lifetime of the app: it would sit in process memory for
/// as long as the window is open, instead of only while a transfer is being
/// stored. It is also what makes the accept loop testable. Loading the real key
/// reaches the OS credential store, which blocks on a user prompt when the
/// calling binary is not already trusted -- so a headless test would hang on a
/// dialog nobody can see, and the loop would be the one part of receiving with no
/// coverage at all.
pub type StorageKeySource = Arc<dyn Fn() -> Result<EncryptionKey> + Send + Sync>;

/// Accept incoming transfers for as long as the app runs, sealing with the key
/// this device's credential store holds.
///
/// Started once, from `init`, immediately after the listener is bound and before
/// discovery starts -- so a peer that discovers this device can connect to a
/// listener that is already being served.
pub fn spawn_accept_loop(
    receiver: TransferReceiver,
    pool: DbPool,
    state: IncomingState,
    local_device_id: String,
) {
    serve_forever(
        receiver,
        pool,
        state,
        local_device_id,
        Arc::new(|| Ok(KeyStorage::load_local_keys()?.storage_key()?)),
    );
}

/// [`spawn_accept_loop`], with the sealing key supplied rather than read from the
/// credential store.
///
/// The loop survives every per-connection failure. A refused handshake, a
/// cancelled send, a digest mismatch, an unauthorised sender and a credential
/// store that will not open all end the *connection*, not the loop: a machine
/// that stopped listening after one bad peer would silently become unreachable
/// for every peer afterwards, which is a far worse failure than the one it was
/// avoiding.
///
/// Each connection is handled on its own task, and the loop goes straight back
/// to accepting. Handling is bounded by the handshake timeout, so handling
/// inline would mean one peer that connects and says nothing delays every other
/// peer by that long -- not a slow transfer, but a machine that has stopped
/// accepting while it waits on a connection nobody is speaking on. That was the
/// behaviour until a test connected a silent socket and measured a legitimate
/// transfer taking fifteen seconds behind it.
pub fn serve_forever(
    receiver: TransferReceiver,
    pool: DbPool,
    state: IncomingState,
    local_device_id: String,
    storage_key: StorageKeySource,
) {
    // `tauri::async_runtime::spawn`, not `tokio::spawn`.
    //
    // This is called from Tauri's `setup` hook, which runs on the main thread
    // with no Tokio runtime entered. `tokio::spawn` panics there -- "there is no
    // reactor running" -- and because `setup` is on the startup path the whole
    // app died on launch, after the listener had bound and announced itself over
    // mDNS. Tauri owns a global runtime and its `spawn` enters it, so it works
    // from either context: with a runtime already entered (a test) or without one
    // (the real startup path).
    tauri::async_runtime::spawn(async move {
        loop {
            let (stream, peer) = match receiver.accept_connection().await {
                Ok(connection) => connection,
                Err(e) => {
                    // The only way `accept` itself fails. Returning stops the app
                    // claiming to be receiving when it is not.
                    warn!("The transfer listener stopped accepting: {e}");
                    return;
                }
            };

            let receiver = receiver.clone();
            let pool = pool.clone();
            let state = state.clone();
            let local_device_id = local_device_id.clone();
            let storage_key = storage_key.clone();

            // Plain `tokio::spawn`, unlike the outer one: this is already running
            // inside the runtime the outer `tauri::async_runtime::spawn`
            // established, so the reactor is present. Only the outermost spawn
            // needs Tauri's wrapper.
            tokio::spawn(async move {
                let received = match receiver.receive_on_with_progress(stream, None).await {
                    Ok(received) => received,
                    Err(e) => {
                        // A refused handshake, a cancelled send, a digest
                        // mismatch. One connection, not the listener.
                        warn!("Incoming transfer from {peer} failed: {e}");
                        return;
                    }
                };

                if let Err(e) =
                    handle_arrival(&pool, &state, &local_device_id, &storage_key, received).await
                {
                    // A failure to *store* an arrival is reported. The bytes are
                    // already gone, so there is nothing to retry, but the next
                    // peer still deserves to be served.
                    warn!("Could not store an incoming transfer: {e}");
                }
            });
        }
    });
}

/// Validate one arrival and store it, recording why either way.
async fn handle_arrival(
    pool: &DbPool,
    state: &IncomingState,
    local_device_id: &str,
    storage_key: &StorageKeySource,
    received: ReceivedTransfer,
) -> Result<()> {
    // Fetched here rather than inside the decision function, so a credential
    // store that will not open fails the arrival loudly instead of looking like a
    // policy refusal -- and so the decision function itself takes no lock on
    // anything it does not need.
    let storage_key = storage_key()?;

    let arrival = accept_or_refuse(pool, &storage_key, local_device_id, received).await?;
    let accepted = arrival.accepted;
    let name = arrival.workspace_name.clone();

    state.record(arrival).await;

    if accepted {
        info!("Received the workspace '{name}'");
    }
    Ok(())
}

/// The authorization and validation a received payload has to pass.
///
/// `Ok` covers both outcomes that are decisions -- stored, or refused with a
/// reason -- because both are things a user needs to be told. `Err` is reserved
/// for a failure to reach a decision at all: an unreadable database, a
/// credential store that will not open, a disk that will not take the file. Those
/// are not refusals, and reporting one as the other would tell a user their
/// workspace was rejected by policy when in fact nothing was checked.
///
/// `storage_key` is this device's manifest-sealing key, passed in rather than
/// loaded here. Loading it inside would reach the OS credential store, which
/// blocks on a user prompt when the calling binary is not already trusted -- so
/// a unit test would hang on a dialog nobody can see, and the one test that
/// matters most (that a received workspace opens with this device's key) would
/// be the one that could not run. Taking it as a parameter makes the dependency
/// visible and the whole path testable.
///
/// `local_device_id` is this device's own id, already derived by `init` and passed
/// in for the same reason as the key: looking it up here would mean reading the
/// credential store again, and it is a foreign key the history row needs.
///
/// Split from [`handle_arrival`] so every step is a plain function over a
/// `ReceivedTransfer` and can be tested without a socket, a credential store, or
/// a running app.
pub async fn accept_or_refuse(
    pool: &DbPool,
    storage_key: &EncryptionKey,
    local_device_id: &str,
    received: ReceivedTransfer,
) -> Result<IncomingTransfer> {
    let now = chrono::Utc::now();
    let mut arrival = IncomingTransfer {
        transfer_id: received.transfer_id.clone(),
        workspace_id: received.workspace_id.clone(),
        workspace_name: String::new(),
        sender_device_id: String::new(),
        sender_device_name: String::new(),
        source_device_id: String::new(),
        accepted: false,
        refusal_reason: None,
        transfer_digest: received.digest.clone(),
        bytes_received: received.payload.len() as u64,
        received_at: now,
    };

    // ---- 1. Who sent this -------------------------------------------------
    //
    // `peer_static` is the key the Noise handshake authenticated, so the device
    // id derived from it is the only sender identity this code can rely on. The
    // `sender_device_id` in the frame headers is a string the sender chose, and
    // it is recorded for display only.
    let sender_id =
        match workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&received.peer_static)
        {
            Ok(id) => id,
            Err(e) => {
                arrival.refusal_reason =
                    Some(format!("The sending device published a key this build cannot read, so it cannot be identified: {e}"));
                return Ok(arrival);
            }
        };
    arrival.sender_device_id = sender_id.clone();

    let devices = DeviceRepository::new(pool.clone());
    let sender = match devices.get_by_noise_key(&received.peer_static).await {
        Ok(Some(device)) => device,
        Ok(None) => {
            arrival.sender_device_name = sender_id.clone();
            arrival.refusal_reason = Some(format!(
                "A device that has not been paired on this machine tried to send a workspace. \
                 Nothing was stored. Pair it on Devices first, then ask it to send again."
            ));
            return Ok(arrival);
        }
        Err(e) => {
            arrival.refusal_reason = Some(format!(
                "The paired-device list could not be read, so the sender could not be checked: {e}"
            ));
            return Ok(arrival);
        }
    };
    arrival.sender_device_name = sender.name.clone();

    if sender.revoked {
        arrival.refusal_reason = Some(format!(
            "'{}' has been revoked on this machine. Nothing was stored.",
            sender.name
        ));
        return Ok(arrival);
    }

    // The scope stored on the peer's row is what *this* device granted it. A peer
    // trusted only to receive workspaces may not push one here, even though it
    // completed a handshake and even though this device granted the mirror-image
    // scope when pairing.
    if !sender.trust_scopes_list().contains(&TrustScope::SendWorkspaces) {
        arrival.refusal_reason = Some(format!(
            "'{}' is trusted to receive workspaces from this machine, not to send any. \
             Nothing was stored. Give it the 'send' scope on Devices if that is what you want.",
            sender.name
        ));
        return Ok(arrival);
    }

    // Keep the pairing fresh, so a device list that shows "last seen" is telling
    // the truth about who is still in use.
    if let Err(e) = devices.update_last_seen(&sender.id).await {
        warn!("Could not record that {} was just seen: {e}", sender.name);
    }

    // ---- 2. Size ----------------------------------------------------------
    if received.payload.len() > MAX_INCOMING_PAYLOAD_BYTES {
        arrival.refusal_reason = Some(format!(
            "The incoming workspace is {} bytes, over this build's {MAX_INCOMING_PAYLOAD_BYTES}-byte \
             limit for a received workspace. Nothing was stored.",
            received.payload.len()
        ));
        return Ok(arrival);
    }

    // ---- 2b. Split the envelope -------------------------------------------
    // Payloads that are not enveloped (every transfer made before files
    // existed) read back as manifest-only; enveloped ones carry the archive
    // the restore side will need.
    let (manifest_bytes, incoming_files) = match workspace_clone_files::transit::unwrap(&received.payload)
    {
        Ok(workspace_clone_files::transit::Unwrapped::ManifestOnly(m)) => (m, None),
        Ok(workspace_clone_files::transit::Unwrapped::WithFiles { manifest, files }) => {
            (manifest, Some(files))
        }
        Err(e) => {
            arrival.refusal_reason =
                Some(format!("The payload was not a workspace this build can read: {e}"));
            return Ok(arrival);
        }
    };

    // ---- 3. Schema and policy ---------------------------------------------
    let manifest: WorkspaceManifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(e) => {
            arrival.refusal_reason =
                Some(format!("The payload was not a workspace this build can read: {e}"));
            return Ok(arrival);
        }
    };

    if let Err(e) = manifest.validate() {
        arrival.refusal_reason = Some(format!("The workspace was refused: {e}"));
        return Ok(arrival);
    }

    // The id the manifest claims has to be the one the transport announced, or the
    // stored copy would not be reachable under the id the database row says.
    if !arrival.workspace_id.is_empty() && manifest.workspace.id != arrival.workspace_id {
        arrival.workspace_name = manifest.workspace.name.clone();
        arrival.refusal_reason = Some(format!(
            "The transfer says it is workspace '{}' but the manifest it carries is '{}'. \
             Nothing was stored.",
            arrival.workspace_id, manifest.workspace.id
        ));
        return Ok(arrival);
    }

    arrival.workspace_id = manifest.workspace.id.clone();
    arrival.workspace_name = manifest.workspace.name.clone();
    arrival.source_device_id = manifest.workspace.source_device.id.clone();

    // ---- 4. The claimed capture device has to be one we know ---------------
    //
    // `workspaces.source_device_id` is a foreign key, so an unknown id fails the
    // insert. More importantly it is a claim: a manifest naming a device this
    // machine has never seen is asserting a provenance that cannot be checked.
    // The sender itself is always known, because pairing put it in this table.
    if arrival.source_device_id != sender.id && devices.get(&arrival.source_device_id).await?.is_none()
    {
        arrival.refusal_reason = Some(format!(
            "The workspace says it was captured on a device this machine has never seen \
             ('{}'). Nothing was stored.",
            arrival.source_device_id
        ));
        return Ok(arrival);
    }

    // ---- 5. Store it ------------------------------------------------------
    //
    // Sealed with *this* device's key, so the stored workspace is readable here
    // and is in the same form a locally captured one is.
    let sealed = workspace_clone_crypto::seal_json(storage_key, &manifest)?;

    let path = crate::capture::manifest_path_for(&manifest.workspace.id)?;
    std::fs::write(&path, sealed.as_bytes()).map_err(|e| {
        workspace_clone_core::DatabaseError::Connection(format!(
            "The received manifest could not be written to {}: {e}",
            path.display()
        ))
    })?;

    // The digest of the *local* sealed bytes, matching what a capture writes --
    // `get_manifest` verifies against it, so it has to be this and not the digest
    // of the payload that arrived.
    let mut hasher = Sha256::new();
    hasher.update(sealed.as_bytes());
    let local_digest = hex(&hasher.finalize());

    WorkspaceRepository::new(pool.clone())
        .upsert(&workspace_clone_db::models::WorkspaceRecord {
            id: manifest.workspace.id.clone(),
            name: manifest.workspace.name.clone(),
            schema_version: manifest.schema_version as i32,
            // Explicit: the newtype's conversion falls back to "now" on a
            // timestamp it cannot parse, which would file this under the arrival
            // time rather than the capture time.
            captured_at: manifest.workspace.captured_at.clone().into(),
            source_device_id: arrival.source_device_id.clone(),
            manifest_digest: local_digest,
            encrypted_manifest_path: path.to_string_lossy().to_string(),
            // Not "captured". This device did not capture it, and the status is
            // what a workspace list groups by, so a received workspace claiming to
            // be a local capture would be a small lie in the one field a user
            // reads before choosing what to restore.
            status: "received".to_string(),
        })
        .await?;

    // ---- 5b. Store the files, when the envelope carried an archive ---------
    //
    // Sealed with this device's key like the manifest, and recorded with the
    // same counts a local capture would record, so the restore side cannot tell
    // (and does not need to tell) whether files arrived or were created here.
    // A refused or dropped archive must not lose the workspace itself, so a
    // failure to persist files is a real error, not a warning: the workspace
    // is in the list but its files would be missing from every restore.
    if let Some(tar) = incoming_files {
        let sealed_files = workspace_clone_crypto::seal_bytes(storage_key, &tar)?;
        let files_path = crate::capture::files_path_for(&manifest.workspace.id)?;
        std::fs::write(&files_path, &sealed_files).map_err(|e| {
            workspace_clone_core::DatabaseError::Connection(format!(
                "The received project files could not be written to {}: {e}",
                files_path.display()
            ))
        })?;

        let (file_count, byte_count) = workspace_clone_files::archive::archive_summary(&tar);
        WorkspaceFilesRepository::new(pool.clone())
            .upsert(&WorkspaceFilesRecord {
                workspace_id: manifest.workspace.id.clone(),
                encrypted_files_path: files_path.to_string_lossy().to_string(),
                byte_count: byte_count as i64,
                file_count: file_count as i64,
                archive_format: "tar".to_string(),
            })
            .await?;
    }

    // ---- 6. Record the transfer -------------------------------------------
    //
    // Best-effort, and explicitly so. The workspace is stored and the arrival is
    // about to be shown, so failing to append a history row must not turn a
    // completed transfer into a reported failure.
    //
    //
    // The destination id is checked rather than assumed. It is a foreign key, so
    // an empty string would fail the constraint -- and because this is
    // best-effort, that failure would be swallowed into a log line nobody reads,
    // leaving a transfer with no history at all and no sign that it happened.
    if local_device_id.is_empty() {
        warn!(
            "Received '{}' but could not record the transfer: this device's own id is not \
             known here, so there is nothing to record the transfer against. The workspace \
             itself is stored.",
            manifest.workspace.name
        );
    } else if let Err(e) = TransferSessionRepository::new(pool.clone())
        .create(&TransferSessionRecord {
            id: received.transfer_id.clone(),
            workspace_id: manifest.workspace.id.clone(),
            source_device_id: sender.id.clone(),
            destination_device_id: local_device_id.to_string(),
            status: "completed".to_string(),
            progress: 1.0,
            started_at: now,
            completed_at: Some(chrono::Utc::now()),
            error: None,
        })
        .await
    {
        warn!("Received '{}' but could not record the transfer: {e}", manifest.workspace.name);
    }

    arrival.accepted = true;
    Ok(arrival)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use workspace_clone_core::{
        device::DateTimeUtc,
        manifest::{DeviceRef, Portability, WorkspaceMeta},
    };
    use workspace_clone_crypto::noise::KeyPair;
    use workspace_clone_db::repository::init_db_at;

    /// A migrated database in a temporary file, deleted when the test ends.
    ///
    /// A file rather than an in-memory pool, because sqlx hands each checkout a
    /// different connection and an in-memory database is per-connection, so the
    /// migrations would not be visible to the code under test.
    struct TempDb(std::path::PathBuf);

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            // WAL mode leaves two sidecar files next to the database.
            let _ = std::fs::remove_file(format!("{}-wal", self.0.display()));
            let _ = std::fs::remove_file(format!("{}-shm", self.0.display()));
        }
    }

    /// A migrated database, a local device row, and a workspace id unique to
    /// this test.
    ///
    /// The local row is not decoration: `init` creates one before anything else
    /// runs, `transfer_sessions.destination_device_id` is a foreign key into it,
    /// and a test that omitted it would exercise a state the real app is never in.
    ///
    /// The id has to be unique because [`crate::capture::manifest_path_for`]
    /// resolves to a real directory -- there is no test-only override, and adding
    /// one would mean a way to redirect where a user's sealed manifests are read
    /// from. Uniqueness stops two parallel tests writing the same file, and the
    /// path is deleted by [`Self::cleanup`].
    struct Fixture {
        /// Held only so the database file outlives the test; never read.
        _db: TempDb,
        pool: DbPool,
        local_id: String,
        workspace_id: String,
        /// Stands in for this device's manifest-sealing key.
        ///
        /// A real credential store cannot be used: reading another binary's
        /// keychain item blocks on a user prompt, and a headless test run has
        /// nobody to answer it. The key is the one thing a test may fake without
        /// weakening what it checks -- "does the stored manifest open with the
        /// key that sealed it" is exactly the property, and it is checked below
        /// with this same key.
        storage_key: EncryptionKey,
    }

    impl Fixture {
        async fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "workspace-clone-recv-{name}-{}-{}.db",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let pool = init_db_at(&path).await.expect("migrations should apply");

            let nonce = chrono::Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or_default();
            let workspace_id = format!("recv-test-{name}-{nonce}");

            // This device's own row, as `init` would have written it before any
            // command ran.
            let local_key = KeyPair::generate().unwrap();
            let local_key_b64 = local_key.public_key().to_base64();
            let local_id =
                workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&local_key_b64)
                    .unwrap();
            DeviceRepository::new(pool.clone())
                .create(&workspace_clone_db::models::DeviceRecord {
                    id: local_id.clone(),
                    name: "This Mac".to_string(),
                    public_key: local_key_b64.clone(),
                    noise_public_key: local_key_b64,
                    fingerprint: local_id.clone(),
                    trust_scopes: "[]".to_string(),
                    os: "macos".to_string(),
                    os_version: "15".to_string(),
                    app_version: "0.1.0".to_string(),
                    created_at: chrono::Utc::now(),
                    last_seen: Some(chrono::Utc::now()),
                    revoked: false,
                    revoked_at: None,
                })
                .await
                .expect("local device row");

            Self {
                _db: TempDb(path),
                pool,
                local_id,
                workspace_id,
                storage_key: EncryptionKey::new([0x5a; 32]),
            }
        }

        /// Delete the sealed manifest this test's arrival wrote.
        fn cleanup(&self) {
            if let Ok(path) = crate::capture::manifest_path_for(&self.workspace_id) {
                let _ = std::fs::remove_file(path);
            }
        }

        async fn row(&self) -> Option<workspace_clone_db::models::WorkspaceRecord> {
            WorkspaceRepository::new(self.pool.clone())
                .get(&self.workspace_id)
                .await
                .expect("the workspace lookup should run")
        }
    }

    /// A paired device with a freshly generated key, trusted with `scopes`.
    async fn paired_sender(
        pool: &DbPool,
        scopes: Vec<TrustScope>,
    ) -> (KeyPair, String) {
        let key = KeyPair::generate().unwrap();
        let key_b64 = key.public_key().to_base64();
        let id = workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&key_b64).unwrap();

        DeviceRepository::new(pool.clone())
            .create(&workspace_clone_db::models::DeviceRecord {
                id: id.clone(),
                name: "Peer Laptop".to_string(),
                public_key: key_b64.clone(),
                noise_public_key: key_b64,
                fingerprint: id.clone(),
                trust_scopes: serde_json::to_string(&scopes).unwrap(),
                os: "windows".to_string(),
                os_version: "11".to_string(),
                app_version: "0.1.0".to_string(),
                created_at: chrono::Utc::now(),
                last_seen: None,
                revoked: false,
                revoked_at: None,
            })
            .await
            .expect("device row");

        (key, id)
    }

    /// An arrival, with `sender_device_id` set to a deliberate lie.
    ///
    /// Every test that accepts a workspace relies on this: the sender has to be
    /// identified by the key the handshake authenticated, not by the string in
    /// the frame header.
    fn arrival_from(peer: &KeyPair, workspace_id: &str, payload: Vec<u8>) -> ReceivedTransfer {
        ReceivedTransfer {
            transfer_id: format!("transfer-{workspace_id}"),
            workspace_id: workspace_id.to_string(),
            payload,
            peer_static: peer.public_key().to_base64(),
            sender_device_id: "not-the-sender".to_string(),
            digest: "digest-of-what-arrived".to_string(),
            duration: Duration::from_millis(1),
        }
    }

    fn manifest_json(id: &str, name: &str, source: &str) -> Vec<u8> {
        let manifest = WorkspaceManifest {
            schema_version: 1,
            workspace: WorkspaceMeta {
                id: id.to_string(),
                name: name.to_string(),
                captured_at: DateTimeUtc::from(chrono::Utc::now()),
                source_device: DeviceRef {
                    id: source.to_string(),
                    os: "windows".to_string(),
                    os_version: "11".to_string(),
                },
                portability: Portability::CrossPlatform,
            },
            ..Default::default()
        };
        serde_json::to_vec(&manifest).unwrap()
    }

    /// `accept_or_refuse` with the "could not reach a decision" case unwrapped.
    ///
    /// A failure to decide -- an unreadable database, a credential store that
    /// will not open -- is a bug in the test's setup, not an expected outcome, so
    /// it panics with the error rather than being asserted about.
    async fn decide(f: &Fixture, received: ReceivedTransfer) -> IncomingTransfer {
        accept_or_refuse(&f.pool, &f.storage_key, &f.local_id, received)
            .await
            .expect("the arrival should reach a decision")
    }

    #[tokio::test]
    async fn an_unpaired_sender_is_refused_and_nothing_is_written() {
        let f = Fixture::new("unpaired").await;
        let stranger = KeyPair::generate().unwrap();
        let payload = manifest_json(&f.workspace_id, "Somebody's workspace", "someone-else");

        let arrival = decide(&f, arrival_from(&stranger, &f.workspace_id, payload)).await;

        assert!(!arrival.accepted, "an unpaired device must not store anything");
        assert!(
            arrival.refusal_reason.as_deref().unwrap_or_default().contains("not been paired"),
            "the reason must say why: {:?}",
            arrival.refusal_reason
        );
        assert!(f.row().await.is_none());
        f.cleanup();
    }

    #[tokio::test]
    async fn a_paired_device_without_the_send_scope_is_refused() {
        let f = Fixture::new("no-scope").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::ReceiveWorkspaces]).await;
        let payload = manifest_json(&f.workspace_id, "Received", &sender_id);

        let arrival = decide(&f, arrival_from(&peer, &f.workspace_id, payload)).await;

        assert!(!arrival.accepted);
        let reason = arrival.refusal_reason.unwrap();
        assert!(reason.contains("not to send"), "got: {reason}");
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn a_revoked_device_is_refused() {
        let f = Fixture::new("revoked").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        let payload = manifest_json(&f.workspace_id, "Received", &sender_id);

        DeviceRepository::new(f.pool.clone())
            .revoke(&sender_id)
            .await
            .unwrap();

        let arrival = decide(&f, arrival_from(&peer, &f.workspace_id, payload)).await;

        assert!(!arrival.accepted);
        assert!(arrival.refusal_reason.unwrap().contains("revoked"));
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn a_paired_device_trusted_to_send_has_its_workspace_stored_and_readable() {
        // The test that would have caught the original bug: what travels used to
        // be the *sender's* sealed manifest, so the row existed and every read of
        // it failed to decrypt.
        let f = Fixture::new("accepted").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        let payload = manifest_json(&f.workspace_id, "My Workspace", &sender_id);

        let arrival =
            decide(&f, arrival_from(&peer, &f.workspace_id, payload.clone())).await;

        assert!(arrival.accepted, "refused: {:?}", arrival.refusal_reason);
        assert_eq!(arrival.workspace_name, "My Workspace");

        let row = f.row().await.expect("the workspace must be recorded");
        assert_eq!(row.source_device_id, sender_id);
        assert_eq!(row.status, "received");

        // The stored bytes must open with the key that sealed them, through the
        // same read path every other command uses. This is the assertion that
        // would have caught the original bug: what travelled used to be the
        // *sender's* sealed manifest, so the row existed and every read of it
        // failed to decrypt.
        let manifest = crate::capture::read_manifest_with_key(
            &f.pool,
            &f.workspace_id,
            &f.storage_key,
        )
        .await
        .expect("a received workspace must be readable like a captured one");
        assert_eq!(manifest.workspace.id, f.workspace_id);
        assert_eq!(
            serde_json::to_vec(&manifest).unwrap(),
            payload,
            "the stored manifest must be the one that arrived"
        );
        f.cleanup();
    }

    #[tokio::test]
    async fn a_manifest_claiming_an_unknown_capture_device_is_refused() {
        let f = Fixture::new("unknown-source").await;
        let (peer, _sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        // A device this machine has never seen, and which is not the sender.
        let payload = manifest_json(
            &f.workspace_id,
            "Forwarded",
            "a-device-nobody-has-heard-of",
        );

        let arrival = decide(&f, arrival_from(&peer, &f.workspace_id, payload)).await;

        assert!(!arrival.accepted);
        assert!(arrival
            .refusal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("never seen"));
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn a_manifest_that_disagrees_about_the_workspace_id_is_refused() {
        let f = Fixture::new("id-mismatch").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        // The transport says one id; the manifest says another.
        let payload = manifest_json("some-other-workspace", "Confused", &sender_id);

        let arrival = decide(&f, arrival_from(&peer, &f.workspace_id, payload)).await;

        assert!(!arrival.accepted);
        assert!(arrival
            .refusal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("some-other-workspace"));
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn a_manifest_that_asks_for_automatic_commands_is_refused() {
        let f = Fixture::new("auto-commands").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;

        let mut manifest: WorkspaceManifest =
            serde_json::from_slice(&manifest_json(&f.workspace_id, "Dangerous", &sender_id)).unwrap();
        manifest.policy.automatic_command_execution = true;

        let arrival = decide(
            &f,
            arrival_from(
                &peer,
                &f.workspace_id,
                serde_json::to_vec(&manifest).unwrap(),
            ),
        )
        .await;

        assert!(!arrival.accepted);
        assert!(arrival
            .refusal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Automatic command execution"));
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn a_payload_that_is_not_a_workspace_is_refused() {
        let f = Fixture::new("not-json").await;
        let (peer, _sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;

        let arrival = decide(
            &f,
            arrival_from(&peer, &f.workspace_id, b"<html>not a manifest".to_vec()),
        )
        .await;

        assert!(!arrival.accepted);
        assert!(arrival
            .refusal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("not a workspace"));
    }

    #[tokio::test]
    async fn the_sender_comes_from_the_authenticated_key_not_the_frame_header() {
        let f = Fixture::new("identity").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        let payload = manifest_json(&f.workspace_id, "My Workspace", &sender_id);

        let arrival =
            decide(&f, arrival_from(&peer, &f.workspace_id, payload)).await;

        assert!(arrival.accepted, "refused: {:?}", arrival.refusal_reason);
        assert_eq!(
            arrival.sender_device_id, sender_id,
            "the sender must be identified by the key the handshake authenticated"
        );
        f.cleanup();
    }

    #[tokio::test]
    async fn a_workspace_sent_twice_is_stored_once_and_keeps_its_history() {
        let f = Fixture::new("resent").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;
        let payload = manifest_json(&f.workspace_id, "My Workspace", &sender_id);

        for round in 0..2 {
            let mut arrival = arrival_from(&peer, &f.workspace_id, payload.clone());
            // Distinct transfer ids, as two real sends would have. Reusing one
            // would collapse the two into a single row, which is correct: it
            // would then be one transfer, not two.
            arrival.transfer_id = format!("{}-{round}", arrival.transfer_id);

            let decision = decide(&f, arrival).await;
            assert!(decision.accepted, "refused: {:?}", decision.refusal_reason);
        }

        let all = WorkspaceRepository::new(f.pool.clone())
            .list(100, 0)
            .await
            .unwrap();
        assert_eq!(
            all.iter().filter(|w| w.id == f.workspace_id).count(),
            1,
            "a re-send must not create a second workspace"
        );

        // Two transfers were received, and both must still be recorded. An
        // `INSERT OR REPLACE` here would have deleted the first one's row along
        // with the workspace row it pointed at.
        let sessions = TransferSessionRepository::new(f.pool.clone())
            .list_for_workspace(&f.workspace_id)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2, "both received transfers must be recorded");
        f.cleanup();
    }

    #[tokio::test]
    async fn an_oversized_payload_is_refused_without_being_written() {
        let f = Fixture::new("oversized").await;
        let (peer, sender_id) = paired_sender(&f.pool, vec![TrustScope::SendWorkspaces]).await;

        let mut big = manifest_json(&f.workspace_id, "Huge", &sender_id);
        big.resize(MAX_INCOMING_PAYLOAD_BYTES + 1, b' ');

        let arrival = decide(&f, arrival_from(&peer, &f.workspace_id, big)).await;

        assert!(!arrival.accepted);
        assert!(arrival
            .refusal_reason
            .as_deref()
            .unwrap_or_default()
            .contains("limit"));
        assert!(f.row().await.is_none());
    }

    #[tokio::test]
    async fn arrivals_are_remembered_newest_first_and_can_be_dismissed() {
        let state = IncomingState::new();

        state
            .record(IncomingTransfer {
                transfer_id: "first".into(),
                workspace_id: "ws-1".into(),
                workspace_name: "First".into(),
                sender_device_id: "peer".into(),
                sender_device_name: "Peer".into(),
                source_device_id: "peer".into(),
                accepted: true,
                refusal_reason: None,
                transfer_digest: String::new(),
                bytes_received: 1,
                received_at: chrono::Utc::now(),
            })
            .await;
        state
            .record(IncomingTransfer {
                transfer_id: "second".into(),
                workspace_id: "ws-2".into(),
                workspace_name: "Second".into(),
                sender_device_id: "peer".into(),
                sender_device_name: "Peer".into(),
                source_device_id: "peer".into(),
                accepted: true,
                refusal_reason: None,
                transfer_digest: String::new(),
                bytes_received: 1,
                received_at: chrono::Utc::now(),
            })
            .await;

        let arrivals = state.arrivals.lock().await;
        assert_eq!(arrivals.len(), 2);
        assert_eq!(
            arrivals.first().map(|a| a.transfer_id.as_str()),
            Some("second"),
            "the newest arrival must be first, or a user reads stale ones first"
        );
        drop(arrivals);

        state.arrivals.lock().await.retain(|a| a.transfer_id != "second");
        assert_eq!(state.arrivals.lock().await.len(), 1);
    }
}

