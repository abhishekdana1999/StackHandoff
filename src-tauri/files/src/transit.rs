//! The wire envelope: one opaque payload that carries a manifest and, when the
//! workspace has files, the file archive behind it.
//!
//! Format (all little-endian):
//!
//! ```text
//! MAGIC (5 bytes) | manifest_len (u32) | manifest bytes | archive bytes
//! ```
//!
//! A payload that does not start with `MAGIC` is treated as a manifest alone.
//! That is the legacy format every pre-files transfer used, so old senders
//! still arrive: the receiver simply has no archive to store, and the restore
//! plan's file step reports "no files in this workspace" instead of failing.

use workspace_clone_core::{Result, WorkspaceError};

/// Versioned magic so a future format change can be handed out cleanly.
pub const MAGIC: &[u8; 5] = b"WCFB1";

const HEADER_LEN: usize = MAGIC.len() + 4;

/// Wrap a manifest and an optional archive into a single transfer payload.
pub fn wrap(manifest: &[u8], files: Option<&[u8]>) -> Result<Vec<u8>> {
    if manifest.is_empty() {
        return Err(WorkspaceError::Files(
            "A transfer payload needs a manifest, not an empty blob".into(),
        ));
    }
    let files = files.unwrap_or(&[]);
    let manifest_len = u32::try_from(manifest.len()).map_err(|_| {
        WorkspaceError::Files("The manifest is too large for the transfer envelope".into())
    })?;

    let mut out = Vec::with_capacity(HEADER_LEN + manifest.len() + files.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&manifest_len.to_le_bytes());
    out.extend_from_slice(manifest);
    out.extend_from_slice(files);
    Ok(out)
}

/// The result of splitting an incoming payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unwrapped {
    /// A legacy or file-less payload: the whole thing is the manifest.
    ManifestOnly(Vec<u8>),
    /// The envelope, with the manifest and the archive (possibly empty).
    WithFiles { manifest: Vec<u8>, files: Vec<u8> },
}

/// Split a received payload into manifest and optional archive.
pub fn unwrap(payload: &[u8]) -> Result<Unwrapped> {
    if payload.len() < MAGIC.len() || &payload[..MAGIC.len()] != MAGIC {
        return Ok(Unwrapped::ManifestOnly(payload.to_vec()));
    }
    if payload.len() < HEADER_LEN {
        return Err(WorkspaceError::Files(
            "A transfer envelope is truncated before its manifest length".into(),
        ));
    }

    let manifest_len = u32::from_le_bytes(payload[MAGIC.len()..HEADER_LEN].try_into().unwrap())
        as usize;
    let manifest_end = HEADER_LEN
        .checked_add(manifest_len)
        .ok_or_else(|| WorkspaceError::Files("The transfer envelope length overflows".into()))?;
    if manifest_end > payload.len() {
        return Err(WorkspaceError::Files(
            "The transfer envelope claims a manifest longer than the payload".into(),
        ));
    }
    if manifest_len == 0 {
        return Err(WorkspaceError::Files(
            "The transfer envelope carries no manifest".into(),
        ));
    }

    let manifest = payload[HEADER_LEN..manifest_end].to_vec();
    let files = payload[manifest_end..].to_vec();
    if files.is_empty() {
        Ok(Unwrapped::ManifestOnly(manifest))
    } else {
        Ok(Unwrapped::WithFiles { manifest, files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_payload_round_trips_as_manifest_only() {
        let payload = br#"{"workspace":{"id":"x"}}"#;
        match unwrap(payload).unwrap() {
            Unwrapped::ManifestOnly(m) => assert_eq!(m, payload),
            Unwrapped::WithFiles { .. } => panic!("legacy payload must not read as enveloped"),
        }
    }

    #[test]
    fn envelope_round_trips_manifest_and_files() {
        let wrapped = wrap(b"manifest-json", Some(b"tar-bytes")).unwrap();
        match unwrap(&wrapped).unwrap() {
            Unwrapped::WithFiles { manifest, files } => {
                assert_eq!(manifest, b"manifest-json");
                assert_eq!(files, b"tar-bytes");
            }
            other => panic!("expected WithFiles, got {other:?}"),
        }
    }

    #[test]
    fn envelope_with_no_files_reads_as_manifest_only() {
        let wrapped = wrap(b"manifest-json", None).unwrap();
        match unwrap(&wrapped).unwrap() {
            Unwrapped::ManifestOnly(m) => assert_eq!(m, b"manifest-json"),
            other => panic!("expected ManifestOnly, got {other:?}"),
        }
    }

    #[test]
    fn truncated_envelope_is_refused() {
        assert!(unwrap(b"WCFB1").is_err());
        assert!(unwrap(b"WCFB1\x05\x00\x00\x00xx").is_err());
        assert!(unwrap(b"WCFB1\x00\x00\x00\x00").is_err());
    }
}