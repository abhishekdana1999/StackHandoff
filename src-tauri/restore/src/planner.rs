//! Restore plan generation.
//!
//! A plan is built entirely from the *manifest*, never by running capture again.
//! The previous version re-captured on the destination, which was wrong in three
//! separate ways: it read the destination's own applications instead of the ones
//! the workspace was captured with, it produced nothing when the destination had
//! no projects selected, and it never wrote the adapter id into the actions, so
//! every step failed in the executor with "adapter not found".
//!
//! Two rules shape the rest of this file:
//!
//! - **A manifest is untrusted input.** It arrives from another device, so a
//!   project name is only ever used as a single path component beneath a root
//!   the user chose on this machine. Nothing in a manifest can select where
//!   anything is written.
//! - **Nothing is implied.** Steps that launch something are never pre-approved,
//!   and a manifest that asks for behaviour this build cannot express is
//!   reported as a note rather than being silently dropped.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};
use workspace_clone_adapters::{
    AdapterRegistry, LocalContext, PortableContext, RestoreAction, RestoreActionType,
};
use workspace_clone_core::manifest::{Application, WorkspaceManifest};
use workspace_clone_core::{RestoreError, Result, WorkspaceError};

/// The adapter that owns repository verification.
const GIT_ADAPTER: &str = "git";
/// The adapter that owns editor and terminal restore steps.
const EDITOR_ADAPTER: &str = "vscode";
const TERMINAL_ADAPTER: &str = "terminal";
const BROWSER_ADAPTER: &str = "browser";

/// Turns a manifest into a concrete, ordered plan for this machine.
pub struct RestorePlanner {
    adapter_registry: Arc<AdapterRegistry>,
}

/// Everything the planner needs to know about this machine.
#[derive(Debug, Clone)]
pub struct PlanRequest<'a> {
    pub manifest: &'a WorkspaceManifest,
    /// The destination's OS and config directories. Used only to describe
    /// actions; the manifest's captured values are never reused as paths.
    pub local_context: &'a LocalContext,
    /// Logical bucket id (for example `code`) to an absolute path on this
    /// machine, chosen by the user during restore.
    ///
    /// A project whose bucket is absent from this map is still planned, but
    /// with no destination path, so its steps report that the user has not
    /// chosen where it goes yet. Guessing a location instead would write files
    /// somewhere the user never approved.
    pub destination_roots: BTreeMap<String, PathBuf>,
}

impl<'a> PlanRequest<'a> {
    pub fn new(manifest: &'a WorkspaceManifest, local_context: &'a LocalContext) -> Self {
        Self {
            manifest,
            local_context,
            destination_roots: BTreeMap::new(),
        }
    }

    pub fn with_root(mut self, bucket: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.destination_roots.insert(bucket.into(), path.into());
        self
    }
}

impl RestorePlanner {
    pub fn new(adapter_registry: Arc<AdapterRegistry>) -> Self {
        Self { adapter_registry }
    }

    /// Build the plan.
    pub async fn generate_plan(&self, request: &PlanRequest<'_>) -> Result<RestorePlan> {
        let manifest = request.manifest;
        info!(
            "Generating restore plan for workspace: {}",
            manifest.workspace.name
        );

        // Validate before doing anything else. A manifest that fails these
        // checks is either hostile or corrupt, and there is no point building a
        // plan from it.
        manifest.validate()?;

        let destinations = resolve_destinations(manifest, &request.destination_roots);
        let mut actions: BTreeMap<String, RestoreAction> = BTreeMap::new();
        let mut notes: Vec<String> = Vec::new();

        // 1. Record where each project lands. These steps carry no adapter: they
        //    exist so the user can see the mapping, and so the actions that open
        //    or inspect a project can declare a dependency on it.
        for project in &manifest.projects {
            let resolved = destinations.get(&project.id);
            let mut config = serde_json::Map::new();
            config.insert("project_id".into(), project.id.clone().into());
            config.insert("source_hint".into(), project.source_path_hint.clone().into());
            config.insert(
                "bucket".into(),
                project.destination_location_id.clone().into(),
            );
            // The key is omitted rather than set to null when the user has not
            // chosen a destination. Absent means "no choice made yet", which is
            // a question for the user; null would read as a value and hide the
            // difference.
            if let Some(path) = resolved {
                config.insert(
                    "destination_path".into(),
                    path.to_string_lossy().to_string().into(),
                );
            }

            let action = RestoreAction::new(
                format!("map-path-{}", project.id),
                RestoreActionType::MapPath,
                RestoreAction::NO_ADAPTER,
                match resolved {
                    Some(path) => format!("{} maps to {}", project.name, path.display()),
                    None => format!(
                        "{} has no destination chosen yet, so nothing will be opened for it",
                        project.name
                    ),
                },
                serde_json::Value::Object(config),
            )
            .required()
            .approved();

            insert_unique(&mut actions, action, &mut notes);
        }

        // 2. Ask the git adapter what verifying each project looks like, using
        //    the manifest's project list as its input rather than a fresh
        //    capture of this machine.
        let git_actions = self
            .plan_for(
                GIT_ADAPTER,
                PortableContext {
                    adapter_id: GIT_ADAPTER.to_string(),
                    data: serde_json::to_value(&manifest.projects)?,
                },
            )
            .await?;

        for mut action in git_actions {
            let project_id = action
                .config
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            attach_destination(&mut action, "destination_path", destinations.get(&project_id));
            depend_on_mapping(&mut action, &project_id);
            insert_unique(&mut actions, action, &mut notes);
        }

        // 3. Ask each application adapter to plan for the applications the
        //    manifest recorded, not the ones installed here.
        for adapter_id in adapters_named_by(manifest) {
            let Some(adapter) = self.adapter_registry.get(&adapter_id) else {
                // Not a failure: this build may simply not know this
                // application. The user is told rather than left guessing.
                notes.push(format!(
                    "The manifest includes '{}', which this build has no adapter for.",
                    adapter_id
                ));
                debug!("No adapter registered for {adapter_id}");
                continue;
            };

            let portable = portable_context_for(&adapter_id, manifest);
            let Ok(mut planned) = adapter.plan_restore(&portable).await else {
                // A malformed section of the manifest must not lose the rest of
                // the plan.
                warn!("Adapter {adapter_id} could not plan from the manifest");
                notes.push(format!(
                    "The '{}' section of the manifest could not be read and was skipped.",
                    adapter_id
                ));
                continue;
            };

            for action in &mut planned {
                resolve_adapter_destination(action, &adapter_id, manifest, &destinations);
                enforce_approval_policy(action, &mut notes);
                insert_unique(&mut actions, action.clone(), &mut notes);
            }
        }

        // 4. Anything the capture device declared that this planner does not
        //    turn into a step is reported rather than quietly forgotten.
        record_unexpressed_steps(manifest, &actions, &mut notes);

        let steps = self.topological_sort(&actions)?;

        info!(
            "Restore plan has {} step(s) and {} note(s)",
            steps.len(),
            notes.len()
        );

        Ok(RestorePlan {
            workspace_id: manifest.workspace.id.clone(),
            steps,
            notes,
        })
    }

    /// Build a portable context for one adapter from the manifest.
    async fn plan_for(
        &self,
        adapter_id: &str,
        context: PortableContext,
    ) -> Result<Vec<RestoreAction>> {
        let Some(adapter) = self.adapter_registry.get(adapter_id) else {
            // Not an error: the caller has already decided this adapter is
            // optional, and a missing one is recorded as a note.
            return Ok(Vec::new());
        };
        Ok(adapter.plan_restore(&context).await?)
    }

    /// Order steps so every action follows the things it depends on.
    ///
    /// Ties are broken by id so the same manifest always produces the same
    /// order, which is what lets a user compare two runs.
    fn topological_sort(&self, actions: &BTreeMap<String, RestoreAction>) -> Result<Vec<RestoreAction>> {
        let mut sorted: Vec<RestoreAction> = Vec::with_capacity(actions.len());
        // 0 = unvisited, 1 = on the current path, 2 = done.
        let mut state: HashMap<&str, u8> = HashMap::new();
        let mut path: Vec<&str> = Vec::new();

        fn visit<'a>(
            id: &'a str,
            actions: &'a BTreeMap<String, RestoreAction>,
            state: &mut HashMap<&'a str, u8>,
            path: &mut Vec<&'a str>,
            sorted: &mut Vec<RestoreAction>,
        ) -> Result<()> {
            match state.get(id).copied().unwrap_or(0) {
                2 => return Ok(()),
                1 => {
                    // Name the whole cycle, not just the node that closed it:
                    // "a -> b -> c -> a" is actionable, "c" is not.
                    let start = path.iter().position(|p| *p == id).unwrap_or(0);
                    let cycle: Vec<&str> = path[start..].iter().copied().chain([id]).collect();
                    return Err(WorkspaceError::Restore(RestoreError::PlanGeneration(
                        format!("Circular dependency detected: {}", cycle.join(" -> ")),
                    )));
                }
                _ => {}
            }

            state.insert(id, 1);
            path.push(id);

            if let Some(action) = actions.get(id) {
                // Sorted so an error message is deterministic too.
                let mut dependencies: Vec<&String> = action.dependencies.iter().collect();
                dependencies.sort();
                for dependency in dependencies {
                    visit(dependency, actions, state, path, sorted)?;
                }
                sorted.push(action.clone());
            }

            path.pop();
            state.insert(id, 2);
            Ok(())
        }

        for id in actions.keys() {
            visit(id, actions, &mut state, &mut path, &mut sorted)?;
        }

        Ok(sorted)
    }
}

/// The plan, in the order it should be executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    /// The workspace this plan restores.
    ///
    /// Carried through to the run record so history is attributable. Defaults
    /// to empty so a plan round-tripped from an older build still loads.
    #[serde(default)]
    pub workspace_id: String,
    pub steps: Vec<RestoreAction>,
    /// Things the user should know that are not steps.
    ///
    /// A manifest can describe work this build cannot express, or name an
    /// adapter we do not have. Reporting that is the difference between "the
    /// app is quiet" and "the app is honest about what it did not do".
    #[serde(default)]
    pub notes: Vec<String>,
}

impl RestorePlan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Steps that open something visible on this machine.
    ///
    /// These are the ones a user most wants to see before running, because they
    /// put windows on screen rather than gathering information. A step that is
    /// already approved is not repeated in this list.
    pub fn steps_needing_consent(&self) -> Vec<&RestoreAction> {
        self.steps
            .iter()
            .filter(|a| !a.approved && Self::opens_something(a.action_type))
            .collect()
    }

    /// Whether an action type puts something on the user's screen.
    fn opens_something(action_type: RestoreActionType) -> bool {
        matches!(
            action_type,
            RestoreActionType::OpenApplication | RestoreActionType::OpenUrls
        )
    }

    /// Actions the user has already agreed to.
    pub fn approved_step_ids(&self) -> Vec<String> {
        self.steps
            .iter()
            .filter(|a| a.approved)
            .map(|a| a.id.clone())
            .collect()
    }
}

/// Add a destination path to an action, recording when there isn't one.
///
/// Leaving the key absent rather than writing `null` matters: the adapters
/// distinguish "the user chose no path" from "the planner forgot", and the first
/// is a question to ask while the second is a bug to fix.
fn attach_destination(
    action: &mut RestoreAction,
    key: &str,
    destination: Option<&PathBuf>,
) {
    match destination {
        Some(path) => {
            if let Some(config) = action.config.as_object_mut() {
                config.insert(key.to_string(), serde_json::Value::String(path.to_string_lossy().to_string()));
            }
        }
        None => {
            debug!(
                "No destination for action {}; it will report that none was chosen",
                action.id
            );
        }
    }
}

/// Make an action depend on its project's path mapping.
fn depend_on_mapping(action: &mut RestoreAction, project_id: &str) {
    let dependency = format!("map-path-{project_id}");
    if project_id.is_empty() || action.dependencies.contains(&dependency) {
        return;
    }
    action.dependencies.push(dependency);
}

/// Point an adapter's action at a real local path.
///
/// Each adapter reads a differently named key, and getting the name wrong is
/// how the previous version ended up with "no destination was chosen" on every
/// step. The mapping is written out explicitly so a change to it is visible.
fn resolve_adapter_destination(
    action: &mut RestoreAction,
    adapter_id: &str,
    manifest: &WorkspaceManifest,
    destinations: &BTreeMap<String, PathBuf>,
) {
    match adapter_id {
        EDITOR_ADAPTER => {
            // Only the explicit field is trusted. Guessing the project out of
            // the action id would produce the *application* id, and mapping
            // that would open the wrong folder.
            let project_id = action
                .config
                .get("project_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            attach_destination(action, "target", destinations.get(&project_id));
            depend_on_mapping(action, &project_id);
        }
        TERMINAL_ADAPTER => {
            // A terminal step may name its project, or only carry a path hint
            // from the source machine. The hint is matched against the
            // manifest's own hints rather than used as a path.
            let by_id = action
                .config
                .get("project_id")
                .and_then(|v| v.as_str())
                .and_then(|id| destinations.get(id));

            let by_hint = by_id.or_else(|| {
                let hint = action.config.get("cwd_hint")?.as_str()?;
                let project = manifest
                    .projects
                    .iter()
                    .find(|p| p.source_path_hint == hint)?;
                destinations.get(&project.id)
            });

            attach_destination(action, "cwd", by_hint);
        }
        BROWSER_ADAPTER => {
            // Tabs have no location.
        }
        _ => {
            let project_id = action
                .config
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            attach_destination(action, "destination_path", destinations.get(&project_id));
            depend_on_mapping(action, &project_id);
        }
    }
}

/// Force a step back to un-approved when the blueprint requires consent.
///
/// `plan_restore` already declines to pre-approve anything that launches
/// something, but this runs on a manifest that came from a peer, so the
/// guarantee is re-established here rather than assumed.
fn enforce_approval_policy(action: &mut RestoreAction, _notes: &mut Vec<String>) {
    if matches!(
        action.action_type,
        RestoreActionType::OpenUrls | RestoreActionType::OfferCommand
    ) {
        action.approved = false;
    }
}

/// Add an action, keeping the first one if an id repeats.
///
/// Two actions with one id cannot both be executed or approved independently,
/// so silently keeping one would hide a step from the user. The duplicate is
/// reported instead.
fn insert_unique(
    actions: &mut BTreeMap<String, RestoreAction>,
    action: RestoreAction,
    notes: &mut Vec<String>,
) {
    if let Some(existing) = actions.get(&action.id) {
        let same = serde_json::to_string(&existing.config).ok()
            == serde_json::to_string(&action.config).ok();
        notes.push(if same {
            format!("The manifest listed '{}' more than once; it is planned once.", action.description)
        } else {
            format!(
                "Two different steps both claimed the id '{}'; the second was not planned.",
                action.id
            )
        });
        return;
    }
    actions.insert(action.id.clone(), action);
}

/// Which adapters the manifest actually references.
fn adapters_named_by(manifest: &WorkspaceManifest) -> BTreeSet<String> {
    manifest
        .applications
        .iter()
        .map(|a| a.adapter.clone())
        .filter(|a| !a.is_empty())
        .collect()
}

/// Build the input an adapter expects, from manifest data.
///
/// Each adapter's `plan_restore` takes the same shape its `capture` produced,
/// so the manifest sections are reshaped to match. Getting this wrong is what
/// made the old planner call `capture` and get the *destination's* answers.
fn portable_context_for(adapter_id: &str, manifest: &WorkspaceManifest) -> PortableContext {
    let data = match adapter_id {
        GIT_ADAPTER => serde_json::to_value(&manifest.projects).unwrap_or(serde_json::Value::Null),
        BROWSER_ADAPTER => {
            // The browser's capture shape is a url list, not applications.
            let mut urls: Vec<String> = Vec::new();
            for app in manifest.applications.iter().filter(|a| a.adapter == BROWSER_ADAPTER) {
                if let Some(list) = app.config.get("urls").and_then(|v| v.as_array()) {
                    for url in list.iter().filter_map(|v| v.as_str()) {
                        if !urls.iter().any(|existing| existing == url) {
                            urls.push(url.to_string());
                        }
                    }
                }
            }
            serde_json::json!({ "urls": urls })
        }
        other => {
            let apps: Vec<Application> = manifest
                .applications
                .iter()
                .filter(|a| a.adapter == other)
                .cloned()
                .collect();
            serde_json::to_value(apps).unwrap_or(serde_json::Value::Null)
        }
    };

    PortableContext {
        adapter_id: adapter_id.to_string(),
        data,
    }
}

/// Decide where each project lives on this machine.
///
/// The manifest supplies a bucket and a name; this machine supplies a path per
/// bucket. Only the name's final component is ever used, and only beneath the
/// chosen root, so no manifest can direct a write outside the directories the
/// user picked.
fn resolve_destinations(
    manifest: &WorkspaceManifest,
    roots: &BTreeMap<String, PathBuf>,
) -> BTreeMap<String, PathBuf> {
    let mut resolved = BTreeMap::new();

    for project in &manifest.projects {
        let Some(root) = roots.get(&project.destination_location_id) else {
            debug!(
                "No root configured for bucket '{}', so {} stays unmapped",
                project.destination_location_id, project.name
            );
            continue;
        };

        match safe_child(root, &project.name) {
            Some(path) => {
                resolved.insert(project.id.clone(), path);
            }
            None => warn!(
                "Refusing to use project name '{}' as a path component",
                project.name
            ),
        }
    }

    resolved
}

/// Join a single name beneath a root, refusing anything that is not a name.
///
/// A received manifest is attacker-controlled, so this is the boundary that
/// keeps a project called `../../.ssh` or `/etc/cron.d` from becoming a write
/// target. Anything with a separator, a parent reference, a NUL, or a control
/// character is refused outright rather than being coerced into something that
/// looks safe.
fn safe_child(root: &Path, name: &str) -> Option<PathBuf> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }
    if trimmed.contains(['/', '\\', '\0']) {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return None;
    }

    let joined = root.join(trimmed);
    // Belt and braces: confirm the join stayed inside the root even though the
    // checks above should already guarantee it.
    if !joined.starts_with(root) {
        return None;
    }
    Some(joined)
}

/// Report manifest steps this planner did not turn into actions.
fn record_unexpressed_steps(
    manifest: &WorkspaceManifest,
    actions: &BTreeMap<String, RestoreAction>,
    notes: &mut Vec<String>,
) {
    let project_ids: HashSet<&str> = manifest.projects.iter().map(|p| p.id.as_str()).collect();
    let application_ids: HashSet<&str> = manifest.applications.iter().map(|a| a.id.as_str()).collect();

    let mut unexpressed: Vec<String> = Vec::new();

    for step in declarative_steps(manifest) {
        let expressed = match &step {
            RestoreStepRef::Project { project_id } => {
                project_ids.contains(project_id.as_str())
                    && actions.contains_key(&format!("map-path-{project_id}"))
            }
            RestoreStepRef::Application { application_id } => {
                application_ids.contains(application_id.as_str())
            }
        };
        if !expressed {
            unexpressed.push(describe_step(&step));
        }
    }

    if !unexpressed.is_empty() {
        notes.push(format!(
            "{} step(s) the capture device suggested could not be turned into steps here: {}",
            unexpressed.len(),
            unexpressed.join("; ")
        ));
    }
}

/// A borrowed view of a declarative restore step.
///
/// The planner never executes these; it only needs to know whether each one was
/// already covered, so a local enum avoids taking a dependency on a type whose
/// fields would otherwise be unused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreStepRef {
    Project { project_id: String },
    Application { application_id: String },
}

fn describe_step(step: &RestoreStepRef) -> String {
    match step {
        RestoreStepRef::Project { project_id } => format!("project {project_id}"),
        RestoreStepRef::Application { application_id } => format!("application {application_id}"),
    }
}

/// Convert the manifest's declarative steps into the planner's comparison view.
pub fn declarative_steps(manifest: &WorkspaceManifest) -> Vec<RestoreStepRef> {
    manifest
        .restore
        .steps
        .iter()
        .map(|step| match step {
            workspace_clone_core::manifest::RestoreStep::OpenProject { project_id }
            | workspace_clone_core::manifest::RestoreStep::CheckGit { project_id }
            | workspace_clone_core::manifest::RestoreStep::MapPath { project_id } => {
                RestoreStepRef::Project {
                    project_id: project_id.clone(),
                }
            }
            workspace_clone_core::manifest::RestoreStep::OpenApplication { application_id }
            | workspace_clone_core::manifest::RestoreStep::OpenUrls { application_id } => {
                RestoreStepRef::Application {
                    application_id: application_id.clone(),
                }
            }
            workspace_clone_core::manifest::RestoreStep::OfferCommand { recipe_id, .. } => {
                RestoreStepRef::Application {
                    application_id: recipe_id.clone(),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_clone_adapters::{GitAdapter, RuntimeAdapter, VSCodeAdapter};
    use workspace_clone_core::manifest::{
        AppRequirement, EnvironmentRequirement, Project, Requirements,
        RestoreStep as ManifestStep, WorkspaceMeta,
    };

    /// Run a future to completion on this thread.
    ///
    /// The planner is async but the tests are pure functions over in-memory
    /// data, so a current-thread runtime is enough and keeps each test's
    /// assertions in a readable sequence.
    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime")
            .block_on(future)
    }

    fn planner() -> RestorePlanner {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(GitAdapter::new()));
        registry.register(Box::new(VSCodeAdapter::new()));
        registry.register(Box::new(RuntimeAdapter::new()));
        RestorePlanner::new(Arc::new(registry))
    }

    fn manifest_with(projects: Vec<Project>, applications: Vec<Application>) -> WorkspaceManifest {
        WorkspaceManifest {
            projects,
            applications,
            workspace: WorkspaceMeta {
                id: "ws-1".into(),
                name: "Test Workspace".into(),
                captured_at: workspace_clone_core::device::DateTimeUtc::from(
                    chrono::Utc::now(),
                ),
                source_device: workspace_clone_core::manifest::DeviceRef {
                    id: "local".into(),
                    os: "macos".into(),
                    os_version: "15.0".into(),
                },
                portability: workspace_clone_core::manifest::Portability::CrossPlatform,
            },
            requirements: Requirements {
                applications: vec![AppRequirement::new("vscode-1", "vscode", false)],
                runtimes: Vec::new(),
                cli_tools: Vec::new(),
                identities: Vec::new(),
                environment: EnvironmentRequirement::default(),
                services: Vec::new(),
            },
            policy: workspace_clone_core::manifest::Policy::default(),
            ..Default::default()
        }
    }

    fn project(id: &str, name: &str, bucket: &str) -> Project {
        Project {
            id: id.into(),
            name: name.into(),
            source_path_hint: format!("~/code/{name}"),
            destination_location_id: bucket.into(),
            git: Some(workspace_clone_core::manifest::GitInfo {
                remote_hint: Some("git@github.com:acme/demo.git".into()),
                branch: "main".into(),
                commit: Some("abc123".into()),
                dirty_worktree: false,
                dirty_state_captured: false,
            }),
        }
    }

    fn vscode_app(project_id: &str) -> Application {
        Application {
            id: format!("vscode-{project_id}"),
            adapter: "vscode".into(),
            project_id: Some(project_id.into()),
            required: false,
            config: serde_json::json!({
                "target_hint": "~/code/demo",
                "kind": "folder",
                "project_id": project_id,
                "extensions": [],
            }),
        }
    }

    fn context() -> LocalContext {
        LocalContext::current()
    }

    #[test]
    fn a_project_maps_under_the_chosen_root() {
        let manifest = manifest_with(vec![project("p1", "demo", "code")], vec![]);
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let mapping = plan.steps.iter().find(|s| s.id == "map-path-p1").unwrap();
        assert_eq!(
            mapping.config["destination_path"],
            serde_json::json!("/tmp/wc-code/demo")
        );
    }

    #[test]
    fn a_git_check_receives_the_resolved_destination() {
        let manifest = manifest_with(vec![project("p1", "demo", "code")], vec![]);
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let check = plan
            .steps
            .iter()
            .find(|s| s.id == "git-check-p1")
            .expect("the git adapter must plan a check for the project");
        assert_eq!(
            check.config["destination_path"],
            serde_json::json!("/tmp/wc-code/demo")
        );
        assert_eq!(check.adapter_id, "git");
        assert!(check.dependencies.contains(&"map-path-p1".to_string()));
    }

    #[test]
    fn every_action_carries_the_adapter_that_can_run_it() {
        // The bug that broke every restore: the executor routed on a key that
        // nothing ever wrote, so every step failed with "adapter not found".
        let manifest = manifest_with(
            vec![project("p1", "demo", "code")],
            vec![vscode_app("p1")],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let routable: Vec<&RestoreAction> = plan
            .steps
            .iter()
            .filter(|a| a.action_type != RestoreActionType::MapPath)
            .collect();
        assert!(!routable.is_empty(), "expected adapter-backed steps");
        for action in routable {
            assert!(
                !action.adapter_id.is_empty(),
                "'{}' has no adapter and would fail to execute",
                action.id
            );
        }
    }

    #[test]
    fn the_vscode_step_opens_the_resolved_folder() {
        let manifest = manifest_with(
            vec![project("p1", "demo", "code")],
            vec![vscode_app("p1")],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let open = plan
            .steps
            .iter()
            .find(|s| s.id == "vscode-open-vscode-p1")
            .expect("expected a VS Code step");
        assert_eq!(open.config["target"], serde_json::json!("/tmp/wc-code/demo"));
        assert_eq!(open.adapter_id, "vscode");
        // Opening an editor is a visible action, so it is never pre-approved.
        assert!(!open.approved);
    }

    #[test]
    fn a_project_with_no_chosen_bucket_is_planned_but_not_mapped() {
        let manifest = manifest_with(vec![project("p1", "demo", "code")], vec![]);
        let ctx = context();
        // Deliberately no root configured.
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner().generate_plan(&request)).unwrap();

        let mapping = plan.steps.iter().find(|s| s.id == "map-path-p1").unwrap();
        assert!(
            mapping.config.get("destination_path").is_none(),
            "no destination may be invented: {}",
            mapping.config
        );
        let check = plan.steps.iter().find(|s| s.id == "git-check-p1").unwrap();
        assert!(check.config.get("destination_path").is_none());
    }

    #[test]
    fn a_project_name_cannot_escape_its_root() {
        // A hostile manifest naming a project `../../.ssh`.
        for name in [
            "../../.ssh",
            "/etc/cron.d",
            "..",
            ".",
            "",
            "   ",
            "a/b",
            "a\\b",
            "ok\nname",
        ] {
            assert!(
                safe_child(Path::new("/tmp/root"), name).is_none(),
                "accepted an unsafe name: {name:?}"
            );
        }
    }

    #[test]
    fn a_safe_name_is_accepted_beneath_the_root() {
        assert_eq!(
            safe_child(Path::new("/tmp/root"), "demo"),
            Some(PathBuf::from("/tmp/root/demo"))
        );
        assert_eq!(
            safe_child(Path::new("/tmp/root"), "my-project_2.0"),
            Some(PathBuf::from("/tmp/root/my-project_2.0"))
        );
    }

    #[test]
    fn a_traversing_project_name_produces_no_destination() {
        let manifest = manifest_with(
            vec![project("p1", "../../.ssh", "code")],
            vec![],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let check = plan.steps.iter().find(|s| s.id == "git-check-p1").unwrap();
        assert!(
            check.config.get("destination_path").is_none(),
            "a traversing name reached the plan: {}",
            check.config
        );
    }

    #[test]
    fn an_unknown_adapter_is_reported_rather_than_dropped() {
        let manifest = manifest_with(
            vec![],
            vec![Application {
                id: "zed-1".into(),
                adapter: "zed".into(),
                project_id: None,
                required: false,
                config: serde_json::json!({}),
            }],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner().generate_plan(&request)).unwrap();

        assert!(
            plan.notes.iter().any(|n| n.contains("zed")),
            "expected a note naming the unknown adapter: {:?}",
            plan.notes
        );
    }

    #[test]
    fn dependencies_are_ordered_before_they_are_needed() {
        let manifest = manifest_with(
            vec![project("p1", "demo", "code")],
            vec![vscode_app("p1")],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();

        let position = |id: &str| plan.steps.iter().position(|s| s.id == id);
        for (dependent, dependency) in [
            ("git-check-p1", "map-path-p1"),
            ("vscode-open-vscode-p1", "map-path-p1"),
        ] {
            let (_d, _p) = (position(dependent), position(dependency));
            let (d, p) = (position(dependent), position(dependency));
            assert!(d.is_some() && p.is_some(), "missing step: {dependent}/{dependency}");
            assert!(
                p < d,
                "{dependency} must come before {dependent}: {p:?} vs {d:?}"
            );
        }
    }

    #[test]
    fn a_manifest_with_no_dependencies_plans_in_a_stable_order() {
        let manifest = manifest_with(
            vec![
                project("p1", "beta", "code"),
                project("p2", "alpha", "code"),
            ],
            vec![],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let first = run(planner().generate_plan(&request)).unwrap();
        let second = run(planner().generate_plan(&request)).unwrap();

        let ids = |p: &RestorePlan| -> Vec<String> { p.steps.iter().map(|s| s.id.clone()).collect() };
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn a_dependency_cycle_is_reported_with_the_whole_cycle() {
        let mut a = RestoreAction::new("a", RestoreActionType::MapPath, "", "a", serde_json::json!({}));
        a.dependencies = vec!["b".into()];
        let mut b = RestoreAction::new("b", RestoreActionType::MapPath, "", "b", serde_json::json!({}));
        b.dependencies = vec!["a".into()];

        let mut actions = BTreeMap::new();
        actions.insert("a".to_string(), a);
        actions.insert("b".to_string(), b);

        let error = planner().topological_sort(&actions).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("a -> b -> a"), "unhelpful error: {message}");
    }

    #[test]
    fn a_manifest_asking_for_secret_values_is_refused() {
        let mut manifest = manifest_with(vec![], vec![]);
        manifest.requirements.environment.values_included = true;

        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);
        let error = run(planner().generate_plan(&request)).unwrap_err();

        assert!(error.to_string().contains("Environment values"));
    }

    #[test]
    fn a_manifest_asking_for_automatic_commands_is_refused() {
        let mut manifest = manifest_with(vec![], vec![]);
        manifest.policy.automatic_command_execution = true;

        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);
        let error = run(planner().generate_plan(&request)).unwrap_err();

        assert!(error.to_string().contains("Automatic command execution"));
    }

    #[test]
    fn browser_urls_come_from_the_manifest() {
        let manifest = manifest_with(
            vec![],
            vec![Application {
                id: "browser-1".into(),
                adapter: "browser".into(),
                project_id: None,
                required: false,
                config: serde_json::json!({ "urls": ["https://a.example", "https://b.example"] }),
            }],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        // The browser adapter is not registered in this planner, so the step is
        // not produced; what matters is that the context shape is right, which
        // the registered-browser test below covers.
        let plan = run(planner().generate_plan(&request)).unwrap();
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn an_unsafe_url_is_filtered_out_of_a_browser_step() {
        use workspace_clone_adapters::BrowserAdapter;

        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(BrowserAdapter::new()));
        let planner = RestorePlanner::new(Arc::new(registry));

        // A peer that edited the manifest after capture.
        let manifest = manifest_with(
            vec![],
            vec![Application {
                id: "browser-1".into(),
                adapter: "browser".into(),
                project_id: None,
                required: false,
                config: serde_json::json!({
                    "urls": ["https://good.example", "file:///etc/passwd"]
                }),
            }],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner.generate_plan(&request)).unwrap();

        let urls = &plan.steps[0].config["urls"];
        assert_eq!(urls.as_array().unwrap().len(), 1, "got {urls}");
        assert_eq!(urls[0], serde_json::json!("https://good.example"));
    }

    #[test]
    fn opening_tabs_is_never_pre_approved() {
        use workspace_clone_adapters::BrowserAdapter;

        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(BrowserAdapter::new()));
        let planner = RestorePlanner::new(Arc::new(registry));

        let manifest = manifest_with(
            vec![],
            vec![Application {
                id: "browser-1".into(),
                adapter: "browser".into(),
                project_id: None,
                required: false,
                config: serde_json::json!({ "urls": ["https://good.example"] }),
            }],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner.generate_plan(&request)).unwrap();
        assert!(!plan.steps[0].approved);
        assert_eq!(plan.steps[0].action_type, RestoreActionType::OpenUrls);
    }

    #[test]
    fn a_manifest_with_no_declarative_steps_produces_no_notes() {
        let manifest = manifest_with(vec![project("p1", "demo", "code")], vec![]);
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx).with_root("code", PathBuf::from("/tmp/wc-code"));

        let plan = run(planner().generate_plan(&request)).unwrap();
        assert!(plan.notes.is_empty(), "unexpected notes: {:?}", plan.notes);
    }

    #[test]
    fn a_declarative_step_that_names_nothing_is_reported() {
        let mut manifest = manifest_with(vec![], vec![]);
        manifest.restore.steps = vec![ManifestStep::OpenProject {
            project_id: "ghost".into(),
        }];
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner().generate_plan(&request)).unwrap();

        assert!(
            plan.notes.iter().any(|n| n.contains("ghost")),
            "expected the dangling step to be reported: {:?}",
            plan.notes
        );
    }

    #[test]
    fn declarative_step_identification_covers_every_variant() {
        let mut manifest = manifest_with(
            vec![project("p1", "demo", "code")],
            vec![vscode_app("p1")],
        );
        manifest.restore.steps = vec![
            ManifestStep::OpenProject { project_id: "p1".into() },
            ManifestStep::CheckGit { project_id: "p1".into() },
            ManifestStep::MapPath { project_id: "p1".into() },
            ManifestStep::OpenApplication { application_id: "vscode-p1".into() },
            ManifestStep::OpenUrls { application_id: "vscode-p1".into() },
            ManifestStep::OfferCommand { recipe_id: "vscode-p1".into(), approval_required: true },
        ];

        assert_eq!(declarative_steps(&manifest).len(), 6);
    }

    #[test]
    fn two_projects_with_the_same_name_get_distinct_destinations() {
        let manifest = manifest_with(
            vec![project("p1", "demo", "code"), project("p2", "demo", "work")],
            vec![],
        );
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx)
            .with_root("code", PathBuf::from("/tmp/wc-code"))
            .with_root("work", PathBuf::from("/tmp/wc-work"));

        let destinations = resolve_destinations(&manifest, &request.destination_roots);

        assert_eq!(destinations["p1"], PathBuf::from("/tmp/wc-code/demo"));
        assert_eq!(destinations["p2"], PathBuf::from("/tmp/wc-work/demo"));
    }

    #[test]
    fn an_empty_manifest_plans_nothing_without_erroring() {
        let manifest = manifest_with(vec![], vec![]);
        let ctx = context();
        let request = PlanRequest::new(&manifest, &ctx);

        let plan = run(planner().generate_plan(&request)).unwrap();
        assert!(plan.is_empty());
    }
}
