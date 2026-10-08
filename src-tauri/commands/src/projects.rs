//! Project discovery commands.
//!
//! Capture needs a list of the repositories on this machine to choose from. The
//! roots are configurable, because "where my code lives" is a fact about the user
//! and not about the app: someone keeps work in `~/src`, someone else in
//! `~/Developer`, and a hardcoded guess that is wrong is worse than no list at
//! all, because it looks authoritative.
//!
//! Two rules this module follows:
//!
//! * **A root that cannot be read is reported, not skipped silently.** The scan
//!   itself tolerates a missing root -- a directory configured on another
//!   machine is not an error -- but the *caller* is told which roots were actually
//!   searched, so "no projects found" is distinguishable from "we looked
//!   somewhere that does not exist".
//! * **A path is never returned to the manifest.** The full path is returned here,
//!   to the capture screen, on this machine. It is the adapters' job to redact it
//!   on the way into a manifest.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{command, State};
use workspace_clone_adapters::scan::{scan_roots, ScannedProject};
use workspace_clone_core::{DatabaseError, Result};
use workspace_clone_db::{repository::SettingsRepository, DbPool};

/// Settings key holding the project roots, as a JSON array of paths.
pub const PROJECT_ROOTS_SETTING: &str = "project_roots";

/// The roots used when the user has not chosen any.
///
/// `~/projects` and `~/Documents` because those are where code most often lives on
/// a fresh machine, and because both are cheap to check. A root that does not
/// exist is skipped by the scan, so including them costs nothing on a machine
/// that uses neither.
pub fn default_project_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    vec![home.join("projects"), home.join("Documents")]
}

/// The configured roots, falling back to the defaults.
///
/// A stored value that is not a list of strings is ignored rather than treated as
/// "no roots", because a corrupt setting should not silently turn a working
/// capture screen into an empty one.
pub async fn configured_roots(pool: &DbPool) -> Vec<PathBuf> {
    let repo = SettingsRepository::new(pool.clone());
    let Ok(Some(stored)) = repo.get(PROJECT_ROOTS_SETTING).await else {
        return default_project_roots();
    };

    match serde_json::from_str::<Vec<String>>(&stored) {
        Ok(paths) if !paths.is_empty() => paths.into_iter().map(PathBuf::from).collect(),
        _ => default_project_roots(),
    }
}

/// A project folder the user may choose to include, whether or not it uses Git.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCandidate {
    /// Stable across scans, derived from the path, so selecting a project and
    /// coming back to it does not lose the selection.
    pub id: String,
    pub name: String,
    /// The absolute path. Stays on this machine.
    pub path: String,
    pub is_git_repo: bool,
    /// What the repository is on, for display. Never anything that needs a
    /// credential to read.
    pub git: Option<ProjectGitSummary>,
}

/// The part of a repository's state worth showing before choosing it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGitSummary {
    pub branch: String,
    pub commit: Option<String>,
    /// Whether the worktree has uncommitted changes. The contents are never
    /// captured, and the manifest says so explicitly.
    pub dirty: bool,
    /// A remote with any credentials removed.
    pub remote_hint: Option<String>,
}

impl From<ScannedProject> for ProjectCandidate {
    fn from(project: ScannedProject) -> Self {
        Self {
            id: project.id,
            name: project.name,
            path: project.repo_root,
            is_git_repo: project.is_git_repo,
            git: project.git.map(|g| ProjectGitSummary {
                branch: g.branch,
                commit: g.commit,
                dirty: g.dirty,
                // Scrubbed here rather than at capture time, so a credential in a
                // remote URL never reaches the screen either. It is the same value
                // the manifest will carry, which is what makes the display a
                // preview rather than a summary.
                remote_hint: g
                    .remote_hint
                    .as_deref()
                    .map(workspace_clone_core::manifest::scrub_url),
            }),
        }
    }
}

/// The result of a scan, including enough context to explain an empty list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScan {
    pub projects: Vec<ProjectCandidate>,
    /// Every root that was searched.
    pub roots: Vec<String>,
    /// Roots that were configured but do not exist. Named separately so the UI
    /// can say "that folder is not there" instead of showing nothing.
    pub missing_roots: Vec<String>,
    /// Set when a root exists but could not be listed, which is a different
    /// problem from a root that is not there.
    pub unreadable_roots: Vec<String>,
}

/// Scan the configured roots for Git repositories and recognizable project folders.
#[command]
pub async fn list_project_roots(pool: State<'_, DbPool>) -> Result<ProjectScan> {
    let roots = configured_roots(pool.inner()).await;

    let missing_roots: Vec<String> = roots
        .iter()
        .filter(|r| !r.exists())
        .map(|r| r.display().to_string())
        .collect();

    // A root that exists but cannot be listed is a permissions problem, not an
    // absent folder, and the two need different fixes from the user.
    let unreadable_roots: Vec<String> = roots
        .iter()
        .filter(|r| r.is_dir())
        .filter(|r| {
            std::fs::read_dir(r)
                .map(|mut d| d.next().is_some() || true)
                .is_err()
        })
        .map(|r| r.display().to_string())
        .collect();

    // The scan runs on the blocking pool: it walks directories, and doing that on
    // an async runtime thread would stall every other command for as long as the
    // filesystem takes.
    let projects = tauri::async_runtime::spawn_blocking(move || scan_roots(&roots)).await
        .map_err(|e| {
            DatabaseError::Connection(format!("The project scan did not finish: {e}"))
        })?;

    let roots: Vec<String> = configured_roots(pool.inner())
        .await
        .iter()
        .map(|r| r.display().to_string())
        .collect();

    Ok(ProjectScan {
        projects: projects.into_iter().map(ProjectCandidate::from).collect(),
        roots,
        missing_roots,
        unreadable_roots,
    })
}

/// Replace the configured roots.
#[command]
pub async fn set_project_roots(
    pool: State<'_, DbPool>,
    roots: Vec<String>,
) -> Result<Vec<String>> {
    let trimmed: Vec<String> = roots
        .into_iter()
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect();

    if trimmed.is_empty() {
        return Err(DatabaseError::Constraint(
            "Choose at least one folder to look for projects in".to_string(),
        )
        .into());
    }

    // Expanded and canonicalised on the way in, so the same folder reached as
    // `~/projects` and `/Users/me/projects` does not appear twice in a list.
    let mut normalized: Vec<String> = Vec::new();
    for root in &trimmed {
        let expanded = expand_home(std::path::Path::new(root));
        let canonical = std::fs::canonicalize(&expanded)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| expanded.display().to_string());
        if !normalized.contains(&canonical) {
            normalized.push(canonical);
        }
    }

    SettingsRepository::new(pool.inner().clone())
        .set(PROJECT_ROOTS_SETTING, &serde_json::to_string(&normalized)?)
        .await?;

    Ok(normalized)
}

/// Expand a leading `~`, which is how a person writes a home directory and not
/// how the filesystem spells one.
fn expand_home(path: &std::path::Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };

    if text == "~" {
        return dirs::home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_tilde_is_expanded() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home(std::path::Path::new("~/code")), home.join("code"));
        assert_eq!(expand_home(&home), home);
    }

    #[test]
    fn a_path_without_a_tilde_is_left_alone() {
        // An absolute path, and a relative one, are both the caller's business.
        assert_eq!(
            expand_home(std::path::Path::new("/opt/code")),
            PathBuf::from("/opt/code")
        );
        assert_eq!(
            expand_home(std::path::Path::new("code")),
            PathBuf::from("code")
        );
    }

    /// The defaults must be inside the home directory and must not be the home
    /// directory itself: scanning `$HOME` would walk the whole of a user's
    /// Documents, Downloads and Library, and is slow enough to look like a hang.
    #[test]
    fn the_default_roots_are_below_the_home_directory() {
        let defaults = default_project_roots();
        let home = dirs::home_dir().unwrap();

        assert!(!defaults.is_empty());
        for root in &defaults {
            assert!(root.starts_with(&home), "{} is not under {}", root.display(), home.display());
            assert_ne!(root, &home, "scanning the whole home directory is not a default");
        }
    }

    /// A remote shown before a capture is chosen must be the same value the
    /// manifest will carry, and must not carry a credential.
    #[test]
    fn a_remote_credential_is_scrubbed_before_it_is_displayed() {
        use workspace_clone_adapters::scan::GitSummary;

        let candidate = ProjectCandidate::from(ScannedProject {
            id: "p1".into(),
            name: "demo".into(),
            path: "/Users/me/code/demo".into(),
            repo_root: "/Users/me/code/demo".into(),
            is_git_repo: true,
            git: Some(GitSummary {
                branch: "main".into(),
                commit: Some("abc123".into()),
                dirty: false,
                remote_hint: Some("https://user:hunter2@github.com/o/demo.git".into()),
            }),
        });

        let remote = candidate.git.unwrap().remote_hint.unwrap();
        assert!(!remote.contains("hunter2"), "leaked: {remote}");
        assert!(remote.contains("github.com"), "the location survives: {remote}");
    }

    /// A project that is not a git repository is still a valid thing to capture,
    /// so it must appear rather than being filtered out.
    #[test]
    fn a_project_with_no_git_state_is_still_offered() {
        let candidate = ProjectCandidate::from(ScannedProject {
            id: "p1".into(),
            name: "notes".into(),
            path: "/Users/me/code/notes".into(),
            repo_root: "/Users/me/code/notes".into(),
            is_git_repo: false,
            git: None,
        });

        assert!(!candidate.is_git_repo);
        assert!(candidate.git.is_none());
    }
}
