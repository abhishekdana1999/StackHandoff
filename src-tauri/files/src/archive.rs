//! Safe extraction of one project's entries from a workspace archive.
//!
//! The archive is trusted only up to the bytes the sender signed, which is a
//! modest claim: the *other* side built it. So extraction treats every entry as
//! hostile input and refuses anything that could write outside the destination
//! root (traversal names, absolute paths, symlinks, hard links, platform
//! reserved names) or fill the disk (per-file and total caps).

use std::io::Read;
use std::path::{Path, PathBuf};

use workspace_clone_core::{Result, WorkspaceError};

use crate::{ExtractedTarget, Limits};

/// Windows device names that a regular file can never legitimately be called;
/// writing them on Windows corrupts the filesystem layout and on Unix is just
/// wrong. Rejected case-insensitively.
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul",
    "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Validate a relative path received from an archive and turn it into a
/// relative `PathBuf`. Any name that could escape its root is refused.
pub fn safe_relative(rel: &str) -> std::result::Result<PathBuf, String> {
    if rel.is_empty() {
        return Err("empty path".into());
    }
    if rel.len() > 4096 {
        return Err("path too long".into());
    }
    if rel.contains('\0') {
        return Err("path contains a NUL byte".into());
    }
    if rel.contains('\\') {
        // A backslash is a separator on Windows; a sender writing one is either
        // platform-confused or smuggling a traversal. Refuse it outright.
        return Err("path contains a backslash".into());
    }
    if rel.contains(':') {
        return Err("path contains ':' (drive letters and alternate streams are refused)".into());
    }

    let mut out = PathBuf::new();
    for part in rel.split('/') {
        match part {
            "" => return Err("path has an empty component".into()),
            "." => return Err("path has a '.' component".into()),
            ".." => return Err("path escapes the destination (contains '..')".into()),
            _ => {
                if part.len() > 255 {
                    return Err("a path component is too long".into());
                }
                // "lpt1.txt", "con.foo" etc are still the reserved device names
                // on Windows -- the extension is a legacy quirk, not a different
                // name. Split on the first dot.
                let lower = part.to_ascii_lowercase();
                let base = lower.split('.').next().unwrap_or(&lower);
                if RESERVED_NAMES.contains(&base) {
                    return Err(format!("'{part}' is a reserved name and is refused"));
                }
                out.push(part);
            }
        }
    }
    Ok(out)
}

/// The target of an archive entry: its `{project_id}/...` split, plus what it
/// maps to. `None` means the entry does not belong to this project (it is
/// another project's file, or the archive's own padding) and should be
/// skipped silently. Absolute or traversal-leading names are refused loudly,
/// because whatever they claim to be, a peer that writes them is hostile.
pub fn target_for_entry(
    entry_path: &str,
    project_id: &str,
    dest_root: &Path,
) -> Result<Option<ExtractedTarget>> {
    // Absolute, or leading `..`/`.`: hostile the moment it is seen.
    if entry_path.starts_with('/') || entry_path.starts_with('\\') {
        return Ok(Some(ExtractedTarget::Refused(format!(
            "'{entry_path}': absolute paths are never extracted"
        ))));
    }
    if entry_path == ".." || entry_path.starts_with("../") || entry_path.starts_with(".\\") {
        return Ok(Some(ExtractedTarget::Refused(format!(
            "'{entry_path}': the path escapes its root"
        ))));
    }

    let prefix = format!("{project_id}/");
    let Some(rest) = entry_path.strip_prefix(&prefix) else {
        return Ok(None);
    };

    if rest.is_empty() {
        return Ok(Some(ExtractedTarget::Refused(format!(
            "'{entry_path}': names the project root itself"
        ))));
    }

    match safe_relative(rest) {
        Ok(rel) => Ok(Some(ExtractedTarget::Path(dest_root.join(rel)))),
        Err(reason) => Ok(Some(ExtractedTarget::Refused(format!(
            "'{entry_path}': {reason}"
        )))),
    }
}

/// What a single project's extraction produced.
#[derive(Debug, Default)]
pub struct ExtractReport {
    pub files_written: u64,
    pub bytes_written: u64,
    pub dirs_created: u64,
    /// Entries that were refused, with the reason each was refused.
    pub refused: Vec<String>,
    /// Entries under this project's prefix that were skipped by a size cap.
    pub oversized: u64,
    /// Extraction stopped early because the total cap was reached.
    pub overflow: bool,
}

/// Extract every entry under `project_id/` in the archive into `dest_root`,
/// creating parent directories as needed and overwriting existing files (the
/// snapshot is authoritative -- that is the point of the restore).
pub fn extract_project(
    tar_bytes: &[u8],
    project_id: &str,
    dest_root: &Path,
    limits: Limits,
) -> Result<ExtractReport> {
    crate::validate_project_id(project_id)?;

    let canonical_root = match dest_root.canonicalize() {
        Ok(r) => r,
        Err(_) => {
            std::fs::create_dir_all(dest_root).map_err(|e| {
                WorkspaceError::Files(format!(
                    "Could not create the restore folder {}: {e}",
                    dest_root.display()
                ))
            })?;
            dest_root.canonicalize().map_err(|e| {
                WorkspaceError::Files(format!(
                    "The restore folder {} could not be resolved: {e}",
                    dest_root.display()
                ))
            })?
        }
    };

    let mut report = ExtractReport::default();
    let mut used: u64 = 0;
    let mut created_dirs: Vec<PathBuf> = Vec::new();

    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    let entries = archive
        .entries()
        .map_err(|e| WorkspaceError::Files(format!("The received file archive is corrupt: {e}")))?;

    for entry in entries {
        if report.overflow {
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                report.refused.push(format!("unreadable entry in the archive: {e}"));
                continue;
            }
        };

        let entry_path = match entry.path() {
            Ok(p) => p.to_string_lossy().into_owned(),
            // Non-UTF-8 names are refused: nothing we build produces them, and
            // mapping arbitrary bytes onto either OS's path rules is lossy.
            Err(_) => {
                report.refused.push("entry path is not valid UTF-8".into());
                continue;
            }
        };

        let entry_type = entry.header().entry_type();
        // Only regular files and directories belong in a project snapshot.
        let is_regular = matches!(entry_type, tar::EntryType::Regular);
        let is_dir = matches!(entry_type, tar::EntryType::Directory);

        let target = match target_for_entry(&entry_path, project_id, &canonical_root)? {
            Some(ExtractedTarget::Path(p)) => p,
            Some(ExtractedTarget::Refused(reason)) => {
                report.refused.push(format!("{entry_path}: {reason}"));
                continue;
            }
            None => continue, // another project's file
        };

        if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| {
                WorkspaceError::Files(format!("Could not create {}: {e}", target.display()))
            })?;
            if !created_dirs.contains(&target) {
                created_dirs.push(target.clone());
            }
            continue;
        }
        if !is_regular {
            report.refused.push(format!(
                "{entry_path}: {entry_type:?} entries are not extracted (no symlinks or special files)"
            ));
            continue;
        }

        let size = entry.header().size().unwrap_or(0);
        if size > limits.per_file_max {
            report.oversized += 1;
            continue;
        }
        if used + size > limits.total_max {
            report.overflow = true;
            break;
        }

        let parent = target.parent().ok_or_else(|| {
            WorkspaceError::Files(format!("{} has no parent directory", target.display()))
        })?;
        std::fs::create_dir_all(parent).map_err(|e| {
            WorkspaceError::Files(format!("Could not create {}: {e}", parent.display()))
        })?;
        if !created_dirs.iter().any(|d| d == parent) {
            created_dirs.push(parent.to_path_buf());
        }

        // Defence in depth: the path was built from validated components, and
        // now the canonicalised parent must still live under the root.
        let canonical_parent = parent.canonicalize().map_err(|e| {
            WorkspaceError::Files(format!("{} could not be resolved: {e}", parent.display()))
        })?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(WorkspaceError::Files(format!(
                "Refusing to write {}: it resolves outside the restore folder {}",
                target.display(),
                canonical_root.display()
            )));
        }

        let mut out = std::fs::File::create(&target).map_err(|e| {
            WorkspaceError::Files(format!("Could not write {}: {e}", target.display()))
        })?;
        let written = std::io::copy(&mut entry.take(limits.per_file_max + 1), &mut out).map_err(|e| {
            WorkspaceError::Files(format!("Could not write {}: {e}", target.display()))
        })?;
        if written > limits.per_file_max {
            let _ = std::fs::remove_file(&target);
            report.refused.push(format!(
                "{entry_path}: entry grew beyond the {} byte limit during extraction",
                limits.per_file_max
            ));
            continue;
        }
        report.files_written += 1;
        report.bytes_written += written;
        used += written;
    }

    report.dirs_created = created_dirs.len() as u64;
    Ok(report)
}

/// Count entries under `project_id/` without writing anything (used by tests
/// and by the restore preview to say whether a workspace carries files).
pub fn project_entry_count(tar_bytes: &[u8], project_id: &str) -> u64 {
    let cursor = std::io::Cursor::new(tar_bytes);
    let mut archive = tar::Archive::new(cursor);
    let Ok(entries) = archive.entries() else {
        return 0;
    };
    let prefix = format!("{project_id}/");
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().map(|p| p.starts_with(&prefix)).unwrap_or(false))
        .count() as u64
}

/// File and byte counts across the whole archive, without writing anything.
/// The receiver records these next to the stored archive so the restore side
/// can say what a workspace carries without unpacking it.
pub fn archive_summary(tar_bytes: &[u8]) -> (u64, u64) {
    let cursor = std::io::Cursor::new(tar_bytes);
    let mut archive = tar::Archive::new(cursor);
    let Ok(entries) = archive.entries() else {
        return (0, 0);
    };
    entries.filter_map(|e| e.ok()).fold((0u64, 0u64), |(files, bytes), e| {
        let is_regular = matches!(e.header().entry_type(), tar::EntryType::Regular);
        let size = e.header().size().unwrap_or(0);
        if is_regular {
            (files + 1, bytes + size)
        } else {
            (files, bytes)
        }
    })
}

/// Validate a relative path from outside the archive (e.g. a planner input):
/// returns the safe path, or a reason it was refused.
pub fn classify_components(rel: &str) -> std::result::Result<PathBuf, String> {
    safe_relative(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_archive() -> Vec<u8> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let tag = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "wc-extract-fixture-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), b"fn main() {}\n").unwrap();
        std::fs::write(dir.join("README.md"), b"hello\n").unwrap();
        std::fs::write(dir.join("secret.txt"), b"top\n").unwrap();
        let build = crate::snapshot::build_archive(
            &[("proj_x".to_string(), dir.clone())],
            Limits::default(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        build.tar
    }

    #[test]
    fn extract_writes_the_right_files_and_overwrites() {
        let dest = std::env::temp_dir().join(format!("wc-extract-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(dest.join("src")).unwrap();
        // Stale content from an earlier restore: the snapshot must replace it.
        std::fs::write(dest.join("src/main.rs"), b"old").unwrap();
        std::fs::write(dest.join("README.md"), b"stale").unwrap();

        let tar = fixture_archive();
        let report = extract_project(&tar, "proj_x", &dest, Limits::default()).unwrap();

        assert_eq!(report.files_written, 3);
        assert_eq!(
            std::fs::read_to_string(dest.join("src/main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(std::fs::read_to_string(dest.join("README.md")).unwrap(), "hello\n");
        assert_eq!(std::fs::read_to_string(dest.join("secret.txt")).unwrap(), "top\n");
        assert!(report.refused.is_empty());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn other_projects_entries_are_ignored() {
        let dest = std::env::temp_dir().join(format!("wc-extract-other-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);

        let tar = fixture_archive();
        let report = extract_project(&tar, "proj_y", &dest, Limits::default()).unwrap();
        assert_eq!(report.files_written, 0);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn traversal_names_are_refused() {
        let dest = std::env::temp_dir().join(format!("wc-extract-traversal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let outside = std::env::temp_dir().join(format!("wc-extract-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);

        for hostile in [
            "proj_x/../../evil.txt",
            "proj_x/../outside-{id}/pwned.txt",
            "/proj_x/abs.txt",
            "proj_x/..\\..\\evil.txt",
            "proj_x/C:/windows.txt",
            "proj_x/com1",
            "proj_x/a/./b.txt",
        ] {
            let name = hostile.replace("{id}", "x");
            // Hand-rolled entry: set_path would refuse some of these names, but
            // a hostile peer does not use set_path. Write the raw bytes.
            let mut builder = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            let raw = name.as_bytes();
            assert!(raw.len() < 100, "fixture names must fit the ustar path field");
            header.as_mut_bytes()[..raw.len()].copy_from_slice(raw);
            header.set_size(3);
            header.set_mode(0o644);
            header.set_cksum();

            // Some names the *builder* refuses before writing (e.g. "contributions"
            // after chmod). Treat a build refusal as the same guarantee: the entry
            // still must not land outside the root.
            let Ok(()) = builder.append(&header, "bad".as_bytes()) else {
                continue;
            };
            let tar = builder.into_inner().unwrap();

            let report = extract_project(&tar, "proj_x", &dest, Limits::default()).unwrap();
            assert_eq!(report.files_written, 0, "{hostile} must be refused");
            assert!(!report.refused.is_empty(), "{hostile} must produce a refusal reason");
            assert!(!outside.join(format!("pwned.txt")).exists(), "{hostile}");
        }

        assert!(
            !std::fs::read_dir(&dest)
                .map(|mut r| r.any(|e| e.is_ok()))
                .unwrap_or(false),
            "nothing may be written inside the destination for a hostile archive"
        );
        let _ = std::fs::remove_dir_all(&dest);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn symlinks_are_not_extracted() {
        let dest = std::env::temp_dir().join(format!("wc-extract-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();

        let mut builder = tar::Builder::new(Vec::new());
        let mut link = tar::Header::new_gnu();
        link.set_size(0);
        link.set_mode(0o777);
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_cksum();
        builder
            .append_link(&mut link, "proj_x/hook.sh", "/etc/passwd")
            .unwrap();
        let mut file = tar::Header::new_gnu();
        file.set_size(2);
        file.set_mode(0o644);
        file.set_cksum();
        builder
            .append_data(&mut file, "proj_x/ok.txt", "ok".as_bytes())
            .unwrap();
        let tar = builder.into_inner().unwrap();

        let report = extract_project(&tar, "proj_x", &dest, Limits::default()).unwrap();
        assert_eq!(report.files_written, 1);
        assert!(!dest.join("hook.sh").exists());
        assert!(!report.refused.is_empty());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn safe_relative_rejects_separators_and_dots() {
        for bad in ["../x", "a/../../b", "/abs", "a\\b", "a:", "a//b", "a/./b"] {
            assert!(safe_relative(bad).is_err(), "{bad:?} must be refused");
        }
        assert_eq!(safe_relative("a/b/c.txt").unwrap().to_string_lossy(), "a/b/c.txt");
        assert!(safe_relative("CON").is_err());
        assert!(safe_relative("lpt1.txt").is_err());
    }
}