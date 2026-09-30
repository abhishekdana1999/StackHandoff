//! Browser adapter.
//!
//! StackHandoff never reads browser history, cookies or session storage.
//! URLs enter a manifest only because the user typed them into the capture
//! screen, and this adapter is the only place that list is turned into a
//! manifest entry.

use crate::traits::*;
use async_trait::async_trait;
use std::process::{Command, Stdio};
use std::time::Instant;
use tracing::debug;
use workspace_clone_core::Result;

pub struct BrowserAdapter;

impl BrowserAdapter {
    pub fn new() -> Self {
        Self
    }

    /// The platform's URL-opening command, if one exists.
    fn url_opener() -> Option<&'static str> {
        if cfg!(target_os = "windows") {
            // `start` is a cmd builtin, so it needs the shell to reach it.
            Some("cmd")
        } else if cfg!(target_os = "macos") {
            Some("open")
        } else {
            Some("xdg-open")
        }
    }

    /// A browser we can name as evidence, preferring an explicit install over
    /// the generic opener.
    fn installed_browser() -> Option<String> {
        #[cfg(target_os = "macos")]
        let candidates = [
            "/Applications/Google Chrome.app",
            "/Applications/Firefox.app",
            "/Applications/Safari.app",
            "/Applications/Microsoft Edge.app",
        ];
        #[cfg(target_os = "windows")]
        let candidates = [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            // Edge is present on every supported Windows and is the system
            // default, so listing only Chrome and Firefox reports "no browser
            // installed" on a machine that certainly has one. Chromium-based,
            // so it can stand in for Chrome where only the path scheme matters.
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        ];
        #[cfg(target_os = "linux")]
        let candidates = [
            "/usr/bin/google-chrome",
            "/usr/bin/firefox",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
        ];
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let candidates: [&str; 0] = [];

        candidates
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|p| (*p).to_string())
    }

    /// Reject anything that is not plain http(s).
    ///
    /// This is a security boundary, not a tidy-up: a manifest is attacker-
    /// controlled input on the destination, and handing `file://` or a custom
    /// scheme to the OS opener would let a peer read local files or launch a
    /// registered handler.
    pub fn is_safe_url(url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    }
}

#[async_trait]
impl WorkspaceAdapter for BrowserAdapter {
    fn id(&self) -> &str {
        "browser"
    }

    fn version(&self) -> u32 {
        2
    }

    fn supported_platforms(&self) -> Vec<&'static str> {
        vec!["windows", "macos", "linux", "all"]
    }

    async fn detect(&self, _context: &LocalContext) -> Result<DetectionResult> {
        let browser = Self::installed_browser();
        let opener = Self::url_opener();
        let available = browser.is_some() || opener.is_some();

        let mut metadata = std::collections::HashMap::new();
        metadata.insert("browser".to_string(), serde_json::json!(browser));
        metadata.insert("opener".to_string(), serde_json::json!(opener));
        // Stated plainly so the UI need not imply we scanned anything.
        metadata.insert(
            "history_access".to_string(),
            serde_json::json!("none: URLs are supplied by the user"),
        );

        Ok(DetectionResult {
            adapter_id: self.id().to_string(),
            available,
            version: None,
            path: browser.or_else(|| opener.map(str::to_string)),
            metadata,
        })
    }

    async fn capture(
        &self,
        _context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext> {
        // Only the user's own list, filtered to schemes we are willing to
        // reopen. Anything rejected is reported rather than silently dropped.
        let (accepted, rejected): (Vec<_>, Vec<_>) = selection
            .browser_urls
            .iter()
            .filter(|u| !u.trim().is_empty())
            .partition(|u| Self::is_safe_url(u));

        if !rejected.is_empty() {
            debug!("Skipped {} unsafe URL(s) during capture", rejected.len());
        }

        Ok(PortableContext {
            adapter_id: self.id().to_string(),
            data: serde_json::json!({
                "urls": accepted,
                "rejected": rejected,
            }),
        })
    }

    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>> {
        let browser = Self::installed_browser();
        let opener = Self::url_opener();

        let mut results = Vec::new();
        for req in requirements {
            if req.adapter_id != self.id() {
                continue;
            }

            let available = browser.is_some() || opener.is_some();
            let (status, evidence) = if let Some(path) = &browser {
                (CheckStatus::ReadyVerified, format!("Browser found at {path}"))
            } else if let Some(opener) = opener {
                (
                    CheckStatus::ConfiguredUnverified,
                    format!(
                        "No specific browser was located, but the system opener '{opener}' is available"
                    ),
                )
            } else {
                (
                    CheckStatus::Unknown,
                    "No browser and no system URL opener were found".to_string(),
                )
            };

            results.push(CheckResult {
                requirement_id: req.id.clone(),
                status,
                evidence,
                freshness: chrono::Utc::now(),
                action: available.then(|| RemediationAction {
                    label: "Install a web browser".to_string(),
                    action_type: ActionType::InstallApp,
                    url: Some("https://www.google.com/chrome/".to_string()),
                    command: None,
                }),
            });
        }

        Ok(results)
    }

    async fn plan_restore(&self, context: &PortableContext) -> Result<Vec<RestoreAction>> {
        let urls: Vec<String> = context
            .data
            .get("urls")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .filter(|u| Self::is_safe_url(u))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        if urls.is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![RestoreAction {
            id: "browser-open-urls".to_string(),
            action_type: RestoreActionType::OpenUrls,
            adapter_id: self.id().to_string(),
            description: format!("Open {} tab(s) in the browser", urls.len()),
            required: false,
            // Approved by default, but the user can untick it in the preview.
            approved: false,
            config: serde_json::json!({ "urls": urls }),
            dependencies: vec![],
        }])
    }

    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        match action.action.action_type {
            RestoreActionType::OpenUrls => {
                let started = Instant::now();

                let urls: Vec<String> = action
                    .action
                    .config
                    .get("urls")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();

                if urls.is_empty() {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Skipped,
                        message: "No URLs were approved".to_string(),
                        duration_ms: 0,
                        details: None,
                    });
                }

                let Some(opener) = Self::url_opener() else {
                    return Ok(ActionResult {
                        action_id: action.action.id.clone(),
                        status: ActionStatus::Failed,
                        message: "This device has no command that can open a URL".to_string(),
                        duration_ms: started.elapsed().as_millis() as u64,
                        details: None,
                    });
                };

                let mut opened: Vec<String> = Vec::new();
                let mut refused: Vec<String> = Vec::new();

                for url in &urls {
                    // Re-validate at the point of use. The manifest may have
                    // been received from a peer, so the plan is not trusted.
                    if !Self::is_safe_url(url) {
                        refused.push(url.clone());
                        continue;
                    }

                    let spawned = if cfg!(target_os = "windows") {
                        Command::new(opener)
                            .args(["/C", "start", "", url])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                    } else {
                        Command::new(opener)
                            .arg(url)
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                    };

                    match spawned {
                        Ok(_) => opened.push(url.clone()),
                        Err(e) => {
                            debug!("Failed to open {url}: {e}");
                            refused.push(url.clone());
                        }
                    }
                }

                let status = if !refused.is_empty() && opened.is_empty() {
                    ActionStatus::Failed
                } else if refused.is_empty() {
                    ActionStatus::Success
                } else {
                    ActionStatus::Manual
                };

                let message = match (opened.len(), refused.len()) {
                    (n, 0) => format!("Opened {n} tab(s)"),
                    (0, n) => format!("Could not open {n} tab(s)"),
                    (o, r) => format!("Opened {o} tab(s); {r} could not be opened"),
                };

                Ok(ActionResult {
                    action_id: action.action.id.clone(),
                    status,
                    message,
                    duration_ms: started.elapsed().as_millis() as u64,
                    details: Some(
                        serde_json::json!({ "opened": opened, "refused": refused }).to_string(),
                    ),
                })
            }
            _ => Ok(ActionResult {
                action_id: action.action.id.clone(),
                status: ActionStatus::Failed,
                message: format!(
                    "The browser adapter cannot execute a {:?} action",
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
    fn accepts_ordinary_web_urls() {
        for url in [
            "http://localhost:3000",
            "https://github.com/acme/api",
            "https://example.com/path?query=1#frag",
        ] {
            assert!(BrowserAdapter::is_safe_url(url), "{url} should be accepted");
        }
    }

    #[test]
    fn rejects_non_http_schemes() {
        // These are the dangerous ones: local file reads and handler launches.
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vnd.ms-word:ofe|u|file:///etc/passwd",
            "smb://evil/share",
        ] {
            assert!(!BrowserAdapter::is_safe_url(url), "{url} must be rejected");
        }
    }

    #[test]
    fn rejects_malformed_and_empty_input() {
        for url in ["", "not a url", "http://", "://missing-scheme", "/just/a/path"] {
            assert!(!BrowserAdapter::is_safe_url(url), "{url} must be rejected");
        }
    }

    #[tokio::test]
    async fn capture_uses_only_the_urls_the_user_supplied() {
        // The old implementation returned two hardcoded placeholders
        // regardless of input.
        let selection = CaptureSelection {
            browser_urls: vec!["https://example.com/dashboard".to_string()],
            ..Default::default()
        };

        let adapter = BrowserAdapter::new();
        let captured = adapter
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();

        assert_eq!(
            captured.data["urls"],
            serde_json::json!(["https://example.com/dashboard"])
        );
    }

    #[tokio::test]
    async fn capture_reports_urls_it_refused() {
        let selection = CaptureSelection {
            browser_urls: vec![
                "https://good.example".to_string(),
                "file:///etc/passwd".to_string(),
            ],
            ..Default::default()
        };

        let adapter = BrowserAdapter::new();
        let captured = adapter
            .capture(&LocalContext::current(), &selection)
            .await
            .unwrap();

        assert_eq!(captured.data["urls"], serde_json::json!(["https://good.example"]));
        assert_eq!(
            captured.data["rejected"],
            serde_json::json!(["file:///etc/passwd"]),
            "a refused URL must be visible, not silently dropped"
        );
    }

    #[tokio::test]
    async fn capture_with_no_urls_produces_nothing() {
        let adapter = BrowserAdapter::new();
        let captured = adapter
            .capture(&LocalContext::current(), &CaptureSelection::default())
            .await
            .unwrap();

        assert!(captured.data["urls"].as_array().unwrap().is_empty());

        let planned = adapter.plan_restore(&captured).await.unwrap();
        assert!(planned.is_empty(), "no URLs means no restore step");
    }

    #[tokio::test]
    async fn plan_restore_filters_a_tampered_url_out_of_the_manifest() {
        // Simulates a peer that edited the manifest after capture.
        let hostile = PortableContext {
            adapter_id: "browser".into(),
            data: serde_json::json!({
                "urls": ["https://ok.example", "file:///etc/passwd"]
            }),
        };

        let actions = BrowserAdapter::new().plan_restore(&hostile).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].config["urls"],
            serde_json::json!(["https://ok.example"]),
            "an unsafe URL must not survive into a plan that gets executed"
        );
    }
}
