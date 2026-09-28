//! Workspace management Tauri commands

use tauri::{command, State};
use workspace_clone_core::Result;
use workspace_clone_db::{
    models::{RestoreRunRecord, SnapshotRecord, WorkspaceRecord},
    repository::{RestoreRunRepository, SnapshotRepository, WorkspaceRepository},
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
    repo.delete(&workspace_id).await
}
