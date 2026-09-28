-- File archives for workspaces.
--
-- A workspace carries files as one sealed tar archive stored beside the sealed
-- manifest. This table records where it lives and what it contains, so the
-- restore side can find it by workspace id without touching the manifest.

CREATE TABLE IF NOT EXISTS workspace_files (
    workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id),
    encrypted_files_path TEXT NOT NULL,
    byte_count INTEGER NOT NULL DEFAULT 0,
    file_count INTEGER NOT NULL DEFAULT 0,
    archive_format TEXT NOT NULL DEFAULT 'tar'
);

CREATE INDEX IF NOT EXISTS idx_workspace_files_workspace ON workspace_files(workspace_id);