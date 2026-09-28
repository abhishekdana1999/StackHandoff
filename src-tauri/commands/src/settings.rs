//! Settings Tauri commands

use serde_json::Value;
use tauri::{command, State};
use workspace_clone_core::Result;
use workspace_clone_db::{repository::SettingsRepository, DbPool};

#[command]
pub async fn get_setting(pool: State<'_, DbPool>, key: String) -> Result<Option<String>> {
    let repo = SettingsRepository::new(pool.inner().clone());
    repo.get(&key).await
}

#[command]
pub async fn set_setting(pool: State<'_, DbPool>, key: String, value: String) -> Result<()> {
    let repo = SettingsRepository::new(pool.inner().clone());
    repo.set(&key, &value).await
}

#[command]
pub async fn delete_setting(pool: State<'_, DbPool>, key: String) -> Result<()> {
    let repo = SettingsRepository::new(pool.inner().clone());
    repo.delete(&key).await
}

#[command]
pub async fn list_settings(pool: State<'_, DbPool>) -> Result<Vec<(String, String)>> {
    let repo = SettingsRepository::new(pool.inner().clone());
    repo.list().await
}

#[command]
pub async fn export_settings(pool: State<'_, DbPool>) -> Result<Value> {
    let repo = SettingsRepository::new(pool.inner().clone());
    let settings = repo.list().await?;
    Ok(serde_json::to_value(settings)?)
}

#[command]
pub async fn import_settings(pool: State<'_, DbPool>, settings_json: String) -> Result<()> {
    let repo = SettingsRepository::new(pool.inner().clone());
    let settings: Vec<(String, String)> = serde_json::from_str(&settings_json)?;

    for (key, value) in settings {
        repo.set(&key, &value).await?;
    }

    Ok(())
}
