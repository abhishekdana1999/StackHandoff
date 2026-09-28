//! Runtime and CLI toolchain adapter.
//!
//! Requirements are derived from what each project *declares* (`.nvmrc`,
//! `package.json` engines, `rust-toolchain.toml`, `go.mod`, …) rather than from
//! whatever happens to be installed globally, because a global Node version
//! says nothing about the version a repository needs.

use crate::traits::*;
use async_trait::async_trait;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::debug;
use workspace_clone_core::{
    manifest::{CliToolRequirement, RuntimeRequirement},
    Result,
};

pub struct RuntimeAdapter;

impl RuntimeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Run `<tool> <arg>` and return its first line, or `None`.
    ///
    /// The timeout matters: a missing or wedged tool would otherwise hang the
    /// whole preflight, which runs while the user waits.
    fn probe(tool: &str, arg: &str) -> Option<String> {
        let child = Command::new(tool)
            .arg(arg)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let output = wait_with_timeout(child, Duration::from_secs(5))?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Pull a version number out of whatever shape the tool prints.
    ///
    /// Tools disagree: `node --version` gives `v20.11.1`, `gh --version` gives
    /// `gh version 2.40.0`, `aws --version` gives `aws-cli/2.15.30
    /// Python/3.11.4`, and `go version` gives `go version go1.22.0 darwin/arm64`.
    /// Rather than pattern-match each tool, this looks for the first dotted
    /// numeric run anywhere in the output.
    pub fn extract_version(output: &str) -> Option<String> {
        first_dotted_number(output)
    }

    /// Compare a detected version against a requirement such as `>=18.0.0`.
    ///
    /// Returns `None` when the requirement cannot be understood, so the caller
    /// reports "present but unverified" rather than a false pass.
    pub fn version_satisfies(found: &str, requirement: &str) -> Option<bool> {
        let requirement = requirement.trim();
        if requirement.is_empty() {
            return Some(true);
        }

        let (op, wanted) = if let Some(rest) = requirement.strip_prefix(">=") {
            (">=", rest)
        } else if let Some(rest) = requirement.strip_prefix("<=") {
            ("<=", rest)
        } else if let Some(rest) = requirement.strip_prefix('>') {
            (">", rest)
        } else if let Some(rest) = requirement.strip_prefix('<') {
            ("<", rest)
        } else if let Some(rest) = requirement.strip_prefix("==") {
            ("==", rest)
        } else {
            // A bare version is an exact expectation, which is the safest
            // reading of a pinned toolchain file.
            ("==", requirement)
        };

        let wanted = wanted.trim().trim_start_matches('v');
        let found = found.trim().trim_start_matches('v');

        let found_parts = parse_version(found)?;
        let wanted_parts = parse_version(wanted)?;

        let ordering = compare(&found_parts, &wanted_parts);
        Some(match op {
            ">=" => ordering != std::cmp::Ordering::Less,
            "<=" => ordering != std::cmp::Ordering::Greater,
            ">" => ordering == std::cmp::Ordering::Greater,
            "<" => ordering == std::cmp::Ordering::Less,
            _ => ordering == std::cmp::Ordering::Equal,
        })
    }

    /// Read a version requirement from a project's own toolchain declarations.
    ///
    /// Returns a requirement only where the project states one. Guessing from
    /// the installed global version would produce requirements that are always
    /// satisfied and therefore never useful.
    pub fn declared_requirements(project_root: &Path) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = Vec::new();

        // .nvmrc, .node-version
        for file in [".nvmrc", ".node-version"] {
            if let Ok(text) = std::fs::read_to_string(project_root.join(file)) {
                let version = text.trim().trim_start_matches('v').to_string();
                // A bare `lts/*` is not a comparable version.
                if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    push_unique(&mut found, "node", format!(">={version}"));
                }
                break;
            }
        }

        // package.json "engines"
        if let Ok(text) = std::fs::read_to_string(project_root.join("package.json")) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(node) = json.pointer("/engines/node").and_then(|v| v.as_str()) {
                    push_unique(&mut found, "node", node.to_string());
                }
                if let Some(pm) = json.pointer("/packageManager").and_then(|v| v.as_str()) {
                    // "pnpm@9.1.0" -> "pnpm", ">=9.1.0"
                    if let Some((name, version)) = pm.rsplit_once('@') {
                        push_unique(&mut found, name, format!(">={version}"));
                    }
                }
            }
        }

        // rust-toolchain.toml / rust-toolchain
        for file in ["rust-toolchain.toml", "rust-toolchain"] {
            if let Ok(text) = std::fs::read_to_string(project_root.join(file)) {
                let toml: toml_lite::Table = text.parse().unwrap_or_default();
                if let Some(channel) = toml.get_str("toolchain.channel") {
                    push_unique(&mut found, "rust", format!(">={channel}"));
                }
                break;
            }
        }

        // Cargo.toml rust-version
        if !has_tool(&found, "rust") {
            if let Ok(text) = std::fs::read_to_string(project_root.join("Cargo.toml")) {
                let toml: toml_lite::Table = text.parse().unwrap_or_default();
                if let Some(v) = toml.get_str("package.rust-version") {
                    push_unique(&mut found, "rust", format!(">={v}"));
                }
            }
        }

        // .python-version
        if let Ok(text) = std::fs::read_to_string(project_root.join(".python-version")) {
            let version = text.trim().to_string();
            if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                push_unique(&mut found, "python", format!(">={version}"));
            }
        }

        // pyproject.toml requires-python
        if !has_tool(&found, "python") {
            if let Ok(text) = std::fs::read_to_string(project_root.join("pyproject.toml")) {
                let toml: toml_lite::Table = text.parse().unwrap_or_default();
                if let Some(v) = toml.get_str("project.requires-python") {
                    push_unique(&mut found, "python", v.to_string());
                }
            }
        }

        // go.mod
        if let Ok(text) = std::fs::read_to_string(project_root.join("go.mod")) {
            if let Some(line) = text.lines().find(|l| l.trim_start().starts_with("go ")) {
                let version = line.trim().trim_start_matches("go ").trim();
                if version.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    push_unique(&mut found, "go", format!(">={version}"));
                }
            }
        }

        // global.json (dotnet)
        if let Ok(text) = std::fs::read_to_string(project_root.join("global.json")) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(v) = json.pointer("/sdk/version").and_then(|v| v.as_str()) {
                    push_unique(&mut found, "dotnet", format!(">={v}"));
                }
            }
        }

        found
    }

    /// Decide how a detected version relates to a requirement.
    ///
    /// Split out from `preflight` so the decision can be exercised directly
    /// rather than only through whatever happens to be installed on the
    /// machine running the tests.
    pub fn classify_version_check(
        detected: Option<&str>,
        tool: &str,
        wanted: &str,
    ) -> (CheckStatus, String) {
        match detected {
            Some(found) if wanted.is_empty() => (
                CheckStatus::ReadyVerified,
                format!("{tool} {found} is installed"),
            ),
            Some(found) => match Self::version_satisfies(found, wanted) {
                Some(true) => (
                    CheckStatus::ReadyVerified,
                    format!("{tool} {found} satisfies '{wanted}'"),
                ),
                Some(false) => (
                    CheckStatus::ReadyAccountMismatch,
                    format!("{tool} {found} is installed but does not satisfy '{wanted}'"),
                ),
                // A requirement we cannot parse is reported honestly rather
                // than being waved through.
                None => (
                    CheckStatus::ConfiguredUnverified,
                    format!("{tool} {found} is installed; '{wanted}' could not be compared"),
                ),
            },
            None => (
                CheckStatus::Unknown,
                format!("{tool} was not found on PATH"),
            ),
        }
    }

    /// The flag that makes a tool print its version.
    fn version_flag(tool: &str) -> &'static str {
        match tool {
            // `go version` takes no flag; the subcommand is required.
            "go" => "version",
            "java" => "-version",
            "dotnet" => "--version",
            _ => "--version",
        }
    }

    fn install_url(tool: &str) -> Option<String> {
        Some(
            match tool {
                "node" => "https://nodejs.org/",
                "pnpm" => "https://pnpm.io/installation",
                "npm" => "https://docs.npmjs.com/downloading-and-installing-node-js-and-npm",
                "yarn" => "https://classic.yarnpkg.com/en/docs/install",
                "rust" => "https://rustup.rs/",
                "go" => "https://go.dev/dl/",
                "python" => "https://www.python.org/downloads/",
                "java" => "https://adoptium.net/",
                "dotnet" => "https://dotnet.microsoft.com/download",
                "docker" => "https://www.docker.com/products/docker-desktop/",
                "gh" => "https://cli.github.com/",
                "aws" => "https://aws.amazon.com/cli/",
                "supabase" => "https://supabase.com/docs/guides/cli",
                _ => return None,
            }
            .to_string(),
        )
    }

    /// A copy-pasteable install hint. Never executed automatically.
    fn install_command(tool: &str) -> Option<String> {
        Some(
            match tool {
                "node" => "brew install node",
                "pnpm" => "corepack enable pnpm",
                "yarn" => "npm install -g yarn",
                "rust" => "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
                "go" => "brew install go",
                "python" => "brew install python",
                "dotnet" => "brew install --cask dotnet-sdk",
                "pnpm@x" => "corepack prepare pnpm@latest --activate",
                _ => return None,
            }
            .to_string(),
        )
    }
}

/// Reap a child process, killing it if it overruns `timeout`.
///
/// Without this a tool that prompts for input would hold preflight open
/// indefinitely, because `output()` waits for the process to exit.
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Option<std::process::Output> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    debug!("Tool version probe timed out and was killed");
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
    child.wait_with_output().ok()
}

/// The first `1.2.3`-shaped run of digits in `text`.
///
/// A single number is not accepted: a date like `2024` or a port is not a
/// version, and treating one as a version would produce a confidently wrong
/// preflight result.
fn first_dotted_number(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }

        let start = i;
        let mut end = i;
        let mut prev_was_dot = false;

        while i < chars.len() {
            let c = chars[i];
            if c.is_ascii_digit() {
                end = i + 1;
                prev_was_dot = false;
            } else if c == '.' && !prev_was_dot {
                end = i + 1;
                prev_was_dot = true;
            } else {
                break;
            }
            i += 1;
        }

        let candidate: String = chars[start..end].iter().collect();
        let candidate = candidate.trim_end_matches('.');

        if candidate.contains('.')
            && candidate
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(candidate.to_string());
        }

        // Resume just past the run we rejected, in case a valid number starts
        // inside what we just skipped.
        i = i.max(start + 1);
    }

    None
}

/// Record a requirement, ignoring duplicates and empty versions.
///
/// A dedicated function rather than a closure: a closure capturing `found` would
/// hold a mutable borrow for the whole body and block the later read-only
/// lookups that decide whether a fallback file should even be read.
fn push_unique(found: &mut Vec<(String, String)>, tool: &str, version: String) {
    if version.is_empty() || found.iter().any(|(t, _)| t == tool) {
        return;
    }
    found.push((tool.to_string(), version));
}

fn has_tool(found: &[(String, String)], tool: &str) -> bool {
    found.iter().any(|(t, _)| t == tool)
}

/// Split a version into numeric components.
///
/// Returns `None` if any component is not numeric. Treating a non-numeric
/// component as `0` would turn a requirement like `lts/*` into `0.0.0` and
/// report a confident match, so an unparseable requirement has to fail loudly
/// and surface as "could not be compared".
fn parse_version(text: &str) -> Option<Vec<u32>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for component in text.split('.') {
        if component.is_empty() || !component.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        parts.push(component.parse().ok()?);
    }
    Some(parts)
}

fn compare(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    for i in 0..a.len().max(b.len()) {
        let left = a.get(i).copied().unwrap_or(0);
        let right = b.get(i).copied().unwrap_or(0);
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// A deliberately tiny TOML reader.
///
/// Only `key.path = "value"` and `[table]` lines are needed for the two
/// toolchain files this adapter consults. Pulling in a full TOML parser for
/// that would be more dependency than the job warrants.
mod toml_lite {
    #[derive(Default)]
    pub struct Table {
        entries: Vec<(String, String)>,
    }

    impl Table {
        /// Look up a dotted path such as `package.rust-version`.
        pub fn get_str(&self, path: &str) -> Option<String> {
            self.entries
                .iter()
                .find(|(k, _)| k == path)
                .map(|(_, v)| v.clone())
        }
    }

    impl std::str::FromStr for Table {
        type Err = std::convert::Infallible;

        fn from_str(text: &str) -> Result<Self, Self::Err> {
            let mut table = Table::default();
            let mut current_section = String::new();

            for raw in text.lines() {
                let line = strip_comment(raw).trim();
                if line.is_empty() {
                    continue;
                }

                if line.starts_with('[') {
                    current_section = line
                        .trim_start_matches('[')
                        .trim_end_matches(']')
                        .trim()
                        .to_string();
                    continue;
                }

                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let key = key.trim().trim_matches('"');
                let value = value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_string();

                let full_key = if current_section.is_empty() {
                    key.to_string()
                } else {
                    format!("{current_section}.{key}")
                };
                table.entries.push((full_key, value));
            }

            Ok(table)
        }
    }

    fn strip_comment(line: &str) -> &str {
        match line.find('#') {
            Some(i) => &line[..i],
            None => line,
        }
    }
}

#[async_trait]
impl WorkspaceAdapter for RuntimeAdapter {
    fn id(&self) -> &str {
        "runtime"
    }

    fn version(&self) -> u32 {
        2
    }

    fn supported_platforms(&self) -> Vec<&'static str> {
        vec!["windows", "macos", "linux", "all"]
    }

    async fn detect(&self, _context: &LocalContext) -> Result<DetectionResult> {
        let mut metadata = std::collections::HashMap::new();
        let mut available = false;

        for tool in [
            "node", "npm", "pnpm", "yarn", "bun", "deno", "python", "python3", "rustc", "cargo",
            "go", "java", "dotnet", "docker", "gh", "aws", "supabase",
        ] {
            if let Some(output) = Self::probe(tool, Self::version_flag(tool)) {
                available = true;
                metadata.insert(
                    tool.to_string(),
                    serde_json::json!(Self::extract_version(&output).unwrap_or(output)),
                );
            }
        }

        Ok(DetectionResult {
            adapter_id: self.id().to_string(),
            available,
            version: None,
            path: None,
            metadata,
        })
    }

    async fn capture(
        &self,
        _context: &LocalContext,
        selection: &CaptureSelection,
    ) -> Result<PortableContext> {
        let mut runtimes: Vec<RuntimeRequirement> = Vec::new();
        let mut cli_tools: Vec<CliToolRequirement> = Vec::new();

        for project in &selection.projects {
            for (tool, version) in Self::declared_requirements(Path::new(&project.source_path)) {
                // A runtime requirement is something the project executes with;
                // package managers are offered as tools instead.
                let is_runtime = matches!(tool.as_str(), "node" | "python" | "rust" | "go" | "java" | "dotnet");

                if is_runtime {
                    if !runtimes.iter().any(|r| r.name == tool) {
                        runtimes.push(RuntimeRequirement {
                            name: tool.clone(),
                            version,
                            required: true,
                        });
                    }
                } else if !cli_tools.iter().any(|c| c.name == tool) {
                    cli_tools.push(CliToolRequirement {
                        name: tool,
                        version: Some(version),
                        required: true,
                    });
                }
            }
        }

        // Record what is installed alongside what is required, so the
        // destination has a baseline even for a project that declares nothing.
        let installed: serde_json::Map<String, serde_json::Value> = ["node", "python", "rustc", "go", "java", "dotnet"]
            .iter()
            .filter_map(|tool| {
                Self::probe(tool, Self::version_flag(tool))
                    .and_then(|out| Self::extract_version(&out))
                    .map(|v| ((*tool).to_string(), serde_json::json!(v)))
            })
            .collect();

        Ok(PortableContext {
            adapter_id: self.id().to_string(),
            data: serde_json::json!({
                "runtimes": runtimes,
                "cli_tools": cli_tools,
                "installed_at_capture": installed,
            }),
        })
    }

    async fn preflight(&self, requirements: &[Requirement]) -> Result<Vec<CheckResult>> {
        let mut results = Vec::new();

        for req in requirements {
            if req.adapter_id != self.id() {
                continue;
            }

            let Some(tool) = req.config.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let wanted = req
                .config
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // `rustc` is the binary that reports the Rust version, but a user
            // thinks in terms of "rust".
            let probe_name = if tool == "rust" { "rustc" } else { tool };

            let detected = Self::probe(probe_name, Self::version_flag(probe_name))
                .and_then(|out| Self::extract_version(&out));

            let (status, evidence) =
                Self::classify_version_check(detected.as_deref(), probe_name, wanted);

            results.push(CheckResult {
                requirement_id: req.id.clone(),
                status,
                evidence,
                freshness: chrono::Utc::now(),
                action: (status != CheckStatus::ReadyVerified).then(|| RemediationAction {
                    label: format!("Install {tool}"),
                    action_type: ActionType::InstallApp,
                    url: Self::install_url(tool),
                    command: Self::install_command(tool),
                }),
            });
        }

        Ok(results)
    }

    async fn plan_restore(&self, _context: &PortableContext) -> Result<Vec<RestoreAction>> {
        // Toolchains are a preflight concern. Nothing here executes on the
        // destination, because installing software is never automatic.
        Ok(Vec::new())
    }

    async fn execute(&self, action: &ApprovedRestoreAction) -> Result<ActionResult> {
        Ok(ActionResult {
            action_id: action.action.id.clone(),
            status: ActionStatus::Failed,
            message: "The runtime adapter does not execute actions; it only reports versions"
                .to_string(),
            duration_ms: 0,
            details: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_versions_from_every_shape_tools_print() {
        assert_eq!(RuntimeAdapter::extract_version("v20.11.1").as_deref(), Some("20.11.1"));
        assert_eq!(RuntimeAdapter::extract_version("20.11.1").as_deref(), Some("20.11.1"));
        assert_eq!(
            RuntimeAdapter::extract_version("gh version 2.40.0 (2023-12-01)").as_deref(),
            Some("2.40.0")
        );
        assert_eq!(
            RuntimeAdapter::extract_version("aws-cli/2.15.30 Python/3.11.4").as_deref(),
            Some("2.15.30")
        );
        assert_eq!(RuntimeAdapter::extract_version("python3.12.1").as_deref(), Some("3.12.1"));
        assert_eq!(RuntimeAdapter::extract_version("go version go1.22.0 darwin/arm64").as_deref(), Some("1.22.0"));
    }

    #[test]
    fn extraction_returns_none_for_output_with_no_version() {
        assert_eq!(RuntimeAdapter::extract_version("command not found"), None);
        assert_eq!(RuntimeAdapter::extract_version(""), None);
    }

    #[test]
    fn satisfies_handles_comparison_operators() {
        assert_eq!(RuntimeAdapter::version_satisfies("20.11.1", ">=18.0.0"), Some(true));
        assert_eq!(RuntimeAdapter::version_satisfies("16.0.0", ">=18.0.0"), Some(false));
        assert_eq!(RuntimeAdapter::version_satisfies("18.0.0", ">=18.0.0"), Some(true));
        assert_eq!(RuntimeAdapter::version_satisfies("18.0.1", ">=18.0.0"), Some(true));
        assert_eq!(RuntimeAdapter::version_satisfies("17.9.9", ">=18.0.0"), Some(false));
    }

    #[test]
    fn satisfies_handles_an_exact_requirement() {
        // The bug this fixes: ">=18.0.0" used to be compared as a bare version
        // and therefore never matched anything.
        assert_eq!(RuntimeAdapter::version_satisfies("18.0.0", "18.0.0"), Some(true));
        assert_eq!(RuntimeAdapter::version_satisfies("18.0.1", "18.0.0"), Some(false));
    }

    #[test]
    fn satisfies_compares_numerically_not_lexically() {
        // Lexically, "9.0.0" > "10.0.0"; numerically it is not.
        assert_eq!(RuntimeAdapter::version_satisfies("9.0.0", ">=10.0.0"), Some(false));
        assert_eq!(RuntimeAdapter::version_satisfies("10.0.0", ">=9.0.0"), Some(true));
    }

    #[test]
    fn satisfies_treats_a_differing_component_count_correctly() {
        assert_eq!(RuntimeAdapter::version_satisfies("20.11", ">=20.11.0"), Some(true));
        assert_eq!(RuntimeAdapter::version_satisfies("20.10", ">=20.11.0"), Some(false));
    }

    #[test]
    fn satisfies_returns_none_for_an_unparseable_requirement() {
        assert_eq!(RuntimeAdapter::version_satisfies("20.0.0", "lts/*"), None);
        assert_eq!(RuntimeAdapter::version_satisfies("20.0.0", "latest"), None);
    }

    #[test]
    fn satisfies_of_an_empty_requirement_is_always_true() {
        assert_eq!(RuntimeAdapter::version_satisfies("20.0.0", ""), Some(true));
    }

    #[test]
    fn reads_node_requirement_from_nvmrc() {
        let dir = std::env::temp_dir().join("wc-toolchain-nvmrc");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".nvmrc"), "v20.11.1\n").unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        assert_eq!(reqs, vec![("node".to_string(), ">=20.11.1".to_string())]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_node_requirement_from_package_json_engines() {
        let dir = std::env::temp_dir().join("wc-toolchain-pkg");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("package.json"),
            r#"{"engines":{"node":">=18"},"packageManager":"pnpm@9.1.0"}"#,
        )
        .unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        assert!(reqs.contains(&("node".to_string(), ">=18".to_string())));
        assert!(reqs.contains(&("pnpm".to_string(), ">=9.1.0".to_string())));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_rust_requirement_from_cargo_toml() {
        let dir = std::env::temp_dir().join("wc-toolchain-cargo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"x\"\nrust-version = \"1.75\"  # comment\n",
        )
        .unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        assert_eq!(reqs, vec![("rust".to_string(), ">=1.75".to_string())]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_go_requirement_from_go_mod() {
        let dir = std::env::temp_dir().join("wc-toolchain-gomod");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("go.mod"), "module x\n\ngo 1.22\n").unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        assert_eq!(reqs, vec![("go".to_string(), ">=1.22".to_string())]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_dotnet_requirement_from_global_json() {
        let dir = std::env::temp_dir().join("wc-toolchain-dotnet");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("global.json"), r#"{"sdk":{"version":"8.0.100"}}"#).unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        assert_eq!(reqs, vec![("dotnet".to_string(), ">=8.0.100".to_string())]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_project_that_declares_nothing_yields_no_requirements() {
        let dir = std::env::temp_dir().join("wc-toolchain-empty");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("README.md"), "hello").unwrap();

        assert!(RuntimeAdapter::declared_requirements(&dir).is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nvmrc_wins_over_package_json_engines() {
        let dir = std::env::temp_dir().join("wc-toolchain-both");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".nvmrc"), "20.11.1").unwrap();
        std::fs::write(dir.join("package.json"), r#"{"engines":{"node":">=16"}}"#).unwrap();

        let reqs = RuntimeAdapter::declared_requirements(&dir);
        let node: Vec<_> = reqs.iter().filter(|(t, _)| t == "node").collect();
        assert_eq!(node.len(), 1, "the same tool must not be listed twice");
        assert_eq!(node[0].1, ">=20.11.1");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn preflight_reports_a_missing_tool_as_unknown() {
        let adapter = RuntimeAdapter::new();
        let results = adapter
            .preflight(&[Requirement {
                id: "rt-missing".into(),
                adapter_id: "runtime".into(),
                required: true,
                config: serde_json::json!({
                    "name": "definitely-not-a-real-tool-xyz",
                    "version": ">=1.0.0"
                }),
            }])
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, CheckStatus::Unknown);
        assert!(results[0].evidence.contains("not found"));
    }

    #[test]
    fn classification_reports_a_satisfied_requirement_as_verified() {
        let (status, evidence) =
            RuntimeAdapter::classify_version_check(Some("20.11.1"), "node", ">=18.0.0");
        assert_eq!(status, CheckStatus::ReadyVerified);
        assert!(evidence.contains("satisfies"), "got {evidence}");
    }

    #[test]
    fn classification_reports_a_version_mismatch_distinctly_from_a_missing_tool() {
        let (mismatch, _) =
            RuntimeAdapter::classify_version_check(Some("16.0.0"), "node", ">=18.0.0");
        let (missing, _) = RuntimeAdapter::classify_version_check(None, "node", ">=18.0.0");

        assert_eq!(mismatch, CheckStatus::ReadyAccountMismatch);
        assert_eq!(missing, CheckStatus::Unknown);
        assert_ne!(
            mismatch, missing,
            "a wrong version and an absent tool need different advice"
        );
    }

    #[test]
    fn classification_does_not_wave_through_an_uncomparable_requirement() {
        let (status, evidence) =
            RuntimeAdapter::classify_version_check(Some("20.11.1"), "node", "lts/*");
        assert_eq!(status, CheckStatus::ConfiguredUnverified);
        assert!(evidence.contains("could not be compared"), "got {evidence}");
    }

    #[test]
    fn classification_with_no_requirement_only_checks_presence() {
        let (status, _) = RuntimeAdapter::classify_version_check(Some("20.11.1"), "node", "");
        assert_eq!(status, CheckStatus::ReadyVerified);
    }

    #[tokio::test]
    async fn preflight_routes_each_requirement_to_its_own_result() {
        let adapter = RuntimeAdapter::new();
        let results = adapter
            .preflight(&[
                Requirement {
                    id: "rt-a".into(),
                    adapter_id: "runtime".into(),
                    required: true,
                    config: serde_json::json!({ "name": "definitely-not-real-xyz" }),
                },
                Requirement {
                    id: "rt-b".into(),
                    // Belongs to another adapter and must be ignored here.
                    adapter_id: "git".into(),
                    required: true,
                    config: serde_json::json!({ "name": "node" }),
                },
            ])
            .await
            .unwrap();

        assert_eq!(results.len(), 1, "only the runtime requirement applies");
        assert_eq!(results[0].requirement_id, "rt-a");
        assert_eq!(results[0].status, CheckStatus::Unknown);
    }

    #[test]
    fn extract_version_ignores_a_bare_number() {
        // A date or a port is not a version; accepting it would produce a
        // confidently wrong preflight verdict.
        assert_eq!(RuntimeAdapter::extract_version("built 2024"), None);
        assert_eq!(RuntimeAdapter::extract_version("listening on 8080"), None);
    }

    #[test]
    fn toml_lite_reads_sections_and_quotes() {
        let table: toml_lite::Table = "[package]\nrust-version = \"1.75\"\nname='x'\n"
            .parse()
            .unwrap();
        assert_eq!(table.get_str("package.rust-version").as_deref(), Some("1.75"));
        assert_eq!(table.get_str("package.name").as_deref(), Some("x"));
        assert_eq!(table.get_str("package.missing"), None);
    }
}
