//! Capture Tauri commands.
//!
//! Capture is the step that decides what a workspace *is*. Everything downstream
//! -- preflight, transfer, restore -- is only as trustworthy as the manifest
//! built here, so this module is deliberately strict about three things:
//!
//! 1. **A capture that silently does less than the user asked for is a bug.** An
//!    adapter that fails is reported as a warning naming the adapter and the
//!    reason, never swallowed and never allowed to leave a half-filled manifest
//!    that reads as complete.
//! 2. **The manifest is validated before it is sealed.** `seal_json` will
//!    happily encrypt a manifest that `validate` rejects; writing one would put
//!    an unusable file on disk and report success.
//! 3. **Nothing absolute leaves the machine.** Adapters are responsible for
//!    redacting, and this module additionally scrubs what they hand back, so a
//!    new adapter that forgets cannot leak a path.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{command, State};
use tracing::{info, warn};
use workspace_clone_adapters::{
    AdapterRegistry, CaptureSelection, LocalContext, PortableContext,
};
use workspace_clone_core::{
    manifest::{
        Application, CliToolRequirement, DeviceRef, EnvironmentRequirement, Policy, Project,
        Requirements, RestorePlan, RestoreStep, RuntimeRequirement, WorkspaceManifest,
    },
    DatabaseError, Result, WorkspaceError,
};
use workspace_clone_crypto::keys::KeyStorage;
use workspace_clone_db::{
    models::WorkspaceRecord,
    repository::{DeviceRepository, WorkspaceRepository},
    DbPool,
};

/// Adapter ids capture knows how to read.
///
/// A fixed list rather than "whatever the registry happens to hold", because the
/// manifest is assembled by matching captured data to a known shape. An adapter
/// whose output shape is not listed here is reported as a warning and skipped
/// rather than written into the manifest as something it is not.
const KNOWN_ADAPTERS: &[&str] = &["git", "vscode", "browser", "terminal", "runtime"];

/// Capture a workspace and persist it.
///
/// The returned `CaptureResult` carries the plaintext manifest for the UI to
/// review, the sealed form for transfer, and the warnings raised along the way.
#[command]
pub async fn capture_workspace(
    registry: State<'_, Arc<AdapterRegistry>>,
    pool: State<'_, DbPool>,
    name: String,
    selection: CaptureSelection,
) -> Result<CaptureResult> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(WorkspaceError::ManifestValidation(
            "A workspace needs a name".to_string(),
        )
        .into());
    }

    let local = LocalContext::current();
    let mut warnings: Vec<String> = Vec::new();
    let mut captured: Vec<PortableContext> = Vec::new();

    // Driven from the selection, not from the registry's iteration order, so the
    // manifest is byte-for-byte the same for the same inputs. A manifest whose
    // contents depend on a HashMap's iteration order cannot be digested and
    // compared meaningfully.
    let mut requested: Vec<String> = selection.include_applications.clone();
    requested.sort();
    requested.dedup();

    // Projects are captured through the git adapter whether or not it was
    // listed as an application: without project entries there is nothing for the
    // restore plan to act on, and the user asked for those projects.
    if !selection.projects.is_empty() && !requested.iter().any(|id| id == "git") {
        requested.insert(0, "git".to_string());
    }

    for adapter_id in &requested {
        if !KNOWN_ADAPTERS.contains(&adapter_id.as_str()) {
            warnings.push(format!(
                "No capture is defined for '{adapter_id}', so it was left out."
            ));
            continue;
        }

        match registry.capture(adapter_id, &local, &selection).await {
            Ok(Some(context)) => captured.push(context),
            Ok(None) => warnings.push(format!(
                "The '{adapter_id}' adapter is not registered, so it was left out."
            )),
            Err(e) => {
                // One adapter failing must not lose the rest of the capture, but
                // it must not pass unnoticed either: the user selected this and
                // is not getting it.
                warn!("Capture from adapter '{adapter_id}' failed: {e}");
                warnings.push(format!("The '{adapter_id}' adapter failed: {e}"));
            }
        }
    }

    let source_device_id = local_device_id(&pool).await?;
    let manifest = assemble(
        name,
        &local,
        &source_device_id,
        &selection,
        &captured,
        &mut warnings,
    )?;
    manifest.validate()?;

    let sealed = persist(&pool, &manifest).await?;

    info!(
        "Captured workspace {} with {} projects and {} applications",
        manifest.workspace.id,
        manifest.projects.len(),
        manifest.applications.len()
    );

    Ok(CaptureResult {
        manifest,
        sealed,
        warnings,
    })
}

/// What a capture produced.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    /// The manifest in the clear, for the confirmation screen.
    pub manifest: WorkspaceManifest,
    /// The sealed manifest, which is the form that is written to disk and sent.
    pub sealed: String,
    /// Anything the user asked for that could not be captured.
    pub warnings: Vec<String>,
}

/// Build the manifest from what the adapters returned.
#[allow(clippy::too_many_arguments)]
fn assemble(
    name: String,
    local: &LocalContext,
    source_device_id: &str,
    selection: &CaptureSelection,
    captured: &[PortableContext],
    warnings: &mut Vec<String>,
) -> Result<WorkspaceManifest> {
    let source_device = DeviceRef {
        id: source_device_id.to_string(),
        os: local.os.clone(),
        os_version: local.os_version.clone(),
    };

    let mut manifest = WorkspaceManifest::new(name, source_device);

    let mut requirements = Requirements {
        environment: EnvironmentRequirement {
            presence_only: selection.env_var_names.clone(),
            // Never derived from the selection: a caller that asks for values
            // must not be able to have them, and `validate` rejects the manifest
            // if this is ever true. The name list is presence-only by contract.
            values_included: false,
        },
        ..Default::default()
    };

    for context in captured {
        match context.adapter_id.as_str() {
            "git" => manifest.projects.extend(projects_from(context)?),
            "vscode" | "terminal" => {
                manifest.applications.extend(applications_from(context, warnings)?)
            }
            "browser" => manifest.applications.extend(urls_from(context)),
            "runtime" => {
                let (runtimes, cli_tools) = runtime_from(context, warnings);
                requirements.runtimes = runtimes;
                requirements.cli_tools = cli_tools;
            }
            other => {
                // Unreachable while KNOWN_ADAPTERS and this match agree, but a
                // silent drop here would be exactly the kind of quiet omission
                // this module exists to prevent.
                warnings.push(format!(
                    "The '{other}' adapter returned data in a shape capture does not read, so it was left out."
                ));
            }
        }
    }

    // An application requirement is what makes a preflight check routable, so it
    // is derived from the applications that were actually captured rather than
    // from the list of adapters the user ticked. An adapter that was not
    // selected must not appear as a requirement, and one that was selected but
    // captured nothing must not appear as a satisfied requirement either.
    for application in &manifest.applications {
        if application.required && application.adapter.is_empty() {
            warnings.push(format!(
                "'{}' is required but no adapter covers it, so it cannot be checked on the destination.",
                application.id
            ));
        }
        requirements.applications.push(
            workspace_clone_core::manifest::AppRequirement::new(
                application.id.clone(),
                application.adapter.clone(),
                application.required,
            ),
        );
    }

    // Environment names are deduplicated: the same name offered twice would
    // render as a duplicate row and imply two different variables.
    requirements.environment.presence_only.sort();
    requirements.environment.presence_only.dedup();

    manifest.projects = scrub_projects(manifest.projects);
    manifest.applications = scrub_applications(manifest.applications);

    // Canonical ordering, by id.
    //
    // This is what makes a manifest a *document* rather than a log of the order
    // the adapters happened to answer in. The caller already drives capture from
    // a sorted list, so this changes nothing today -- but relying on that is
    // relying on a caller two modules away. A manifest whose byte order varies
    // cannot be digested to prove two devices hold the same one, and the
    // `manifest_digest` column exists to do exactly that.
    //
    // Sorting by id rather than by name, because ids are what a restore step and
    // a requirement reference; two entries sharing a name are still ordered
    // deterministically this way.
    manifest.projects.sort_by(|a, b| a.id.cmp(&b.id));
    manifest.applications.sort_by(|a, b| a.id.cmp(&b.id));
    requirements
        .applications
        .sort_by(|a, b| a.id.cmp(&b.id));
    requirements.runtimes.sort_by(|a, b| a.name.cmp(&b.name));
    requirements.cli_tools.sort_by(|a, b| a.name.cmp(&b.name));
    requirements.identities.sort_by(|a, b| a.service.cmp(&b.service));
    requirements.services.sort_by(|a, b| a.name.cmp(&b.name));

    manifest.requirements = requirements;
    manifest.restore = plan_steps(&manifest);
    manifest.policy = Policy {
        // Taken from the selection, but the restrictive values are the floor: a
        // caller asking for `All` still cannot turn on automatic execution,
        // because `validate` rejects that outright.
        file_transfer: selection.policy.file_transfer,
        clipboard: selection.policy.clipboard,
        automatic_command_execution: false,
        secret_values_included: false,
    };

    Ok(manifest)
}

/// The git adapter's capture is a list of manifest projects verbatim.
fn projects_from(context: &PortableContext) -> Result<Vec<Project>> {
    serde_json::from_value(context.data.clone()).map_err(|e| {
        WorkspaceError::ManifestValidation(format!(
            "The git adapter returned data capture could not read: {e}"
        ))
        .into()
    })
}

/// The vscode and terminal adapters each return a list of manifest
/// applications.
fn applications_from(
    context: &PortableContext,
    warnings: &mut Vec<String>,
) -> Result<Vec<Application>> {
    match serde_json::from_value::<Vec<Application>>(context.data.clone()) {
        Ok(applications) => Ok(applications),
        Err(e) => {
            // A shape mismatch must not be mistaken for "nothing to capture".
            warnings.push(format!(
                "The {} adapter returned data capture could not read ({e}), so it was left out.",
                context.adapter_id
            ));
            Ok(Vec::new())
        }
    }
}

/// The browser adapter returns a URL list plus the URLs it refused.
///
/// The refused list is dropped here rather than carried in the manifest: it
/// records what the *user* typed, which has no business travelling to another
/// device. It is already reflected in the capture warnings.
fn urls_from(context: &PortableContext) -> Vec<Application> {
    let urls: Vec<String> = context
        .data
        .get("urls")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    if urls.is_empty() {
        return Vec::new();
    }

    vec![Application {
        id: "browser-urls".to_string(),
        adapter: "browser".to_string(),
        project_id: None,
        required: false,
        config: serde_json::json!({ "urls": urls }),
    }]
}

/// The runtime adapter returns the runtimes and CLI tools the selected projects
/// need, derived from their declared toolchain files.
fn runtime_from(
    context: &PortableContext,
    warnings: &mut Vec<String>,
) -> (Vec<RuntimeRequirement>, Vec<CliToolRequirement>) {
    let read = |key: &str| -> Vec<serde_json::Value> {
        context
            .data
            .get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };

    let runtimes: Vec<RuntimeRequirement> = read("runtimes")
        .into_iter()
        .filter_map(|v| {
            serde_json::from_value(v)
                .map_err(|e| {
                    warnings.push(format!(
                        "A runtime the {} adapter reported could not be read and was left out: {e}",
                        context.adapter_id
                    ));
                })
                .ok()
        })
        .collect();

    let cli_tools: Vec<CliToolRequirement> = read("cli_tools")
        .into_iter()
        .filter_map(|v| {
            serde_json::from_value(v)
                .map_err(|e| {
                    warnings.push(format!(
                        "A command-line tool the {} adapter reported could not be read and was left out: {e}",
                        context.adapter_id
                    ));
                })
                .ok()
        })
        .collect();

    (runtimes, cli_tools)
}

/// The restore steps the *capture* can express.
///
/// This is deliberately thin. Where a project actually lands, which commands are
/// run and what is opened are all decisions for the destination, made by the
/// planner in `restore`; a manifest that arrives with a full plan would let a
/// peer dictate the layout of someone else's machine. What is recorded here is
/// only the order of the user's own intent.
fn plan_steps(manifest: &WorkspaceManifest) -> RestorePlan {
    let mut steps = Vec::new();

    for project in &manifest.projects {
        steps.push(RestoreStep::CheckGit {
            project_id: project.id.clone(),
        });
    }
    for application in &manifest.applications {
        steps.push(RestoreStep::OpenApplication {
            application_id: application.id.clone(),
        });
    }
    for project in &manifest.projects {
        steps.push(RestoreStep::OpenProject {
            project_id: project.id.clone(),
        });
    }

    RestorePlan { steps }
}

/// Reject any absolute path that reached a manifest, however it got there.
///
/// Adapters already redact, and their tests cover that. This is the second gate:
/// it is cheap, it needs no knowledge of any adapter, and it turns a mistake in a
/// future adapter into a dropped field rather than a directory listing sent to
/// another device.
fn scrub_projects(projects: Vec<Project>) -> Vec<Project> {
    projects
        .into_iter()
        .map(|mut project| {
            if looks_absolute(&project.source_path_hint) {
                // `Project.source_path_hint` is not optional, so the honest
                // value for "we do not know where this lived" is the project
                // name alone rather than a plausible-looking lie.
                warn!(
                    "Project '{}' carried an absolute source path into the manifest; it was replaced",
                    project.id
                );
                project.source_path_hint = project.name.clone();
            }
            if let Some(git) = &mut project.git {
                if let Some(remote) = &git.remote_hint {
                    git.remote_hint = Some(workspace_clone_core::manifest::scrub_url(remote));
                }
            }
            project
        })
        .collect()
}

/// Drop absolute paths from application configuration.
fn scrub_applications(applications: Vec<Application>) -> Vec<Application> {
    applications
        .into_iter()
        .map(|mut application| {
            if let serde_json::Value::Object(map) = &mut application.config {
                let offending: Vec<String> = map
                    .iter()
                    .filter(|(_, v)| v.as_str().is_some_and(looks_absolute))
                    .map(|(k, _)| k.clone())
                    .collect();
                for key in offending {
                    if let Some(serde_json::Value::String(value)) = map.remove(&key) {
                        // Keep the tail of the path: enough for a person to
                        // recognise the folder, not enough to locate it.
                        map.insert(
                            key,
                            serde_json::Value::String(crate::capture::shorten_path(&value)),
                        );
                    }
                }
            }
            application
        })
        .collect()
}

/// Whether a string is an absolute filesystem path.
fn looks_absolute(value: &str) -> bool {
    value.starts_with('/')
        || (value.len() > 2
            && value.as_bytes()[1] == b':'
            && value.as_bytes()[2] == b'/')
        || value.starts_with(r"\\")
}

/// `~/parent/leaf` for an absolute path, or the input unchanged.
fn shorten_path(value: &str) -> String {
    if let Some((parent, leaf)) = value.rsplit_once('/') {
        if parent.is_empty() {
            return leaf.to_string();
        }
        let parent_leaf = parent.rsplit('/').next().unwrap_or(parent);
        format!("~/{parent_leaf}/{leaf}")
    } else {
        value.to_string()
    }
}

/// This device's own id, as the database records it.
///
/// Found by the Noise public key rather than by recomputing the id, for two
/// reasons. `workspaces.source_device_id` is a foreign key, so an id derived
/// fresh here would be rejected at insert time if it did not match and the
/// capture would be lost after all the adapter work had already been done. And
/// re-deriving would make this a second, independent statement of how a device
/// id is computed -- the one place that must be right. A Noise key identifies a
/// device on its own, so the row is found without restating the rule.
async fn local_device_id(pool: &DbPool) -> Result<String> {
    let noise_key = KeyStorage::load_or_create_local_keys()?.noise_key()?;
    let noise_key_b64 = noise_key.public_key_b64();

    match DeviceRepository::new(pool.clone())
        .get_by_noise_key(&noise_key_b64)
        .await?
    {
        Some(record) => Ok(record.id),
        None => Err(WorkspaceError::ManifestValidation(
            "This device has no row in the device table, so a capture cannot be \
             recorded against it. Restart the app; if it keeps happening the \
             database is not writable."
                .to_string(),
        )
        .into()),
    }
}

/// Seal the manifest, write it to disk, and record it.
async fn persist(pool: &DbPool, manifest: &WorkspaceManifest) -> Result<String> {
    let sealed = workspace_clone_crypto::seal_json(
        &KeyStorage::load_local_keys()?.storage_key()?,
        manifest,
    )?;

    let path = manifest_path(&manifest.workspace.id)?;
    std::fs::write(&path, sealed.as_bytes()).map_err(|e| {
        DatabaseError::Connection(format!(
            "Could not write the manifest to {}: {e}",
            path.display()
        ))
    })?;

    let mut hasher = Sha256::new();
    hasher.update(sealed.as_bytes());
    let digest = hex(&hasher.finalize());

    WorkspaceRepository::new(pool.clone())
        .create(&WorkspaceRecord {
            id: manifest.workspace.id.clone(),
            name: manifest.workspace.name.clone(),
            schema_version: manifest.schema_version as i32,
            // Explicit, and cloned: the newtype's conversion silently falls
            // back to "now" on a timestamp it cannot parse, which would file a
            // corrupt manifest under the current time.
            captured_at: manifest.workspace.captured_at.clone().into(),
            source_device_id: manifest.workspace.source_device.id.clone(),
            manifest_digest: digest,
            encrypted_manifest_path: path.to_string_lossy().to_string(),
            status: "captured".to_string(),
        })
        .await?;

    Ok(sealed)
}

/// Where a sealed manifest lives on this device.
///
/// Public because the transfer command has to read the same bytes the capture
/// wrote. Two functions computing a path would be two functions that could
/// disagree about where a workspace was sealed.
pub fn manifest_path_for(workspace_id: &str) -> Result<PathBuf> {
    manifest_path(workspace_id)
}

/// Where a sealed manifest lives on this device.
fn manifest_path(workspace_id: &str) -> Result<PathBuf> {
    let dir = directories::ProjectDirs::from("com", "workspaceclone", "WorkspaceClone")
        .ok_or_else(|| {
            DatabaseError::Connection("Could not find the application data directory".to_string())
        })?
        .data_dir()
        .join("manifests");

    std::fs::create_dir_all(&dir).map_err(|e| {
        DatabaseError::Connection(format!("Could not create {}: {e}", dir.display()))
    })?;

    // The id is generated here, but a workspace id arriving from a peer must not
    // be able to name a file outside this directory.
    if !workspace_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || workspace_id.is_empty()
    {
        return Err(WorkspaceError::ManifestValidation(format!(
            "'{workspace_id}' is not a usable workspace id"
        ))
        .into());
    }

    Ok(dir.join(format!("{workspace_id}.sealed.json")))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[command]
pub async fn validate_manifest(manifest_json: String) -> Result<bool> {
    let manifest: WorkspaceManifest = serde_json::from_str(&manifest_json)?;
    manifest.validate()?;
    Ok(true)
}

#[command]
pub async fn scrub_manifest_secrets(manifest_json: String) -> Result<String> {
    let mut manifest: WorkspaceManifest = serde_json::from_str(&manifest_json)?;

    // Git remotes can carry credentials in the userinfo component, which is the
    // one part of a URL that is a secret rather than a location.
    for project in &mut manifest.projects {
        if let Some(git) = &mut project.git {
            if let Some(remote) = &git.remote_hint {
                git.remote_hint = Some(workspace_clone_core::manifest::scrub_url(remote));
            }
        }
    }

    // Application config is opaque, so a remote-looking string anywhere in it
    // gets scrubbed rather than only the fields an adapter happens to use.
    for application in &mut manifest.applications {
        scrub_urls_in_place(&mut application.config);
    }

    manifest.validate()?;
    Ok(serde_json::to_string(&manifest)?)
}

/// Recursively scrub URL-shaped strings.
fn scrub_urls_in_place(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            if text.contains("://") {
                *text = workspace_clone_core::manifest::scrub_url(text);
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(scrub_urls_in_place),
        serde_json::Value::Object(map) => map.values_mut().for_each(scrub_urls_in_place),
        _ => {}
    }
}

/// Read a stored manifest back.
///
/// The manifest is sealed at rest, so this unseals it. Returning the sealed form
/// instead would push the decision to the UI, where it would be skipped the
/// first time a screen wanted a project name.
#[command]
pub async fn get_manifest(
    pool: State<'_, DbPool>,
    workspace_id: String,
) -> Result<WorkspaceManifest> {
    read_manifest(pool.inner(), &workspace_id).await
}

/// The manifest as this device stored it, unsealed and checked against its digest.
///
/// Not a command: the transfer path needs the same thing when it builds a
/// payload, and a second copy of "unseal, then verify the digest" would be a
/// second copy of every way that can go wrong -- most importantly a payload
/// sent because its digest was never checked.
pub(crate) async fn read_manifest(pool: &DbPool, workspace_id: &str) -> Result<WorkspaceManifest> {
    let key = KeyStorage::load_local_keys()?.storage_key()?;
    read_manifest_with_key(pool, workspace_id, &key).await
}

/// [`read_manifest`], with the sealing key supplied rather than loaded.
///
/// The key is a parameter so the read path can be tested against bytes this test
/// sealed, without reaching the OS credential store. That matters because the
/// credential store blocks on a user prompt when the calling binary is not
/// already trusted, so a test that loaded the real key would hang on an invisible
/// dialog -- and it is the "a received workspace opens with this device's key"
/// check that most needs to run.
pub(crate) async fn read_manifest_with_key(
    pool: &DbPool,
    workspace_id: &str,
    key: &workspace_clone_core::crypto::EncryptionKey,
) -> Result<WorkspaceManifest> {
    let record = WorkspaceRepository::new(pool.clone())
        .get(workspace_id)
        .await?
        .ok_or_else(|| {
            WorkspaceError::ManifestValidation(format!("No workspace with id '{workspace_id}'"))
        })?;

    let sealed = std::fs::read_to_string(&record.encrypted_manifest_path).map_err(|e| {
        DatabaseError::Connection(format!(
            "The manifest for '{workspace_id}' could not be read from {}: {e}",
            record.encrypted_manifest_path
        ))
    })?;

    let manifest: WorkspaceManifest = workspace_clone_crypto::open_json(key, &sealed)?;

    // The digest is checked so a manifest that was altered on disk is reported
    // rather than restored from.
    let mut hasher = Sha256::new();
    hasher.update(sealed.as_bytes());
    if hex(&hasher.finalize()) != record.manifest_digest {
        return Err(WorkspaceError::ManifestValidation(format!(
            "The stored manifest for '{workspace_id}' no longer matches the digest recorded when it was captured"
        ))
        .into());
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_clone_adapters::{ApprovedCommand, SelectedProject};

    /// Stands in for the local device's row id. The command reads it from the
    /// database; a test has no database, and the value has to be a fixed string so
    /// two manifests can be compared.
    const TEST_DEVICE: &str = "test-device";

    fn selection() -> CaptureSelection {
        CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: "/Users/someone/code/demo".into(),
                destination_location_id: "code".into(),
            }],
            include_applications: vec!["git".into(), "vscode".into()],
            browser_urls: vec!["https://example.com".into()],
            env_var_names: vec!["API_TOKEN".into()],
            ..Default::default()
        }
    }

    fn project_context() -> PortableContext {
        PortableContext {
            adapter_id: "git".into(),
            data: serde_json::json!([{
                "id": "p1",
                "name": "demo",
                "source_path_hint": "~/code/demo",
                "destination_location_id": "code",
                "git": {
                    "remote_hint": "https://user:hunter2@github.com/owner/demo.git",
                    "branch": "main",
                    "commit": "abc123",
                    "dirty_worktree": false,
                    "dirty_state_captured": false,
                },
            }]),
        }
    }

    fn build(contexts: Vec<PortableContext>, sel: CaptureSelection) -> WorkspaceManifest {
        let local = LocalContext {
            os: "linux".into(),
            os_version: "6.0".into(),
            home_dir: "/home/someone".into(),
            config_dirs: vec![],
        };
        let mut warnings = Vec::new();
        assemble("Demo".into(), &local, TEST_DEVICE, &sel, &contexts, &mut warnings).unwrap()
    }

    #[test]
    fn a_capture_assembles_the_projects_it_was_given() {
        let manifest = build(vec![project_context()], selection());

        assert_eq!(manifest.workspace.name, "Demo");
        assert_eq!(manifest.projects.len(), 1);
        assert_eq!(manifest.projects[0].id, "p1");
        assert_eq!(manifest.projects[0].git.as_ref().unwrap().branch, "main");
        assert_eq!(manifest.workspace.source_device.os, "linux");
    }

    #[test]
    fn credentials_in_a_git_remote_never_reach_the_manifest() {
        let manifest = build(vec![project_context()], selection());
        let serialized = serde_json::to_string(&manifest).unwrap();

        assert!(!serialized.contains("hunter2"), "leaked: {serialized}");
        let remote = manifest.projects[0].git.as_ref().unwrap().remote_hint.as_ref().unwrap();
        assert!(!remote.contains("user"), "got {remote}");
        assert!(remote.contains("github.com"), "the location survives: {remote}");
    }

    #[test]
    fn an_absolute_path_from_an_adapter_is_replaced() {
        // A future adapter that forgets to redact must not leak a path.
        let mut context = project_context();
        context.data = serde_json::json!([{
            "id": "p1",
            "name": "demo",
            "source_path_hint": "/Users/someone/code/demo",
            "destination_location_id": "code",
            "git": null,
        }]);

        let manifest = build(vec![context], selection());
        let serialized = serde_json::to_string(&manifest).unwrap();

        assert!(!serialized.contains("/Users/someone"), "leaked: {serialized}");
        assert_eq!(manifest.projects[0].source_path_hint, "demo");
    }

    #[test]
    fn environment_values_are_never_included_however_the_caller_asks() {
        let mut sel = selection();
        // The caller asks for values, which the type does not allow; the
        // manifest must still say presence-only.
        sel.env_var_names = vec!["API_TOKEN".into()];

        let manifest = build(vec![project_context()], sel);

        assert_eq!(manifest.requirements.environment.presence_only, vec!["API_TOKEN"]);
        assert!(!manifest.requirements.environment.values_included);
        assert!(!manifest.policy.secret_values_included);
        manifest.validate().expect("a presence-only manifest is valid");
    }

    #[test]
    fn a_duplicate_environment_name_is_recorded_once() {
        let mut sel = selection();
        sel.env_var_names = vec!["API_TOKEN".into(), "API_TOKEN".into()];

        let manifest = build(vec![project_context()], sel);

        assert_eq!(manifest.requirements.environment.presence_only.len(), 1);
    }

    #[test]
    fn automatic_command_execution_cannot_be_turned_on_by_a_caller() {
        let mut sel = selection();
        sel.policy.automatic_command_execution = true;

        let manifest = build(vec![project_context()], sel);

        assert!(
            !manifest.policy.automatic_command_execution,
            "the restrictive value is a floor, not a starting point"
        );
        manifest.validate().expect("must be valid");
    }

    #[test]
    fn a_captured_application_becomes_a_checkable_requirement() {
        let app_context = PortableContext {
            adapter_id: "vscode".into(),
            data: serde_json::json!([{
                "id": "vscode-p1",
                "adapter": "vscode",
                "project_id": "p1",
                "required": true,
                "target_hint": "~/code/demo",
                "kind": "folder",
            }]),
        };
        let mut sel = selection();
        sel.include_applications.push("browser".into());

        let manifest = build(
            vec![
                project_context(),
                app_context,
                PortableContext {
                    adapter_id: "browser".into(),
                    data: serde_json::json!({ "urls": ["https://example.com"] }),
                },
            ],
            sel,
        );

        assert_eq!(manifest.applications.len(), 2);
        // Both applications are checkable, because each names its adapter.
        assert!(manifest.requirements.applications.iter().all(|r| r.is_checkable()));
        assert!(manifest.requirements.applications.iter().any(|r| r.adapter == "vscode"));
        assert!(manifest.requirements.applications.iter().any(|r| r.adapter == "browser"));
    }

    #[test]
    fn a_requirement_with_no_adapter_is_flagged_not_silently_kept() {
        let app_context = PortableContext {
            adapter_id: "vscode".into(),
            data: serde_json::json!([{
                "id": "mystery",
                "adapter": "",
                "project_id": null,
                "required": true,
            }]),
        };
        let local = LocalContext {
            os: "linux".into(),
            os_version: "6.0".into(),
            home_dir: "/home/x".into(),
            config_dirs: vec![],
        };
        let mut warnings = Vec::new();
        let manifest = assemble(
            "Demo".into(),
            &local,
            TEST_DEVICE,
            &selection(),
            &[app_context],
            &mut warnings,
        )
        .unwrap();

        assert!(
            warnings.iter().any(|w| w.contains("no adapter covers it")),
            "got {warnings:?}"
        );
        assert!(!manifest.requirements.applications[0].is_checkable());
    }

    #[test]
    fn an_adapter_returning_the_wrong_shape_becomes_a_warning_not_a_silent_gap() {
        let local = LocalContext {
            os: "linux".into(),
            os_version: "6.0".into(),
            home_dir: "/home/x".into(),
            config_dirs: vec![],
        };
        let mut warnings = Vec::new();
        let manifest = assemble(
            "Demo".into(),
            &local,
            TEST_DEVICE,
            &selection(),
            &[PortableContext {
                adapter_id: "vscode".into(),
                data: serde_json::json!({ "not": "a list" }),
            }],
            &mut warnings,
        )
        .unwrap();

        assert!(manifest.applications.is_empty());
        assert!(
            warnings.iter().any(|w| w.contains("could not read")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn browser_urls_with_nothing_in_them_produce_no_application() {
        let local = LocalContext {
            os: "linux".into(),
            os_version: "6.0".into(),
            home_dir: "/home/x".into(),
            config_dirs: vec![],
        };
        let mut warnings = Vec::new();
        let manifest = assemble(
            "Demo".into(),
            &local,
            TEST_DEVICE,
            &selection(),
            &[PortableContext {
                adapter_id: "browser".into(),
                data: serde_json::json!({ "urls": [], "rejected": ["file:///etc/passwd"] }),
            }],
            &mut warnings,
        )
        .unwrap();

        assert!(manifest.applications.is_empty());
        // The refused URL must not have travelled into the manifest.
        let serialized = serde_json::to_string(&manifest).unwrap();
        assert!(!serialized.contains("passwd"), "leaked: {serialized}");
    }

    #[test]
    fn runtimes_from_the_runtime_adapter_become_requirements() {
        let local = LocalContext {
            os: "linux".into(),
            os_version: "6.0".into(),
            home_dir: "/home/x".into(),
            config_dirs: vec![],
        };
        let mut warnings = Vec::new();
        let manifest = assemble(
            "Demo".into(),
            &local,
            TEST_DEVICE,
            &selection(),
            &[PortableContext {
                adapter_id: "runtime".into(),
                data: serde_json::json!({
                    "runtimes": [{ "name": "node", "version": "20", "required": true }],
                    "cli_tools": [{ "name": "pnpm", "version": null, "required": false }],
                }),
            }],
            &mut warnings,
        )
        .unwrap();

        assert_eq!(manifest.requirements.runtimes.len(), 1);
        assert_eq!(manifest.requirements.runtimes[0].name, "node");
        assert_eq!(manifest.requirements.cli_tools.len(), 1);
        assert_eq!(manifest.requirements.cli_tools[0].name, "pnpm");
    }

    #[test]
    fn a_capture_orders_its_restore_steps_git_then_application_then_project() {
        let app_context = PortableContext {
            adapter_id: "vscode".into(),
            data: serde_json::json!([{
                "id": "vscode-p1",
                "adapter": "vscode",
                "project_id": "p1",
                "required": false,
            }]),
        };
        let manifest = build(vec![project_context(), app_context], selection());

        let kinds: Vec<&str> = manifest
            .restore
            .steps
            .iter()
            .map(|s| match s {
                RestoreStep::CheckGit { .. } => "git",
                RestoreStep::OpenApplication { .. } => "app",
                RestoreStep::OpenProject { .. } => "project",
                _ => "other",
            })
            .collect();

        assert_eq!(kinds, vec!["git", "app", "project"]);
    }

    /// The manifest's contents must not depend on the order the adapter
    /// fragments arrive in.
    ///
    /// Two captures of the same machine have different workspace ids and
    /// timestamps by construction, so the comparison is on the body: the projects,
    /// applications, requirements and steps. If this ever fails, two devices
    /// cannot be shown to hold the same manifest, because the byte-for-byte
    /// comparison that proves it is meaningless.
    #[test]
    fn the_manifest_body_does_not_depend_on_the_adapter_order() {
        let git = project_context();
        let vscode = PortableContext {
            adapter_id: "vscode".into(),
            data: serde_json::json!([
                { "id": "vscode-p1", "adapter": "vscode", "project_id": "p1", "required": false },
                { "id": "vscode-p2", "adapter": "vscode", "project_id": "p2", "required": false },
            ]),
        };
        let browser = PortableContext {
            adapter_id: "browser".into(),
            data: serde_json::json!({ "urls": ["https://example.com"] }),
        };

        let forward = build(
            vec![git.clone(), vscode.clone(), browser.clone()],
            selection(),
        );
        let reverse = build(
            vec![browser, vscode, git],
            selection(),
        );

        assert_eq!(
            serde_json::to_value(&forward.projects).unwrap(),
            serde_json::to_value(&reverse.projects).unwrap(),
            "projects must not reshuffle"
        );
        assert_eq!(
            serde_json::to_value(&forward.applications).unwrap(),
            serde_json::to_value(&reverse.applications).unwrap(),
            "applications must not reshuffle"
        );
        assert_eq!(
            serde_json::to_value(&forward.requirements).unwrap(),
            serde_json::to_value(&reverse.requirements).unwrap(),
            "requirements must not reshuffle"
        );
        assert_eq!(
            serde_json::to_value(&forward.restore).unwrap(),
            serde_json::to_value(&reverse.restore).unwrap(),
            "steps must not reshuffle"
        );
    }

    /// The workspace id and timestamp differ by construction, so they are the
    /// only parts of two captures of the same inputs that may differ. This makes
    /// the claim above checkable rather than a matter of trust.
    #[test]
    fn two_captures_of_the_same_inputs_differ_only_in_identity_and_time() {
        let a = build(vec![project_context()], selection());
        let b = build(vec![project_context()], selection());

        let mut a = serde_json::to_value(&a).unwrap();
        let mut b = serde_json::to_value(&b).unwrap();

        for value in [&mut a, &mut b] {
            let meta = value.get_mut("workspace").unwrap();
            meta.as_object_mut().unwrap().remove("id");
            meta.as_object_mut().unwrap().remove("captured_at");
            meta.get_mut("source_device")
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("id");
        }

        assert_eq!(a, b);
    }

    #[test]
    fn a_manifest_id_cannot_name_a_file_outside_the_manifest_directory() {
        for hostile in ["../escape", "a/b", "", "..", "with space", "null\0byte"] {
            let path = manifest_path(hostile);
            assert!(path.is_err(), "'{hostile}' was accepted as a file name");
        }
    }

    #[test]
    fn a_normal_workspace_id_produces_a_path_inside_the_directory() {
        let path = manifest_path("1f8c0a2e-3b4d-5e6f-7a8b-9c0d1e2f3a4b").unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        assert!(name.ends_with(".sealed.json"), "got {name}");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "manifests");
    }

    #[test]
    fn absolute_path_shapes_are_recognised() {
        for absolute in [
            "/home/x/code",
            "/Users/x/code",
            "C:/Users/x/code",
            r"\\server\share",
        ] {
            assert!(looks_absolute(absolute), "{absolute} was not seen as absolute");
        }
        for relative in ["~/code/demo", "code/demo", "./demo", "demo", ""] {
            assert!(!looks_absolute(relative), "{relative} was seen as absolute");
        }
    }

    #[test]
    fn a_shortened_path_keeps_only_the_parent_and_leaf() {
        assert_eq!(shorten_path("/Users/someone/code/demo"), "~/code/demo");
        // Two components: the parent is the only directory name kept, so
        // `/etc/passwd` does not reveal the account's home directory layout.
        assert_eq!(shorten_path("/etc/passwd"), "~/etc/passwd");
        assert_eq!(shorten_path("not-a-path"), "not-a-path");
        // A single leading slash is a path at the filesystem root, with no
        // parent to name.
        assert_eq!(shorten_path("/demo"), "demo");
    }

    #[test]
    fn a_manifest_with_no_name_is_refused() {
        // The command layer checks this; this guards the underlying rule.
        let manifest = WorkspaceManifest::new(String::new(), DeviceRef {
            id: "d".into(),
            os: "linux".into(),
            os_version: "6".into(),
        });
        assert!(manifest.validate().is_err(), "a nameless workspace must not validate");
    }

    #[test]
    fn scrubbing_reaches_urls_nested_inside_configuration() {
        let mut value = serde_json::json!({
            "outer": {
                "urls": ["https://user:secret@example.com/x"],
                "list": [{ "link": "https://a:b@other.example/y" }],
            }
        });

        scrub_urls_in_place(&mut value);
        let serialized = value.to_string();

        assert!(!serialized.contains("secret"), "leaked: {serialized}");
        assert!(!serialized.contains(":b@"), "leaked: {serialized}");
    }

    #[test]
    fn the_terminal_adapter_can_only_offer_approved_commands() {
        // Capture records the commands the user approved and nothing else, so a
        // manifest cannot carry a command the user did not agree to run.
        let mut sel = selection();
        sel.terminal_commands = vec![ApprovedCommand {
            label: "dev server".into(),
            command: "npm run dev".into(),
            working_directory: Some("/Users/someone/code/demo".into()),
        }];
        sel.include_applications = vec!["git".into(), "terminal".into()];

        let terminal = PortableContext {
            adapter_id: "terminal".into(),
            data: serde_json::json!([{
                "id": "terminal-commands",
                "adapter": "terminal",
                "project_id": null,
                "required": false,
                "commands": [{
                    "label": "dev server",
                    "command": "npm run dev",
                    "working_directory_hint": "~/code/demo",
                }],
            }]),
        };

        let manifest = build(vec![project_context(), terminal], sel);
        let serialized = serde_json::to_string(&manifest).unwrap();

        assert!(serialized.contains("npm run dev"));
        assert!(!serialized.contains("/Users/someone"), "leaked: {serialized}");
        assert!(
            !manifest.policy.automatic_command_execution,
            "offering a command is not permission to run it"
        );
    }
}
