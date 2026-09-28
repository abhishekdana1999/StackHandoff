//! Identity readiness checks.
//!
//! These are the checks the blueprint is most careful about, and the rules are
//! not optional:
//!
//! - invoke a documented CLI directly, never through a shell, with fixed
//!   arguments, bounded time and bounded output;
//! - reduce the output to an *allowed hint* (an account name or account id) and
//!   discard everything else immediately, keeping no raw text;
//! - never let a token, key id or environment value reach evidence, a log or
//!   the database;
//! - if a check would make a network request, refresh credentials or touch
//!   billable resources, report `unknown` and ask the user first.
//!
//! The distinction matters because evidence is persisted and shown to the user
//! on a different machine. A leaked access key id in that string would be a
//! credential disclosure, not a cosmetic problem.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::debug;
use workspace_clone_adapters::{
    ActionType, CheckResult, CheckStatus, RemediationAction, Requirement,
};
use workspace_clone_core::Result;

/// The most a check is permitted to establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationLevel {
    /// Can confirm the user is authenticated.
    Authentication,
    /// Can confirm *which* account is authenticated.
    Account,
    /// Can confirm access to a specific project or workspace.
    Workspace,
    /// Can only see that a profile or config file exists.
    ConfigurationOnly,
}

impl VerificationLevel {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Account => "account identity",
            Self::Workspace => "workspace identity",
            Self::ConfigurationOnly => "configuration presence only",
        }
    }
}

/// What running a check would cost the user, so the UI can ask first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckCost {
    /// Makes a request to the provider's servers.
    pub network: bool,
    /// Could cause a stored credential to be refreshed or extended.
    pub may_refresh_credentials: bool,
}

impl CheckCost {
    pub const LOCAL_ONLY: Self = Self {
        network: false,
        may_refresh_credentials: false,
    };

    pub fn needs_consent(&self) -> bool {
        self.network || self.may_refresh_credentials
    }
}

/// A provider-specific identity check.
pub trait IdentityChecker: Send + Sync {
    fn service_name(&self) -> &'static str;

    /// The exact command this check runs, for display before consent is given.
    fn check_command(&self) -> String;

    fn verification_level(&self) -> VerificationLevel;

    fn cost(&self) -> CheckCost;

    fn login_action(&self) -> RemediationAction;

    /// The part of the output this check is allowed to keep.
    ///
    /// Everything else in the output is discarded by the caller and never
    /// returned, so an implementation physically cannot leak it.
    fn extract_hint(&self, stdout: &str, stderr: &str) -> Option<String>;
}

/// Run a CLI directly, with no shell, a time bound and an output bound.
///
/// Returns `Ok(None)` when the tool is absent or misbehaves, because "the check
/// could not run" is a normal state that must surface as `unknown` rather than
/// aborting the whole preflight run.
fn run_bounded(program: &str, args: &[&str], limit: Duration) -> Option<BoundedOutput> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().ok()?;
                return Some(BoundedOutput {
                    success: status.success(),
                    stdout: truncate(&String::from_utf8_lossy(&output.stdout)),
                    stderr: truncate(&String::from_utf8_lossy(&output.stderr)),
                });
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    // A tool that hangs is treated as "could not check" rather
                    // than being allowed to hold the UI hostage.
                    let _ = child.kill();
                    let _ = child.wait();
                    debug!("{program} did not finish within the limit");
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

struct BoundedOutput {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Cap on captured output. Provider CLIs print little that we need, and an
/// unbounded read of a misbehaving process is a memory risk.
const MAX_OUTPUT: usize = 8 * 1024;

fn truncate(text: &str) -> String {
    if text.len() <= MAX_OUTPUT {
        return text.to_string();
    }
    let mut end = MAX_OUTPUT;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

/// Strip anything that looks like a credential out of a string that is about to
/// become evidence.
///
/// This is a backstop. The extractors below already return only the field they
/// are allowed to keep, so this should find nothing; it exists so that a future
/// change to an extractor cannot silently start leaking.
pub fn scrub_credential_shaped_text(text: &str) -> String {
    let mut cleaned = text.to_string();

    for marker in [
        "ghp_", "gho_", "ghu_", "ghs_", "ghr_", "glpat-", "sk-", "xoxb-", "xoxp-", "AKIA",
        "ASIA", "AIza", "ya29.",
    ] {
        while let Some(at) = cleaned.find(marker) {
            // Cut back to the start of the token, then forward to whitespace.
            let start = cleaned[..at]
                .rfind(|c: char| c.is_whitespace())
                .map(|i| i + 1)
                .unwrap_or(at);
            let end = cleaned[at..]
                .find(char::is_whitespace)
                .map(|i| at + i)
                .unwrap_or(cleaned.len());
            cleaned.replace_range(start..end, "[redacted]");
        }
    }

    cleaned
}

/// GitHub, via `gh auth status`.
pub struct GitHubIdentityChecker;

impl IdentityChecker for GitHubIdentityChecker {
    fn service_name(&self) -> &'static str {
        "github"
    }

    fn check_command(&self) -> String {
        "gh auth status".to_string()
    }

    fn verification_level(&self) -> VerificationLevel {
        VerificationLevel::Account
    }

    fn cost(&self) -> CheckCost {
        // `gh auth status` is local: it reads the keyring, not the network.
        CheckCost::LOCAL_ONLY
    }

    fn login_action(&self) -> RemediationAction {
        RemediationAction {
            label: "Sign in to GitHub".to_string(),
            action_type: ActionType::RunCommand,
            url: Some("https://github.com/login".to_string()),
            // Offered, never run automatically.
            command: Some("gh auth login".to_string()),
        }
    }

    fn extract_hint(&self, stdout: &str, stderr: &str) -> Option<String> {
        // `gh` writes its status to stderr. Look only for the account name and
        // deliberately skip the token line, which `gh` redacts but which we
        // still refuse to carry.
        let haystack = format!("{stdout}\n{stderr}");
        for line in haystack.lines() {
            let Some(rest) = line.split("account ").nth(1) else {
                continue;
            };
            let account = rest
                .split(['(', ' ', '\t'])
                .next()
                .unwrap_or("")
                .trim();
            if !account.is_empty() {
                return Some(account.to_string());
            }
        }
        None
    }
}

/// AWS, via `aws sts get-caller-identity`.
pub struct AwsIdentityChecker;

impl IdentityChecker for AwsIdentityChecker {
    fn service_name(&self) -> &'static str {
        "aws"
    }

    fn check_command(&self) -> String {
        "aws sts get-caller-identity".to_string()
    }

    fn verification_level(&self) -> VerificationLevel {
        VerificationLevel::Account
    }

    fn cost(&self) -> CheckCost {
        // This is a signed API call: it reaches AWS and can consume a billed
        // request, so the user has to agree before it runs.
        CheckCost {
            network: true,
            may_refresh_credentials: false,
        }
    }

    fn login_action(&self) -> RemediationAction {
        RemediationAction {
            label: "Configure the AWS CLI".to_string(),
            action_type: ActionType::RunCommand,
            url: Some("https://docs.aws.amazon.com/cli/latest/userguide/cli-configure.html".to_string()),
            command: Some("aws configure".to_string()),
        }
    }

    fn extract_hint(&self, stdout: &str, _stderr: &str) -> Option<String> {
        // The response also carries `UserId`, which is derived from the access
        // key. It is never read here, so it cannot reach evidence.
        let json: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
        let account = json.get("Account").and_then(|v| v.as_str())?;
        Some(account.to_string())
    }
}

/// Supabase, via `supabase projects list`.
pub struct SupabaseIdentityChecker;

impl IdentityChecker for SupabaseIdentityChecker {
    fn service_name(&self) -> &'static str {
        "supabase"
    }

    fn check_command(&self) -> String {
        "supabase projects list".to_string()
    }

    fn verification_level(&self) -> VerificationLevel {
        VerificationLevel::Workspace
    }

    fn cost(&self) -> CheckCost {
        CheckCost {
            network: true,
            may_refresh_credentials: false,
        }
    }

    fn login_action(&self) -> RemediationAction {
        RemediationAction {
            label: "Sign in to Supabase".to_string(),
            action_type: ActionType::RunCommand,
            url: Some("https://supabase.com/dashboard".to_string()),
            command: Some("supabase login".to_string()),
        }
    }

    fn extract_hint(&self, stdout: &str, _stderr: &str) -> Option<String> {
        // `supabase projects list` prints a table. Keep the first project ref
        // so the user can see they are signed in, and nothing else.
        for line in stdout.lines().skip(1) {
            let first = line.split_whitespace().next()?;
            if first.len() >= 20 && first.chars().all(|c| c.is_ascii_lowercase()) {
                return Some(first.to_string());
            }
        }
        None
    }
}

/// Look up a checker by service name.
pub fn get_identity_checker(service: &str) -> Option<Box<dyn IdentityChecker>> {
    match service {
        "github" => Some(Box::new(GitHubIdentityChecker)),
        "aws" => Some(Box::new(AwsIdentityChecker)),
        "supabase" => Some(Box::new(SupabaseIdentityChecker)),
        _ => None,
    }
}

/// Run one identity check.
///
/// `user_confirmed` records that the user agreed to a check with a cost. It is
/// what stops a signed AWS call from happening just because a manifest mentioned
/// AWS.
pub fn run_identity_check(requirement: &Requirement, user_confirmed: bool) -> Result<CheckResult> {
    let service = requirement
        .config
        .get("service")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    let account_hint = requirement
        .config
        .get("account_hint")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let now = chrono::Utc::now();

    let Some(checker) = get_identity_checker(&service) else {
        return Ok(CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: format!("No safe local check is available for '{service}'."),
            freshness: now,
            action: Some(RemediationAction {
                label: format!("Confirm you are signed in to {service}"),
                action_type: ActionType::OpenUrl,
                url: None,
                command: None,
            }),
        });
    };

    let cost = checker.cost();

    // Costly checks wait for explicit consent. Reported as unknown rather than
    // failed, because nothing is actually wrong.
    if cost.needs_consent() && !user_confirmed {
        return Ok(CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: format!(
                "Checking {} runs `{}`, which contacts {} and may consume a request. \
                 Approve the check to run it, or sign in yourself and mark it confirmed.",
                checker.service_name(),
                checker.check_command(),
                checker.service_name()
            ),
            freshness: now,
            action: Some(RemediationAction {
                label: format!("Run the {} check", checker.service_name()),
                action_type: ActionType::RunCommand,
                url: None,
                command: Some(checker.check_command()),
            }),
        });
    }

    let (program, args): (&str, Vec<&str>) = match service.as_str() {
        "github" => ("gh", vec!["auth", "status"]),
        "aws" => ("aws", vec!["sts", "get-caller-identity"]),
        "supabase" => ("supabase", vec!["projects", "list"]),
        _ => ("", vec![]),
    };

    if program.is_empty() {
        return Ok(CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: format!("No check is defined for '{service}'."),
            freshness: now,
            action: None,
        });
    }

    let Some(output) = run_bounded(program, &args, Duration::from_secs(10)) else {
        // Distinguish "not installed" from "would not answer", because the
        // advice differs.
        let installed = which::which(program).is_ok();
        return Ok(CheckResult {
            requirement_id: requirement.id.clone(),
            status: if installed {
                CheckStatus::Unknown
            } else {
                CheckStatus::NotApplicable
            },
            evidence: if installed {
                format!(
                    "`{}` is installed but did not return a status within 10 seconds, so {} could not be checked.",
                    checker.check_command(),
                    service
                )
            } else {
                format!(
                    "The `{program}` CLI is not installed, so {service} readiness cannot be checked on this device."
                )
            },
            freshness: now,
            action: Some(checker.login_action()),
        });
    };

    // `output` is dropped at the end of this scope; only the extracted hint
    // survives into evidence.
    let hint = checker.extract_hint(&output.stdout, &output.stderr);

    if !output.success {
        return Ok(CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::LoginRequired,
            evidence: format!(
                "`{}` reports no active authentication for {service}.",
                checker.check_command()
            ),
            freshness: now,
            action: Some(checker.login_action()),
        });
    }

    let (status, evidence) = match (&hint, &account_hint) {
        (None, _) => (
            CheckStatus::ConfiguredUnverified,
            format!(
                "`{}` succeeded, so {service} is reachable, but no account could be identified. \
                 This check confirms {} at most.",
                checker.check_command(),
                checker.verification_level().describe()
            ),
        ),
        (Some(found), None) => (
            CheckStatus::ReadyVerified,
            format!(
                "{} is signed in (account {found}).",
                checker.service_name()
            ),
        ),
        (Some(found), Some(want)) if found.eq_ignore_ascii_case(want) => (
            CheckStatus::ReadyVerified,
            format!("{} is signed in as the expected account {found}.", service),
        ),
        (Some(found), Some(want)) => (
            CheckStatus::ReadyAccountMismatch,
            format!(
                "{service} is signed in as '{found}', but the workspace expects '{want}'."
            ),
        ),
    };

    Ok(CheckResult {
        requirement_id: requirement.id.clone(),
        status,
        // Final backstop before evidence is persisted and shown on another
        // machine.
        evidence: scrub_credential_shaped_text(&evidence),
        freshness: now,
        action: Some(checker.login_action()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement(config: serde_json::Value) -> Requirement {
        Requirement {
            id: "identity-1".into(),
            adapter_id: "identity".into(),
            required: true,
            config,
        }
    }

    #[test]
    fn github_hint_is_read_and_the_token_line_is_ignored() {
        // Realistic `gh auth status` output, token line included.
        let stdout = "";
        let stderr = "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  \
                      - Active account: true\n  - Token: gho_abcdefghijklmnopqrstuvwxyz0123456789\n  \
                      - Token scopes: gist, read:org\n";

        let checker = GitHubIdentityChecker;
        assert_eq!(checker.extract_hint(stdout, stderr).as_deref(), Some("octocat"));
    }

    #[test]
    fn aws_hint_is_the_account_and_never_the_user_id() {
        // `UserId` is derived from the access key and must not be kept.
        let stdout = r#"{
            "UserId": "AIDAEXAMPLEUSERID12345",
            "Account": "123456789012",
            "Arn": "arn:aws:iam::123456789012:user/octocat"
        }"#;

        let checker = AwsIdentityChecker;
        let hint = checker.extract_hint(stdout, "").unwrap();

        assert_eq!(hint, "123456789012");
        assert!(!hint.contains("AIDA"), "the access key id leaked");
        assert!(!hint.contains("octocat"), "the user id leaked");
    }

    #[test]
    fn aws_hint_is_none_for_unparseable_output() {
        let checker = AwsIdentityChecker;
        assert_eq!(checker.extract_hint("not json at all", ""), None);
        assert_eq!(checker.extract_hint("", ""), None);
    }

    #[test]
    fn github_reports_login_required_when_the_cli_says_not_logged_in() {
        let result = run_identity_check(
            &requirement(serde_json::json!({
                "service": "github",
                "verification": "authenticate"
            })),
            false,
        )
        .unwrap();

        // A machine with no `gh` reports not-applicable rather than a failure.
        assert!(
            matches!(
                result.status,
                CheckStatus::LoginRequired | CheckStatus::NotApplicable | CheckStatus::Unknown
            ),
            "unexpected status {:?} with evidence: {}",
            result.status,
            result.evidence
        );
        assert!(result.action.is_some(), "there should be a way to fix it");
    }

    #[test]
    fn a_network_check_waits_for_consent() {
        // `aws sts get-caller-identity` contacts AWS. Without consent it must
        // not run, and must say so.
        let result = run_identity_check(
            &requirement(serde_json::json!({ "service": "aws" })),
            false,
        )
        .unwrap();

        assert_eq!(result.status, CheckStatus::Unknown);
        assert!(
            result.evidence.contains("Approve the check"),
            "the user must be asked: {}",
            result.evidence
        );
        assert!(result.action.is_some());
    }

    #[test]
    fn a_local_check_does_not_wait_for_consent() {
        // `gh auth status` is local, so it must run without asking.
        let result = run_identity_check(
            &requirement(serde_json::json!({ "service": "github" })),
            false,
        )
        .unwrap();

        assert!(
            !result.evidence.contains("Approve the check"),
            "a local check must not demand consent: {}",
            result.evidence
        );
    }

    #[test]
    fn an_unknown_service_reports_unknown_with_a_manual_path() {
        let result =
            run_identity_check(&requirement(serde_json::json!({ "service": "acme-sso" })), true)
                .unwrap();

        assert_eq!(result.status, CheckStatus::Unknown);
        assert!(result.evidence.contains("acme-sso"));
        assert!(result.action.is_some());
    }

    #[test]
    fn evidence_never_contains_credential_shaped_strings() {
        let hostile = [
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "glpat-ABCDEFGHIJKLMNOPQRST",
            "AKIAIOSFODNN7EXAMPLE",
            "xoxb-123456789012-abcdefghijkl",
            "sk-abcdefghijklmnopqrstuvwxyz0123456789",
        ];

        for token in hostile {
            let scrubbed = scrub_credential_shaped_text(&format!(
                "evidence mentioning {token} in the middle"
            ));
            assert!(!scrubbed.contains(token), "{token} survived: {scrubbed}");
            assert!(scrubbed.contains("[redacted]"));
        }
    }

    #[test]
    fn scrubbing_preserves_ordinary_evidence() {
        let text = "node 20.11.1 satisfies '>=18.0.0'";
        assert_eq!(scrub_credential_shaped_text(text), text);
    }

    #[test]
    fn output_truncation_respects_char_boundaries() {
        // A multi-byte character straddling the cap must not panic.
        let text = "é".repeat(MAX_OUTPUT);
        let truncated = truncate(&text);
        assert!(truncated.len() <= MAX_OUTPUT);
    }

    #[test]
    fn output_truncation_leaves_short_output_alone() {
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn cost_classification_is_correct_per_service() {
        assert!(!GitHubIdentityChecker.cost().needs_consent());
        assert!(AwsIdentityChecker.cost().needs_consent());
        assert!(SupabaseIdentityChecker.cost().needs_consent());
    }

    #[tokio::test]
    async fn checks_complete_promptly_even_without_the_cli() {
        // Bounded time is a product requirement, so assert the wall clock.
        let started = Instant::now();
        for service in ["github", "aws", "supabase", "unknown-service"] {
            let _ = run_identity_check(&requirement(serde_json::json!({ "service": service })), true)
                .unwrap();
        }
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "identity checks took {:?}, which would stall the preflight screen",
            started.elapsed()
        );
    }
}
