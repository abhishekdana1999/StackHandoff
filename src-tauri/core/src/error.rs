//! Error types for StackHandoff

use thiserror::Error;

#[derive(Error, Debug)]
pub enum WorkspaceError {
    #[error("Manifest validation error: {0}")]
    ManifestValidation(String),

    #[error("Secret detected in field '{field}': {pattern}")]
    SecretDetected { field: String, pattern: String },

    #[error("Schema version {found} not supported (max: {max})")]
    SchemaVersionMismatch { found: u32, max: u32 },

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device already paired: {0}")]
    DeviceAlreadyPaired(String),

    #[error("Pairing invitation expired")]
    PairingExpired,

    #[error("Pairing verification failed: {0}")]
    PairingVerificationFailed(String),

    #[error("Crypto error: {0}")]
    Crypto(#[from] CryptoError),

    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("Adapter error: {0}")]
    Adapter(#[from] AdapterError),

    #[error("Preflight error: {0}")]
    Preflight(#[from] PreflightError),

    #[error("Restore error: {0}")]
    Restore(#[from] RestoreError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("File transfer failed: {0}")]
    Files(String),
}

impl serde::Serialize for WorkspaceError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Encryption failed: {0}")]
    Encryption(String),

    #[error("Decryption failed: {0}")]
    Decryption(String),

    #[error("Key storage failed: {0}")]
    KeyStorage(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),

    #[error("Noise protocol error: {0}")]
    NoiseProtocol(String),
}

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Migration failed: {0}")]
    Migration(String),

    #[error("Query failed: {0}")]
    Query(String),

    #[error("Record not found: {0}")]
    NotFound(String),

    #[error("Constraint violation: {0}")]
    Constraint(String),

    #[error("Encryption error: {0}")]
    Encryption(String),
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Discovery failed: {0}")]
    Discovery(String),

    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("Handshake failed: {0}")]
    Handshake(String),

    #[error("Transfer failed: {0}")]
    Transfer(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),
}

#[derive(Error, Debug)]
pub enum AdapterError {
    #[error("Detection failed: {0}")]
    Detection(String),

    #[error("Capture failed: {0}")]
    Capture(String),

    #[error("Preflight check failed: {0}")]
    PreflightCheck(String),

    #[error("Restore action failed: {0}")]
    RestoreAction(String),

    #[error("Unsupported adapter: {0}")]
    Unsupported(String),
}

#[derive(Error, Debug)]
pub enum PreflightError {
    #[error("Check execution failed: {0}")]
    Execution(String),

    #[error("Identity check failed: {0}")]
    IdentityCheck(String),

    #[error("Environment check failed: {0}")]
    EnvironmentCheck(String),
}

#[derive(Error, Debug)]
pub enum RestoreError {
    #[error("Plan generation failed: {0}")]
    PlanGeneration(String),

    #[error("Action execution failed: {0}")]
    ActionExecution(String),

    #[error("Git operation failed: {0}")]
    GitOperation(String),

    #[error("File transfer failed: {0}")]
    FileTransfer(String),

    #[error("Command execution failed: {0}")]
    CommandExecution(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Approval required: {0}")]
    ApprovalRequired(String),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;
