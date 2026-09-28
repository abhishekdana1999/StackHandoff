//! Database models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use workspace_clone_core::device::TrustScope;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    /// Ed25519 public key, base64. Used for signing and for display.
    pub public_key: String,
    /// X25519 static public key, base64. The Noise handshake authenticates
    /// against this, not against `public_key`.
    ///
    /// Empty means the device was paired by a build that did not record it, and
    /// it cannot be connected to. Callers must treat that as a real limitation
    /// rather than falling back to `public_key`, which would produce a handshake
    /// that cannot complete.
    #[serde(default)]
    pub noise_public_key: String,
    pub fingerprint: String,
    pub trust_scopes: String, // JSON array
    pub os: String,
    pub os_version: String,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub revoked: bool,
    pub revoked_at: Option<DateTime<Utc>>,
}

impl DeviceRecord {
    pub fn trust_scopes_list(&self) -> Vec<TrustScope> {
        serde_json::from_str(&self.trust_scopes).unwrap_or_default()
    }

    pub fn set_trust_scopes(&mut self, scopes: &[TrustScope]) {
        self.trust_scopes = serde_json::to_string(scopes).unwrap_or_default();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkspaceRecord {
    pub id: String,
    pub name: String,
    pub schema_version: i32,
    pub captured_at: DateTime<Utc>,
    pub source_device_id: String,
    pub manifest_digest: String,
    pub encrypted_manifest_path: String,
    pub status: String,
}

/// The on-disk sealed file archive backing a workspace's project files.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct WorkspaceFilesRecord {
    pub workspace_id: String,
    pub encrypted_files_path: String,
    pub byte_count: i64,
    pub file_count: i64,
    pub archive_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SnapshotRecord {
    pub id: String,
    pub workspace_id: String,
    pub captured_at: DateTime<Utc>,
    pub source_device_id: String,
    pub size_bytes: i64,
    pub transfer_status: String,
    pub transfer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RestoreRunRecord {
    pub id: String,
    pub workspace_id: String,
    pub destination_device_id: String,
    pub plan_digest: String,
    pub approved_steps: String, // JSON array
    pub result_summary: String, // JSON
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AdapterCheckRecord {
    pub id: String,
    pub adapter_id: String,
    pub adapter_version: i32,
    pub result_state: String,
    pub safe_evidence: String, // JSON
    pub checked_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TransferSessionRecord {
    pub id: String,
    pub workspace_id: String,
    pub source_device_id: String,
    pub destination_device_id: String,
    pub status: String,
    pub progress: f32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}
