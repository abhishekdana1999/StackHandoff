//! VS Code adapter for editor detection and workspace capture.

use crate::traits::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tracing::debug;
use workspace_clone_core::{manifest::*, AdapterError, Result};

pub struct VSCodeAdapter;

impl VSCodeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Locate the `code` CLI, preferring PATH and falling back to the standard
    /// install locations for each platform.
    fn find_vscode() -> Option<PathBuf> {
        if let Ok(path) = which::which("code") {
            return Some(path);
        }
        Self::find_vscode_well_known()
    }

    /// Well-known install locations, tried after PATH.
    #[cfg(target_os = "windows")]
    fn find_vscode_well_known() -> Option<PathBuf> {
        // The machine-wide install: what you get when VS Code is installed with
        // administrator rights.
        for candidate in [
            r"C:\Program Files\Microsoft VS Code\bin\code.cmd",
            r"C:\Program Files (x86)\Microsoft VS Code\bin\code.cmd",
        ] {
            let candidate = PathBuf::from(candidate);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        // The per-user install, which is VS Code's default when the account has
        // no administrator rights -- i.e. most laptops, including the machine
        // this app is designed to restore onto.
        //
        // This used to be the literal path
        //   C:\Users\USERNAME\AppData\Local\Programs\Microsoft VS Code\bin\code.cmd
        // with "USERNAME" standing in for the account name. Nothing expands it:
        // `Path::exists()` reads it as a real directory named "USERNAME", the
        // check fails, and every per-user install was invisible to the adapter.
        //
        // The failure mode was not a crash but a lie in the manifest. The
        // laptop's editor was recorded as absent, and the preflight screen
        // told the user to install something they already had -- the exact
        // false alarm the preflight screen exists to prevent.
        for var in ["LOCALAPPDATA", "APPDATA"] {
            let Some(base) = std::env::var_os(var) else {
                continue;
            };
            let candidate = PathBuf::from(base)
                .join("Programs")
                .join("Microsoft VS Code")
                .join("bin")
                .join("code.cmd");
            if candidate.exists() {
                return Some(candidate);
            }
        }

        None
    }

    #[cfg(not(target_os = "windows"))]
    fn find_vscode_well_known() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
            "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin/code",
            "/usr/local/bin/code",
            "/opt/homebrew/bin/code",
        ];
        #[cfg(target_os = "linux")]
        let candidates = ["/usr/bin/code", "/usr/local/bin/code", "/snap/bin/code"];
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        let candidates: [&str; 0] = [];

        candidates
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
    }

    /// Installed extension identifiers, or an empty list if the CLI cannot be
    /// queried.
    ///
    /// Extensions are captured because a missing language server silently
    /// breaks an editor session, which is exactly the kind of surprise the
    /// preflight screen exists to prevent.
    fn get_extensions(vscode_path: &Path) -> Vec<String> {
        let Ok(output) = Command::new(vscode_path)
            .args(["--list-extensions"])
            // Without this, `code` waits on stdin and the capture hangs.
            .stdin(Stdio::null())
            .output()
        else {
            debug!("Could not list VS Code extensions");
            return Vec::new();
        };

        if !output.status.success() {
            return Vec::new();
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// A `.code-workspace` file directly inside `dir`, if there is one.
    fn workspace_file_in(dir: &Path) -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "code-workspace"))
    }
}

#[async_trait]
impl WorkspaceAdapter for VSCodeAdapter {
    fn id(&self) -> &str {
        "vscode"
    }

    fn version(&self) -> u32 {
        2
    }

    fn supported_platforms(&self) -> Vec<&'static str> {
        vec!["windows", "macos", "linux", "all"]
    }

    async fn detect(&self, _context: &LocalContext) -> Result<DetectionResult> {
        let vscode_path = Self::find_vscode();
        let available = vscode_path.is_some();

        let version = vscode_path.as_ref().and_then(|path| {
            Command::new(path)
                .args(["--version"])
                .stdin(Stdio::null())
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.lines().next().map(str::trim).map(str::to_string))
                .filter(|v| !v.is_empty())
        });

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "path".to_string(),
            serde_json::json!(vscode_path
                .as_ref()
                .map_or_else(String::new, |p| p.to_string_lossy().to_string())),
        );
        metadata.insert("version".to_string(), serde_json::json!(version.clone()));
        metadata.insert(
            "extension_count".to_string(),
            serde_json::json!(vscode_path
                .as_ref()
                .map(|p| Self::get_extensions(p).len())
                .unwrap_or(0)),
        );

        Ok(DetectionResult {
            adapter_id: self.id().to_string(),
            available,
            version,
            path: vscode_path.map(|p| p.to_string_lossy().to_string()),
            metadata,
        })
    }

    async fn capture(
        &self,
        _context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext> {
        let mut applications = Vec::new();
        let vscode_path = Self::find_vscode();

        // Only query the extension list once: it shells out to the editor and
        // is the slowest thing in the capture path.
        let extensions = vscode_path
            .as_ref()
            .map(|p| Self::get_extensions(p))
            .unwrap_or_default();

        for project in &selection.projects {
            if !selection.includes(self.id()) {
                break;
            }
            let path = PathBuf::from(&project.source_path);

            // A multi-root workspace file is the more faithful thing to reopen
            // than the folder, so prefer it when one exists.
            let target = Self::workspace_file_in(&path)
                .map(|file| (file, "workspace_file"))
                .unwrap_or_else(|| (path.clone(), "folder"));

            let (target_path, kind) = target;

            applications.push(Application {
                id: format!("vscode-{}", project.id),
                adapter: self.id().to_string(),
                project_id: Some(project.id.clone()),
                required: false,
                config: serde_json::json!({
                    // Recorded as a hint, not a path, for the same reason the
                    // project path is redacted. This used to be written as
                    // `"kind": <absolute path>` and then overwritten by the
                    // real `kind` below, which meant the live path only escaped
                    // capture because a duplicate JSON key happened to shadow
                    // it. That is not a safety mechanism.
                    "target_hint": crate::git::redact_path(&target_path),
                    "kind": kind,
                    "project_id": project.id,
                    "extensions": extensions,
                }),
            });
        }

        Ok(PortableContext {
            adapter_id: self.id().to_string(),
            data: serde_json::to_value(applications)?,
        })
    }

    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>> {
        let vscode_path = Self::find_vscode();
        let installed_extensions: Vec<String> = vscode_path
            .as_ref()
            .map(|p| Self::get_extensions(p))
            .unwrap_or_default();

        let mut results = Vec::new();

        for req in requirements {
            if req.adapter_id != self.id() {
                continue;
            }

            let available = vscode_path.is_some();
            // Extensions the source expected, from the manifest requirement.
            let wanted: Vec<String> = req
                .config
                .get("extensions")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            let missing: Vec<String> = wanted
                .iter()
                .filter(|e| !installed_extensions.contains(e))
                .cloned()
                .collect();

            let (status, evidence) = if !available {
                (
                    CheckStatus::Unknown,
                    "VS Code was not found in PATH or any standard location".to_string(),
                )
            } else if wanted.is_empty() {
                (
                    CheckStatus::ReadyVerified,
                    format!("VS Code found at {}", vscode_path.as_ref().unwrap().display()),
                )
            } else if missing.is_empty() {
                (
                    CheckStatus::ReadyVerified,
                    format!(
                        "VS Code found and all {} expected extensions are installed",
                        wanted.len()
                    ),
                )
            } else {
                (
                    CheckStatus::ConfiguredUnverified,
                    format!(
                        "VS Code is missing {} of the expected extensions: {}",
                        missing.len(),
                        missing.join(", ")
                    ),
                )
            };

            results.push(CheckResult {
                requirement_id: req.id.clone(),
                status,
                evidence,
                freshness: chrono::Utc::now(),
                action: if available {
                    (!missing.is_empty()).then(|| RemediationAction {
                        label: "Install missing extensions".to_string(),
                        action_type: ActionType::RunCommand,
                        url: None,
                        // Offered for the user to run, never run automatically.
                        command: Some(format!(
                            "code --install-extension {}",
                            missing.join(" --install-extension ")
                        )),
                    })
                } else {
                    Some(RemediationAction {
                        label: "Install VS Code".to_string(),
                        action_type: ActionType::InstallApp,
                        url: Some("https://code.visualstudio.com/".to_string()),
                        command: None,
                    })
                },
            });
        }

        Ok(results)
    }

    async fn plan_restore(&self, context: &PortableContext) -> Result<Vec<RestoreAction>> {
        let apps: Vec<Application> = serde_json::from_value(context.data.clone())
            .map_err(|e| AdapterError::Capture(e.to_string()))?;

        Ok(apps
            .into_iter()
            .map(|app| {
                let hint = app
                    .config
                    .get("target_hint")
                    .and_then(|v| v.as_str())
                    .unwrap_or("the project");

                // The action repeats the project id because `config` alone does
                // not carry it: `Application` declares `project_id` as a named
                // field, so serde's flattening strips it out of the flattened
                // value. Without this the destination step cannot tell which
                // project to open and every folder resolves to nothing.
                let project_id = app
                    .project_id
                    .clone()
                    .or_else(|| {
                        app.config
                            .get("project_id")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_default();

                let mut config = app.config.clone();
                if let Some(object) = config.as_object_mut() {
                    object.insert("project_id".into(), serde_json::Value::String(project_id));
                }

                RestoreAction {
                    id: format!("vscode-open-{}", app.id),
                    action_type: RestoreActionType::OpenApplication,
                    adapter_id: self.id().to_string(),
                    description: format!("Open {hint} in VS Code"),
                    required: false,
                    approved: false,
                    config,
                    dependencies: vec![],
                }
            })
            .collect())
    }

    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        match action.action.action_type {
            RestoreActionType::OpenApplication => {
                let started = Instant::now();

                // The planner supplies the destination; the captured value is a
                // path on the *other* machine and would be wrong here.
                let Some(target) = action
                    .resolved_config
                    .get("target")
                    .and_then(|v| v.as_str())
                else {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Manual,
                        message: "No destination folder was chosen for this project, so VS Code was not opened"
                            .to_string(),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let Some(vscode_path) = Self::find_vscode() else {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Failed,
                        message: "VS Code is not installed on this device".to_string(),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let target_path = PathBuf::from(target);
                if !target_path.exists() {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Failed,
                        message: format!("{} does not exist, so it was not opened", target),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                }

                match Command::new(&vscode_path)
                    .arg(&target_path)
                    .stdin(Stdio::null())
                    // Detached so the editor outlives this process and we do
                    // not hold a pipe open waiting for it to exit.
                    .spawn()
                {
                    Ok(_) => Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Success,
                        message: format!("Opened {} in VS Code", target_path.display()),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: Some(serde_json::json!({
                            "target": target,
                            "executable": vscode_path.to_string_lossy(),
                        })
                        .to_string()),
                    }),
                    Err(e) => Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Failed,
                        message: format!("Could not launch VS Code: {e}"),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    }),
                }
            }
            _ => Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Failed,
                message: format!(
                    "The VS Code adapter cannot execute a {:?} action",
                    action.action.action_type
                ),
                duration_ms: 0,
                details: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_file_detection_finds_a_code_workspace() {
        let dir = std::env::temp_dir().join("wc-vscode-ws-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("demo.code-workspace");
        std::fs::write(&file, "{}").unwrap();

        assert_eq!(VSCodeAdapter::workspace_file_in(&dir), Some(file));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn workspace_file_detection_ignores_other_files() {
        let dir = std::env::temp_dir().join("wc-vscode-nows-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), "{}").unwrap();

        assert_eq!(VSCodeAdapter::workspace_file_in(&dir), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn workspace_file_detection_tolerates_a_missing_directory() {
        assert_eq!(
            VSCodeAdapter::workspace_file_in(Path::new("/definitely/not/here")),
            None
        );
    }

    #[tokio::test]
    async fn capture_uses_the_real_selected_paths() {
        // The old implementation built `~/projects/<id>`, so a project with an
        // absolute path elsewhere was silently reported as missing.
        let dir = std::env::temp_dir().join("wc-vscode-capture-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("marker.txt"), "x").unwrap();

        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: dir.to_string_lossy().to_string(),
                destination_location_id: "code".into(),
            }],
            include_applications: vec!["vscode".into()],
            ..Default::default()
        };

        let context = VSCodeAdapter::new();
        let captured = context.capture(&LocalContext::current(), &selection).await.unwrap();
        let apps: Vec<Application> = serde_json::from_value(captured.data).unwrap();

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].project_id.as_deref(), Some("p1"));
        assert_eq!(apps[0].config["kind"], "folder");
    }

    #[tokio::test]
    async fn capture_never_puts_a_live_source_path_in_the_manifest() {
        // The manifest is sealed and sent elsewhere, so the absolute target
        // must not travel with it. It used to, hidden behind a duplicate JSON
        // key that happened to shadow it.
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "openshorts".into(),
                source_path: "/Users/someone/code/openshorts".into(),
                destination_location_id: "code".into(),
            }],
            include_applications: vec!["vscode".into()],
            ..Default::default()
        };

        let captured = VSCodeAdapter::new()
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();

        let serialized = captured.data.to_string();
        assert!(
            !serialized.contains("/Users/someone"),
            "the capture leaked the source path: {serialized}"
        );
        assert!(serialized.contains("~/code/openshorts"));
    }

    #[tokio::test]
    async fn capture_skips_projects_when_the_adapter_is_not_selected() {
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: "/tmp".into(),
                destination_location_id: "code".into(),
            }],
            include_applications: vec![],
            ..Default::default()
        };

        let context = VSCodeAdapter::new();
        let captured = context.capture(&LocalContext::current(), &selection).await.unwrap();
        let apps: Vec<Application> = serde_json::from_value(captured.data).unwrap();

        assert!(apps.is_empty(), "nothing was opted in to");
    }
}
