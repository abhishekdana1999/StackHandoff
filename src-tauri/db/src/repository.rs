//! Database repository pattern

use crate::models::*;
use directories::ProjectDirs;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::info;
use workspace_clone_core::{DatabaseError, Result};

/// Database connection pool
pub type DbPool = SqlitePool;

/// Initialize database connection
pub async fn init_db() -> Result<DbPool> {
    let db_path = get_db_path()?;

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DatabaseError::Connection(e.to_string()))?;
    }

    init_db_at(&db_path).await
}

/// Initialize a database at an explicit path.
///
/// Split out from [`init_db`] so tests can exercise the real repositories and
/// migrations against a throwaway file instead of the user's actual database.
/// An in-memory pool is not usable here because sqlx hands out a different
/// connection per checkout, so each would see its own empty database.
pub async fn init_db_at(db_path: &Path) -> Result<DbPool> {
    let mut options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
        .map_err(|e| DatabaseError::Connection(e.to_string()))?
        .create_if_missing(true)
        .foreign_keys(true);

    // Enable WAL mode for better concurrency
    options = options.pragma("journal_mode", "WAL");
    options = options.pragma("synchronous", "NORMAL");
    options = options.pragma("busy_timeout", "5000");

    let pool = SqlitePool::connect_with(options)
        .await
        .map_err(|e| DatabaseError::Connection(e.to_string()))?;

    // Run migrations
    run_migrations(&pool).await?;

    info!("Database initialized at {}", db_path.display());
    Ok(pool)
}

/// Get database file path
fn get_db_path() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from("com", "workspaceclone", "WorkspaceClone")
        .ok_or_else(|| DatabaseError::Connection("Could not find config directory".to_string()))?;

    Ok(proj_dirs.data_dir().join("workspace-clone.db"))
}

/// Run database migrations
async fn run_migrations(pool: &DbPool) -> Result<()> {
    // Run embedded migrations
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| DatabaseError::Migration(e.to_string()))?;

    info!("Database migrations completed");
    Ok(())
}

/// Device repository
pub struct DeviceRepository {
    pool: DbPool,
}

impl DeviceRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, device: &DeviceRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO devices (id, name, public_key, noise_public_key, fingerprint, trust_scopes, os, os_version, app_version, created_at, revoked)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&device.id)
        .bind(&device.name)
        .bind(&device.public_key)
        .bind(&device.noise_public_key)
        .bind(&device.fingerprint)
        .bind(&device.trust_scopes)
        .bind(&device.os)
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(device.created_at)
        .bind(device.revoked)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    /// Insert this device's own row, or leave the existing one alone.
    ///
    /// Required, not optional: `workspaces.source_device_id` is a foreign key,
    /// so a capture fails to persist unless this device is already a row. The
    /// keys are *not* rewritten if the row exists, because doing so would
    /// invalidate every device already paired against them.
    ///
    /// The row is found by its Noise public key rather than by its id. That is
    /// what makes the re-key in [`Self::adopt_superseded_id`] possible: a device
    /// that changed how it derives its own id is still the *same* device, and
    /// only its Noise key says so.
    pub async fn ensure_local(&self, device: &DeviceRecord) -> Result<DeviceRecord> {
        if let Some(existing) = self.get(&device.id).await? {
            return Ok(existing);
        }

        // A row holding our Noise key under a different id is this same device,
        // recorded by a build that derived its id some other way. Re-keying is
        // strictly better than inserting a second row: the old row would keep
        // the name in every device list and the captures in the history.
        if let Some(stale) = self.get_by_noise_key(&device.noise_public_key).await? {
            return self.adopt_superseded_id(&stale.id, device).await;
        }

        self.create(device).await?;
        Ok(device.clone())
    }

    /// Re-record this device's own row under a newly derived id.
    ///
    /// The four tables that reference `devices.id` are updated in one
    /// transaction, because foreign keys are enforced. A partial re-key would
    /// leave a workspace whose `source_device_id` names a device that no longer
    /// exists, which is a row the user can see in their history and cannot open.
    pub async fn adopt_superseded_id(
        &self,
        old_id: &str,
        device: &DeviceRecord,
    ) -> Result<DeviceRecord> {
        if old_id == device.id {
            // Re-keying an id to itself would delete the row and then fail to
            // find it. Reaching here means the caller's lookup and its insert
            // disagreed, which is worth reporting rather than papering over.
            return self.get(old_id).await?.ok_or_else(|| {
                workspace_clone_core::WorkspaceError::from(DatabaseError::NotFound(format!(
                    "the device row '{old_id}' disappeared between being read and being re-keyed"
                )))
            });
        }

        let mut tx = self.pool.begin().await.map_err(|e| DatabaseError::Query(e.to_string()))?;

        // Order matters, and it is the order the foreign keys demand.
        //
        // 1. The new device row has to exist before anything can point at it.
        //    `workspaces.source_device_id` is a real foreign key and this pool
        //    enforces them, so repointing first fails outright.
        // 2. Then every reference moves. Nothing can be lost by a failure here:
        //    the worst case is two rows for one machine, which is what the user
        //    would have got from the naive approach anyway, and the next launch
        //    repairs it.
        // 3. The old row goes last, once nothing refers to it.
        sqlx::query(
            "INSERT INTO devices (id, name, public_key, noise_public_key, fingerprint, \
             trust_scopes, os, os_version, app_version, created_at, last_seen, revoked, revoked_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&device.id)
        .bind(&device.name)
        .bind(&device.public_key)
        .bind(&device.noise_public_key)
        .bind(&device.fingerprint)
        .bind(&device.trust_scopes)
        .bind(&device.os)
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(device.created_at)
        .bind(device.last_seen)
        .bind(device.revoked)
        .bind(device.revoked_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        for sql in [
            "UPDATE workspaces SET source_device_id = ? WHERE source_device_id = ?",
            "UPDATE snapshots SET source_device_id = ? WHERE source_device_id = ?",
            "UPDATE restore_runs SET destination_device_id = ? WHERE destination_device_id = ?",
        ] {
            sqlx::query(sql)
                .bind(&device.id)
                .bind(old_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
        }

        sqlx::query("DELETE FROM devices WHERE id = ?")
            .bind(old_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        tx.commit().await.map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(device.clone())
    }

    pub async fn get_by_noise_key(&self, noise_public_key: &str) -> Result<Option<DeviceRecord>> {
        // A row written before the Noise column existed has an empty value, and
        // an empty value must never match another empty one.
        if noise_public_key.is_empty() {
            return Ok(None);
        }

        let row = sqlx::query_as::<_, DeviceRecord>("SELECT * FROM devices WHERE noise_public_key = ?")
            .bind(noise_public_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    pub async fn get(&self, id: &str) -> Result<Option<DeviceRecord>> {
        let row = sqlx::query_as::<_, DeviceRecord>("SELECT * FROM devices WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    pub async fn get_by_fingerprint(&self, fingerprint: &str) -> Result<Option<DeviceRecord>> {
        let row = sqlx::query_as::<_, DeviceRecord>("SELECT * FROM devices WHERE fingerprint = ?")
            .bind(fingerprint)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    pub async fn list(&self, include_revoked: bool) -> Result<Vec<DeviceRecord>> {
        let query = if include_revoked {
            "SELECT * FROM devices ORDER BY created_at DESC"
        } else {
            "SELECT * FROM devices WHERE revoked = FALSE ORDER BY created_at DESC"
        };

        let rows = sqlx::query_as::<_, DeviceRecord>(query)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    /// Update a device in place.
    ///
    /// Reports `NotFound` when no row matched. Without that, renaming a device
    /// that was deleted on another screen reports success and the user's new
    /// name is silently discarded.
    pub async fn update(&self, device: &DeviceRecord) -> Result<()> {
        let changed = sqlx::query(
            r#"
            UPDATE devices SET
                name = ?,
                public_key = ?,
                noise_public_key = ?,
                fingerprint = ?,
                trust_scopes = ?,
                os = ?,
                os_version = ?,
                app_version = ?,
                last_seen = ?,
                revoked = ?,
                revoked_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&device.name)
        .bind(&device.public_key)
        .bind(&device.noise_public_key)
        .bind(&device.fingerprint)
        .bind(&device.trust_scopes)
        .bind(&device.os)
        .bind(&device.os_version)
        .bind(&device.app_version)
        .bind(device.last_seen)
        .bind(device.revoked)
        .bind(device.revoked_at)
        .bind(&device.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
        .rows_affected();

        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no paired device with id '{}'",
                device.id
            ))
            .into());
        }

        Ok(())
    }

    pub async fn update_last_seen(&self, id: &str) -> Result<()> {
        let changed = sqlx::query("UPDATE devices SET last_seen = ? WHERE id = ?")
            .bind(chrono::Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
            .rows_affected();

        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no paired device with id '{id}'"
            ))
            .into());
        }

        Ok(())
    }

    /// Mark a device revoked.
    ///
    /// Revoking an already-revoked device is reported, not silently accepted: the
    /// user asked to remove trust and needs to know it was already gone.
    pub async fn revoke(&self, id: &str) -> Result<()> {
        let changed = sqlx::query("UPDATE devices SET revoked = TRUE, revoked_at = ? WHERE id = ?")
            .bind(chrono::Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
            .rows_affected();

        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no paired device with id '{id}'"
            ))
            .into());
        }

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        // Several tables hold real foreign keys back to `devices(id)` and this
        // pool enforces them, so a bare `DELETE FROM devices` fails while any
        // of those rows remain. Remove every reference first, children before
        // parents, in one transaction. Un-pairing a device also removes the
        // workspaces it sent to this machine (with their own children) and its
        // transfer and restore records; keeping them would mean keeping a row
        // that must point at a device row that no longer exists.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        sqlx::query(
            "DELETE FROM workspace_files WHERE workspace_id IN \
             (SELECT id FROM workspaces WHERE source_device_id = ?)",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        sqlx::query(
            "DELETE FROM snapshots WHERE workspace_id IN \
             (SELECT id FROM workspaces WHERE source_device_id = ?) OR source_device_id = ?",
        )
        .bind(id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        sqlx::query(
            "DELETE FROM restore_runs WHERE workspace_id IN \
             (SELECT id FROM workspaces WHERE source_device_id = ?) OR destination_device_id = ?",
        )
        .bind(id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        sqlx::query(
            "DELETE FROM transfer_sessions WHERE source_device_id = ? OR destination_device_id = ?",
        )
        .bind(id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        sqlx::query("DELETE FROM workspaces WHERE source_device_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let changed = sqlx::query("DELETE FROM devices WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
            .rows_affected();

        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no paired device with id '{id}'"
            ))
            .into());
        }

        tx.commit()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }
}

/// Workspace repository
pub struct WorkspaceRepository {
    pool: DbPool,
}

impl WorkspaceRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, workspace: &WorkspaceRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, schema_version, captured_at, source_device_id, manifest_digest, encrypted_manifest_path, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&workspace.id)
        .bind(&workspace.name)
        .bind(workspace.schema_version)
        .bind(workspace.captured_at)
        .bind(&workspace.source_device_id)
        .bind(&workspace.manifest_digest)
        .bind(&workspace.encrypted_manifest_path)
        .bind(&workspace.status)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<WorkspaceRecord>> {
        let row = sqlx::query_as::<_, WorkspaceRecord>("SELECT * FROM workspaces WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    /// Store a workspace, updating it in place if the id is already present.
    ///
    /// Written for the receiving side of a transfer. A workspace id is generated
    /// by the device that captured it, so a re-send of the *same* workspace
    /// arrives with the same id, and a plain `create` would fail on the primary
    /// key -- leaving the newer manifest sealed on disk with no row pointing at
    /// it, and the user looking at a workspace that is one capture out of date.
    ///
    /// Updating in place rather than `INSERT OR REPLACE` is deliberate. A
    /// workspace is the target of three foreign keys (`snapshots`,
    /// `transfer_sessions`, `restore_runs`), and `INSERT OR REPLACE` implements a
    /// replace as delete-then-insert -- which deletes every one of those rows.
    /// The second send of a workspace would then erase the record of the first,
    /// which is exactly the history a user asks this app to keep.
    ///
    /// Updating is also the honest semantics: a capture is immutable, so two
    /// manifests sharing one id are the same workspace, and the incoming bytes
    /// are the newer copy of it.
    pub async fn upsert(&self, workspace: &WorkspaceRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO workspaces (id, name, schema_version, captured_at, source_device_id, manifest_digest, encrypted_manifest_path, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                schema_version = excluded.schema_version,
                captured_at = excluded.captured_at,
                source_device_id = excluded.source_device_id,
                manifest_digest = excluded.manifest_digest,
                encrypted_manifest_path = excluded.encrypted_manifest_path,
                status = excluded.status
            "#,
        )
        .bind(&workspace.id)
        .bind(&workspace.name)
        .bind(workspace.schema_version)
        .bind(workspace.captured_at)
        .bind(&workspace.source_device_id)
        .bind(&workspace.manifest_digest)
        .bind(&workspace.encrypted_manifest_path)
        .bind(&workspace.status)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn list(&self, limit: i64, offset: i64) -> Result<Vec<WorkspaceRecord>> {
        let rows = sqlx::query_as::<_, WorkspaceRecord>(
            "SELECT * FROM workspaces ORDER BY captured_at DESC LIMIT ? OFFSET ?",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    pub async fn list_by_device(&self, device_id: &str) -> Result<Vec<WorkspaceRecord>> {
        let rows = sqlx::query_as::<_, WorkspaceRecord>(
            "SELECT * FROM workspaces WHERE source_device_id = ? ORDER BY captured_at DESC",
        )
        .bind(device_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    pub async fn update_status(&self, id: &str, status: &str) -> Result<()> {
        let changed = sqlx::query("UPDATE workspaces SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
            .rows_affected();

        // A transfer that reports "sent" against a workspace that is not in the
        // database is a lie the user will act on.
        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no workspace with id '{id}'"
            ))
            .into());
        }

        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        // `workspace_files`, `snapshots`, `restore_runs` and `transfer_sessions`
        // all hold real foreign keys back to `workspaces(id)` and this pool
        // enforces them, so a bare `DELETE FROM workspaces` fails while any of
        // those rows exist. Remove the children first, in one transaction: the
        // delete either fully lands or fully does not.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        for sql in [
            "DELETE FROM workspace_files WHERE workspace_id = ?",
            "DELETE FROM snapshots WHERE workspace_id = ?",
            "DELETE FROM restore_runs WHERE workspace_id = ?",
            "DELETE FROM transfer_sessions WHERE workspace_id = ?",
        ] {
            sqlx::query(sql)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
        }

        let changed = sqlx::query("DELETE FROM workspaces WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
            .rows_affected();

        if changed == 0 {
            return Err(DatabaseError::NotFound(format!(
                "There is no workspace with id '{id}'"
            ))
            .into());
        }

        tx.commit()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }
}

/// Repository for the sealed file archive that backs a workspace's files.
///
/// One archive per workspace id, matching the manifest's id (a workspace is
/// immutable, so a re-send of the same id is the same workspace and carries the
/// newer archive -- the same reasoning as `WorkspaceRepository::upsert`).
pub struct WorkspaceFilesRepository {
    pool: DbPool,
}

impl WorkspaceFilesRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Record the sealed archive, replacing an older one for the same
    /// workspace id (a re-send supersedes the previous copy).
    pub async fn upsert(&self, record: &WorkspaceFilesRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO workspace_files (workspace_id, encrypted_files_path, byte_count, file_count, archive_format)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(workspace_id) DO UPDATE SET
                encrypted_files_path = excluded.encrypted_files_path,
                byte_count = excluded.byte_count,
                file_count = excluded.file_count,
                archive_format = excluded.archive_format
            "#
        )
        .bind(&record.workspace_id)
        .bind(&record.encrypted_files_path)
        .bind(record.byte_count)
        .bind(record.file_count)
        .bind(&record.archive_format)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, workspace_id: &str) -> Result<Option<WorkspaceFilesRecord>> {
        let row = sqlx::query_as::<_, WorkspaceFilesRecord>(
            "SELECT * FROM workspace_files WHERE workspace_id = ?",
        )
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }
}

/// Snapshot repository
pub struct SnapshotRepository {
    pool: DbPool,
}

impl SnapshotRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, snapshot: &SnapshotRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO snapshots (id, workspace_id, captured_at, source_device_id, size_bytes, transfer_status, transfer_id)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&snapshot.id)
        .bind(&snapshot.workspace_id)
        .bind(snapshot.captured_at)
        .bind(&snapshot.source_device_id)
        .bind(snapshot.size_bytes)
        .bind(&snapshot.transfer_status)
        .bind(&snapshot.transfer_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get_by_workspace(&self, workspace_id: &str) -> Result<Vec<SnapshotRecord>> {
        let rows = sqlx::query_as::<_, SnapshotRecord>(
            "SELECT * FROM snapshots WHERE workspace_id = ? ORDER BY captured_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    pub async fn update_transfer_status(
        &self,
        id: &str,
        status: &str,
        transfer_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE snapshots SET transfer_status = ?, transfer_id = ? WHERE id = ?")
            .bind(status)
            .bind(transfer_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }
}

/// Restore run repository
pub struct RestoreRunRepository {
    pool: DbPool,
}

impl RestoreRunRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, run: &RestoreRunRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO restore_runs (id, workspace_id, destination_device_id, plan_digest, approved_steps, result_summary, status, started_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(&run.id)
        .bind(&run.workspace_id)
        .bind(&run.destination_device_id)
        .bind(&run.plan_digest)
        .bind(&run.approved_steps)
        .bind(&run.result_summary)
        .bind(&run.status)
        .bind(run.started_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<RestoreRunRecord>> {
        let row = sqlx::query_as::<_, RestoreRunRecord>("SELECT * FROM restore_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    pub async fn list_by_workspace(&self, workspace_id: &str) -> Result<Vec<RestoreRunRecord>> {
        let rows = sqlx::query_as::<_, RestoreRunRecord>(
            "SELECT * FROM restore_runs WHERE workspace_id = ? ORDER BY started_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    pub async fn update(&self, run: &RestoreRunRecord) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE restore_runs SET
                result_summary = ?,
                status = ?,
                completed_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&run.result_summary)
        .bind(&run.status)
        .bind(run.completed_at)
        .bind(&run.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }
}

/// Adapter check repository
pub struct AdapterCheckRepository {
    pool: DbPool,
}

impl AdapterCheckRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn upsert(&self, check: &AdapterCheckRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO adapter_checks (id, adapter_id, adapter_version, result_state, safe_evidence, checked_at, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                adapter_id = ?,
                adapter_version = ?,
                result_state = ?,
                safe_evidence = ?,
                checked_at = ?,
                expires_at = ?
            "#
        )
        .bind(&check.id)
        .bind(&check.adapter_id)
        .bind(check.adapter_version)
        .bind(&check.result_state)
        .bind(&check.safe_evidence)
        .bind(check.checked_at)
        .bind(check.expires_at)
        .bind(&check.adapter_id)
        .bind(check.adapter_version)
        .bind(&check.result_state)
        .bind(&check.safe_evidence)
        .bind(check.checked_at)
        .bind(check.expires_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get_latest(&self, adapter_id: &str) -> Result<Option<AdapterCheckRecord>> {
        let row = sqlx::query_as::<_, AdapterCheckRecord>(
            "SELECT * FROM adapter_checks WHERE adapter_id = ? ORDER BY checked_at DESC LIMIT 1",
        )
        .bind(adapter_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    /// Fetch a cached check by its stable key.
    ///
    /// The key is the preflight engine's `adapter::requirement` string, so two
    /// requirements handled by the same adapter get their own rows instead of
    /// sharing whatever was written last.
    pub async fn get_by_id(&self, id: &str) -> Result<Option<AdapterCheckRecord>> {
        let row = sqlx::query_as::<_, AdapterCheckRecord>(
            "SELECT * FROM adapter_checks WHERE id = ? LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    /// Drop every cached check. Used when a manifest changes, since a cached
    /// answer to a different question is worse than no answer.
    pub async fn clear(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM adapter_checks")
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(result.rows_affected())
    }

    pub async fn cleanup_expired(&self) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM adapter_checks WHERE expires_at IS NOT NULL AND expires_at < ?",
        )
        .bind(chrono::Utc::now())
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(result.rows_affected())
    }
}

/// Settings repository
pub struct SettingsRepository {
    pool: DbPool,
}

impl SettingsRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row.map(|r| r.get("value")))
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = ?, updated_at = ?",
        )
        .bind(key)
        .bind(value)
        .bind(chrono::Utc::now())
        .bind(value)
        .bind(chrono::Utc::now())
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| (r.get("key"), r.get("value")))
            .collect())
    }
}

/// Transfer session repository
///
/// The `transfer_sessions` table was in the schema from the first migration and
/// nothing ever wrote to it, so every transfer this app performed existed only in
/// the window between the send finishing and the window closing. If a user asked
/// "did that workspace actually get there?", the honest answer was that the
/// app had thrown the information away.
///
/// Both ends of a transfer are recorded: the sender writes a row when it starts
/// and updates it as the outcome is known, and the receiver writes one for what
/// it accepted. The `id` is the transfer id the network layer generated, so a row
/// can be matched against a `TransferSession` the UI is already holding.
pub struct TransferSessionRepository {
    pool: DbPool,
}

impl TransferSessionRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    /// Record a transfer that is starting.
    ///
    /// `INSERT OR REPLACE` rather than `INSERT`: a re-sent workspace reuses the
    /// workspace id but gets a fresh transfer id, and a retried insert of the
    /// same id should leave one row with the newest state rather than fail on
    /// the primary key and lose the newer state.
    pub async fn create(&self, session: &TransferSessionRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO transfer_sessions
                (id, workspace_id, source_device_id, destination_device_id,
                 status, progress, started_at, completed_at, error)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&session.id)
        .bind(&session.workspace_id)
        .bind(&session.source_device_id)
        .bind(&session.destination_device_id)
        .bind(&session.status)
        .bind(session.progress)
        .bind(session.started_at)
        .bind(session.completed_at)
        .bind(&session.error)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    /// Record how a transfer ended.
    pub async fn finish(
        &self,
        id: &str,
        status: &str,
        progress: f32,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE transfer_sessions
             SET status = ?, progress = ?, completed_at = ?, error = ?
             WHERE id = ?",
        )
        .bind(status)
        .bind(progress)
        .bind(chrono::Utc::now())
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, id: &str) -> Result<Option<TransferSessionRecord>> {
        let row = sqlx::query_as::<_, TransferSessionRecord>(
            "SELECT * FROM transfer_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(row)
    }

    /// Every transfer of a workspace, newest first.
    pub async fn list_for_workspace(&self, workspace_id: &str) -> Result<Vec<TransferSessionRecord>> {
        let rows = sqlx::query_as::<_, TransferSessionRecord>(
            "SELECT * FROM transfer_sessions WHERE workspace_id = ? ORDER BY started_at DESC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }

    /// Every transfer this device has taken part in, newest first.
    pub async fn list(&self) -> Result<Vec<TransferSessionRecord>> {
        let rows = sqlx::query_as::<_, TransferSessionRecord>(
            "SELECT * FROM transfer_sessions ORDER BY started_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the local-device row and, in particular, re-keying it.
    //!
    //! The re-key path is the one that can quietly destroy a user's history, so
    //! it is tested against a real migrated database with foreign keys on --
    //! which is exactly the configuration that would reject a partial update.

    use super::*;
    use chrono::Utc;

    /// A temporary database file, removed when the test ends.
    ///
    /// A file rather than an in-memory pool, because sqlx hands each checkout a
    /// different connection and an in-memory database is per-connection, so the
    /// migrations would not be visible to the code under test.
    struct TempDb(PathBuf);

    impl TempDb {
        async fn open(name: &str) -> (Self, DbPool) {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "workspace-clone-{name}-{}-{}.db",
                std::process::id(),
                Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let pool = init_db_at(&path).await.expect("migrations should apply");
            (TempDb(path), pool)
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            // The WAL and shared-memory files are separate; leaving them behind
            // is untidy but harmless, and a leftover of the same name is
            // impossible because the name carries a nanosecond timestamp.
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.0.display(), suffix));
            }
        }
    }

    fn device(id: &str, noise_key: &str) -> DeviceRecord {
        DeviceRecord {
            id: id.to_string(),
            name: "Test Machine".to_string(),
            public_key: "ed25519-public".to_string(),
            noise_public_key: noise_key.to_string(),
            fingerprint: id.to_string(),
            trust_scopes: "[]".to_string(),
            os: "macos".to_string(),
            os_version: "15.0".to_string(),
            app_version: "0.1.0".to_string(),
            created_at: Utc::now(),
            last_seen: Some(Utc::now()),
            revoked: false,
            revoked_at: None,
        }
    }

    #[tokio::test]
    async fn ensure_local_inserts_on_first_run_and_is_a_noop_after() {
        let (_db, pool) = TempDb::open("ensure-local").await;
        let repo = DeviceRepository::new(pool.clone());
        let record = device("noise-fingerprint-abc", "noise-key-1");

        let first = repo.ensure_local(&record).await.unwrap();
        assert_eq!(first.id, "noise-fingerprint-abc");

        // A second call must not create a second row, or the device list grows
        // by one on every launch.
        repo.ensure_local(&record).await.unwrap();
        assert_eq!(repo.list(true).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_device_whose_id_derivation_changed_is_rekeyed_not_duplicated() {
        // The situation this exists for: an earlier build derived a device's id
        // from a different key, so the row for *this same device* sits under an
        // id the current build would never compute. Inserting a second row
        // instead would leave two rows for one machine, both named the same, and
        // the old one would keep every capture the user ever made.
        let (_db, pool) = TempDb::open("rekey").await;
        let repo = DeviceRepository::new(pool.clone());

        // The row as the old build wrote it.
        let stale = device("ed25519-derived-id", "noise-key-1");
        repo.create(&stale).await.unwrap();

        // The same device, as the current build identifies it.
        let current = device("noise-derived-id", "noise-key-1");
        let result = repo.ensure_local(&current).await.unwrap();

        assert_eq!(result.id, "noise-derived-id");
        let all = repo.list(true).await.unwrap();
        assert_eq!(all.len(), 1, "one machine, one row");
        assert_eq!(all[0].id, "noise-derived-id");
        assert!(
            repo.get("ed25519-derived-id").await.unwrap().is_none(),
            "the superseded id must not remain, or it shows up in the device list"
        );
    }

    #[tokio::test]
    async fn rekeying_keeps_every_workspace_that_referenced_the_old_id() {
        // The reason this is a transaction rather than two statements. Foreign
        // keys are enforced, so repointing the device row without the captures
        // would leave a workspace naming a device that no longer exists: a row
        // the user can see in their history and cannot open.
        let (_db, pool) = TempDb::open("rekey-workspaces").await;
        let repo = DeviceRepository::new(pool.clone());

        repo.create(&device("old-id", "noise-key-1")).await.unwrap();
        repo.create(&device("peer-id", "noise-key-2")).await.unwrap();

        let workspaces = WorkspaceRepository::new(pool.clone());
        for id in ["ws-1", "ws-2"] {
            workspaces
                .create(&WorkspaceRecord {
                    id: id.to_string(),
                    name: format!("Workspace {id}"),
                    schema_version: 1,
                    captured_at: Utc::now(),
                    source_device_id: "old-id".to_string(),
                    manifest_digest: "digest".to_string(),
                    encrypted_manifest_path: "/tmp/manifest".to_string(),
                    status: "captured".to_string(),
                })
                .await
                .unwrap();
        }

        // A workspace captured by a *peer* must be left alone: its source is a
        // different machine and re-keying ours says nothing about it.
        workspaces
            .create(&WorkspaceRecord {
                id: "ws-3".to_string(),
                name: "Someone else's workspace".to_string(),
                schema_version: 1,
                captured_at: Utc::now(),
                source_device_id: "peer-id".to_string(),
                manifest_digest: "digest".to_string(),
                encrypted_manifest_path: "/tmp/manifest".to_string(),
                status: "captured".to_string(),
            })
            .await
            .unwrap();

        DeviceRepository::new(pool.clone())
            .ensure_local(&device("new-id", "noise-key-1"))
            .await
            .unwrap();

        for id in ["ws-1", "ws-2"] {
            let ws = workspaces.get(id).await.unwrap().expect("still there");
            assert_eq!(
                ws.source_device_id, "new-id",
                "{id} must follow the device to its new id"
            );
        }

        let peer_workspace = workspaces.get("ws-3").await.unwrap().unwrap();
        assert_eq!(
            peer_workspace.source_device_id, "peer-id",
            "another device's capture is not ours to rewrite"
        );
    }

    #[tokio::test]
    async fn looking_a_device_up_by_an_empty_noise_key_matches_nothing() {
        // A row written before the Noise column existed holds an empty string
        // there. Matching every such row against an empty search key would make
        // a device with no key adopt a stranger's identity on next launch.
        let (_db, pool) = TempDb::open("empty-noise-key").await;
        let repo = DeviceRepository::new(pool.clone());

        repo.create(&device("legacy-id", "")).await.unwrap();

        assert!(
            repo.get_by_noise_key("").await.unwrap().is_none(),
            "an empty key is not an identity"
        );
    }

    #[tokio::test]
    async fn a_device_is_not_found_rather_than_silently_created() {
        // These return `NotFound` so a rename of a deleted device reports
        // failure instead of claiming success and discarding the new name.
        let (_db, pool) = TempDb::open("not-found").await;
        let repo = DeviceRepository::new(pool.clone());

        let mut missing = device("ghost", "noise-key-9");
        missing.name = "Renamed".to_string();
        assert!(repo.update(&missing).await.is_err());

        assert!(WorkspaceRepository::new(pool.clone())
            .delete("no-such-workspace")
            .await
            .is_err());
    }

    /// Creates a peer-owned workspace with one row in every table that
    /// `WorkspaceRepository::delete` and `DeviceRepository::delete` must clear.
    async fn workspace_with_children(
        workspaces: &WorkspaceRepository,
        files: &WorkspaceFilesRepository,
        snapshots: &SnapshotRepository,
        runs: &RestoreRunRepository,
        sessions: &TransferSessionRepository,
        id: &str,
        source_device: &str,
    ) {
        workspaces
            .create(&WorkspaceRecord {
                id: id.to_string(),
                name: "Workspace".to_string(),
                schema_version: 1,
                captured_at: Utc::now(),
                source_device_id: source_device.to_string(),
                manifest_digest: "digest".to_string(),
                encrypted_manifest_path: format!("/tmp/{id}.sealed.json"),
                status: "received".to_string(),
            })
            .await
            .unwrap();
        files
            .upsert(&WorkspaceFilesRecord {
                workspace_id: id.to_string(),
                encrypted_files_path: format!("/tmp/{id}.files.sealed"),
                byte_count: 10,
                file_count: 2,
                archive_format: "tar".to_string(),
            })
            .await
            .unwrap();
        snapshots
            .create(&SnapshotRecord {
                id: format!("{id}-snap"),
                workspace_id: id.to_string(),
                captured_at: Utc::now(),
                source_device_id: source_device.to_string(),
                size_bytes: 10,
                transfer_status: "completed".to_string(),
                transfer_id: Some(format!("{id}-tr")),
            })
            .await
            .unwrap();
        runs
            .create(&RestoreRunRecord {
                id: format!("{id}-run"),
                workspace_id: id.to_string(),
                destination_device_id: source_device.to_string(),
                plan_digest: "digest".to_string(),
                approved_steps: "[]".to_string(),
                result_summary: "{}".to_string(),
                status: "completed".to_string(),
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
            })
            .await
            .unwrap();
        sessions
            .create(&TransferSessionRecord {
                id: format!("{id}-tr"),
                workspace_id: id.to_string(),
                source_device_id: source_device.to_string(),
                destination_device_id: "local-id".to_string(),
                status: "completed".to_string(),
                progress: 1.0,
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
                error: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn deleting_a_workspace_removes_every_referencing_row() {
        // This is the "Delete workspace" the UI offers. It used to be a bare
        // `DELETE FROM workspaces`, which failed whenever any of the four child
        // tables held a row for it -- which is every workspace with files, a
        // snapshot, or a transfer record. The delete has to clear the children
        // first, and all inside one transaction.
        let (_db, pool) = TempDb::open("delete-workspace").await;
        DeviceRepository::new(pool.clone())
            .create(&device("local-id", "noise-key-1"))
            .await
            .unwrap();
        DeviceRepository::new(pool.clone())
            .create(&device("peer-id", "noise-key-2"))
            .await
            .unwrap();

        let workspaces = WorkspaceRepository::new(pool.clone());
        let files = WorkspaceFilesRepository::new(pool.clone());
        let snapshots = SnapshotRepository::new(pool.clone());
        let runs = RestoreRunRepository::new(pool.clone());
        let sessions = TransferSessionRepository::new(pool.clone());

        workspace_with_children(
            &workspaces,
            &files,
            &snapshots,
            &runs,
            &sessions,
            "ws-1",
            "peer-id",
        )
        .await;

        workspaces.delete("ws-1").await.unwrap();

        assert!(workspaces.get("ws-1").await.unwrap().is_none());
        assert!(files.get("ws-1").await.unwrap().is_none());
        assert!(snapshots.get_by_workspace("ws-1").await.unwrap().is_empty());
        assert!(runs.list_by_workspace("ws-1").await.unwrap().is_empty());
        assert!(sessions.list_for_workspace("ws-1").await.unwrap().is_empty());
        assert!(
            DeviceRepository::new(pool.clone())
                .get("peer-id")
                .await
                .unwrap()
                .is_some(),
            "deleting a workspace must not delete the device that sent it"
        );
    }

    #[tokio::test]
    async fn deleting_a_paired_device_removes_its_workspaces_and_records() {
        // The "Forget" action on the Devices screen. Same trap as the workspace
        // delete, one level up: `workspaces`, `snapshots`, `restore_runs` and
        // `transfer_sessions` all hold real foreign keys to `devices(id)`, so a
        // bare delete fails while a received workspace or a transfer record
        // still names the device. Everything that referenced the device must go
        // with it, while workspaces this machine captured itself stay.
        let (_db, pool) = TempDb::open("delete-device").await;
        DeviceRepository::new(pool.clone())
            .create(&device("local-id", "noise-key-1"))
            .await
            .unwrap();
        DeviceRepository::new(pool.clone())
            .create(&device("peer-id", "noise-key-2"))
            .await
            .unwrap();

        let workspaces = WorkspaceRepository::new(pool.clone());
        let files = WorkspaceFilesRepository::new(pool.clone());
        let snapshots = SnapshotRepository::new(pool.clone());
        let runs = RestoreRunRepository::new(pool.clone());
        let sessions = TransferSessionRepository::new(pool.clone());

        // A workspace this machine captured: the device delete must not touch it.
        workspace_with_children(
            &workspaces,
            &files,
            &snapshots,
            &runs,
            &sessions,
            "ws-mine",
            "local-id",
        )
        .await;
        // Workspaces the peer sent (received) plus a transfer without a workspace.
        for id in ["ws-peer-1", "ws-peer-2"] {
            workspace_with_children(
                &workspaces,
                &files,
                &snapshots,
                &runs,
                &sessions,
                id,
                "peer-id",
            )
            .await;
        }
        sessions
            .create(&TransferSessionRecord {
                id: "tr-peer-only".to_string(),
                workspace_id: "ws-peer-1".to_string(),
                source_device_id: "peer-id".to_string(),
                destination_device_id: "local-id".to_string(),
                status: "refused".to_string(),
                progress: 0.0,
                started_at: Utc::now(),
                completed_at: None,
                error: Some("not paired".to_string()),
            })
            .await
            .unwrap();

        DeviceRepository::new(pool.clone())
            .delete("peer-id")
            .await
            .unwrap();

        let devices = DeviceRepository::new(pool.clone());
        assert!(devices.get("peer-id").await.unwrap().is_none());
        assert!(
            devices.get("local-id").await.unwrap().is_some(),
            "forgetting a peer must not forget this machine"
        );

        for id in ["ws-peer-1", "ws-peer-2"] {
            assert!(workspaces.get(id).await.unwrap().is_none());
            assert!(files.get(id).await.unwrap().is_none());
        }
        assert!(sessions.list_for_workspace("ws-peer-1").await.unwrap().is_empty());
        assert!(
            sessions.list().await.unwrap().iter().all(|s| s.id != "tr-peer-only"),
            "a transfer session naming the deleted device must not survive"
        );

        // The locally captured workspace is untouched, children and all.
        assert!(workspaces.get("ws-mine").await.unwrap().is_some());
        assert!(files.get("ws-mine").await.unwrap().is_some());
        assert_eq!(sessions.list_for_workspace("ws-mine").await.unwrap().len(), 1);
    }
}
