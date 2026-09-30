//! Project file transfer for StackHandoff.
//!
//! A workspace transfer has always carried the *manifest* -- descriptions,
//! requirements, policy -- but never the bytes of the projects it describes.
//! This crate fills that gap in three pieces:
//!
//! * [`snapshot`] walks a selected project folder, applies the junk-and-secret
//!   denylist, applies size caps, and builds a single deterministic tar archive
//!   whose entries are keyed `{project_id}/{relative_path}`.
//! * [`archive`] extracts one project's entries from such an archive into a
//!   destination folder, refusing traversal names, absolute paths, symlinks
//!   and oversized entries so a hostile archive cannot write outside the
//!   destination or fill the disk.
//! * [`transit`] wraps a manifest and its archive into a single opaque payload
//!   for the existing transfer channel (which is already chunked, ACKed and
//!   size-capped), and splits it back on arrival. Payloads that are not
//!   wrapped -- every transfer made before this feature -- read back as
//!   manifest-only, so old senders are not suddenly opaque.
//!
//! All of it is plain functions over bytes and paths, so each property can be
//! tested without a socket, a database, or a running app.

pub mod archive;
pub mod snapshot;
pub mod transit;

use std::path::PathBuf;

use workspace_clone_core::{Result, WorkspaceError};

/// The largest single file a snapshot will carry (128 MiB).
pub const PER_FILE_MAX_BYTES: u64 = 128 * 1024 * 1024;

/// The largest total of *file contents* an archive may hold (512 MiB).
pub const TOTAL_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Bounds applied to every walk and every extraction. Held by value so the
/// builder and the extractor can be driven by different policies without
/// sharing state.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub per_file_max: u64,
    pub total_max: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            per_file_max: PER_FILE_MAX_BYTES,
            total_max: TOTAL_MAX_BYTES,
        }
    }
}

/// A project id may only contain characters that are safe in a path component
/// on every supported OS. This is the same spirit as the workspace-id check in
/// the capture layer: whatever becomes part of an archive entry name must not
/// be able to change the meaning of that name.
pub fn validate_project_id(project_id: &str) -> Result<()> {
    if project_id.is_empty() {
        return Err(WorkspaceError::Files(
            "A project id used for file transfer cannot be empty".into(),
        ));
    }
    if project_id.len() > 128 {
        return Err(WorkspaceError::Files(format!(
            "Project id '{project_id}' is too long to use as an archive entry prefix"
        )));
    }
    let ok = project_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !ok {
        return Err(WorkspaceError::Files(format!(
            "Project id '{project_id}' contains characters that are not safe in a file path"
        )));
    }
    Ok(())
}

/// Normalise a filesystem path to the POSIX-style relative string used inside
/// archives (`/` separators, no leading slash). Filesystem walks only produce
/// children of the walked root, so the result is inherently within the root;
/// the sanity asserts below are belt-and-braces for a future caller that feeds
/// the function anything else.
pub fn to_archive_rel(path: &std::path::Path) -> Result<String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| WorkspaceError::Files("A path in a project is not valid UTF-8 and cannot be transferred".into()))?,
            ),
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(WorkspaceError::Files(
                    "A path outside the project root cannot be part of a transfer".into(),
                ))
            }
        }
    }
    if parts.is_empty() {
        return Err(WorkspaceError::Files(
            "An empty relative path cannot be an archive entry".into(),
        ));
    }
    Ok(parts.join("/"))
}

/// The resolved target of a single archive entry, or a refusal reason.
#[derive(Debug)]
pub enum ExtractedTarget {
    /// Absolute path under the destination root that the entry maps to.
    Path(PathBuf),
    /// The entry was refused; the string tells a human why.
    Refused(String),
}