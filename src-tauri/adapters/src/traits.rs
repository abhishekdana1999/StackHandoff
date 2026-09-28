//! Adapter traits and types

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use workspace_clone_core::{manifest::*, Result};

/// Detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub adapter_id: String,
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Portable context captured by adapter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableContext {
    pub adapter_id: String,
    pub data: serde_json::Value,
}

/// Requirement for preflight check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub adapter_id: String,
    pub required: bool,
    pub config: serde_json::Value,
}

/// Preflight check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub requirement_id: String,
    pub status: CheckStatus,
    pub evidence: String,
    pub freshness: chrono::DateTime<chrono::Utc>,
    pub action: Option<RemediationAction>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// A check on this machine established the state.
    ReadyVerified,
    /// The user asserted the state and no safe automatic check could confirm it.
    ///
    /// Kept separate from `ReadyVerified` on purpose: the blueprint requires
    /// user-confirmed to be *labelled* as such and never presented as a machine
    /// result, so the UI can say "you told us" rather than "we checked".
    ReadyUserConfirmed,
    /// Authenticated, but as a different account than the workspace expects.
    ReadyAccountMismatch,
    /// The tool is present but the user is not signed in.
    LoginRequired,
    /// Something is configured, but nothing verified it.
    ConfiguredUnverified,
    /// The check could not be performed. Never a failure claim.
    Unknown,
    /// The check does not apply to this machine.
    NotApplicable,
}

impl CheckStatus {
    /// Whether this status means the requirement is satisfied.
    ///
    /// `NotApplicable` and `Unknown` deliberately do not count: a check that
    /// could not run has not established anything, so treating it as satisfied
    /// would let a report claim readiness it never verified.
    pub fn is_satisfied(&self) -> bool {
        matches!(self, Self::ReadyVerified | Self::ReadyUserConfirmed)
    }

    /// Whether this status is something the user should be shown prominently.
    pub fn needs_attention(&self) -> bool {
        matches!(
            self,
            Self::LoginRequired
                | Self::ReadyAccountMismatch
                | Self::ConfiguredUnverified
                | Self::Unknown
        )
    }

    /// A short label for the UI.
    pub fn label(&self) -> &'static str {
        match self {
            Self::ReadyVerified => "Verified",
            Self::ReadyUserConfirmed => "Confirmed by you",
            Self::ReadyAccountMismatch => "Wrong account",
            Self::LoginRequired => "Sign-in required",
            Self::ConfiguredUnverified => "Unverified",
            Self::Unknown => "Unknown",
            Self::NotApplicable => "Not applicable",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemediationAction {
    pub label: String,
    pub action_type: ActionType,
    pub url: Option<String>,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    OpenUrl,
    RunCommand,
    InstallApp,
    ConfigureSetting,
}

/// Restore action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreAction {
    pub id: String,
    pub action_type: RestoreActionType,
    /// The adapter that knows how to execute this action.
    ///
    /// This is a typed field rather than a key inside `config` because the
    /// executor has to route on it: smuggling it through `config` is how steps
    /// ended up unroutable and failing with "adapter not found".
    pub adapter_id: String,
    pub description: String,
    pub required: bool,
    pub approved: bool,
    pub config: serde_json::Value,
    pub dependencies: Vec<String>,
}

impl RestoreAction {
    /// An action that only records intent and needs no adapter to run, such as
    /// mapping a destination path.
    pub const NO_ADAPTER: &'static str = "";

    pub fn new(
        id: impl Into<String>,
        action_type: RestoreActionType,
        adapter_id: impl Into<String>,
        description: impl Into<String>,
        config: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            action_type,
            adapter_id: adapter_id.into(),
            description: description.into(),
            required: false,
            approved: false,
            config,
            dependencies: Vec::new(),
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn approved(mut self) -> Self {
        self.approved = true;
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RestoreActionType {
    OpenProject,
    OpenApplication,
    OpenUrls,
    OfferCommand,
    CheckGit,
    MapPath,
}

/// Approved restore action for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedRestoreAction {
    pub action: RestoreAction,
    pub resolved_config: serde_json::Value,
}

/// Action execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub action_id: String,
    pub status: ActionStatus,
    pub message: String,
    pub duration_ms: u64,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Success,
    Skipped,
    Failed,
    Manual,
}

/// Adapter trait
#[async_trait]
pub trait WorkspaceAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn version(&self) -> u32;
    fn supported_platforms(&self) -> Vec<&'static str>;

    async fn detect(&self, context: &LocalContext) -> Result<DetectionResult>;
    async fn capture(
        &self,
        context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext>;
    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>>;
    async fn plan_restore(&self, context: &PortableContext) -> Result<Vec<RestoreAction>>;
    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult>;
}

/// Local context for detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalContext {
    pub os: String,
    pub os_version: String,
    pub home_dir: String,
    pub config_dirs: Vec<String>,
}

impl LocalContext {
    /// Build the context for the machine this process is running on.
    pub fn current() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            os_version: os_version(),
            home_dir: dirs::home_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            config_dirs: default_config_dirs(),
        }
    }

    /// Expand a path relative to the user's home directory.
    ///
    /// Adapters take absolute paths from the selection, so this is only used
    /// for well-known application locations.
    pub fn in_home(&self, relative: impl AsRef<std::path::Path>) -> std::path::PathBuf {
        std::path::Path::new(&self.home_dir).join(relative)
    }
}

/// A project the user has chosen to include in a capture.
///
/// The `source_path` is the absolute location on the capturing device. It is
/// used to run the git and editor adapters and is recorded in the manifest only
/// as a non-identifying hint, never as a live path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedProject {
    pub id: String,
    pub name: String,
    pub source_path: String,
    /// Logical bucket the destination should map this project into, such as
    /// `code` or `work`. Keeps the manifest portable across differing layouts.
    pub destination_location_id: String,
}

/// A command the user has explicitly approved for transfer.
///
/// The blueprint forbids automatically replaying history, so these are typed
/// by the user and offered on the destination rather than captured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovedCommand {
    pub label: String,
    pub command: String,
    pub working_directory: Option<String>,
}

/// Everything the user chose to include in a capture.
///
/// The lists are the authority: there are no parallel boolean flags, because a
/// flag and an empty list can disagree and the adapters would have to guess
/// which one wins.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureSelection {
    pub projects: Vec<SelectedProject>,
    /// Adapter ids whose captured context should be included.
    pub include_applications: Vec<String>,
    /// URLs the user pasted in. Browser history is never read.
    pub browser_urls: Vec<String>,
    /// Terminal working directories, normally the selected projects.
    pub terminal_dirs: Vec<String>,
    /// Commands the user chose to offer on the destination.
    pub terminal_commands: Vec<ApprovedCommand>,
    /// Environment variable *names* only; values are never captured.
    pub env_var_names: Vec<String>,
    pub policy: Policy,
}

impl CaptureSelection {
    /// Whether the given adapter's context should be captured.
    pub fn includes(&self, adapter_id: &str) -> bool {
        self.include_applications.iter().any(|id| id == adapter_id)
    }

    /// The subset of selected projects that are git repositories.
    pub fn project_at(&self, project_id: &str) -> Option<&SelectedProject> {
        self.projects.iter().find(|p| p.id == project_id)
    }
}

/// Adapter registry
pub struct AdapterRegistry {
    adapters: HashMap<String, Box<dyn WorkspaceAdapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    pub fn register(&mut self, adapter: Box<dyn WorkspaceAdapter>) {
        self.adapters.insert(adapter.id().to_string(), adapter);
    }

    pub fn get(&self, id: &str) -> Option<&dyn WorkspaceAdapter> {
        self.adapters.get(id).map(|a| a.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn WorkspaceAdapter> {
        self.adapters.values().map(|a| a.as_ref()).collect()
    }

    pub async fn detect_all(&self, context: &LocalContext) -> Result<Vec<DetectionResult>> {
        let mut results = Vec::new();
        for adapter in self.adapters.values() {
            if adapter.supported_platforms().contains(&context.os.as_str())
                || adapter.supported_platforms().contains(&"all")
            {
                match adapter.detect(context).await {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        tracing::warn!("Adapter {} detection failed: {}", adapter.id(), e);
                    }
                }
            }
        }
        // Stable order so the capture screen does not reshuffle between loads.
        results.sort_by(|a, b| a.adapter_id.cmp(&b.adapter_id));
        Ok(results)
    }

    /// Run `capture` on the given adapter, returning `None` if it is not
    /// registered.
    pub async fn capture(
        &self,
        adapter_id: &str,
        context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<Option<PortableContext>> {
        let Some(adapter) = self.get(adapter_id) else {
            tracing::warn!("Capture requested for unregistered adapter {adapter_id}");
            return Ok(None);
        };
        Ok(Some(adapter.capture(context, selection).await?))
    }

    /// Run `preflight` on the given adapter, returning `None` if it is not
    /// registered.
    pub async fn preflight(
        &self,
        adapter_id: &str,
        requirements: &[Requirement],
    ) -> Result<Option<Vec<CheckResult>>> {
        let Some(adapter) = self.get(adapter_id) else {
            return Ok(None);
        };
        Ok(Some(adapter.preflight(requirements).await?))
    }

    /// Run `execute` for a planned action, routing on the action's adapter.
    pub async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        if action.action.adapter_id.is_empty() {
            return Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Skipped,
                message: format!("No adapter is needed to satisfy: {}", action.action.description),
                duration_ms: 0,
                details: None,
            });
        }

        let Some(adapter) = self.get(&action.action.adapter_id) else {
            return Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Failed,
                message: format!(
                    "No adapter named '{}' is registered, so '{}' cannot run",
                    action.action.adapter_id, action.action.description
                ),
                duration_ms: 0,
                details: None,
            });
        };

        adapter.execute(action).await
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The OS version string, read from the platform rather than hardcoded.
///
/// Adapters record this in manifests as evidence, so an "unknown" here would
/// silently weaken every portability decision made later.
pub fn os_version() -> String {
    if let Ok(ver) = std::env::var("WINDOWSCLONE_OS_VERSION_OVERRIDE") {
        return ver;
    }
    platform_os_version()
}

/// Per-platform OS version.
#[cfg(target_os = "macos")]
fn platform_os_version() -> String {
    // `sw_vers` is present on every supported macOS and is far more
    // accurate than the kernel version alone.
    if let Ok(out) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    std::env::consts::OS.to_string()
}

/// Windows exposes its real build in the registry, and `cmd /c ver` returns a
/// stripped "10.0.19045" with no service pack. `RUSTVERSION` is read from the
/// environment rather than compiled in, because the OS is upgraded underneath a
/// running app and a baked-in constant would go stale.
#[cfg(target_os = "windows")]
fn platform_os_version() -> String {
    match std::env::var("RUSTVERSION") {
        Ok(build) if !build.trim().is_empty() => {
            format!("Windows (build {})", build.trim())
        }
        _ => "Windows".to_string(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_os_version() -> String {
    if let Ok(out) = std::process::Command::new("uname").arg("-r").output() {
        if out.status.success() {
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    std::env::consts::OS.to_string()
}

/// Well-known per-user configuration directories for the running platform.
pub fn default_config_dirs() -> Vec<String> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        let support = home.join("Library/Application Support");
        dirs.push(support.join("Code/User"));
        dirs.push(support.join("com.microsoft.VSCode"));
        dirs.push(support.join("GitHub Desktop"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let appdata = std::path::PathBuf::from(appdata);
            dirs.push(appdata.join("Code/User"));
            dirs.push(appdata.join("GitHub Desktop"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
            let config = std::path::PathBuf::from(config);
            dirs.push(config.join("Code/User"));
        } else {
            dirs.push(home.join(".config/Code/User"));
        }
        dirs.push(home.join(".config/Code/User/globalStorage/storage.json"));
    }

    dirs.retain(|d| d.exists());
    dirs.into_iter()
        .map(|d| d.to_string_lossy().to_string())
        .collect()
}
