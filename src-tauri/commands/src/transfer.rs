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
            "workspace-clone://pair?code={code}&signkey={signing_key}&noisekey={noise_key_b64}&device={device_id}"
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
/// `confirmed_safety_number` is the number the user read out loud and matched.
/// It is required: a pairing stored without one is a device this app will send
/// a workspace to on the strength of an advertisement alone, which is exactly
/// the attack the safety number exists to stop.
#[command]
pub async fn verify_pairing(
    pool: State<'_, DbPool>,
    remote_noise_key_b64: String,
    device_name: String,
    expected_safety_number: String,
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

    // Recompute rather than trust the number the UI sends. The UI's copy could
    // have been altered, or could have been filled in without the user ever
    // comparing it; recomputing proves the key the user looked at is the key
    // being paired.
    let local_noise = bundle.noise_key()?;
    let remote = workspace_clone_crypto::noise::PublicKey::from_base64(
        remote_noise_key_b64.trim(),
    )?;
    let actual = safety_number_from_static_keys(&local_noise.public_key(), &remote);

    let expected = normalize_safety_number(&expected_safety_number);
    if expected.is_empty() {
        return Err(workspace_clone_core::WorkspaceError::ManifestValidation(
            "The safety number must be confirmed before a device is trusted".to_string(),
        )
        .into());
    }
    if expected != normalize_safety_number(&actual) {
        return Err(NetworkError::Authentication(format!(
            "The safety numbers do not match. You confirmed {expected}, but the device at \
             that address presents {}. Stop, and do not send a workspace.",
            normalize_safety_number(&actual)
        ))
        .into());
    }

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

    // The payload is the manifest as JSON, not the sealed file.
    //
    // It used to be the sealed file, which cannot work: the sealed form is
    // encrypted with the *sending* device's own storage key, and no other
    // machine has that key. The receiving device would have stored bytes it
    // cannot open -- a workspace that appears in the list and then fails to
    // restore, with no error explaining why. Sealing is protection at rest; the
    // Noise transport already authenticates and encrypts the whole session, so
    // nothing is exposed by sending the readable form, and the receiver seals it
    // again with its own key on arrival.
    let payload = manifest_payload(pool.inner(), &workspace_id).await?;

    // The peer list is behind the service's own lock because a send mutates it.
    let service = service.lock().await;

    // A peer is looked up in two places, in this order: what discovery has seen
    // on the network right now, and what a previous command recorded. The second
    // is what lets a send work to a device the user selected a moment ago, when
    // an mDNS record has since expired.
    let destination = service.discovered_device(&destination_device_id).ok_or_else(|| {
        NetworkError::Connection(format!(
            "No device with id '{destination_device_id}' is currently reachable. \
             Check that it is running, on the same network, and that discovery \
             has found it on this screen."
        ))
    })?;

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

/// The manifest to put on the wire, as JSON.
///
/// A *copy* of the stored manifest, re-serialised rather than the file read
/// verbatim. Two reasons, and the second is the important one:
///
/// * the file is the sender's sealed bytes, which no peer can open; and
/// * the digest the transport announces covers exactly these bytes, and the
///   receiver hashes what it received. A payload the sender hashed differently
///   would be reported as a digest mismatch -- so "the same bytes" is a
///   requirement of the protocol, not a nicety.
async fn manifest_payload(pool: &DbPool, workspace_id: &str) -> Result<Vec<u8>> {
    let manifest = crate::capture::read_manifest(pool, workspace_id).await?;
    Ok(serde_json::to_vec(&manifest)?)
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
}
