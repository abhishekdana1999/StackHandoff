//! Git adapter for repository detection and operations.

use crate::scan::read_git_summary;
use crate::traits::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tracing::debug;
use workspace_clone_core::{manifest::*, AdapterError, Result};
use workspace_clone_files::snapshot::{is_excluded_dir, is_excluded_file};

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
            let git = if let Some(summary) = summary {
                // A dirty worktree now travels as a `git apply`-able delta in
                // `patch`; the restore layer applies that delta to the
                // destination checkout instead of overwriting the whole tree,
                // so `git status` where it lands shows exactly what changed
                // here. `dirty_state_captured` records whether the contents
                // made it.
                let patch = capture_patch(&repo_root);
                Some(GitInfo {
                    remote_hint: summary.remote_hint,
                    branch: summary.branch,
                    commit: summary.commit,
                    dirty_worktree: summary.dirty,
                    dirty_state_captured: patch.is_some(),
                    patch,
                })
            } else {
                None
            };

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

            // When the capture device recorded the working-tree delta, restoring
            // means *applying that delta* to the destination checkout -- not
            // rewriting the whole tree, which is what made `git status` on the
            // destination list every file as modified. The planner skips the
            // whole-tree file extraction for such projects, so this step is
            // their one and only content step. `required` stays false: a patch
            // that cannot apply (wrong base commit, e.g.) is reported with the
            // remedy, never allowed to block the rest of the restore.
            let patch = project
                .git
                .as_ref()
                .and_then(|g| g.patch.as_deref())
                .filter(|p| !p.trim().is_empty());
            if let Some(patch) = patch {
                let mut apply_config = serde_json::to_value(&project)?;
                if let Some(map) = apply_config.as_object_mut() {
                    // The patch is stored verbatim: `git apply` rejects a hunk
                    // whose final body line was trimmed of its newline.
                    map.insert(
                        "patch".to_string(),
                        serde_json::Value::String(patch.to_string()),
                    );
                    if let Some(commit) = project.git.as_ref().and_then(|g| g.commit.as_ref()) {
                        map.insert(
                            "source_commit".to_string(),
                            serde_json::Value::String(commit.clone()),
                        );
                    }
                }
                actions.push(RestoreAction {
                    id: format!("git-apply-{}", project.id),
                    action_type: RestoreActionType::ApplyGitPatch,
                    adapter_id: self.id().to_string(),
                    description: format!(
                        "Apply the captured git changes for {} to its destination",
                        project.name
                    ),
                    required: false,
                    approved: true,
                    config: apply_config,
                    dependencies: vec![],
                });
            }
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
            RestoreActionType::ApplyGitPatch => {
                let started = Instant::now();
                let id = action.action.id.clone();
                let name = action
                    .action
                    .config
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("the project");

                let Some(destination) = action
                    .resolved_config
                    .get("destination_path")
                    .and_then(|v| v.as_str())
                else {
                    return Ok(ActionResult {
                        action_id: id,
                        status: ActionStatus::Manual,
                        message: format!(
                            "No destination path was chosen for {name}, so its captured changes were not applied"
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let path = PathBuf::from(destination);
                if !path.exists() {
                    return Ok(ActionResult {
                        action_id: id,
                        status: ActionStatus::Manual,
                        message: format!(
                            "{name} does not exist at the chosen destination. Clone it, then re-run restore."
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                }

                if !path.join(".git").exists() {
                    return Ok(ActionResult {
                        action_id: id,
                        status: ActionStatus::Manual,
                        message: format!(
                            "{name} exists but is not a Git checkout. Clone it there, then re-run restore, so the captured changes can be applied."
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                }

                let Some(patch) = action
                    .action
                    .config
                    .get("patch")
                    .and_then(|v| v.as_str())
                    .filter(|p| !p.trim().is_empty())
                else {
                    return Ok(ActionResult {
                        action_id: id,
                        status: ActionStatus::Failed,
                        message: format!(
                            "The restore plan carried no patch to apply for {name}"
                        ),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let source_commit = action
                    .action
                    .config
                    .get("source_commit")
                    .and_then(|v| v.as_str());

                Ok(apply_git_patch(
                    &id,
                    &name,
                    &path,
                    patch,
                    source_commit,
                    started,
                ))
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

/// Capture a repository's working-tree delta as a patch `git apply` can apply.
///
/// The delta is the tracked changes (`git diff HEAD`; staged and unstaged
/// together) plus one new-file hunk per untracked, non-ignored file. Untracked
/// files travel as hunks rather than via `git add -N`, because that rewrites
/// the user's index and a capture must never change the repository it is
/// reading.
///
/// `None` means "nothing to apply": an unborn `HEAD`, a clean worktree, or git
/// failing mid-way. A missing patch degrades the restore to the old whole-tree
/// copy, never to a hard error.
fn capture_patch(repo_root: &Path) -> Option<String> {
    // No HEAD means there is nothing to diff against (a fresh `git init`), and
    // `git diff HEAD` would error rather than produce an empty patch.
    if git_ok(repo_root, &["rev-parse", "--verify", "HEAD"]).is_none() {
        return None;
    }

    let mut patch = String::new();
    if let Some(tracked) = git_out(
        repo_root,
        &["diff", "HEAD", "--binary", "--no-color", "--no-ext-diff"],
    ) {
        patch.push_str(&tracked);
    }

    // Untracked, non-ignored files, sorted so the same repo yields the same
    // patch. One `git diff --no-index /dev/null <rel>` call per file keeps a
    // hostile or broken name from aborting the whole capture, and git emits the
    // correct new-file hunk (mode included, binary handled) for any regular
    // file. An absolute path here would land in the patch as an absolute
    // destination, so the repository-relative path is what gets diffed.
    let mut untracked =
        git_out_nul(repo_root, &["ls-files", "--others", "--exclude-standard", "-z"])
            .unwrap_or_default();
    untracked.sort();
    for rel in untracked {
        let abs = repo_root.join(&rel);
        if let Some(reason) = patch_skips(&rel, &abs) {
            debug!("git patch skips untracked {rel}: {reason}");
            continue;
        }
        let args = [
            "diff",
            "--no-index",
            "--binary",
            "--no-color",
            "--no-ext-diff",
            "/dev/null",
            rel.as_str(),
        ];
        // `git diff --no-index -- /dev/null <file>` exits 1 when the files
        // differ -- which is every call that produced a hunk worth keeping --
        // so this path accepts the "differences found" exit as success.
        if let Some(hunk) = git_out_accept_diff(repo_root, &args) {
            patch.push_str(&hunk);
        }
    }

    if patch.trim().is_empty() {
        return None;
    }
    Some(patch)
}

/// Why an untracked file is *not* put in the patch, when it should be left out.
///
/// Mirrors the file snapshot's denylist so the manifest and the archive refuse
/// the same things: a secret or machine-local file must not slip into the
/// manifest through a patch hunk when the archive would not carry it either.
fn patch_skips(rel: &str, abs: &Path) -> Option<&'static str> {
    match std::fs::symlink_metadata(abs) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Some("symlinks are not carried (the snapshot refuses them too)")
        }
        Ok(meta) if !meta.file_type().is_file() => return Some("not a regular file"),
        Err(_) => return Some("could not be read"),
        _ => {}
    }
    if rel.split('/').any(is_excluded_dir) {
        return Some("inside a denylisted directory");
    }
    if rel
        .rsplit('/')
        .next()
        .is_some_and(|name| is_excluded_file(name))
    {
        return Some("on the file denylist");
    }
    None
}

/// Run git inside `repo`; return its stdout when the command succeeded.
fn git_ok(repo: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// `git_ok`'s stdout as text.
fn git_out(repo: &Path, args: &[&str]) -> Option<String> {
    git_ok(repo, args).and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Like [`git_out`], but accepts the "differences found" exit code 1 that
/// `git diff --no-index` returns.
fn git_out_accept_diff(repo: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .ok()?;
    let code = output.status.code()?;
    match code {
        0 | 1 => String::from_utf8(output.stdout).ok(),
        _ => None,
    }
}

/// NUL-separated output (e.g. `git ls-files -z`) as a list of strings.
fn git_out_nul(repo: &Path, args: &[&str]) -> Option<Vec<String>> {
    let bytes = git_ok(repo, args)?;
    let mut out = Vec::new();
    for chunk in bytes.split(|b| *b == 0) {
        if chunk.is_empty() {
            continue;
        }
        out.push(String::from_utf8_lossy(chunk).into_owned());
    }
    Some(out)
}

/// Apply a captured git delta to a destination checkout and report the result.
///
/// `git apply` is the engine: its path validation refuses anything that would
/// write outside the checkout, and `--binary` covers binary files. The patch
/// came from another device, but every write it performs is a write inside the
/// destination the user chose, which is the same trust the file archive runs
/// under. A patch that does not apply (the checkout's history differs from the
/// capture) is reported with the exact commit to check out, not silently
/// dropped.
fn apply_git_patch(
    id: &str,
    project: &str,
    destination: &Path,
    patch: &str,
    source_commit: Option<&str>,
    started: Instant,
) -> ActionResult {
    // A unique temp file keeps the patch out of the command line and gives
    // `git apply` something to read like a reviewable artifact.
    let token = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let patch_path = std::env::temp_dir().join(format!(
        "wc-git-delta-{}-{token}.patch",
        std::process::id()
    ));
    if let Err(e) = std::fs::write(&patch_path, patch) {
        return ActionResult {
            action_id: id.to_string(),
            status: ActionStatus::Failed,
            message: format!("The captured changes for {project} could not be staged: {e}"),
            duration_ms: 0,
            details: None,
        };
    }

    // `git apply --numstat` validates and counts without touching the working
    // tree, so the report can say exactly how much changed.
    let stats = Command::new("git")
        .args(["apply", "--numstat", &patch_path.to_string_lossy()])
        .current_dir(destination)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|o| summarize_numstat(&o));

    let apply = Command::new("git")
        .args(["apply", "--binary", "--whitespace=nowarn", &patch_path.to_string_lossy()])
        .current_dir(destination)
        .output();
    let _ = std::fs::remove_file(&patch_path);

    let Ok(output) = apply else {
        return ActionResult {
            action_id: id.to_string(),
            status: ActionStatus::Failed,
            message: format!("git could not run to apply the captured changes for {project}"),
            duration_ms: started.elapsed().as_millis() as u64,
            details: None,
        };
    };

    if output.status.success() {
        let summary = stats
            .unwrap_or_else(|| "the captured changes".to_string());
        return ActionResult {
            action_id: id.to_string(),
            status: ActionStatus::Success,
            message: format!("Applied {summary} to {}", destination.display()),
            duration_ms: started.elapsed().as_millis() as u64,
            details: Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        };
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let guidance = match source_commit {
        Some(commit) => format!(
            "Put this checkout on the commit the workspace was captured from \
             (`git checkout {commit}`), resolve any local changes, and restore again."
        ),
        None => "Check the checkout is on the commit the workspace was captured \
                 from, resolve any local changes, and restore again."
            .to_string(),
    };
    ActionResult {
        action_id: id.to_string(),
        status: ActionStatus::Manual,
        message: format!(
            "The captured changes for {project} did not apply to {}: {stderr} {guidance}",
            destination.display()
        ),
        duration_ms: started.elapsed().as_millis() as u64,
        details: None,
    }
}

/// Turn `git apply --numstat`'s `added<TAB>deleted<TAB>path` lines into a human
/// summary.
fn summarize_numstat(output: &str) -> String {
    let mut files = 0u64;
    let mut added = 0u64;
    let mut deleted = 0u64;
    for line in output.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 2 {
            continue;
        }
        files += 1;
        if let Ok(n) = parts[0].trim().parse::<u64>() {
            added += n;
        }
        if let Ok(n) = parts[1].trim().parse::<u64>() {
            deleted += n;
        }
    }
    fn plural(n: u64, word: &str) -> String {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    }
    format!(
        "{} ({}; {})",
        plural(files, "file change"),
        plural(added, "insertion"),
        plural(deleted, "deletion")
    )
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

    #[tokio::test]
    async fn a_non_git_project_is_captured_without_git_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "wc-git-adapter-non-git-project-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<h1>Project</h1>").unwrap();
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "plain-project".into(),
                name: "plain-project".into(),
                source_path: dir.to_string_lossy().into_owned(),
                destination_location_id: "code".into(),
            }],
            ..CaptureSelection::default()
        };

        let context = GitAdapter
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();
        let projects: Vec<Project> = serde_json::from_value(context.data).unwrap();
        let absolute_source = dir.to_string_lossy().into_owned();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "plain-project");
        assert!(projects[0].git.is_none());
        assert!(
            !projects[0]
                .source_path_hint
                .contains(absolute_source.as_str()),
            "the absolute source path must not enter the manifest"
        );

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

    // ---------------------------------------------------------------------
    // Working-tree delta capture and patch restore: the "I changed one file,
    // so restoring must change one file, not a hundred and forty-six" story.
    // ---------------------------------------------------------------------

    /// Run git in a scratch directory and assert it succeeded.
    fn git(scratch: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(scratch)
            .output()
            .expect("git must be installed to test the git adapter");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A scratch repository with one committed file and a known identity.
    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wc-git-delta-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "delta@test"]);
        git(&dir, &["config", "user.name", "Delta Test"]);
        std::fs::write(dir.join("tracked.txt"), "base\n").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-qm", "base"]);
        dir
    }

    fn status_short(repo: &Path) -> Vec<String> {
        let out = Command::new("git")
            .args(["status", "--short"])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect()
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn apply_patch_in(dest: &Path, patch: &str) {
        let patch_path =
            std::env::temp_dir().join(format!("wc-git-delta-apply-{}", std::process::id()));
        std::fs::write(&patch_path, patch).unwrap();
        git(
            dest,
            &["apply", "--binary", "--whitespace=nowarn", patch_path.to_string_lossy().as_ref()],
        );
        let _ = std::fs::remove_file(&patch_path);
    }

    fn fresh_clone(source: &Path, label: &str) -> PathBuf {
        let scratch =
            std::env::temp_dir().join(format!("wc-git-delta-clone-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).unwrap();
        let dest = scratch.join("dest");
        git(
            &scratch,
            &["clone", "-q", source.to_string_lossy().as_ref(), dest.to_string_lossy().as_ref()],
        );
        dest
    }

    fn apply_action(project: &Project, patch: &str) -> RestoreAction {
        // Carries the whole project plus the patch, exactly as plan_restore does.
        let mut map = serde_json::to_value(project)
            .unwrap()
            .as_object()
            .cloned()
            .unwrap();
        map.insert("patch".to_string(), serde_json::Value::String(patch.to_string()));
        if let Some(commit) = project.git.as_ref().and_then(|g| g.commit.as_ref()) {
            map.insert(
                "source_commit".to_string(),
                serde_json::Value::String(commit.clone()),
            );
        }
        RestoreAction {
            id: format!("git-apply-{}", project.id),
            action_type: RestoreActionType::ApplyGitPatch,
            adapter_id: "git".to_string(),
            description: "Apply the captured git changes".to_string(),
            required: false,
            approved: true,
            config: serde_json::Value::Object(map),
            dependencies: vec![],
        }
    }

    #[test]
    fn a_dirty_worktree_captures_a_patch_that_applies_to_an_identical_clone() {
        let source = repo("source");
        // One tracked modification plus one untracked file: the single change
        // the user made, plus the new file that used to ride the archive.
        std::fs::write(source.join("tracked.txt"), "base\nchanged\n").unwrap();
        std::fs::write(source.join("new.txt"), "new file\n").unwrap();

        let patch = capture_patch(&source).expect("a dirty worktree must capture its delta");
        assert!(patch.contains("diff --git a/tracked.txt"), "{patch}");
        assert!(patch.contains("diff --git a/new.txt"), "{patch}");

        // A fresh clone at the same commit, patched, ends at exactly the
        // source's uncommitted state -- one modification, one new file, and
        // nothing else (the pre-patch restore copied the whole tree over the
        // checkout and every tracked file showed as modified).
        let dest = fresh_clone(&source, "identical");
        apply_patch_in(&dest, &patch);

        let mut status = status_short(&dest);
        status.sort();
        assert_eq!(
            status,
            vec![" M tracked.txt", "?? new.txt"],
            "restoring must produce exactly the source's delta, nothing more"
        );
    }

    #[test]
    fn restoring_through_the_adapter_reports_and_applies_the_exact_delta() {
        let project = Project {
            id: "p1".to_string(),
            name: "demo".to_string(),
            source_path_hint: "~/code/demo".to_string(),
            destination_location_id: "code".to_string(),
            git: Some(GitInfo {
                remote_hint: None,
                branch: "main".to_string(),
                commit: None,
                dirty_worktree: true,
                dirty_state_captured: true,
                patch: None,
            }),
        };

        let source = repo("adapter-src");
        std::fs::write(source.join("tracked.txt"), "base\nchanged\n").unwrap();
        std::fs::write(source.join("new.txt"), "new\n").unwrap();
        std::fs::remove_file(source.join("tracked.txt")).unwrap();
        let patch = capture_patch(&source).unwrap();

        let dest = fresh_clone(&source, "adapter");
        let mut action = apply_action(&project, &patch);
        action.config["destination_path"] =
            serde_json::Value::String(dest.to_string_lossy().into_owned());
        let approved = ApprovedRestoreAction {
            action: action.clone(),
            resolved_config: action.config.clone(),
        };

        let result = rt().block_on(GitAdapter::new().execute(&approved)).unwrap();
        assert_eq!(result.status, ActionStatus::Success, "{}", result.message);
        assert!(
            result.message.contains("file change"),
            "the success must name the delta: {}",
            result.message
        );

        let mut status = status_short(&dest);
        status.sort();
        assert_eq!(
            status,
            vec![" D tracked.txt", "?? new.txt"],
            "the destination must show exactly the source's state"
        );
    }

    #[test]
    fn applying_to_a_different_base_reports_the_remedy() {
        let project = Project {
            id: "p1".to_string(),
            name: "demo".to_string(),
            source_path_hint: "~/code/demo".to_string(),
            destination_location_id: "code".to_string(),
            git: Some(GitInfo {
                remote_hint: None,
                branch: "main".to_string(),
                commit: Some("abc123".to_string()),
                dirty_worktree: true,
                dirty_state_captured: true,
                patch: None,
            }),
        };

        let source = repo("drift-src");
        std::fs::write(source.join("tracked.txt"), "base\nchanged\n").unwrap();
        let patch = capture_patch(&source).unwrap();

        // The destination clone has moved on: the same file was rewritten and
        // committed, so the captured context lines no longer match.
        let dest = fresh_clone(&source, "drift");
        std::fs::write(dest.join("tracked.txt"), "someone else's change\n").unwrap();
        git(&dest, &["commit", "-aqm", "drift"]);

        let mut action = apply_action(&project, &patch);
        action.config["destination_path"] =
            serde_json::Value::String(dest.to_string_lossy().into_owned());
        let approved = ApprovedRestoreAction {
            action: action.clone(),
            resolved_config: action.config.clone(),
        };

        let result = rt().block_on(GitAdapter::new().execute(&approved)).unwrap();
        assert_eq!(
            result.status,
            ActionStatus::Manual,
            "a base mismatch is guidance, not silence: {}",
            result.message
        );
        assert!(
            result.message.contains("git checkout abc123"),
            "the remedy must name the captured commit: {}",
            result.message
        );
    }

    #[test]
    fn the_delta_skips_files_the_snapshot_would_not_carry() {
        let source = repo("denied");
        std::fs::write(source.join(".env"), "API_KEY=secret\n").unwrap();
        std::fs::create_dir_all(source.join("node_modules/pkg")).unwrap();
        std::fs::write(source.join("node_modules/pkg/index.js"), "dep\n").unwrap();
        std::fs::write(source.join("wanted.txt"), "track me\n").unwrap();

        let patch = capture_patch(&source).expect("wanted.txt must make the delta non-empty");
        assert!(patch.contains("wanted.txt"), "{patch}");
        assert!(!patch.contains(".env"), "a secret must not leak through the patch: {patch}");
        assert!(
            !patch.contains("node_modules"),
            "a denylisted tree must not leak through the patch: {patch}"
        );
    }

    #[test]
    fn a_clean_worktree_captures_no_delta() {
        assert_eq!(capture_patch(&repo("clean")), None);
    }

    #[test]
    fn plan_restore_emits_an_apply_step_only_when_a_delta_was_captured() {
        let context = PortableContext {
            adapter_id: "git".to_string(),
            data: serde_json::json!([
                {
                    "id": "p1",
                    "name": "Patched",
                    "source_path_hint": "~/code/patched",
                    "destination_location_id": "code",
                    "git": {
                        "remote_hint": null,
                        "branch": "main",
                        "commit": "abc123",
                        "dirty_worktree": true,
                        "dirty_state_captured": true,
                        "patch": "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1 +1,2 @@\n base\n+change\n"
                    }
                },
                {
                    "id": "p2",
                    "name": "Clean",
                    "source_path_hint": "~/code/clean",
                    "destination_location_id": "code",
                    "git": {
                        "remote_hint": null,
                        "branch": "main",
                        "commit": "def456",
                        "dirty_worktree": false,
                        "dirty_state_captured": false,
                        "patch": null
                    }
                }
            ]),
        };

        let actions = rt()
            .block_on(GitAdapter::new().plan_restore(&context))
            .unwrap();
        let ids: Vec<&str> = actions.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"git-apply-p1"), "a captured delta must be applied: {ids:?}");
        assert!(
            !ids.contains(&"git-apply-p2"),
            "a clean repo has no delta to apply: {ids:?}"
        );
        assert!(ids.contains(&"git-check-p1"));
        assert!(ids.contains(&"git-check-p2"));
    }
}
