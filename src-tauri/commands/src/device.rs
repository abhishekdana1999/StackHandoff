//! Device management Tauri commands.

use chrono::Utc;
use tauri::{command, State};
use workspace_clone_core::{device::PairedDevice, NetworkError, Result, WorkspaceError};
use workspace_clone_crypto::{keys::KeyStorage, noise::PublicKey};
use workspace_clone_db::{models::DeviceRecord, repository::DeviceRepository, DbPool};

/// The id of this machine, as its peers would derive it from its advertised key.
///
/// The local device keeps a row in the `devices` table (it has to: a foreign
/// key like `workspaces.source_device_id` needs a device row to point at), so a
/// list of "devices on this machine" needs this id to tell it and its peers
/// apart.
fn local_device_id() -> Result<String> {
    let bundle = KeyStorage::load_local_keys()?;
    let noise = bundle.noise_key()?;
    workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&noise.public_key_b64())
}

#[command]
pub async fn list_paired_devices(pool: State<'_, DbPool>) -> Result<Vec<PairedDevice>> {
    let repo = DeviceRepository::new(pool.inner().clone());
    let local_id = local_device_id()?;
    let devices = repo.list(false).await?;

    Ok(devices
        .into_iter()
        .filter(|d| d.id != local_id)
        .map(|d| {
            let trust_scopes = d.trust_scopes_list();
            PairedDevice {
                id: d.id,
                name: d.name,
                public_key: d.public_key,
                fingerprint: d.fingerprint,
                trust_scopes,
                os: d.os,
                os_version: d.os_version,
                app_version: d.app_version,
                created_at: d.created_at.into(),
                last_seen: d.last_seen.map(Into::into),
                revoked: d.revoked,
                revoked_at: d.revoked_at.map(Into::into),
            }
        })
        .collect())
}

#[command]
pub async fn get_paired_device(
    pool: State<'_, DbPool>,
    device_id: String,
) -> Result<Option<PairedDevice>> {
    let repo = DeviceRepository::new(pool.inner().clone());
    let device = repo.get(&device_id).await?;

    Ok(device.map(|d| {
        let trust_scopes = d.trust_scopes_list();
        PairedDevice {
            id: d.id,
            name: d.name,
            public_key: d.public_key,
            fingerprint: d.fingerprint,
            trust_scopes,
            os: d.os,
            os_version: d.os_version,
            app_version: d.app_version,
            created_at: d.created_at.into(),
            last_seen: d.last_seen.map(Into::into),
            revoked: d.revoked,
            revoked_at: d.revoked_at.map(Into::into),
        }
    }))
}

/// Add a device the user has already paired by some other means.
///
/// This exists for a device added ahead of time, so a capture can name it as its
/// source before the two machines are on the same network. Two constraints come
/// from that role:
///
/// * **The Noise key is required.** Without it the device is visible in the list
///   and cannot be connected to, and there is no way for the user to tell that
///   apart from a bug. A device added without one is refused here rather than
///   stored broken.
/// * **Unknown trust scopes are refused.** Silently dropping one leaves the user
///   believing the device can do something it cannot.
#[command]
pub async fn add_paired_device(
    pool: State<'_, DbPool>,
    name: String,
    public_key: String,
    noise_public_key: String,
    os: String,
    os_version: String,
    app_version: String,
    trust_scopes: Vec<String>,
) -> Result<PairedDevice> {
    let repo = DeviceRepository::new(pool.inner().clone());

    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(WorkspaceError::ManifestValidation(
            "A device needs a name".to_string(),
        )
        .into());
    }

    // Both keys are parsed rather than stored verbatim. A key this build cannot
    // read would otherwise fail at connect time, with a far less obvious message
    // and after the user believed pairing had worked.
    let fingerprint =
        workspace_clone_core::crypto::fingerprint_from_public_key_b64(public_key.trim())?;
    let noise = PublicKey::from_base64(noise_public_key.trim()).map_err(|e| {
        NetworkError::Authentication(format!(
            "That device's connection key cannot be read: {e}. A device added by hand \
             needs the X25519 key the other device shows as its 'connection key', \
             not its fingerprint."
        ))
    })?;

    let scopes = crate::transfer::parse_trust_scopes(&trust_scopes)?;

    // The id is derived from the fingerprint, so adding the same device twice
    // updates one row rather than accumulating two that each look like a
    // different machine and each need revoking.
    if let Some(existing) = repo.get_by_fingerprint(&fingerprint).await? {
        return Err(WorkspaceError::ManifestValidation(format!(
            "'{}' is already paired as '{}'. Revoke it first if you mean to replace it.",
            name, existing.id
        ))
        .into());
    }

    let device = DeviceRecord {
        id: crate::transfer::device_id_from_fingerprint(&fingerprint),
        name,
        public_key: public_key.trim().to_string(),
        noise_public_key: noise.to_base64(),
        fingerprint,
        trust_scopes: serde_json::to_string(&scopes)?,
        os,
        os_version,
        app_version,
        created_at: Utc::now(),
        last_seen: None,
        revoked: false,
        revoked_at: None,
    };

    repo.create(&device).await?;

    Ok(PairedDevice {
        id: device.id,
        name: device.name,
        public_key: device.public_key,
        fingerprint: device.fingerprint,
        trust_scopes: scopes,
        os: device.os,
        os_version: device.os_version,
        app_version: device.app_version,
        created_at: device.created_at.into(),
        last_seen: None,
        revoked: false,
        revoked_at: None,
    })
}

/// Rename a paired device.
#[command]
pub async fn update_paired_device(
    pool: State<'_, DbPool>,
    device_id: String,
    name: Option<String>,
) -> Result<()> {
    let repo = DeviceRepository::new(pool.inner().clone());

    // An unknown id is reported rather than treated as a no-op. The previous
    // version returned success for a device that did not exist, so a rename
    // that silently did nothing looked like it had worked.
    let mut device = repo.get(&device_id).await?.ok_or_else(|| {
        WorkspaceError::ManifestValidation(format!("There is no paired device '{device_id}'"))
    })?;

    if let Some(new_name) = name {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            return Err(
                WorkspaceError::ManifestValidation("A device needs a name".to_string()).into(),
            );
        }
        device.name = trimmed;
    }

    repo.update(&device).await
}

#[command]
pub async fn revoke_paired_device(pool: State<'_, DbPool>, device_id: String) -> Result<()> {
    let repo = DeviceRepository::new(pool.inner().clone());
    repo.revoke(&device_id).await?;

    // Also delete stored public key
    workspace_clone_crypto::keys::KeyStorage::delete_paired_device_key(&device_id)?;

    Ok(())
}

#[command]
pub async fn delete_paired_device(pool: State<'_, DbPool>, device_id: String) -> Result<()> {
    let repo = DeviceRepository::new(pool.inner().clone());
    repo.delete(&device_id).await?;
    workspace_clone_crypto::keys::KeyStorage::delete_paired_device_key(&device_id)?;
    Ok(())
}
