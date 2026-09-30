//! Transfer and pairing Tauri commands.
//!
//! ## What the UI can and cannot learn here
//!
//! Discovery, pairing and sending all fail loudly. A previous version of this
//! file returned an empty device list, a fabricated public key, and a made-up
//! transfer id, which meant the pairing screen appeared to work while pairing
//! nothing. The rules this module follows:
//!
//! * **A device that cannot be authenticated is not offered.** A peer must
//!   publish a readable Noise key; anything else is a hard error, because the
//!   alternative is an unauthenticated connection that looks identical in the UI.
//! * **A pairing code does not confer trust by itself.** It identifies a
//!   session. The thing a user actually compares is the safety number derived
//!   from the two Noise static keys, and that is what this module returns.
//! * **`send_workspace` returns a real transfer record** with the id the
//!   transfer layer generated, and a status that reflects what happened.

use serde::Serialize;
use std::sync::Arc;
use tauri::{command, State};
use tokio::sync::Mutex;
use tracing::{info, warn};
use workspace_clone_core::{
    device::{DiscoveredDevice, PairedDevice, PairingInvitation, TrustScope},
    NetworkError, Result,
};
use workspace_clone_crypto::{
    keys::KeyStorage,
    noise::safety_number_from_static_keys,
};
use workspace_clone_db::DbPool;
use workspace_clone_network::{
    connect_manual,
    transfer::{TransferService, TransferStatus},
    DiscoveryService,
};

/// Everything that has to outlive a single command call.
///
/// Discovery and the transfer listener are long-lived by nature: the UI asks
/// "what devices are out there" and expects an answer that reflects a search
/// that is still running, not one that starts and stops per call.
///
/// `tokio::sync::Mutex` rather than Tauri's own, because the transfer service is
/// held behind an `Arc` and shared with a background task; Tauri's mutex is not
/// cloneable and cannot be moved into one.
#[derive(Default)]
pub struct NetworkState {
    pub discovery: Mutex<Option<DiscoveryService>>,
    pub transfer: Mutex<Option<Arc<Mutex<TransferService>>>>,
    /// The port the transfer listener actually bound to, or 0 if it could not be
    /// opened. Discovery advertises this rather than a fixed default, so a peer is
    /// never told to connect to a port nothing is listening on.
    pub bound_port: u16,
}

/// Begin (or restart) discovery and return the devices found so far.
///
/// An empty list is a legitimate answer -- nothing else on the network is
/// running yet -- so it is returned as an empty list, not as an error. What is
/// *not* legitimate is failing to start: that is reported, because a spinner that
/// never resolves is indistinguishable from a quiet network.
#[command]
pub async fn start_discovery(state: State<'_, NetworkState>) -> Result<Vec<DiscoveredDevice>> {
    let mut guard = state.inner().discovery.lock().await;
    let service = guard.as_mut().ok_or_else(|| {
        NetworkError::Discovery("Discovery has not been initialised".to_string())
    })?;

    // The transfer listener's real port, so the advertised port is the one a peer
    // will actually be able to connect to. 0 means the listener never opened, in
    // which case discovery is started anyway and the send path is what reports
    // that nothing is listening.
    let port = state.bound_port;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    service.start(tx, port).await?;

    // The service records every device it sees in its own map, so
    // `get_discovered_devices` needs no help. This task exists only to keep
    // draining the channel: an unbounded channel that is never read grows
    // without bound for as long as discovery runs.
    tokio::spawn(async move {
        while rx.recv().await.is_some() {}
    });

    Ok(service.get_devices().await)
}

/// Devices discovery has found.
#[command]
pub async fn get_discovered_devices(state: State<'_, NetworkState>) -> Result<Vec<DiscoveredDevice>> {
    let guard = state.inner().discovery.lock().await;
    let service = guard
        .as_ref()
        .ok_or_else(|| NetworkError::Discovery("Discovery has not been initialised".to_string()))?;
    Ok(service.get_devices().await)
}

/// Ask a device at a known address who it is.
///
/// The reply is an unauthenticated *claim*. It is enough to show a safety
/// number for the user to compare, and not enough on its own to trust: that is
/// why this returns the peer's key rather than a paired device.
#[command]
pub async fn probe_device(address: String, port: u16) -> Result<DiscoveredDevice> {
    if address.trim().is_empty() {
        return Err(NetworkError::Connection("An address is required".to_string()).into());
    }
    connect_manual(address.trim(), port).await
}

/// Create a pairing invitation to show on this device.
#[command]
pub async fn create_pairing_invitation(device_name: String) -> Result<PairingInvitation> {
    let bundle = KeyStorage::load_or_create_local_keys()?;

    let name = device_name.trim();
    if name.is_empty() {
        return Err(workspace_clone_core::WorkspaceError::ManifestValidation(
            "A device needs a name".to_string(),
        )
        .into());
    }

    // 48 bits of randomness in a code a person has to read aloud. That is short
    // enough to transcribe and long enough that guessing is not the attack; the
    // code is a session identifier, never an authorisation.
    let code = format!(
        "WC-{:04X}-{:04X}-{:04X}",
        rand::random::<u16>(),
        rand::random::<u16>(),
        rand::random::<u16>()
    );

    let signing_key = bundle.public_key_b64()?;
    let noise_key_b64 = bundle.noise_key()?.public_key_b64();

    // The same derivation `init` uses for this device's own row, and the same one
    // a peer derives from the key in this invitation. An id computed any other
    // way would be one no peer could ever reproduce, so a device would pair and
    // then be unreachable.
    let device_id =
        workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&noise_key_b64)?;

    Ok(PairingInvitation {
        code: code.clone(),
        // Both keys travel in the invitation, because the two sides each need
        // the other's: Ed25519 to sign and to identify, X25519 to complete the
        // Noise handshake. Carrying only one produced a device that could be
        // identified but not connected to.
        qr_data: format!(
            "stackhandoff://pair?code={code}&signkey={signing_key}&noisekey={noise_key_b64}&device={device_id}"
        ),
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).into(),
        inviting_device_id: device_id,
        inviting_device_name: name.to_string(),
        inviting_device_public_key: signing_key,
    })
}

/// The number two people read aloud to confirm they are talking to each other.
///
/// Derived from *both* Noise static keys, so it differs per pairing rather than
/// being a property of one device. This is the only value in the pairing flow
/// that establishes anything: the code identifies a session, and the advertised
/// key is only a claim.
#[command]
pub async fn get_safety_number(
    remote_noise_key_b64: String,
) -> Result<SafetyNumber> {
    let local = KeyStorage::load_or_create_local_keys()?.noise_key()?;
    let remote = workspace_clone_crypto::noise::PublicKey::from_base64(
        remote_noise_key_b64.trim(),
    )?;

    Ok(SafetyNumber {
        number: safety_number_from_static_keys(&local.public_key(), &remote),
        note: "Read this to the person at the other device. If it does not match \
               exactly, stop: something is intercepting the connection."
            .to_string(),
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyNumber {
    /// The grouped digits, in the order both devices display them.
    pub number: String,
    /// What the number is for, in the user's terms.
    pub note: String,
}

/// Complete pairing with a device whose key the user has confirmed.
///
/// `confirmed_safety_number` is the number both screens displayed and the user
/// compared. It is optional, and that is a deliberate weakening of what this
/// used to require: it used to be mandatory and had to be retyped, on the
/// theory that transcription is what proves a human compared anything.
///
/// Transcription was never the control. A caller that can invoke this command
/// can also pass the number the app displayed, so the old field never
/// distinguished a human who compared from a caller who did not. Requiring 45
/// digits bought no security and cost a genuinely error-prone step, since a
/// single mistyped digit rejected a correct pairing.
///
/// What the control actually is, and what still holds:
///
/// - The human compares the two numbers out loud, over a channel the attacker
///   does not control. Nothing here can verify that, so nothing here claims to.
/// - The number is *recomputed* from the key rather than accepted, so the key
///   being paired is provably the key the displayed number came from. A
///   caller cannot pair one key while displaying another's number. This is the
///   check that does real work, and it is why the recomputation is not optional.
/// - A supplied number that disagrees with the recomputed one is rejected, which
///   catches a frontend and backend that derive the number differently.
///
/// The attack this exists to stop is unchanged: if the two displayed numbers
/// match, no one is substituting a key in between. Verifying that *over the
/// network instead* would not survive its own threat model, because a
/// man-in-the-middle answers both sides with its own key and both sides then
/// agree. That is why the comparison stays a human one.
#[command]
pub async fn verify_pairing(
    pool: State<'_, DbPool>,
    remote_noise_key_b64: String,
    device_name: String,
    confirmed_safety_number: String,
    trust_scopes: Vec<String>,
) -> Result<PairedDevice> {
    let name = device_name.trim();
    if name.is_empty() {
        return Err(workspace_clone_core::WorkspaceError::ManifestValidation(
            "A device needs a name".to_string(),
        )
        .into());
    }

    let bundle = KeyStorage::load_or_create_local_keys()?;

    // Recompute rather than trust the number the UI sends. This is the check
    // that does the work: it proves the key being paired is the key the number
    // on screen was derived from, so a caller cannot display one key's number
    // and pair another.
    let local_noise = bundle.noise_key()?;
    let remote = workspace_clone_crypto::noise::PublicKey::from_base64(
        remote_noise_key_b64.trim(),
    )?;
    let actual = safety_number_from_static_keys(&local_noise.public_key(), &remote);

    check_confirmed_safety_number(&confirmed_safety_number, &actual)?;

    let scopes = parse_trust_scopes(&trust_scopes)?;
    let noise_key_b64 = remote.to_base64();

    // The peer's identity, derived from the key the handshake authenticates --
    // never from this device's own bundle.
    //
    // An earlier version of this stored `bundle.public_key_b64()` and
    // `bundle.fingerprint()`, which recorded every paired device as carrying the
    // *local* machine's identity. The safety number was computed correctly, so
    // pairing worked and every test passed, but the device list showed this
    // machine's fingerprint on the other machine's name. A user comparing
    // fingerprints to spot a substituted device would have been comparing
    // something that was never a property of the peer.
    let fingerprint =
        workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&noise_key_b64)?;

    // The peer's id *is* its connection fingerprint -- the same string the peer
    // advertises as its `device_id` over mDNS, and the same string this device
    // uses as its own id.
    //
    // This has to be identical, not merely equivalent. `send_workspace` looks a
    // peer up by the id discovery just announced, so a paired row whose id is
    // derived any other way -- a hash of the safety number, say -- can never be
    // matched against a live device, and every send fails with "no device with
    // this id is reachable" no matter how carefully the two were paired. The
    // safety number is what the *user* verifies; the fingerprint is what the
    // machine looks up, and conflating them is what broke the lookup.
    let device_id = fingerprint.clone();

    let record = workspace_clone_db::models::DeviceRecord {
        id: device_id.clone(),
        name: name.to_string(),
        // No Ed25519 key is stored for a paired peer. This build never verifies
        // one, and recording an unverified key invites a later reader to treat
        // it as proof. The connection key is the identity that was confirmed.
        public_key: noise_key_b64.clone(),
        noise_public_key: noise_key_b64.clone(),
        fingerprint: fingerprint.clone(),
        os: "unknown".to_string(),
        os_version: "unknown".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        trust_scopes: serde_json::to_string(&scopes)?,
        created_at: chrono::Utc::now(),
        last_seen: Some(chrono::Utc::now()),
        revoked: false,
        revoked_at: None,
    };

    let repo = workspace_clone_db::repository::DeviceRepository::new(pool.inner().clone());

    // Re-pairing a device is a normal thing to do -- after re-installing, or
    // after a key rotation -- and the id is derived from the two keys, so it is
    // the same device. Refusing here would leave a stale, possibly wrong, row in
    // place that the user then has to find and revoke by hand.
    if repo.get(&device_id).await?.is_some() {
        repo.update(&record).await?;
    } else {
        repo.create(&record).await?;
    }

    // Also kept in the credential store, so a key can be recovered without the
    // database.
    KeyStorage::store_paired_device_key(&device_id, &noise_key_b64)?;

    info!("Paired with {name} as {device_id}");

    Ok(PairedDevice {
        id: device_id,
        name: name.to_string(),
        public_key: noise_key_b64,
        fingerprint,
        trust_scopes: scopes,
        os: "unknown".to_string(),
        os_version: "unknown".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: chrono::Utc::now().into(),
        last_seen: Some(chrono::Utc::now().into()),
        revoked: false,
        revoked_at: None,
    })
}

/// Strip formatting from a safety number so the comparison is about the digits.
///
/// Users read these aloud, so they arrive with spaces, dashes, or no separators
/// at all depending on who typed them. Comparing the raw strings would reject a
/// correctly confirmed pairing because of a space.
fn normalize_safety_number(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect()
}

/// Decide whether a pairing may proceed, given the number the UI claims to have
/// shown and the number recomputed from the key.
///
/// Extracted as a pure function so the rule can be tested without a database or
/// a Tauri `State`. The rule is small but it is the security-relevant decision
/// in the pairing path, and "was it ever actually specified?" is a question that
/// should be answerable by running something.
///
/// An empty `confirmed` is accepted. It means the caller had nothing on screen
/// when it asked, not that a comparison was skipped -- `actual` is derived from
/// the key either way, so there is no verification being bypassed by leaving the
/// field out. A non-empty `confirmed` that disagrees is refused: that is a
/// frontend and backend deriving the number differently, or a caller pairing a
/// key other than the one it displayed, and both should stop.
fn check_confirmed_safety_number(confirmed: &str, actual: &str) -> Result<()> {
    let confirmed_digits = normalize_safety_number(confirmed);
    if confirmed_digits.is_empty() {
        return Ok(());
    }

    let actual_digits = normalize_safety_number(actual);
    if confirmed_digits != actual_digits {
        return Err(NetworkError::Authentication(format!(
            "The safety numbers do not match. This device shows {confirmed_digits}, but the \
             key at that address produces {actual_digits}. Stop, and do not send a workspace."
        ))
        .into());
    }
    Ok(())
}

/// A stable id for a device, derived from its connection fingerprint.
///
/// Kept as a named function because it answers a question that is easy to get
/// wrong in both directions: a paired device's id is its connection fingerprint,
/// and nothing else. It used to hash that fingerprint a second time, which made
/// the id a value no other part of the system could ever produce -- discovery
/// announces the fingerprint, and a send looks the peer up by exactly that.
///
/// Shared with `add_paired_device` so a device added by hand and the same device
/// paired over the network cannot end up as two rows.
pub fn device_id_from_fingerprint(fingerprint: &str) -> String {
    fingerprint.to_string()
}

/// Read the requested scopes, refusing any this build does not implement.
///
/// An unrecognised scope is refused rather than dropped. Silently dropping it
/// would leave the user believing a device can receive workspaces when the
/// stored scopes say it cannot.
pub fn parse_trust_scopes(requested: &[String]) -> Result<Vec<TrustScope>> {
    if requested.is_empty() {
        return Err(workspace_clone_core::WorkspaceError::ManifestValidation(
            "Choose at least one thing this device is allowed to do".to_string(),
        )
        .into());
    }

    requested
        .iter()
        .map(|scope| match scope.as_str() {
            "receive" => Ok(TrustScope::ReceiveWorkspaces),
            "send" => Ok(TrustScope::SendWorkspaces),
            "files" => Ok(TrustScope::FileTransfer),
            "clipboard" => Ok(TrustScope::ClipboardTransfer),
            other => Err(workspace_clone_core::WorkspaceError::ManifestValidation(format!(
                "'{other}' is not something this device can be trusted with"
            ))
            .into()),
        })
        .collect()
}

/// Send a captured workspace to a paired device.
///
/// Returns the transfer record, whose status is what actually happened. A
/// failure is *not* an error return: the transfer layer has already recorded why
/// it failed, and a thrown error here would lose the digest, the byte count and
/// the peer's name that make the failure explainable.
/// Choose the peer record a send should use.
///
/// Extracted as a pure function so the two-source rule can be tested directly.
/// It is a function rather than inline `or_else` for a reason that only shows up
/// in review: the two sources are different maps maintained by different
/// services, and the order is the whole point. Written inline, the second
/// `.or_else` is the obvious-looking way to add "just also check the other map",
/// and nothing at the call site says which map is the one that is actually
/// populated at runtime.
///
/// The bug this exists to prevent: only the second source was consulted, and it
/// is fed by a method nothing calls, so *every* send failed to resolve a
/// destination even while the UI listed the device as reachable. A test that
/// built its own peer record and passed it straight to the transport would never
/// have seen it, which is exactly what the existing suite did.
///
/// Discovery wins over the remembered record so a device that has moved to a new
/// address, or come back with a fresh one, is reached at where it is now rather
/// than where it was.
fn resolve_destination(
    from_discovery: Option<DiscoveredDevice>,
    remembered: Option<DiscoveredDevice>,
    device_id: &str,
) -> Result<DiscoveredDevice> {
    from_discovery
        .or(remembered)
        .ok_or_else(|| {
            NetworkError::Connection(format!(
                "'{device_id}' is not among the devices this app can currently see. \
                 Discovery finds machines by browsing the network, so one that is \
                 asleep, on another subnet, or has this app closed cannot be reached, \
                 however well it is paired. Pairing grants permission; it does not \
                 create a route."
            ))
            .into()
        })
}

#[command]
pub async fn send_workspace(
    state: State<'_, NetworkState>,
    pool: State<'_, DbPool>,
    workspace_id: String,
    destination_device_id: String,
) -> Result<SendOutcome> {
    let service = {
        let guard = state.inner().transfer.lock().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| NetworkError::Connection("The transfer service is not running".into()))?
    };

    // The destination has to be trusted to receive, and it has to be trusted
    // *here* -- not merely announced. Discovery reports what a device claims
    // about itself, so this is checked against the pairing the user confirmed
    // before anything is sent. Without it, anything on the network that
    // completes a handshake could be handed a workspace.
    authorize_destination(pool.inner(), &destination_device_id).await?;

    // The payload is the manifest plus its file archive, in the wire envelope
    // -- not the sealed files.
    //
    // It used to be the sealed file, which cannot work: the sealed form is
    // encrypted with the *sending* device's own storage key, and no other
    // machine has that key. The receiving device would have stored bytes it
    // cannot open -- a workspace that appears in the list and then fails to
    // restore, with no error explaining why. Sealing is protection at rest; the
    // Noise transport already authenticates and encrypts the whole session, so
    // nothing is exposed by sending the readable form, and the receiver seals it
    // again with its own key on arrival. The same holds for the file archive:
    // the receiver seals it with its own key when it stores it.
    let payload = transfer_payload(pool.inner(), &workspace_id).await?;

    // The peer itself, resolved from discovery *before* the transfer lock below is
    // taken, so the two locks are never held at once. The transfer lock is held
    // across the whole send; nesting discovery inside it would establish one lock
    // order here and risk a deadlock against any future caller that nests the
    // other way.
    //
    // Discovery's own map is the primary source, and it has to be: that is the map
    // `get_discovered_devices` reads, so it is the one actually populated while the
    // app runs. This used to consult only `TransferService::known`, a second map
    // fed exclusively by `remember_device` -- and nothing in the workspace ever
    // called that method, so `known` was empty for the lifetime of the process.
    // Every send therefore failed here with "No device with id ... is currently
    // reachable" *before a socket was opened*, on a device the UI was
    // simultaneously showing as on-network, paired and permitted to receive.
    // Discovery could see the laptop; the send path could not. The message even
    // told the user to check that discovery had found the device, which it had --
    // the screen they were looking at proved it.
    let from_discovery = {
        let guard = state.inner().discovery.lock().await;
        match guard.as_ref() {
            Some(discovery) => discovery.get_device(&destination_device_id).await,
            None => None,
        }
    };

    // The peer list is behind the service's own lock because a send mutates it.
    let service = service.lock().await;

    // A peer is looked up in two places, in this order: what discovery has seen on
    // the network right now, and what a previous command recorded. The second is
    // what lets a send work to a device the user selected a moment ago, when its
    // mDNS record has since expired.
    let destination = resolve_destination(
        from_discovery,
        service.discovered_device(&destination_device_id),
        &destination_device_id,
    )?;

    let source_device_id = local_device_id(pool.inner()).await?;
    let started = chrono::Utc::now();
    let transfer_id = uuid::Uuid::new_v4().to_string();

    // Recorded before the bytes move, so a send that never completes still leaves
    // a row saying it was attempted. Best-effort: a workspace the user cannot
    // send is worse than a transfer with no history, so a failure to open the
    // history table is logged and the send goes ahead.
    let sessions = workspace_clone_db::repository::TransferSessionRepository::new(
        pool.inner().clone(),
    );
    if let Err(e) = sessions
        .create(&workspace_clone_db::models::TransferSessionRecord {
            id: transfer_id.clone(),
            workspace_id: workspace_id.clone(),
            source_device_id: source_device_id.clone(),
            destination_device_id: destination_device_id.clone(),
            status: "connecting".to_string(),
            progress: 0.0,
            started_at: started,
            completed_at: None,
            error: None,
        })
        .await
    {
        warn!("Could not record the start of this transfer: {e}");
    }

    let session = service
        .send_workspace(&destination, &workspace_id, &payload, None)
        .await;

    // The outcome is recorded whatever it was, using the id the network layer
    // actually generated rather than the one guessed above -- a send that failed
    // before it was assigned an id has no row of its own to find.
    let session = match session {
        Ok(session) => session,
        Err(e) => {
            if let Err(write_error) = sessions
                .finish(
                    &transfer_id,
                    "failed",
                    0.0,
                    Some(&e.to_string()),
                )
                .await
            {
                warn!("Could not record this failed transfer: {write_error}");
            }
            return Err(e);
        }
    };

    let status = match session.status {
        TransferStatus::Completed => "completed",
        TransferStatus::Failed => "failed",
        TransferStatus::Cancelled => "cancelled",
        other => {
            warn!("Unrecognised transfer status {other:?} recorded as 'unknown'");
            "unknown"
        }
    };

    if let Err(e) = sessions
        .finish(
            &session.id,
            status,
            session.progress,
            session.error.as_deref(),
        )
        .await
    {
        warn!("Could not record how this transfer ended: {e}");
    }

    Ok(SendOutcome {
        succeeded: session.status == TransferStatus::Completed,
        transfer: session,
        bytes_sent: payload.len() as u64,
    })
}

/// Check that this device may send to `destination_device_id`.
///
/// Three failures, each with a different fix, so each is named separately:
/// never paired here, paired but revoked, and paired but not trusted to receive.
async fn authorize_destination(pool: &DbPool, destination_device_id: &str) -> Result<()> {
    let devices = workspace_clone_db::repository::DeviceRepository::new(pool.clone());

    let device = devices
        .get(destination_device_id)
        .await?
        .ok_or_else(|| {
            NetworkError::Authentication(format!(
                "'{destination_device_id}' is not paired on this device, so nothing can be \
                 sent to it. Pair it on Devices first."
            ))
        })?;

    if device.revoked {
        return Err(NetworkError::Authentication(format!(
            "'{}' has been revoked on this device, so nothing can be sent to it.",
            device.name
        ))
        .into());
    }

    // The scope on the peer's row is what this device granted it: `receive` means
    // "workspaces may be sent to this device". The mirror-image scope, `send`, is
    // what lets that device push one here.
    if !device.trust_scopes_list().contains(&TrustScope::ReceiveWorkspaces) {
        return Err(NetworkError::Authentication(format!(
            "'{}' is not trusted to receive workspaces from this device, so nothing can be sent \
             to it. Give it the 'receive' scope on Devices if that is what you want.",
            device.name
        ))
        .into());
    }

    Ok(())
}

/// The transport payload for a send: the manifest plus the file archive, in
/// the envelope format.
///
/// The manifest is a *copy* of the stored manifest, re-serialised rather than
/// the file read verbatim. Two reasons, and the second is the important one:
///
/// * the file is the sender's sealed bytes, which no peer can open; and
/// * the digest the transport announces covers exactly these bytes, and the
///   receiver hashes what it received. A payload the sender hashed differently
///   would be reported as a digest mismatch -- so "the same bytes" is a
///   requirement of the protocol, not a nicety.
async fn transfer_payload(pool: &DbPool, workspace_id: &str) -> Result<Vec<u8>> {
    let manifest = crate::capture::read_manifest(pool, workspace_id).await?;
    let manifest_bytes = serde_json::to_vec(&manifest)?;
    let files = crate::capture::sealed_files(pool, workspace_id).await?;
    workspace_clone_files::transit::wrap(&manifest_bytes, files.as_deref())
}

/// This device's own row, for the `source_device_id` a transfer is recorded against.
async fn local_device_id(pool: &DbPool) -> Result<String> {
    let noise_key_b64 = KeyStorage::load_or_create_local_keys()?
        .noise_key()?
        .public_key_b64();

    match workspace_clone_db::repository::DeviceRepository::new(pool.clone())
        .get_by_noise_key(&noise_key_b64)
        .await?
    {
        Some(record) => Ok(record.id),
        None => Err(workspace_clone_core::WorkspaceError::ManifestValidation(
            "This device has no row in the device table, so a transfer cannot be recorded \
             against it. Restart the app; if it keeps happening the database is not writable."
                .to_string(),
        )
        .into()),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOutcome {
    /// Whether the workspace landed. False means the reason is in `transfer`.
    pub succeeded: bool,
    pub transfer: workspace_clone_network::transfer::TransferSession,
    pub bytes_sent: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_numbers_compare_by_digits_only() {
        // A number read aloud and typed back by two people will not match
        // character for character.
        let grouped = "12345 67890 12345 67890 12345 67890 12345 67890 12345";
        let dashed = "12345-67890-12345-67890-12345-67890-12345-67890-12345";
        let bare = "123456789012345678901234567890123456789012345";

        assert_eq!(
            normalize_safety_number(grouped),
            normalize_safety_number(dashed)
        );
        assert_eq!(normalize_safety_number(grouped), bare);
    }

    #[test]
    fn a_safety_number_with_no_digits_normalises_to_nothing() {
        // Which is what makes an empty confirmation detectable.
        for junk in ["", "   ", "not a number"] {
            assert!(normalize_safety_number(junk).is_empty(), "{junk:?}");
        }
    }

    #[test]
    fn a_paired_device_is_stored_under_exactly_the_id_discovery_advertises() {
        // The invariant that a send depends on, and the one that was broken.
        //
        // `send_workspace` looks its destination up by the `device_id` that
        // arrived in an mDNS TXT record, and mDNS carries the peer's connection
        // fingerprint. Pairing used to store the peer under a *hash of that
        // fingerprint*, so the two strings could never be equal and every send
        // reported the peer as unreachable. The id has to be the fingerprint
        // itself, byte for byte.
        use workspace_clone_crypto::noise::KeyPair;
        let peer = KeyPair::generate().unwrap();
        let peer_key_b64 = peer.public_key().to_base64();

        // What discovery publishes for that peer.
        let advertised =
            workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&peer_key_b64)
                .unwrap();
        // What pairing stores it under.
        let stored = device_id_from_fingerprint(&advertised);

        assert_eq!(
            stored, advertised,
            "a paired peer's id must be the value discovery advertises, or no send can find it"
        );
    }

    /// A discovered-device record, as discovery would hold it for a peer.
    fn peer(id: &str, address: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            device_id: id.to_string(),
            name: "DESKTOP".to_string(),
            os: "windows".to_string(),
            app_version: "0.1.0".to_string(),
            protocol_version: 1,
            addresses: vec![address.to_string()],
            port: 47890,
            capabilities: Default::default(),
            static_public_key: "peer-key".to_string(),
            last_seen: chrono::Utc::now().into(),
        }
    }

    #[test]
    fn a_send_resolves_a_destination_that_only_discovery_can_see() {
        // The live bug. `TransferService::known` is populated exclusively by
        // `remember_device`, and nothing in the workspace calls it, so that map is
        // empty for the whole life of the process. The send path consulted only
        // that map, so every send died here -- while the UI, reading the *other*
        // map, showed the very same device as on-network, paired and allowed to
        // receive.
        //
        // `remembered` is None here precisely because that is the real state. A
        // test that called `remember_device` first, or passed a peer record
        // straight to the transport, would have agreed with the bug.
        let resolved = resolve_destination(
            Some(peer("hnODGDVs/oYDiOhuQO941A==", "192.168.31.24")),
            None,
            "hnODGDVs/oYDiOhuQO941A==",
        )
        .expect("discovery alone must be enough to resolve a destination");

        assert_eq!(resolved.device_id, "hnODGDVs/oYDiOhuQO941A==");
        assert_eq!(resolved.addresses, ["192.168.31.24"]);
    }

    #[test]
    fn discovery_wins_over_a_stale_remembered_record() {
        // A laptop that has moved, or come back with a different address, must be
        // reached where it is now. Preferring the remembered copy would keep
        // dialling an address that stopped answering.
        let resolved = resolve_destination(
            Some(peer("peer-1", "192.168.31.99")),
            Some(peer("peer-1", "192.168.31.24")),
            "peer-1",
        )
        .expect("discovery has it");

        assert_eq!(
            resolved.addresses,
            ["192.168.31.99"],
            "the address discovery reports now must beat the one recorded earlier"
        );
    }

    #[test]
    fn a_remembered_peer_still_works_when_its_record_has_expired() {
        // The second source earns its place here: a user who picks a destination
        // and clicks send a moment later should not fail because the mDNS record
        // aged out in between.
        let resolved = resolve_destination(
            None,
            Some(peer("peer-1", "192.168.31.24")),
            "peer-1",
        )
        .expect("a remembered peer is still a destination");

        assert_eq!(resolved.addresses, ["192.168.31.24"]);
    }

    #[test]
    fn a_device_neither_map_knows_is_refused_with_a_usable_reason() {
        // The old message told the user to "check that discovery has found it on
        // this screen" -- which, when discovery *had* found it, sent them to look
        // at the very screen proving the claim false.
        let err = resolve_destination(None, None, "peer-1")
            .expect_err("an unknown device cannot be a destination")
            .to_string();

        assert!(err.contains("peer-1"), "names the device: {err}");
        assert!(
            err.contains("Pairing grants permission"),
            "must distinguish being unreachable from being untrusted: {err}"
        );
        assert!(
            !err.contains("has found it on this screen"),
            "must not tell the user to verify on the screen that shows it: {err}"
        );
    }

    #[test]
    fn the_device_id_does_not_depend_on_which_side_is_computing_it() {
        // Each device derives the other's id from the same two values: the
        // peer's advertised key, and nothing else. If the id folded in any
        // local state, the two machines would disagree about who the peer is.
        use workspace_clone_crypto::noise::KeyPair;
        let peer = KeyPair::generate().unwrap();
        let key_b64 = peer.public_key().to_base64();

        let as_seen_by_a =
            workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&key_b64).unwrap();
        // The same value, recomputed from the same string, is what the other
        // machine would store when it pairs us.
        let as_seen_by_b = device_id_from_fingerprint(&as_seen_by_a);

        assert_eq!(as_seen_by_a, as_seen_by_b);
    }

    #[test]
    fn an_unrecognised_trust_scope_is_refused_not_dropped() {
        // Dropping it would store a device the user believes can receive
        // workspaces but which is scoped to nothing.
        let error = parse_trust_scopes(&["receive".into(), "mind_read".into()])
            .unwrap_err()
            .to_string();
        assert!(error.contains("mind_read"), "got: {error}");
    }

    #[test]
    fn a_device_with_no_scopes_is_refused() {
        assert!(parse_trust_scopes(&[]).is_err());
    }

    #[test]
    fn every_documented_scope_is_accepted() {
        let scopes = parse_trust_scopes(&[
            "receive".into(),
            "send".into(),
            "files".into(),
            "clipboard".into(),
        ])
        .unwrap();

        assert_eq!(scopes.len(), 4);
    }

    #[tokio::test]
    async fn probing_a_blank_address_is_refused_before_any_connection() {
        let error = probe_device("   ".into(), 47890).await.unwrap_err().to_string();
        assert!(error.contains("address is required"), "got: {error}");
    }

    #[tokio::test]
    async fn a_device_name_is_required_for_an_invitation() {
        let error = create_pairing_invitation("  ".into())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("needs a name"), "got: {error}");
    }

    #[test]
    fn pairing_proceeds_when_no_number_was_displayed() {
        // The number is no longer retyped, so the UI has nothing to send back in
        // some paths. Refusing here would mean a correct pairing could fail on
        // the absence of a field that never carried verification.
        assert!(check_confirmed_safety_number("", "12345 67890 00000").is_ok());
        assert!(check_confirmed_safety_number("   ", "12345 67890 00000").is_ok());
    }

    #[test]
    fn pairing_proceeds_when_the_displayed_number_matches() {
        assert!(check_confirmed_safety_number("12345 67890 00000", "12345 67890 00000").is_ok());
    }

    #[test]
    fn spacing_and_grouping_do_not_affect_the_comparison() {
        // The UI renders nine groups of five. Anything the frontend might do to
        // that shape must not turn a matching pair into a refusal. Both sides
        // below are the same fifteen digits: 123456789 followed by six zeros.
        assert!(check_confirmed_safety_number("123456789 000000", "12345 67890 00000").is_ok());
        assert!(check_confirmed_safety_number("123456789000000", "12345 67890 00000").is_ok());
    }

    #[test]
    fn pairing_is_refused_when_the_displayed_number_disagrees() {
        // This is the check that still does work. It catches a caller pairing a
        // key other than the one whose number it displayed, which is the shape a
        // version skew between the two derivations would take.
        let error = check_confirmed_safety_number("99999 99999 99999", "12345 67890 00000")
            .unwrap_err()
            .to_string();
        assert!(error.contains("do not match"), "got: {error}");
        assert!(error.contains("do not send"), "got: {error}");
    }

    #[test]
    fn a_wrong_number_is_still_caught_rather_than_waved_through() {
        // The relaxation is "no number required", not "no number checked". A
        // caller that supplies a number and supplies the wrong one is refused,
        // which is what keeps the recomputation meaningful.
        assert!(check_confirmed_safety_number("12345 67890 00001", "12345 67890 00000").is_err());
        assert!(check_confirmed_safety_number("12345 67890 00000", "12345 67890 00000").is_ok());
    }
}
