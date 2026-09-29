//! Workspace management Tauri commands

use tauri::{command, State};
use workspace_clone_core::Result;
use workspace_clone_db::{
    models::{RestoreRunRecord, SnapshotRecord, WorkspaceRecord},
    repository::{RestoreRunRepository, SnapshotRepository, WorkspaceFilesRepository, WorkspaceRepository},
    DbPool,
};

#[command]
pub async fn list_workspaces(
    pool: State<'_, DbPool>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<WorkspaceRecord>> {
    let repo = WorkspaceRepository::new(pool.inner().clone());
    repo.list(limit.unwrap_or(50), offset.unwrap_or(0)).await
}

#[command]
pub async fn get_workspace(
    pool: State<'_, DbPool>,
    workspace_id: String,
) -> Result<Option<WorkspaceRecord>> {
    let repo = WorkspaceRepository::new(pool.inner().clone());
    repo.get(&workspace_id).await
}

#[command]
pub async fn get_workspace_snapshots(
    pool: State<'_, DbPool>,
    workspace_id: String,
) -> Result<Vec<SnapshotRecord>> {
    let repo = SnapshotRepository::new(pool.inner().clone());
    repo.get_by_workspace(&workspace_id).await
}

#[command]
pub async fn get_workspace_restore_runs(
    pool: State<'_, DbPool>,
    workspace_id: String,
) -> Result<Vec<RestoreRunRecord>> {
    let repo = RestoreRunRepository::new(pool.inner().clone());
    repo.list_by_workspace(&workspace_id).await
}

#[command]
pub async fn delete_workspace(pool: State<'_, DbPool>, workspace_id: String) -> Result<()> {
    let repo = WorkspaceRepository::new(pool.inner().clone());

    // The paths that would be orphaned by the row deletion, removed after the
    // database has committed. A workspace's sealed manifest, and -- when files
    // were captured or received -- the sealed archive beside it.
    let manifest_path = repo
        .get(&workspace_id)
        .await?
        .map(|record| record.encrypted_manifest_path);
    let files_path = WorkspaceFilesRepository::new(pool.inner().clone())
        .get(&workspace_id)
        .await?
        .map(|record| record.encrypted_files_path);

    repo.delete(&workspace_id).await?;

    // Deleting rows is the operation that matters; a file that cannot be
    // removed (permissions, a concurrent reader) must not fail a delete the
    // database already committed. The remaining file is reported rather than
    // half-removing the workspace.
    for path in [manifest_path, files_path].into_iter().flatten() {
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(
                "Deleted workspace '{workspace_id}' but could not remove its file '{}': {e}",
                path
            ),
        }
    }

    Ok(())
}
