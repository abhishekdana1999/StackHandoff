//! Application discovery commands.
//!
//! Discovers running user-facing applications and their open folders for
//! open context capture. Privacy-preserving: only what the user selects
//! travels in the manifest.

use serde::Serialize;
use tauri::command;
use tracing::{debug, info};
use workspace_clone_adapters::{AppDiscoveryAdapter, ApplicationDiscoveryRequest};
use workspace_clone_core::Result;

/// Discovered application with its open folders
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredApplication {
    pub id: String,
    pub name: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    pub open_folders: Vec<DiscoveredFolder>,
    pub has_adapter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter_id: Option<String>,
}

/// A folder discovered as open in an application
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredFolder {
    pub path: String,
    pub name: String,
    pub is_git_repo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub git_dirty: bool,
}

/// Result of application discovery
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationDiscoveryResult {
    pub applications: Vec<DiscoveredApplication>,
    pub warnings: Vec<String>,
}

/// Discover running applications and their open folders.
///
/// This is an explicit opt-in operation. The user must request discovery,
/// and only the applications and folders they select are carried forward.
///
/// Browser tab discovery requires an additional explicit opt-in flag.
#[command]
pub async fn discover_applications(
    include_browser_tabs: Option<bool>,
    browser_ids: Option<Vec<String>>,
) -> Result<ApplicationDiscoveryResult> {
    let request = ApplicationDiscoveryRequest {
        include_browser_tabs: include_browser_tabs.unwrap_or(false),
        browser_ids: browser_ids.unwrap_or_default(),
    };

    info!(
        "Discovering applications (browser_tabs={:?}, browser_ids={:?})",
        request.include_browser_tabs, request.browser_ids
    );

    let adapter = AppDiscoveryAdapter::new();
    let result = adapter.discover(&request).await?;

    // Convert to command types
    let applications: Vec<DiscoveredApplication> = result
        .applications
        .into_iter()
        .map(|app| DiscoveredApplication {
            id: app.id,
            name: app.name,
            category: format!("{:?}", app.category).to_lowercase(),
            executable_path: app.executable_path,
            open_folders: app
                .open_folders
                .into_iter()
                .map(|f| DiscoveredFolder {
                    path: f.path,
                    name: f.name,
                    is_git_repo: f.is_git_repo,
                    git_branch: f.git_branch,
                    git_dirty: f.git_dirty,
                })
                .collect(),
            has_adapter: app.has_adapter,
            adapter_id: app.adapter_id,
        })
        .collect();

    debug!("Discovered {} applications", applications.len());

    Ok(ApplicationDiscoveryResult {
        applications,
        warnings: result.warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_result_serializes_correctly() {
        let result = ApplicationDiscoveryResult {
            applications: vec![DiscoveredApplication {
                id: "editor-vscode".to_string(),
                name: "Visual Studio Code".to_string(),
                category: "editor".to_string(),
                executable_path: Some("/Applications/Visual Studio Code.app".to_string()),
                open_folders: vec![DiscoveredFolder {
                    path: "/Users/me/code/project".to_string(),
                    name: "project".to_string(),
                    is_git_repo: true,
                    git_branch: Some("main".to_string()),
                    git_dirty: false,
                }],
                has_adapter: true,
                adapter_id: Some("vscode".to_string()),
            }],
            warnings: vec!["Browser tab discovery requires explicit opt-in".to_string()],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("editor-vscode"));
        assert!(json.contains("Visual Studio Code"));
        assert!(json.contains("editor"));
        assert!(json.contains("project"));
        assert!(json.contains("vscode"));
    }
}