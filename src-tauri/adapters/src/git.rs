//! Git adapter for repository detection and operations.

use crate::scan::read_git_summary;
use crate::traits::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tracing::debug;
use workspace_clone_core::{manifest::*, AdapterError, Result};

pub struct GitAdapter;

impl GitAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Walk up from `path` to the repository root, so selecting a subdirectory
    /// still captures the repository that contains it.
    fn repo_root(path: &Path) -> Option<PathBuf> {
        let mut current = Some(path);
        while let Some(dir) = current {
            if dir.join(".git").exists() {
                return Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
        None
    }
}

#[async_trait]
impl WorkspaceAdapter for GitAdapter {
    fn id(&self) -> &str {
        "git"
    }

    fn version(&self) -> u32 {
        2
    }

    fn supported_platforms(&self) -> Vec<&'static str> {
        vec!["windows", "macos", "linux", "all"]
    }

    async fn detect(&self, _context: &LocalContext) -> Result<DetectionResult> {
        let mut metadata = std::collections::HashMap::new();

        // Treat a non-zero exit as "git is absent", not as an error: a machine
        // without git is a normal state that preflight must report on, not a
        // failure that aborts detection for every adapter.
        let version = Command::new("git")
            .args(["--version"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        let available = version.is_some();
        metadata.insert(
            "version".to_string(),
            serde_json::json!(version.clone().unwrap_or_default()),
        );
        metadata.insert("available".to_string(), serde_json::json!(available));

        Ok(DetectionResult {
            adapter_id: self.id().to_string(),
            available,
            version,
            path: which::which("git")
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            metadata,
        })
    }

    async fn capture(
        &self,
        _context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext> {
        let mut projects: Vec<Project> = Vec::new();

        for selected in &selection.projects {
            let source = PathBuf::from(&selected.source_path);
            let Some(repo_root) = Self::repo_root(&source) else {
                // A plain directory is still a valid project; it just has no
                // git state. Recording it is better than dropping it silently,
                // because the user chose it.
                debug!("{} is not inside a git repository", selected.source_path);
                projects.push(Project {
                    id: selected.id.clone(),
                    name: selected.name.clone(),
                    source_path_hint: redact_path(&source),
                    destination_location_id: selected.destination_location_id.clone(),
                    git: None,
                });
                continue;
            };

            let summary = read_git_summary(&repo_root);
            let git = summary.map(|s| GitInfo {
                remote_hint: s.remote_hint,
                branch: s.branch,
                commit: s.commit,
                dirty_worktree: s.dirty,
                // The dirty *flag* is captured; the contents are not, and never
                // will be. Saying so explicitly keeps the manifest honest.
                dirty_state_captured: false,
            });

            projects.push(Project {
                id: selected.id.clone(),
                name: selected.name.clone(),
                source_path_hint: redact_path(&repo_root),
                destination_location_id: selected.destination_location_id.clone(),
                git,
            });
        }

        Ok(PortableContext {
            adapter_id: self.id().to_string(),
            data: serde_json::to_value(projects)?,
        })
    }

    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>> {
        let mut results = Vec::new();

        let git_path = which::which("git").ok();
        let version = Command::new("git")
            .args(["--version"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        for req in requirements {
            if req.adapter_id != self.id() {
                continue;
            }

            let available = version.is_some();
            let (status, evidence) = match &git_path {
                Some(path) if available => (
                    CheckStatus::ReadyVerified,
                    format!("git found at {}", path.display()),
                ),
                _ => (CheckStatus::Unknown, "git is not on PATH".to_string()),
            };

            results.push(CheckResult {
                requirement_id: req.id.clone(),
                status,
                evidence,
                freshness: chrono::Utc::now(),
                action: if available {
                    None
                } else {
                    Some(RemediationAction {
                        label: "Install Git".to_string(),
                        action_type: ActionType::InstallApp,
                        url: Some("https://git-scm.com/downloads".to_string()),
                        command: None,
                    })
                },
            });
        }

        Ok(results)
    }

    async fn plan_restore(&self, context: &PortableContext) -> Result<Vec<RestoreAction>> {
        let projects: Vec<Project> = serde_json::from_value(context.data.clone())
            .map_err(|e| AdapterError::Capture(e.to_string()))?;

        let mut actions = Vec::new();
        for project in projects {
            let config = serde_json::to_value(&project)?;

            actions.push(RestoreAction {
                id: format!("git-check-{}", project.id),
                action_type: RestoreActionType::CheckGit,
                adapter_id: self.id().to_string(),
                description: if project.git.is_some() {
                    format!("Verify the Git checkout for {}", project.name)
                } else {
                    format!("Open {} (no repository detected)", project.name)
                },
                // A missing repository is reported, never repaired: cloning
                // or resetting is the user's decision, not ours.
                required: false,
                approved: true,
                config,
                dependencies: vec![],
            });
        }

        Ok(actions)
    }

    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        match action.action.action_type {
            RestoreActionType::CheckGit => {
                let started = Instant::now();
                let project: Project = serde_json::from_value(action.action.config.clone())
                    .map_err(|e| AdapterError::RestoreAction(e.to_string()))?;

                // The planner resolves the destination path; without it there
                // is nothing to inspect.
                let Some(destination) = action
                    .resolved_config
                    .get("destination_path")
                    .and_then(|v| v.as_str())
                else {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Manual,
                        message: format!(
                            "No destination path was chosen for {}, so the repository was not inspected",
                            project.name
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let path = PathBuf::from(destination);
                if !path.exists() {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Manual,
                        message: format!(
                            "{} does not exist at the chosen destination. Clone it, then re-run restore.",
                            project.name
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                }

                if !path.join(".git").exists() {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Manual,
                        message: format!("{} exists but is not a Git checkout", project.name),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                }

                let summary = read_git_summary(&path);
                let Some(summary) = summary else {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Manual,
                        message: format!(
                            "{} is a Git checkout but its state could not be read",
                            project.name
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                // Report a branch mismatch as information, not failure. The
                // destination legitimately sits on a different branch.
                let (expected, actual) = (
                    project.git.as_ref().map(|g| g.branch.as_str()),
                    summary.branch.as_str(),
                );
                let branch_note = match expected {
                    Some(want) if want != actual => {
                        format!(" Expected branch '{want}' but the checkout is on '{actual}'.")
                    }
                    _ => String::new(),
                };

                let state_note = if summary.dirty {
                    " with uncommitted changes, which were left untouched."
                } else {
                    " and the worktree is clean."
                };

                let message = format!(
                    "{} is on '{actual}'{branch_note}{state_note}",
                    project.name
                );

                Ok(ActionResult {
                    action_id: action.action.id.clone(),
                    status: ActionStatus::Success,
                    message,
                    duration_ms: started.elapsed().as_millis() as u64,
                    details: Some(serde_json::to_string(&serde_json::json!({
                        "path": destination,
                        "branch": summary.branch,
                        "commit": summary.commit,
                        "dirty": summary.dirty,
                        "remote_hint": summary.remote_hint,
                    }))?),
                })
            }
            _ => Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Failed,
                message: format!(
                    "The git adapter cannot execute a {:?} action",
                    action.action.action_type
                ),
                duration_ms: 0,
                details: None,
            }),
        }
    }
}

/// Reduce an absolute path to a recognisable but non-identifying hint.
///
/// The user's home directory often contains their real name, so it is replaced
/// with `~`. The last two components are kept because that is what makes a path
/// recognisable to its owner.
///
/// A home-shaped prefix is dropped even when it is not *this* user's home. A
/// path like `/Users/alice/notes` is outside the home directory on any machine
/// that is not Alice's, so the prefix is not stripped by the first step and
/// `alice` would otherwise survive into the hint -- and the hint is sealed and
/// sent to another device.
pub fn redact_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let home = dirs::home_dir().unwrap_or_default();
    let home = home.to_string_lossy().to_string();

    let relative = if !home.is_empty() && text.starts_with(&home) {
        text[home.len()..].trim_start_matches(['/', '\\']).to_string()
    } else {
        text.to_string()
    };

    let mut components: Vec<&str> = relative
        .split(['/', '\\'])
        .filter(|c| !c.is_empty())
        .collect();

    // Drop a leading account-directory prefix. The name is a person's name far
    // more often than it is a useful directory name.
    if components.len() > 2 && is_home_shaped(&components) {
        components.drain(0..2);
    }

    if components.is_empty() {
        "~".to_string()
    } else if components.len() == 1 {
        format!("~/{}", components[0])
    } else {
        // Keep the parent and the leaf, drop everything in between.
        let leaf = components[components.len() - 1];
        let parent = components[components.len() - 2];
        format!("~/{parent}/{leaf}")
    }
}

/// Whether a path's leading components are a home directory prefix.
///
/// `/Users/alice` and `/home/alice` are the two layouts in use, with `/home` also
/// covering the `SUSE`-style `/home/alice` on Linux.
fn is_home_shaped(components: &[&str]) -> bool {
    matches!(components.first().copied(), Some("Users") | Some("home"))
}

#[cfg(test)]
mod tests {
    use super::*;
    // The scrubber lives in `scan`; `git` calls it rather than owning a copy, and
    // the tests assert on that single implementation.
    use crate::scan::scrub_remote_url;

    #[test]
    fn redaction_strips_the_home_prefix() {
        let home = dirs::home_dir().unwrap();
        let project = home.join("code/openshorts");
        let hint = redact_path(&project);

        assert!(hint.starts_with("~/"), "got {hint}");
        assert!(!hint.contains(&home.to_string_lossy().to_string()));
        assert!(hint.contains("openshorts"), "the leaf name must survive: {hint}");
    }

    #[test]
    fn redaction_keeps_only_the_last_two_components() {
        let path = Path::new("/opt/Deeply/Nested/Project");
        let hint = redact_path(path);
        assert_eq!(hint, "~/Nested/Project");
    }

    #[test]
    fn redaction_never_carries_an_account_name() {
        // The hint is sealed and sent to another device, so a person's name in
        // the path is a leak. This applies to paths outside the home directory,
        // where the home prefix is not stripped.
        for path in [
            "/Users/alice/notes",
            "/home/alice/notes",
            "/Users/alice/code/openshorts",
        ] {
            let hint = redact_path(Path::new(path));
            assert!(
                !hint.contains("alice"),
                "{path} redacted to {hint}, which still names the account"
            );
            assert!(hint.starts_with("~/"), "got {hint}");
        }
    }

    #[test]
    fn redaction_keeps_a_useful_directory_name_from_a_home_shaped_path() {
        // Dropping the account name must not reduce the hint to nothing.
        assert_eq!(redact_path(Path::new("/Users/alice/code/openshorts")), "~/code/openshorts");
    }

    #[test]
    fn redaction_handles_a_path_outside_home() {
        // A path with no home prefix is still reduced to two components so a
        // username elsewhere on the box is not transmitted.
        let hint = redact_path(Path::new("/opt/var/secret-place/project"));
        assert_eq!(hint, "~/secret-place/project");
    }

    #[test]
    fn redaction_of_a_bare_root_is_safe() {
        assert_eq!(redact_path(Path::new("/")), "~");
    }

    #[test]
    fn repo_root_is_found_from_a_subdirectory() {
        let dir = std::env::temp_dir().join("wc-git-root-test");
        let repo = dir.join("repo");
        let nested = repo.join("src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();

        assert_eq!(GitAdapter::repo_root(&nested), Some(repo.clone()));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn repo_root_is_none_outside_a_repository() {
        let dir = std::env::temp_dir().join("wc-not-a-repo-test");
        std::fs::create_dir_all(&dir).unwrap();
        // The temp dir itself is not a repository, so the walk reaches the
        // filesystem root and gives up.
        assert_eq!(GitAdapter::repo_root(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remote_scrubbing_is_reused_from_the_scanner() {
        // Guards against the two modules drifting apart.
        assert_eq!(
            scrub_remote_url("https://tok@github.com/a/b.git"),
            "https://github.com/a/b.git"
        );
    }
}
