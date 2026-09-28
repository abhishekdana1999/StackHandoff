//! Device types and trust management

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Trust scopes for paired devices
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum TrustScope {
    ReceiveWorkspaces,
    SendWorkspaces,
    FileTransfer,
    ClipboardTransfer,
}

impl TrustScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrustScope::ReceiveWorkspaces => "receive",
            TrustScope::SendWorkspaces => "send",
            TrustScope::FileTransfer => "files",
            TrustScope::ClipboardTransfer => "clipboard",
        }
    }

    pub fn all() -> Vec<TrustScope> {
        vec![
            TrustScope::ReceiveWorkspaces,
            TrustScope::SendWorkspaces,
            TrustScope::FileTransfer,
            TrustScope::ClipboardTransfer,
        ]
    }
}

/// Wrapper for DateTime<Utc> to support JsonSchema
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct DateTimeUtc(pub String);

impl From<DateTime<Utc>> for DateTimeUtc {
    fn from(dt: DateTime<Utc>) -> Self {
        Self(dt.to_rfc3339())
    }
}

impl From<DateTimeUtc> for DateTime<Utc> {
    fn from(wrapper: DateTimeUtc) -> Self {
        DateTime::parse_from_rfc3339(&wrapper.0)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now())
    }
}

// Conversion functions
pub fn datetime_to_utc(dt: DateTime<Utc>) -> DateTimeUtc {
    DateTimeUtc::from(dt)
}

pub fn utc_to_datetime(wrapper: DateTimeUtc) -> DateTime<Utc> {
    DateTime::<Utc>::from(wrapper)
}

pub fn option_datetime_to_utc(opt: Option<DateTime<Utc>>) -> Option<DateTimeUtc> {
    opt.map(DateTimeUtc::from)
}

pub fn option_utc_to_datetime(opt: Option<DateTimeUtc>) -> Option<DateTime<Utc>> {
    opt.map(DateTime::<Utc>::from)
}

/// Paired device information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub public_key: String,  // base64 encoded Ed25519 public key
    pub fingerprint: String, // Short fingerprint for display
    pub trust_scopes: Vec<TrustScope>,
    pub os: String,
    pub os_version: String,
    pub app_version: String,
    pub created_at: DateTimeUtc,
    pub last_seen: Option<DateTimeUtc>,
    pub revoked: bool,
    pub revoked_at: Option<DateTimeUtc>,
}

/// Device status for UI
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    Online,
    Offline,
    Unknown,
}

/// Device with runtime status
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DeviceWithStatus {
    #[serde(flatten)]
    pub device: PairedDevice,
    pub status: DeviceStatus,
    pub capabilities: DeviceCapabilities,
}

/// Device capabilities (protocol version, supported features)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DeviceCapabilities {
    pub protocol_version: u32,
    pub supports_file_transfer: bool,
    pub supports_clipboard: bool,
    pub supports_relay: bool,
    pub adapters: Vec<String>,
}

impl Default for DeviceCapabilities {
    fn default() -> Self {
        Self {
            protocol_version: 1,
            supports_file_transfer: false,
            supports_clipboard: false,
            supports_relay: false,
            adapters: vec![
                "git".to_string(),
                "vscode".to_string(),
                "browser".to_string(),
                "terminal".to_string(),
                "runtime".to_string(),
            ],
        }
    }
}

/// Pairing invitation
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PairingInvitation {
    pub code: String,
    pub qr_data: String,
    pub expires_at: DateTimeUtc,
    pub inviting_device_id: String,
    pub inviting_device_name: String,
    pub inviting_device_public_key: String,
}

/// Pairing ceremony state
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Idle,
    Inviting,
    WaitingForVerification,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

/// Pairing session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PairingSession {
    pub id: String,
    pub state: PairingState,
    pub invitation: Option<PairingInvitation>,
    pub safety_number: Option<String>,
    pub created_at: DateTimeUtc,
    pub expires_at: DateTimeUtc,
}

impl PairingSession {
    pub fn new(invitation: PairingInvitation, safety_number: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            state: PairingState::WaitingForVerification,
            invitation: Some(invitation),
            safety_number: Some(safety_number),
            created_at: DateTimeUtc::from(Utc::now()),
            expires_at: DateTimeUtc::from(Utc::now() + chrono::Duration::minutes(5)),
        }
    }

    pub fn is_expired(&self) -> bool {
        let now = DateTimeUtc::from(Utc::now());
        now > self.expires_at
    }
}

/// Device discovery info (from mDNS)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DiscoveredDevice {
    pub device_id: String,
    pub name: String,
    pub os: String,
    pub app_version: String,
    pub protocol_version: u32,
    pub addresses: Vec<String>,
    pub port: u16,
    pub capabilities: DeviceCapabilities,
    /// This device's Noise X25519 static public key, base64.
    ///
    /// Noise_IK needs the peer's static key to start a handshake, so a
    /// discovered device without one is unusable. Publishing it in the mDNS TXT
    /// record is what makes the handshake possible at all; the key is
    /// deliberately public, and the safety number derived from it is what the
    /// user actually compares.
    ///
    /// Empty means the peer did not advertise a key -- on an older build, or a
    /// device that lost its key bundle. Callers must treat that as
    /// "cannot connect" rather than falling back to an unauthenticated path.
    #[serde(default)]
    pub static_public_key: String,
    pub last_seen: DateTimeUtc,
}

impl DiscoveredDevice {
    pub fn is_compatible(&self, local_protocol_version: u32) -> bool {
        self.protocol_version <= local_protocol_version
    }
}
