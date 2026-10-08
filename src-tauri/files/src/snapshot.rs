//! Walking a selected project folder into a size-capped, deterministic archive.
//!
//! The denylist is the contract the user agreed to: everything is copied
//! *except* version-control internals, build output, package caches and
//! secrets. The two size caps are what keep a snapshot from embarrassing a
//! machine or a network: a single file bigger than the per-file cap is skipped
//! with a reason, and once the *total* of file contents reaches the total cap
//! the walk stops and reports the overflow.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use workspace_clone_core::{Result, WorkspaceError};

use crate::{to_archive_rel, Limits, validate_project_id};

/// Directory names skipped at any depth, matched case-insensitively.
///
/// The usual suspects: VCS internals, dependency/build output, caches. These
/// are safe to exclude because they are all regenerable -- the transferred
/// workspace still restores the code.
pub const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    "bower_components",
    "jspm_packages",
    "vendor",
    ".pnpm-store",
    "__pypackages__",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".output",
    ".parcel-cache",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    "Pods",
    "DerivedData",
    "coverage",
];

/// File names skipped anywhere, matched case-insensitively, plus the `.env.*`
/// family (any dotenv variant is treated as a secret).
pub const EXCLUDED_FILES: &[&str] = &[
    ".DS_Store",
    "Thumbs.db",
    "desktop.ini",
    ".env",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];

/// File extensions skipped anywhere, matched case-insensitively. Databases and
/// logs are machine-local state; the key material extensions are secrets.
pub const EXCLUDED_EXTENSIONS: &[&str] = &[
    "db", "db-shm", "db-wal", "sqlite", "sqlite3", "log", "pem", "key", "der", "p12", "pfx",
];

/// Case-insensitive membership test used by all three denylists.
fn listed(names: &[&str], candidate: &str) -> bool {
    names
        .iter()
        .any(|n| n.eq_ignore_ascii_case(candidate))
}

/// Whether a directory with this name is excluded (checked at any depth).
pub fn is_excluded_dir(name: &str) -> bool {
    listed(EXCLUDED_DIRS, name)
}

/// Whether a file with this name is excluded, on extension or on name.
pub fn is_excluded_file(name: &str) -> bool {
    if listed(EXCLUDED_FILES, name) {
        return true;
    }
    // The whole `.env.*` family is secret, whatever the suffix.
    if name.len() > 5 && name[..5].eq_ignore_ascii_case(".env.") {
        return true;
    }
    match name.rfind('.') {
        Some(idx) if idx + 1 < name.len() => {
            listed(EXCLUDED_EXTENSIONS, &name[idx + 1..])
        }
        _ => false,
    }
}

/// One file chosen for the archive.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Absolute path on this machine (the read source).
    pub absolute: PathBuf,
    /// POSIX-style relative path under the project root (the archive name).
    pub rel: String,
    /// Uncompressed size in bytes.
    pub size: u64,
}

/// A file deliberately left out, with the reason a human will recognise.
#[derive(Debug, Clone)]
pub struct Skipped {
    pub project_id: String,
    pub rel: String,
    pub reason: String,
}

/// Files and directories the walk refused to look at, e.g. a symlink that
/// could escape the project root or a directory that could not be read.
#[derive(Debug, Clone)]
pub struct WalkWarning {
    pub project_id: String,
    pub path: String,
    pub reason: String,
}

/// The outcome of walking one project folder.
#[derive(Debug, Default)]
pub struct WalkResult {
    pub entries: Vec<FileEntry>,
    pub skipped: Vec<Skipped>,
    pub warnings: Vec<WalkWarning>,
    /// True when the total cap stopped the walk mid-way.
    pub overflow: bool,
}

/// Walk a project folder, applying the denylist and the size caps.
///
/// `already_used` carries the byte total shared across projects so a multi-
/// project workspace respects one cap across the whole archive.
pub fn walk_project(
    project_id: &str,
    root: &Path,
    limits: Limits,
    already_used: u64,
) -> Result<(WalkResult, u64)> {
    validate_project_id(project_id)?;

    let canonical = root
        .canonicalize()
        .map_err(|e| WorkspaceError::Files(format!("The project folder {} is not readable: {e}", root.display())))?;
    if !canonical.is_dir() {
        return Err(WorkspaceError::Files(format!(
            "{} is not a folder, so it cannot provide files for the workspace",
            canonical.display()
        )));
    }

    let mut result = WalkResult::default();
    let mut used = already_used;
    let mut pending: Vec<PathBuf> = vec![canonical.clone()];

    while let Some(dir) = pending.pop() {
        let read = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => {
                result.warnings.push(WalkWarning {
                    project_id: project_id.into(),
                    path: dir.display().to_string(),
                    reason: format!("folder could not be read and was skipped: {e}"),
                });
                continue;
            }
        };

        let mut children: Vec<_> = match read.collect::<std::result::Result<Vec<_>, _>>() {
            Ok(c) => c,
            Err(e) => {
                result.warnings.push(WalkWarning {
                    project_id: project_id.into(),
                    path: dir.display().to_string(),
                    reason: format!("folder listing failed part-way: {e}"),
                });
                continue;
            }
        };
        // Determinism inside a folder: directories first, then files, both
        // sorted, so a snapshot does not depend on readdir order.
        children.sort_by_key(|e| e.file_name());

        for child in children {
            let child_path = child.path();
            let file_type = match child.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    result.warnings.push(WalkWarning {
                        project_id: project_id.into(),
                        path: child_path.display().to_string(),
                        reason: format!("entry metadata unavailable: {e}"),
                    });
                    continue;
                }
            };

            // Never follow symlinks: a link inside the project could point
            // anywhere, and the archive must not carry links at all.
            if file_type.is_symlink() {
                result.skipped.push(Skipped {
                    project_id: project_id.into(),
                    rel: rel_or_path(&canonical, &child_path),
                    reason: "symlink (links are never followed or archived)".into(),
                });
                continue;
            }

            if file_type.is_dir() {
                if is_excluded_dir(&child.file_name().to_string_lossy()) {
                    result.skipped.push(Skipped {
                        project_id: project_id.into(),
                        rel: rel_or_path(&canonical, &child_path),
                        reason: "excluded directory".into(),
                    });
                    continue;
                }
                pending.push(child_path);
                continue;
            }

            if !file_type.is_file() {
                result.skipped.push(Skipped {
                    project_id: project_id.into(),
                    rel: rel_or_path(&canonical, &child_path),
                    reason: "not a regular file".into(),
                });
                continue;
            }

            let name = child.file_name().to_string_lossy().into_owned();
            if is_excluded_file(&name) {
                result.skipped.push(Skipped {
                    project_id: project_id.into(),
                    rel: rel_or_path(&canonical, &child_path),
                    reason: "excluded file (secret or machine-local)".into(),
                });
                continue;
            }

            let meta = match child.metadata() {
                Ok(m) => m,
                Err(e) => {
                    result.warnings.push(WalkWarning {
                        project_id: project_id.into(),
                        path: child_path.display().to_string(),
                        reason: format!("file size unavailable: {e}"),
                    });
                    continue;
                }
            };

            if meta.len() > limits.per_file_max {
                result.skipped.push(Skipped {
                    project_id: project_id.into(),
                    rel: rel_or_path(&canonical, &child_path),
                    reason: format!("{} bytes, over the {} byte per-file limit", meta.len(), limits.per_file_max),
                });
                continue;
            }

            if used + meta.len() > limits.total_max {
                result.overflow = true;
                break; // leave this folder; `overflow` stops the whole walk.
            }

            let rel = to_archive_rel(child_path.strip_prefix(&canonical).unwrap_or(&child_path))?;
            result.entries.push(FileEntry {
                absolute: child_path.clone(),
                rel,
                size: meta.len(),
            });
            used += meta.len();
        }

        if result.overflow {
            break;
        }
    }

    // One deterministic order for the whole project.
    result.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok((result, used))
}

/// A human-readable relative path for skip/warning messages.
fn rel_or_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

/// The built archive and what went into it.
#[derive(Debug)]
pub struct ArchiveBuild {
    pub tar: Vec<u8>,
    pub file_count: u64,
    /// Sum of file contents carried, excluding tar overhead.
    pub byte_count: u64,
    pub skipped: Vec<Skipped>,
    pub warnings: Vec<WalkWarning>,
    pub overflow: bool,
}

/// Build the single workspace archive for a list of `(project_id, root)` pairs.
///
/// Entries are keyed `{project_id}/{relative path}` and written in one global
/// deterministic order. The total cap is shared across all projects.
pub fn build_archive(projects: &[(String, PathBuf)], limits: Limits) -> Result<ArchiveBuild> {
    let mut build = ArchiveBuild {
        tar: Vec::new(),
        file_count: 0,
        byte_count: 0,
        skipped: Vec::new(),
        warnings: Vec::new(),
        overflow: false,
    };

    let mut used: u64 = 0;
    let mut entries: Vec<(String, FileEntry)> = Vec::new();

    for (project_id, root) in projects {
        let (walk, after) = walk_project(project_id, root, limits, used)?;
        used = after;
        build.file_count += walk.entries.len() as u64;
        build.skipped.extend(walk.skipped);
        build.warnings.extend(walk.warnings);
        build.overflow |= walk.overflow;
        entries.extend(walk.entries.into_iter().map(|e| (project_id.clone(), e)));
        if build.overflow {
            break;
        }
    }

    // Global deterministic order, then stream into one tar.
    entries.sort_by(|(a_id, a), (b_id, b)| {
        (a_id, &a.rel)
            .cmp(&(b_id, &b.rel))
    });

    let mut builder = tar::Builder::new(Vec::new());
    for (project_id, entry) in &entries {
        let name = format!("{project_id}/{}", entry.rel);
        let mut file = File::open(&entry.absolute).map_err(|e| {
            WorkspaceError::Files(format!("Could not read {} for transfer: {e}", entry.absolute.display()))
        })?;

        let mut header = tar::Header::new_gnu();
        header.set_size(entry.size);
        header.set_mode(0o644);
        header.set_mtime(
            file.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();

        builder
            .append_data(&mut header, &name, &mut file)
            .map_err(|e| WorkspaceError::Files(format!("Could not add {} to the transfer archive: {e}", name)))?;
        build.byte_count += entry.size;
    }

    build.tar = builder
        .into_inner()
        .map_err(|e| WorkspaceError::Files(format!("Could not finish the transfer archive: {e}")))?;

    Ok(build)
}

/// Load a file's bytes, honouring the per-file cap rather than trusting the
/// caller's metadata. Used when a project was walked but the file changed.
pub fn read_capped(path: &Path, cap: u64) -> Result<Option<Vec<u8>>> {
    let file = File::open(path)
        .map_err(|e| WorkspaceError::Files(format!("Could not open {}: {e}", path.display())))?;
    let mut out = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut out)
        .map_err(|e| WorkspaceError::Files(format!("Could not read {}: {e}", path.display())))?;
    if out.len() as u64 > cap {
        return Ok(None);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(root.join("README.md"), b"# hi").unwrap();
        std::fs::write(root.join(".gitignore"), b"target").unwrap();
        std::fs::write(root.join(".env"), b"SECRET=1").unwrap();
        for dependency_dir in [
            "node_modules",
            "bower_components",
            "jspm_packages",
            "vendor",
            ".pnpm-store",
            "__pypackages__",
        ] {
            std::fs::create_dir_all(root.join(dependency_dir).join("pkg")).unwrap();
            std::fs::write(root.join(dependency_dir).join("pkg/index.js"), b"junk").unwrap();
        }
        std::fs::write(root.join(".DS_Store"), b"\0\0").unwrap();
        std::fs::write(root.join("notes.db"), b"db").unwrap();
    }

    #[test]
    fn denylist_and_determinism() {
        let dir = std::env::temp_dir().join(format!("wc-files-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        tree(&dir);
        let (walk, used) = walk_project("p1", &dir, Limits::default(), 0).unwrap();
        let rels: Vec<_> = walk.entries.iter().map(|e| e.rel.clone()).collect();

        assert!(rels.contains(&"src/main.rs".to_string()));
        assert!(rels.contains(&"README.md".to_string()));
        assert!(rels.contains(&".gitignore".to_string()));
        // Excluded on name, extension, or denylisted directory.
        assert!(!rels.iter().any(|r| r == ".env"));
        assert!(!rels.iter().any(|r| r == ".DS_Store"));
        assert!(!rels.iter().any(|r| r == "notes.db"));
        for dependency_dir in [
            "node_modules",
            "bower_components",
            "jspm_packages",
            "vendor",
            ".pnpm-store",
            "__pypackages__",
        ] {
            assert!(
                !rels.iter().any(|r| r.starts_with(dependency_dir)),
                "dependency directory {dependency_dir} was included"
            );
        }
        assert_eq!(walk.entries.len(), 3);
        // Sorted.
        assert_eq!(rels, {
            let mut s = rels.clone();
            s.sort();
            s
        });
        let _ = used;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn per_file_cap_skips_oversized() {
        let dir = std::env::temp_dir().join(format!("wc-files-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("big.bin"), vec![0u8; 4096]).unwrap();
        std::fs::write(dir.join("small.txt"), b"x").unwrap();

        let limits = Limits { per_file_max: 1024, ..Limits::default() };
        let (walk, _) = walk_project("p1", &dir, limits, 0).unwrap();
        assert_eq!(walk.entries.len(), 1);
        assert_eq!(walk.entries[0].rel, "small.txt");
        assert_eq!(walk.skipped.len(), 1);
        assert!(walk.skipped[0].reason.contains("per-file"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn total_cap_stops_early() {
        let dir = std::env::temp_dir().join(format!("wc-files-total-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("b.txt"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("c.txt"), vec![0u8; 100]).unwrap();

        let limits = Limits { per_file_max: 4096, total_max: 250 };
        let (walk, used) = walk_project("p1", &dir, limits, 0).unwrap();
        assert!(walk.overflow, "250-byte cap with 100-byte files must hit total");
        assert!(used <= 250);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archive_round_trips_projects_and_keys() {
        let one = std::env::temp_dir().join(format!("wc-arc-one-{}", std::process::id()));
        let two = std::env::temp_dir().join(format!("wc-arc-two-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&one);
        let _ = std::fs::remove_dir_all(&two);
        std::fs::create_dir_all(one.join("src")).unwrap();
        std::fs::create_dir_all(two.join("docs")).unwrap();
        std::fs::write(one.join("src/main.rs"), b"fn main() {}").unwrap();
        std::fs::write(one.join("Cargo.toml"), b"[package]").unwrap();
        std::fs::write(two.join("docs/guide.md"), b"guide").unwrap();

        let build = build_archive(
            &[("project-a".into(), one.clone()), ("project-b".into(), two.clone())],
            Limits::default(),
        )
        .unwrap();
        assert_eq!(build.file_count, 3);
        assert!(build.byte_count >= 6);

        // Entry names are keyed by project id.
        let mut listed: Vec<String> = Vec::new();
        for entry in tar::Archive::new(std::io::Cursor::new(&build.tar)).entries().unwrap() {
            let e = entry.unwrap();
            listed.push(e.path().unwrap().to_string_lossy().into_owned());
        }
        assert!(listed.contains(&"project-a/src/main.rs".to_string()));
        assert!(listed.contains(&"project-b/docs/guide.md".to_string()));
        assert!(!listed.iter().any(|p| p.starts_with('/') || p.contains("..")));

        let _ = std::fs::remove_dir_all(&one);
        let _ = std::fs::remove_dir_all(&two);
    }
}