//! Terminal adapter.
//!
//! Per the blueprint this adapter captures *intent*, not history. It records
//! working directories and commands the user explicitly approved, and on the
//! destination it opens a visible terminal rather than running anything behind
//! the user's back.

use crate::traits::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tracing::debug;
use workspace_clone_core::{manifest::*, AdapterError, Result};

pub struct TerminalAdapter;

impl TerminalAdapter {
    pub fn new() -> Self {
        Self
    }

    /// How to open a visible terminal window on this platform.
    ///
    /// Returns the program plus the argument prefix that precedes a working
    /// directory. Everything here launches a *new visible window*: a restore
    /// that silently ran commands in the background would be a surprise, and
    /// the user must be able to see and interrupt what runs.
    fn terminal_launcher() -> (String, Vec<String>) {
        #[cfg(target_os = "macos")]
        {
            // iTerm first if present, since `open -a` is the only reliable way
            // to pass arguments through on macOS.
            for app in ["iTerm", "Terminal"] {
                if Path::new(&format!("/Applications/{app}.app")).exists() {
                    return ("open".to_string(), vec!["-a".to_string(), app.to_string()]);
                }
            }
            ("open".to_string(), vec!["-a".to_string(), "Terminal".to_string()])
        }
        #[cfg(target_os = "windows")]
        {
            if which::which("wt").is_ok() {
                return ("wt".to_string(), vec!["-w".to_string(), "0".to_string(), "new-tab".to_string(), "--startingDirectory".to_string()]);
            }
            ("cmd".to_string(), vec!["/C".to_string(), "start".to_string(), "\"cmd\"".to_string()])
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            for (prog, args) in [
                ("gnome-terminal", vec!["--".to_string()]),
                ("konsole", vec!["-e".to_string()]),
                ("xfce4-terminal", vec!["--command".to_string()]),
                ("x-terminal-emulator", vec!["-e".to_string()]),
            ] {
                if which::which(prog).is_ok() {
                    return (prog.to_string(), args);
                }
            }
            ("x-terminal-emulator".to_string(), vec!["-e".to_string()])
        }
    }
}

#[async_trait]
impl WorkspaceAdapter for TerminalAdapter {
    fn id(&self) -> &str {
        "terminal"
    }

    fn version(&self) -> u32 {
        2
    }

    fn supported_platforms(&self) -> Vec<&'static str> {
        vec!["windows", "macos", "linux", "all"]
    }

    async fn detect(&self, _context: &LocalContext) -> Result<DetectionResult> {
        let (cmd, args) = Self::terminal_launcher();
        let available = which::which(&cmd).is_ok() || Path::new("/bin/sh").exists();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("terminal_command".to_string(), serde_json::json!(cmd));
        metadata.insert("terminal_args".to_string(), serde_json::to_value(&args)?);
        metadata.insert(
            "history_access".to_string(),
            serde_json::json!("none: commands are approved explicitly by the user"),
        );

        Ok(DetectionResult {
            adapter_id: self.id().to_string(),
            available,
            version: None,
            path: available.then_some(cmd),
            metadata,
        })
    }

    async fn capture(
        &self,
        _context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext> {
        // Working directories come from the selected projects, redacted, plus
        // any extra directories the user named.
        //
        // The absolute source path is deliberately kept out of every entry: this
        // data becomes a manifest that is sealed and sent to another device, and
        // a live path would tell the receiving side where the project lives on
        // the sending machine. The redacted hint plus the project id is enough
        // for the destination to map the directory onto its own layout.
        let mut entries: Vec<serde_json::Value> = Vec::new();
        let mut seen_dirs: Vec<&str> = Vec::new();

        for project in &selection.projects {
            seen_dirs.push(project.source_path.as_str());
            entries.push(serde_json::json!({
                "label": project.name,
                "project_id": project.id,
                "cwd_hint": crate::git::redact_path(Path::new(&project.source_path)),
            }));
        }

        for dir in &selection.terminal_dirs {
            // Skip anything already covered by a selected project.
            if seen_dirs.contains(&dir.as_str()) {
                continue;
            }
            seen_dirs.push(dir.as_str());
            let name = Path::new(dir)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.clone());
            entries.push(serde_json::json!({
                "label": name,
                "project_id": serde_json::Value::Null,
                "cwd_hint": crate::git::redact_path(Path::new(dir)),
            }));
        }

        // Commands are only ever those the user approved. There is no code path
        // that reads shell history.
        let commands: Vec<serde_json::Value> = selection
            .terminal_commands
            .iter()
            .map(|c| {
                serde_json::json!({
                    "label": c.label,
                    "command": c.command,
                    "working_directory_hint": c.working_directory.as_ref()
                        .map(|d| crate::git::redact_path(Path::new(d))),
                })
            })
            .collect();

        let applications = vec![Application {
            id: "terminal-session".to_string(),
            adapter: self.id().to_string(),
            project_id: None,
            required: false,
            config: serde_json::json!({
                "directories": entries,
                "commands": commands,
            }),
        }];

        Ok(PortableContext {
            adapter_id: self.id().to_string(),
            data: serde_json::to_value(applications)?,
        })
    }

    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>> {
        let (cmd, _) = Self::terminal_launcher();
        let available = which::which(&cmd).is_ok();

        let mut results = Vec::new();
        for req in requirements {
            if req.adapter_id != self.id() {
                continue;
            }

            results.push(CheckResult {
                requirement_id: req.id.clone(),
                status: if available {
                    CheckStatus::ReadyVerified
                } else {
                    CheckStatus::Unknown
                },
                evidence: if available {
                    format!("A terminal emulator is available via '{cmd}'")
                } else {
                    format!("No terminal emulator was found; '{cmd}' is unavailable")
                },
                freshness: chrono::Utc::now(),
                action: (!available).then(|| RemediationAction {
                    label: "Install a terminal emulator".to_string(),
                    action_type: ActionType::InstallApp,
                    url: None,
                    command: None,
                }),
            });
        }

        Ok(results)
    }

    async fn plan_restore(&self, context: &PortableContext) -> Result<Vec<RestoreAction>> {
        let apps: Vec<Application> = serde_json::from_value(context.data.clone())
            .map_err(|e| AdapterError::Capture(e.to_string()))?;

        let mut actions = Vec::new();

        for app in apps {
            let directories = app
                .config
                .get("directories")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            // Every directory becomes its own step. The index is in the id
            // because two directories under one session previously produced two
            // actions with the same id, and the planner's map silently kept only
            // the last one -- the user would have lost a step without being told.
            for (index, dir) in directories.iter().enumerate() {
                let Some(hint) = dir["cwd_hint"].as_str() else {
                    continue;
                };
                let label = dir["label"].as_str().unwrap_or(hint);

                actions.push(RestoreAction {
                    id: format!("terminal-open-{}-{index}", app.id),
                    action_type: RestoreActionType::OfferCommand,
                    adapter_id: self.id().to_string(),
                    description: format!("Open a terminal at {hint}"),
                    required: false,
                    // Never pre-approved: launching a window is a visible
                    // action, so the user opts in from the restore preview.
                    approved: false,
                    config: serde_json::json!({
                        "label": label,
                        "cwd_hint": hint,
                        // Carried so the destination can map the hint onto its
                        // own layout without guessing from the string.
                        "project_id": dir["project_id"],
                        "command": serde_json::Value::Null,
                    }),
                    dependencies: vec![],
                });
            }

            // Each approved command becomes its own step so the user can accept
            // the directories and decline the commands independently.
            for (index, command) in app
                .config
                .get("commands")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                let Some(text) = command["command"].as_str() else {
                    continue;
                };
                actions.push(RestoreAction {
                    id: format!("terminal-cmd-{}-{index}", app.id),
                    action_type: RestoreActionType::OfferCommand,
                    adapter_id: self.id().to_string(),
                    description: format!("Run in a new terminal: {text}"),
                    required: false,
                    approved: false,
                    config: serde_json::json!({
                        "label": command["label"],
                        "command": text,
                        "cwd_hint": command["working_directory_hint"],
                    }),
                    dependencies: vec![],
                });
            }
        }

        Ok(actions)
    }

    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        match action.action.action_type {
            RestoreActionType::OfferCommand => {
                let started = Instant::now();

                // Where to run. The planner resolves this to a real local path;
                // the captured value points at the other machine.
                let cwd = action
                    .resolved_config
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(PathBuf::from);

                if let Some(cwd) = &cwd {
                    if !cwd.exists() {
                        return Ok(ActionResult {
                            action_id: action.action.id.clone(),
                            status: ActionStatus::Manual,
                            message: format!(
                                "{} does not exist on this device, so no terminal was opened",
                                cwd.display()
                            ),
                            duration_ms: started.elapsed().as_millis() as u64,
                            details: None,
                        });
                    }
                }

                let command = action
                    .resolved_config
                    .get("command")
                    .and_then(|v| v.as_str())
                    .filter(|c| !c.is_empty());

                let (launcher, mut args) = Self::terminal_launcher();

                match cwd {
                    Some(ref dir) => {
                        // macOS `open` cannot pass a working directory, so run
                        // the shell explicitly inside a new window instead.
                        #[cfg(target_os = "macos")]
                        {
                            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
                            args.push("-c".to_string());
                            args.push(match command {
                                Some(c) => format!("cd {} && {c}; exec $SHELL", shell_quote(dir)),
                                None => format!("cd {} && exec $SHELL", shell_quote(dir)),
                            });
                            let _ = shell;
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            if command.is_some() {
                                args.push("-c".to_string());
                            }
                            if let Some(c) = command {
                                args.push(c.to_string());
                            }
                        }
                    }
                    None => {
                        if let Some(c) = command {
                            args.push("-c".to_string());
                            args.push(c.to_string());
                        }
                    }
                }

                debug!("Launching terminal: {launcher} {args:?}");

                let spawned = Command::new(&launcher)
                    .args(&args)
                    .current_dir(cwd.as_deref().unwrap_or_else(|| Path::new(".")))
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();

                match spawned {
                    Ok(_) => Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Success,
                        message: match command {
                            Some(c) => format!("Opened a terminal and offered: {c}"),
                            None => "Opened a terminal at the chosen directory".to_string(),
                        },
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: Some(
                            serde_json::json!({
                                "launcher": launcher,
                                "cwd": cwd.map(|p| p.to_string_lossy().to_string()),
                                "command": command,
                            })
                            .to_string(),
                        ),
                    }),
                    Err(e) => Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Failed,
                        message: format!("Could not open a terminal: {e}"),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    }),
                }
            }
            _ => Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Failed,
                message: format!(
                    "The terminal adapter cannot execute a {:?} action",
                    action.action.action_type
                ),
                duration_ms: 0,
                details: None,
            }),
        }
    }
}

/// Single-quote a path for safe inclusion in a POSIX shell command.
#[cfg(target_os = "macos")]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn capture_records_the_real_selected_paths() {
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "openshorts".into(),
                source_path: "/Users/someone/code/openshorts".into(),
                destination_location_id: "code".into(),
            }],
            ..Default::default()
        };

        let captured = TerminalAdapter::new()
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();
        let apps: Vec<Application> = serde_json::from_value(captured.data).unwrap();

        let dirs = apps[0].config["directories"].as_array().unwrap();
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0]["cwd_hint"], "~/code/openshorts");
        assert_eq!(dirs[0]["project_id"], "p1");
    }

    #[tokio::test]
    async fn capture_never_puts_a_live_source_path_in_the_manifest() {
        // This data is sealed and sent to another device, so the source
        // machine's real directory layout must not travel with it.
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "openshorts".into(),
                source_path: "/Users/someone/code/openshorts".into(),
                destination_location_id: "code".into(),
            }],
            terminal_dirs: vec!["/Users/someone/notes".into()],
            terminal_commands: vec![ApprovedCommand {
                label: "dev".into(),
                command: "npm run dev".into(),
                working_directory: Some("/Users/someone/code/openshorts".into()),
            }],
            ..Default::default()
        };

        let captured = TerminalAdapter::new()
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();

        let serialized = captured.data.to_string();
        // The absolute prefix must not appear anywhere.
        assert!(
            !serialized.contains("/Users/someone"),
            "the capture leaked the source path: {serialized}"
        );
        // The hint is what the destination gets instead.
        assert!(serialized.contains("~/code/openshorts"));
    }

    #[tokio::test]
    async fn capture_does_not_duplicate_a_project_listed_as_a_directory() {
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: "/tmp/demo".into(),
                destination_location_id: "code".into(),
            }],
            terminal_dirs: vec!["/tmp/demo".to_string(), "/tmp/other".to_string()],
            ..Default::default()
        };

        let captured = TerminalAdapter::new()
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();
        let apps: Vec<Application> = serde_json::from_value(captured.data).unwrap();
        let dirs = apps[0].config["directories"].as_array().unwrap();

        assert_eq!(dirs.len(), 2, "the duplicate must collapse");
    }

    #[tokio::test]
    async fn capture_only_records_user_approved_commands() {
        let selection = CaptureSelection {
            terminal_commands: vec![ApprovedCommand {
                label: "start dev server".into(),
                command: "npm run dev".into(),
                working_directory: Some("/tmp/demo".into()),
            }],
            ..Default::default()
        };

        let captured = TerminalAdapter::new()
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();
        let apps: Vec<Application> = serde_json::from_value(captured.data).unwrap();
        let commands = apps[0].config["commands"].as_array().unwrap();

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0]["command"], "npm run dev");
        assert_eq!(commands[0]["working_directory_hint"], "~/tmp/demo");
    }

    #[tokio::test]
    async fn planned_steps_are_never_pre_approved() {
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: "/tmp/demo".into(),
                destination_location_id: "code".into(),
            }],
            terminal_commands: vec![ApprovedCommand {
                label: "install".into(),
                command: "npm ci".into(),
                working_directory: None,
            }],
            ..Default::default()
        };

        let adapter = TerminalAdapter::new();
        let captured = adapter
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();
        let actions = adapter.plan_restore(&captured).await.unwrap();

        assert_eq!(actions.len(), 2);
        assert!(
            actions.iter().all(|a| !a.approved),
            "a terminal must never launch without the user ticking it"
        );
        assert!(
            actions.iter().all(|a| a.adapter_id == "terminal"),
            "every step must be routable"
        );
    }

    #[tokio::test]
    async fn execute_reports_a_missing_destination_instead_of_launching() {
        let action = ApprovedRestoreAction {
            action: RestoreAction::new(
                "t1",
                RestoreActionType::OfferCommand,
                "terminal",
                "open terminal",
                serde_json::json!({}),
            ),
            resolved_config: serde_json::json!({
                "cwd": "/definitely/not/here",
            }),
        };

        let result = TerminalAdapter::new().execute(&action).await.unwrap();

        assert_eq!(result.status, ActionStatus::Manual);
        assert!(result.message.contains("does not exist"), "{}", result.message);
    }
}
