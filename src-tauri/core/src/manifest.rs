//! Workspace manifest types and validation

use crate::device::DateTimeUtc;
use crate::error::{Result, WorkspaceError};
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Current manifest schema version
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Workspace manifest - the portable description of a work session
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceManifest {
    pub schema_version: u32,
    pub workspace: WorkspaceMeta,
    pub projects: Vec<Project>,
    pub applications: Vec<Application>,
    pub requirements: Requirements,
    pub restore: RestorePlan,
    pub policy: Policy,
}

/// Workspace metadata
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceMeta {
    pub id: String,
    pub name: String,
    pub captured_at: DateTimeUtc,
    pub source_device: DeviceRef,
    pub portability: Portability,
}

/// Reference to source device (opaque ID for privacy)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DeviceRef {
    pub id: String,
    pub os: String,
    pub os_version: String,
}

/// Portability classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Portability {
    CrossPlatform,
    WindowsOnly,
    MacosOnly,
    LinuxOnly,
}

/// Project/repository information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub source_path_hint: String,
    pub destination_location_id: String,
    pub git: Option<GitInfo>,
}

/// Git repository information
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct GitInfo {
    pub remote_hint: Option<String>,
    pub branch: String,
    pub commit: Option<String>,
    pub dirty_worktree: bool,
    pub dirty_state_captured: bool,
}

/// Application to open/restore
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Application {
    pub id: String,
    pub adapter: String,
    pub project_id: Option<String>,
    pub required: bool,
    #[serde(flatten)]
    pub config: serde_json::Value,
}

/// Requirements for the destination
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Requirements {
    pub applications: Vec<AppRequirement>,
    pub runtimes: Vec<RuntimeRequirement>,
    pub cli_tools: Vec<CliToolRequirement>,
    pub identities: Vec<IdentityRequirement>,
    pub environment: EnvironmentRequirement,
    pub services: Vec<ServiceRequirement>,
}

/// Application requirement
///
/// The adapter is what makes this checkable. An earlier version of this schema
/// carried only an opaque `id`, which meant the preflight engine had no way to
/// route the check to an adapter and every application check failed as
/// "adapter not available". The new fields default so older manifests still
/// load.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AppRequirement {
    pub id: String,
    pub required: bool,
    /// Adapter id, for example `vscode`. Empty for an app no adapter covers.
    #[serde(default)]
    pub adapter: String,
    /// The adapter version the capture device used, for compatibility checks.
    #[serde(default)]
    pub captured_version: Option<u32>,
    /// Adapter-specific expectation, such as the VS Code extension list.
    #[serde(default)]
    pub config: serde_json::Value,
}

impl AppRequirement {
    /// A requirement that can actually be dispatched to an adapter.
    pub fn new(id: impl Into<String>, adapter: impl Into<String>, required: bool) -> Self {
        Self {
            id: id.into(),
            required,
            adapter: adapter.into(),
            captured_version: None,
            config: serde_json::Value::Null,
        }
    }

    /// Whether this requirement names an adapter that can check it.
    pub fn is_checkable(&self) -> bool {
        !self.adapter.is_empty()
    }
}

/// Runtime requirement (Node, Python, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RuntimeRequirement {
    pub name: String,
    pub version: String,
    pub required: bool,
}

/// CLI tool requirement
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CliToolRequirement {
    pub name: String,
    pub version: Option<String>,
    pub required: bool,
}

/// Identity service requirement
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct IdentityRequirement {
    pub service: String,
    pub account_hint: Option<String>,
    pub project_hint: Option<String>,
    pub verification: String,
    pub required: bool,
}

/// Environment variable requirements
///
/// `presence_only` is the whole contract: Workspace Clone learns which *names*
/// matter and nothing about their values. `values_included` is retained only so
/// a hostile manifest can be detected and rejected rather than silently obeyed
/// (see `Requirements::validate`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct EnvironmentRequirement {
    pub presence_only: Vec<String>,
    #[serde(default)]
    pub values_included: bool,
}

impl Default for EnvironmentRequirement {
    fn default() -> Self {
        Self {
            presence_only: Vec::new(),
            // The safe reading is the default, so a manifest that omits the flag
            // cannot accidentally authorise value transfer.
            values_included: false,
        }
    }
}

/// Local service requirement
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ServiceRequirement {
    pub name: String,
    pub required: bool,
    /// The port the service listens on.
    ///
    /// A service check cannot be meaningful without a port, so an old manifest
    /// that omits this produces a check that reports `unknown` rather than a
    /// silently wrong pass.
    #[serde(default)]
    pub port: Option<u16>,
    /// Defaults to loopback. A non-local host is refused by the service check.
    #[serde(default = "default_service_host")]
    pub host: String,
}

fn default_service_host() -> String {
    "localhost".to_string()
}

impl ServiceRequirement {
    pub fn new(name: impl Into<String>, port: u16, required: bool) -> Self {
        Self {
            name: name.into(),
            required,
            port: Some(port),
            host: default_service_host(),
        }
    }

    /// Whether the service carries enough information to be checked.
    pub fn is_checkable(&self) -> bool {
        self.port.is_some()
    }
}

/// Restore plan steps
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct RestorePlan {
    pub steps: Vec<RestoreStep>,
}

/// Restore step types
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RestoreStep {
    OpenProject {
        project_id: String,
    },
    OpenApplication {
        application_id: String,
    },
    OpenUrls {
        application_id: String,
    },
    OfferCommand {
        recipe_id: String,
        approval_required: bool,
    },
    CheckGit {
        project_id: String,
    },
    MapPath {
        project_id: String,
    },
}

/// Policy settings
///
/// The defaults are the most restrictive values the type allows. A caller that
/// forgets to set a field therefore gets the safe behaviour rather than an
/// accidental opt-in to file or clipboard transfer.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct Policy {
    pub file_transfer: FileTransferPolicy,
    pub clipboard: ClipboardPolicy,
    pub automatic_command_execution: bool,
    pub secret_values_included: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            file_transfer: FileTransferPolicy::None,
            clipboard: ClipboardPolicy::Excluded,
            automatic_command_execution: false,
            secret_values_included: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FileTransferPolicy {
    None,
    Explicit,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClipboardPolicy {
    Excluded,
    OptIn,
    Included,
}

impl Default for Requirements {
    fn default() -> Self {
        Self {
            applications: Vec::new(),
            runtimes: Vec::new(),
            cli_tools: Vec::new(),
            identities: Vec::new(),
            environment: EnvironmentRequirement::default(),
            services: Vec::new(),
        }
    }
}

impl Default for WorkspaceManifest {
    fn default() -> Self {
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            workspace: WorkspaceMeta {
                id: Uuid::new_v4().to_string(),
                name: "Untitled Workspace".to_string(),
                captured_at: DateTimeUtc::from(Utc::now()),
                source_device: DeviceRef {
                    id: "local".to_string(),
                    os: std::env::consts::OS.to_string(),
                    os_version: "unknown".to_string(),
                },
                portability: Portability::CrossPlatform,
            },
            projects: Vec::new(),
            applications: Vec::new(),
            requirements: Requirements {
                applications: Vec::new(),
                runtimes: Vec::new(),
                cli_tools: Vec::new(),
                identities: Vec::new(),
                environment: EnvironmentRequirement {
                    presence_only: Vec::new(),
                    values_included: false,
                },
                services: Vec::new(),
            },
            restore: RestorePlan { steps: Vec::new() },
            policy: Policy {
                file_transfer: FileTransferPolicy::None,
                clipboard: ClipboardPolicy::Excluded,
                automatic_command_execution: false,
                secret_values_included: false,
            },
        }
    }
}

impl WorkspaceManifest {
    /// Validate the manifest
    pub fn validate(&self) -> Result<()> {
        // Check schema version
        if self.schema_version > MANIFEST_SCHEMA_VERSION {
            return Err(WorkspaceError::SchemaVersionMismatch {
                found: self.schema_version,
                max: MANIFEST_SCHEMA_VERSION,
            });
        }

        // Validate required fields
        if self.workspace.id.is_empty() {
            return Err(WorkspaceError::ManifestValidation(
                "Workspace ID is required".to_string(),
            ));
        }
        if self.workspace.name.is_empty() {
            return Err(WorkspaceError::ManifestValidation(
                "Workspace name is required".to_string(),
            ));
        }

        // Validate no secret values in environment
        if self.requirements.environment.values_included {
            return Err(WorkspaceError::ManifestValidation(
                "Environment values must not be included".to_string(),
            ));
        }

        // Validate no auto command execution
        if self.policy.automatic_command_execution {
            return Err(WorkspaceError::ManifestValidation(
                "Automatic command execution not allowed".to_string(),
            ));
        }

        // Validate no secret values included
        if self.policy.secret_values_included {
            return Err(WorkspaceError::ManifestValidation(
                "Secret values must not be included".to_string(),
            ));
        }

        Ok(())
    }

    /// Create a new manifest with defaults
    pub fn new(name: String, source_device: DeviceRef) -> Self {
        Self {
            workspace: WorkspaceMeta {
                id: Uuid::new_v4().to_string(),
                name,
                captured_at: DateTimeUtc::from(Utc::now()),
                source_device,
                portability: Portability::CrossPlatform,
            },
            ..Default::default()
        }
    }
}

/// Secret patterns for scrubbing
pub static SECRET_PATTERNS: &[&str] = &[
    r"(?i)(password|passwd|pwd)\s*[:=]\s*\S+",
    r"(?i)(api[_-]?key|apikey)\s*[:=]\s*\S+",
    r"(?i)(secret|token)\s*[:=]\s*\S+",
    r"(?i)(private[_-]?key)\s*[:=]\s*\S+",
    r"(?i)(aws[_-]?secret)\s*[:=]\s*\S+",
    r"(?i)(github[_-]?token)\s*[:=]\s*\S+",
    r"-----BEGIN (RSA |EC |OPENSSH )?PRIVATE KEY-----",
    r"ssh-(rsa|ed25519|ecdsa) AAAA",
    r"gh[pousr]_[A-Za-z0-9_]{36,}",
    r"sk-[A-Za-z0-9]{48,}",
    r"xox[baprs]-[A-Za-z0-9-]{10,}",
];

/// Scrub secrets from a string
pub fn scrub_secrets(input: &str) -> String {
    let mut result = input.to_string();
    for pattern in SECRET_PATTERNS {
        let regex = regex::Regex::new(pattern).ok();
        if let Some(re) = regex {
            result = re.replace_all(&result, "[REDACTED]").to_string();
        }
    }
    result
}

/// Strip credentials from a URL.
///
/// Removes the userinfo component, the query and the fragment. All three are
/// places a token is routinely put, and none of them is needed to identify a
/// remote: `https://github.com/org/repo` is enough.
///
/// A string that does not parse as a URL is still scrubbed if it has the shape of
/// one, because a remote that is *almost* a URL is exactly the case where
/// returning it unchanged would leak `https://user:token@host/...`. Anything with
/// no `://` is returned as-is, since a bare path or a non-URL string has no
/// userinfo to remove.
pub fn scrub_url(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url) {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.query_pairs_mut().clear();
        parsed.set_fragment(None);

        // `Url` keeps the separators once emptied, so
        // `https://host/repo?token=x#y` comes back as `https://host/repo?`. A
        // remote hint with a trailing `?` is not a remote anyone can use, and
        // leaving it makes two identical remotes compare as different strings.
        let scrubbed = parsed.to_string();
        return scrubbed
            .trim_end_matches('?')
            .trim_end_matches('#')
            .to_string();
    }

    if !url.contains("://") {
        return url.to_string();
    }

    // Unparseable but URL-shaped. Cut everything from the `//` up to the next
    // `/`, `?` or `#` that follows the authority, which is exactly the userinfo.
    let Some(scheme_end) = url.find("://").map(|i| i + 3) else {
        return url.to_string();
    };
    let authority_end = url[scheme_end..]
        .find(['/', '?', '#'])
        .map(|i| scheme_end + i)
        .unwrap_or(url.len());

    let (authority, rest) = url.split_at(authority_end);
    match authority.rsplit_once('@') {
        // The part before `@` is userinfo, and it is all removed.
        Some((_, host)) => format!("{}{}{}", &url[..scheme_end], host, rest),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_default() {
        let manifest = WorkspaceManifest::default();
        assert_eq!(manifest.schema_version, MANIFEST_SCHEMA_VERSION);
        assert!(!manifest.policy.automatic_command_execution);
        assert!(!manifest.policy.secret_values_included);
    }

    #[test]
    fn test_scrub_url() {
        let url = "https://user:pass@github.com/org/repo?token=secret";
        let scrubbed = scrub_url(url);
        assert!(!scrubbed.contains("user"));
        assert!(!scrubbed.contains("pass"));
        assert!(!scrubbed.contains("token"));
    }

    /// A fragment is a place a token is routinely put, and it is not needed to
    /// identify a remote.
    #[test]
    fn a_fragment_token_is_removed() {
        let scrubbed = scrub_url("https://github.com/org/repo#access_token=abc123");
        assert!(!scrubbed.contains("abc123"), "got {scrubbed}");
        assert!(scrubbed.starts_with("https://github.com/org/repo"), "got {scrubbed}");
    }

    /// Clearing the query used to leave a trailing `?`, so two remotes that were
    /// the same remote compared as different strings.
    #[test]
    fn an_emptied_query_leaves_no_dangling_separator() {
        let scrubbed = scrub_url("https://user:pass@github.com/org/repo?token=secret");
        assert!(!scrubbed.contains('?'), "got {scrubbed}");
        assert!(!scrubbed.contains('#'), "got {scrubbed}");
        assert_eq!(scrubbed, "https://github.com/org/repo");
    }

    /// Two spellings of the same remote must scrub to the same string, or a
    /// manifest digest comparison will disagree with itself.
    #[test]
    fn the_same_remote_scrubs_to_one_value() {
        let a = scrub_url("https://github.com/org/repo");
        let b = scrub_url("https://token:x-oauth-basic@github.com/org/repo?x=1");
        assert_eq!(a, b, "got {a} and {b}");
    }

    /// A remote that does not quite parse is the case where returning the input
    /// unchanged leaks a credential.
    #[test]
    fn an_unparseable_url_shaped_string_still_loses_its_userinfo() {
        let scrubbed = scrub_url("ssh://git@github.com/org/repo");
        assert!(!scrubbed.contains("git@"), "got {scrubbed}");
    }

    /// Nothing that is not URL-shaped is touched: a filesystem path has no
    /// userinfo, and mangling it would lose information for nothing.
    #[test]
    fn a_non_url_is_returned_unchanged() {
        for plain in ["/Users/someone/code/repo", "~/code/repo", "not a url", ""] {
            assert_eq!(scrub_url(plain), plain);
        }
    }

    #[test]
    fn test_scrub_secrets() {
        let input = "password=secret123 api_key=abc123 normal=value";
        let scrubbed = scrub_secrets(input);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("secret123"));
        assert!(!scrubbed.contains("abc123"));
    }
}
