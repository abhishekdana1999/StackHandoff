-- Initial schema for StackHandoff

CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    trust_scopes TEXT NOT NULL DEFAULT '[]',
    os TEXT NOT NULL,
    os_version TEXT NOT NULL,
    app_version TEXT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen DATETIME,
    revoked BOOLEAN NOT NULL DEFAULT FALSE,
    revoked_at DATETIME
);

CREATE INDEX IF NOT EXISTS idx_devices_revoked ON devices(revoked);
CREATE INDEX IF NOT EXISTS idx_devices_last_seen ON devices(last_seen);

CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    captured_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    source_device_id TEXT NOT NULL REFERENCES devices(id),
    manifest_digest TEXT NOT NULL,
    encrypted_manifest_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'captured'
);

CREATE INDEX IF NOT EXISTS idx_workspaces_source_device ON workspaces(source_device_id);
CREATE INDEX IF NOT EXISTS idx_workspaces_captured_at ON workspaces(captured_at);

CREATE TABLE IF NOT EXISTS snapshots (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    captured_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    source_device_id TEXT NOT NULL REFERENCES devices(id),
    size_bytes INTEGER NOT NULL,
    transfer_status TEXT NOT NULL DEFAULT 'pending',
    transfer_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_snapshots_workspace ON snapshots(workspace_id);
CREATE INDEX IF NOT EXISTS idx_snapshots_transfer_status ON snapshots(transfer_status);

CREATE TABLE IF NOT EXISTS restore_runs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    destination_device_id TEXT NOT NULL REFERENCES devices(id),
    plan_digest TEXT NOT NULL,
    approved_steps TEXT NOT NULL DEFAULT '[]',
    result_summary TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME
);

CREATE INDEX IF NOT EXISTS idx_restore_runs_workspace ON restore_runs(workspace_id);
CREATE INDEX IF NOT EXISTS idx_restore_runs_destination ON restore_runs(destination_device_id);

CREATE TABLE IF NOT EXISTS adapter_checks (
    id TEXT PRIMARY KEY,
    adapter_id TEXT NOT NULL,
    adapter_version INTEGER NOT NULL,
    result_state TEXT NOT NULL,
    safe_evidence TEXT NOT NULL DEFAULT '{}',
    checked_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at DATETIME
);

CREATE INDEX IF NOT EXISTS idx_adapter_checks_adapter ON adapter_checks(adapter_id);
CREATE INDEX IF NOT EXISTS idx_adapter_checks_expires ON adapter_checks(expires_at);

CREATE TABLE IF NOT EXISTS transfer_sessions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    source_device_id TEXT NOT NULL REFERENCES devices(id),
    destination_device_id TEXT NOT NULL REFERENCES devices(id),
    status TEXT NOT NULL DEFAULT 'connecting',
    progress REAL NOT NULL DEFAULT 0.0,
    started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at DATETIME,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_transfer_sessions_workspace ON transfer_sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_transfer_sessions_status ON transfer_sessions(status);

-- Settings table for app configuration
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);