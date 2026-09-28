//! Project discovery.
//!
//! Walks the user's configured project roots and reports the git repositories it
//! finds, so the capture screen can show real repositories with real branch and
//! dirty-state data instead of placeholders.
//!
//! The walk is bounded in both depth and breadth. A user's home directory can
//! contain hundreds of thousands of files, and an unbounded scan would hang the
//! UI thread and traverse `node_modules`-scale trees.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::debug;

/// How deep below a root a repository may sit. `~/projects/api/service` is
/// three levels; anything deeper is usually a vendored or nested checkout.
const MAX_DEPTH: usize = 4;

/// Never descend into these. They hold dependencies and build output, not
/// projects, and are large enough to dominate the scan.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".nuxt",
    "dist",
    "build",
    "vendor",
    "Pods",
    "DerivedData",
    ".gradle",
    ".cache",
    ".git",
    "Library",
    "Applications",
];

/// Upper bound on repositories reported from a single root, so one monorepo
/// cannot produce an unusable list.
const MAX_RESULTS_PER_ROOT: usize = 200;

/// A git repository found on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedProject {
    pub id: String,
    pub name: String,
    /// Absolute path on this device. Never included in a manifest.
    pub path: String,
    /// Repository root, which may differ from `path` if the user selected a
    /// subdirectory.
    pub repo_root: String,
    pub is_git_repo: bool,
    pub git: Option<GitSummary>,
}

/// The git facts worth carrying forward.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitSummary {
    pub branch: String,
    pub commit: Option<String>,
    pub dirty: bool,
    /// Remote with any embedded credentials removed, and with the owner name
    /// kept, because "github.com/acme/api" is portable and
    /// "https://token@github.com/acme/api" is a leaked secret.
    pub remote_hint: Option<String>,
}

impl ScannedProject {
    /// A stable identifier derived from the absolute path, so re-scanning the
    /// same tree yields the same ids and selections survive a rescan.
    pub fn id_for(path: &Path) -> String {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in canonical.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let name = canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string());
        format!("{}-{:016x}", slugify(&name), hash)
    }
}

fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Scan each root for repositories, newest configuration first.
///
/// Roots that do not exist are skipped rather than reported as errors: a user
/// who configured `~/projects` on one machine and not another should still get
/// a usable list.
pub fn scan_roots(roots: &[PathBuf]) -> Vec<ScannedProject> {
    let mut found: Vec<ScannedProject> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            debug!("Skipping project root that is not a directory: {}", root.display());
            continue;
        }

        for dir in walk(root, MAX_DEPTH) {
            if found.len() >= MAX_RESULTS_PER_ROOT {
                debug!("Reached the per-root result cap at {}", root.display());
                break;
            }
            if !dir.join(".git").exists() {
                continue;
            }

            let repo_root = match dir.canonicalize() {
                Ok(p) => p,
                Err(_) => continue,
            };
            // A nested repository inside one already reported would otherwise
            // appear twice.
            if !seen.insert(repo_root.clone()) {
                continue;
            }

            let name = repo_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| repo_root.to_string_lossy().to_string());

            found.push(ScannedProject {
                id: ScannedProject::id_for(&repo_root),
                name,
                path: repo_root.to_string_lossy().to_string(),
                repo_root: repo_root.to_string_lossy().to_string(),
                is_git_repo: true,
                git: read_git_summary(&repo_root),
            });
        }
    }

    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Depth-first directory walk with the skip list applied.
fn walk(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut queue: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = queue.pop() {
        if depth > max_depth {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            // Unreadable directory: skip it rather than failing the whole scan.
            continue;
        };

        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') && name != ".config" {
                continue;
            }
            if SKIPPED_DIRS.contains(&name.as_ref()) {
                continue;
            }

            let path = entry.path();
            out.push(path.clone());
            queue.push((path, depth + 1));
        }
    }

    out
}

/// Read branch, commit, dirty state and a scrubbed remote from a repository.
///
/// Every git call is best-effort: a repository with a corrupt index, or one
/// mid-rebase, should still show up in the list with whatever is readable.
pub fn read_git_summary(repo_root: &Path) -> Option<GitSummary> {
    let branch = git_output(repo_root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|b| !b.is_empty() && b != "HEAD")
        .or_else(|| git_output(repo_root, &["symbolic-ref", "--short", "HEAD"]))
        .or_else(|| {
            // A detached HEAD is a real state worth reporting rather than
            // hiding, so fall back to naming the commit it points at.
            let commit = git_output(repo_root, &["rev-parse", "--short", "HEAD"])?;
            let short: String = commit.chars().take(7).collect();
            Some(format!("detached@{short}"))
        });

    // Not a usable repository: no HEAD at all (a fresh `git init`, or a
    // corrupt one). Report the path but not git state.
    let branch = branch?;

    let commit = git_output(repo_root, &["rev-parse", "HEAD"]);

    // `git status --porcelain` is empty exactly when the worktree is clean.
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let remote_hint = git_output(repo_root, &["remote", "get-url", "origin"])
        .map(|url| scrub_remote_url(&url));

    Some(GitSummary {
        branch,
        commit,
        dirty,
        remote_hint,
    })
}

fn git_output(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(repo_root).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Reduce a git remote to a non-identifying, portable hint.
///
/// Strips any userinfo, which is where embedded personal access tokens live.
/// A remote that is nothing but credentials is dropped entirely rather than
/// reported as a bare host.
pub fn scrub_remote_url(url: &str) -> String {
    // scp-style syntax: git@github.com:acme/api.git
    if let Some(rest) = url.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return format!("{host}/{path}");
        }
    }

    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };

    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, p),
        None => (rest, ""),
    };

    // Drop everything before the last '@': that is userinfo, and it is the
    // only part of a remote that can carry a secret.
    let host = match authority.rsplit_once('@') {
        Some((_userinfo, host)) => host,
        None => authority,
    };

    if host.is_empty() {
        return String::new();
    }

    // A host with no path carries no project identity worth keeping.
    if path.is_empty() {
        return host.to_string();
    }

    format!("{scheme}://{host}/{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_embedded_credentials_from_https_remotes() {
        let url = "https://ghp_abcdefghijklmnopqrstuvwxyz0123456789@github.com/acme/api.git";
        let scrubbed = scrub_remote_url(url);

        assert_eq!(scrubbed, "https://github.com/acme/api.git");
        assert!(!scrubbed.contains("ghp_"));
    }

    #[test]
    fn scrubs_credentials_from_ssh_urls() {
        let url = "ssh://git:password123@github.com/acme/api.git";
        assert_eq!(
            scrub_remote_url(url),
            "ssh://github.com/acme/api.git",
            "the password must not survive"
        );
    }

    #[test]
    fn handles_scp_style_remotes() {
        assert_eq!(
            scrub_remote_url("git@github.com:acme/api.git"),
            "github.com/acme/api.git"
        );
    }

    #[test]
    fn leaves_clean_urls_intact() {
        assert_eq!(
            scrub_remote_url("https://github.com/acme/api.git"),
            "https://github.com/acme/api.git"
        );
    }

    #[test]
    fn handles_credential_only_remote() {
        // Nothing but a token and a host: keep the host, drop the token.
        let scrubbed = scrub_remote_url("https://tok@github.com");
        assert!(!scrubbed.contains("tok@"));
    }

    #[test]
    fn remote_hints_never_leak_the_token_pattern() {
        let hostile = [
            "https://x-access-token:ghp_012345678901234567890123456789012345@github.com/a/b",
            "https://oauth2:glpat-ABCDEFGHIJKLMNOPQRST@gitlab.com/a/b",
            "https://user:s3cr3tpassw0rd@bitbucket.org/a/b",
        ];
        for url in hostile {
            let scrubbed = scrub_remote_url(url);
            assert!(!scrubbed.contains('@'), "userinfo survived in {scrubbed}");
            assert!(!scrubbed.contains("ghp_"));
            assert!(!scrubbed.contains("glpat-"));
            assert!(!scrubbed.contains("s3cr3t"));
        }
    }

    #[test]
    fn ids_are_stable_for_the_same_path() {
        let a = ScannedProject::id_for(Path::new("/tmp/example-project"));
        let b = ScannedProject::id_for(Path::new("/tmp/example-project"));
        assert_eq!(a, b, "rescanning must not invalidate a selection");
    }

    #[test]
    fn ids_differ_between_projects() {
        let a = ScannedProject::id_for(Path::new("/tmp/project-one"));
        let b = ScannedProject::id_for(Path::new("/tmp/project-two"));
        assert_ne!(a, b);
    }

    #[test]
    fn slugify_replaces_path_hostile_characters() {
        assert_eq!(slugify("my project!"), "my-project-");
    }

    #[test]
    fn walk_stops_at_the_depth_limit() {
        let deep = Path::new("/a/b/c/d/e/f/g");
        let dirs = walk(deep, 2);
        assert!(dirs.iter().all(|d| d.components().count() <= 8));
    }

    #[test]
    fn scanning_a_missing_root_is_not_an_error() {
        let results = scan_roots(&[PathBuf::from("/definitely/not/here/at/all")]);
        assert!(results.is_empty());
    }
}
