//! Environment-variable presence checks.
//!
//! The rule the blueprint sets is narrow and absolute: check that a *named*
//! variable is **present**, and never read its **value** into a report,
//! manifest, log or transfer.
//!
//! This module is written so that a value cannot leak by accident. Every check
//! is written as `env::var_os(name).is_some()` so the value is never bound to a
//! variable, never formatted, and never reaches a `CheckResult`. The only thing
//! that leaves this module is a boolean.

use std::time::Duration;
use tracing::debug;
use workspace_clone_adapters::{
    ActionType, CheckResult, CheckStatus, RemediationAction, Requirement,
};

/// How a variable's presence is established.
///
/// Prescribing the source matters: a variable found in a `.env` file on disk is
/// a different assurance from one exported by the current shell, and conflating
/// them would let preflight claim a readiness the user does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceSource {
    /// Present in this process's environment.
    Process,
    /// Present in a `.env` file the user explicitly pointed us at.
    EnvFile,
}

impl PresenceSource {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Process => "the current process environment",
            Self::EnvFile => "a configured .env file",
        }
    }
}

/// Whether a named variable is present, without ever looking at its value.
fn is_present_in_process(name: &str) -> bool {
    // Written as a single chained expression on purpose: the returned `Option`
    // is consumed by `is_some()` and dropped, so no binding of the value exists
    // anywhere in this function.
    std::env::var_os(name).is_some()
}

/// Whether a name is present in a `.env` file.
///
/// Only the names on the left of an `=` are read. The right-hand side is never
/// parsed into a string, so a secret in a `.env` file cannot reach a report even
/// by accident.
fn is_present_in_env_file(name: &str, path: &std::path::Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };

    contents.lines().any(|line| {
        let line = line.trim();
        // Skip comments and blanks.
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        // `export FOO=bar` is legal in a shell-style .env file.
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        // Compare the key only; the value is never extracted.
        let key = line.split_once('=').map(|(k, _)| k.trim()).unwrap_or(line);
        // `.env` files sometimes quote the key.
        key.trim_matches(['"', '\'']) == name
    })
}

/// Run every environment check in `requirements`.
///
/// `env_files` is the list of files the user explicitly configured. Nothing
/// else on disk is opened.
pub fn run_environment_checks(
    requirements: &[Requirement],
    env_files: &[std::path::PathBuf],
) -> Vec<CheckResult> {
    requirements
        .iter()
        .map(|req| check_one(req, env_files))
        .collect()
}

fn check_one(requirement: &Requirement, env_files: &[std::path::PathBuf]) -> CheckResult {
    let now = chrono::Utc::now();

    let Some(name) = requirement
        .config
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: "The requirement does not name a variable, so presence cannot be checked."
                .to_string(),
            freshness: now,
            action: None,
        };
    };

    // An empty name would match nothing and report a false negative forever.
    if name.trim().is_empty() {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: "The requirement has an empty variable name.".to_string(),
            freshness: now,
            action: None,
        };
    }

    if is_present_in_process(&name) {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::ReadyVerified,
            // Names the variable and the source. Never its value.
            evidence: format!("{name} is present in {}", PresenceSource::Process.describe()),
            freshness: now,
            action: None,
        };
    }

    // Fall back only to files the user configured, and say which file matched so
    // the evidence is not misleading about where the value would come from.
    for path in env_files {
        if is_present_in_env_file(&name, path) {
            return CheckResult {
                requirement_id: requirement.id.clone(),
                status: CheckStatus::ReadyVerified,
                evidence: format!(
                    "{name} is not in the process environment, but it is present in {}",
                    path.display()
                ),
                freshness: now,
                action: Some(RemediationAction {
                    label: format!("Export {name} before launching"),
                    action_type: ActionType::RunCommand,
                    url: None,
                    command: Some(format!("export {name}=...")),
                }),
            };
        }
    }

    debug!("Environment variable {name} is not present");

    CheckResult {
        requirement_id: requirement.id.clone(),
        status: CheckStatus::Unknown,
        // "Unknown" rather than "failed": a variable being unset is a statement
        // about the current shell, not a broken machine.
        evidence: format!("{name} is not set in the current environment"),
        freshness: now,
        action: Some(RemediationAction {
            label: format!("Set {name}"),
            action_type: ActionType::RunCommand,
            url: None,
            // The value is left as a placeholder. This command is offered for
            // the user to fill in and run themselves; nothing here invents or
            // guesses a secret.
            command: Some(format!("export {name}=<value>")),
        }),
    }
}

/// Whether an adapter exists that can satisfy a requirement without installing
/// anything. Kept here so the engine can classify an unknown adapter id.
pub fn is_environment_adapter(adapter_id: &str) -> bool {
    adapter_id == "environment"
}

/// How long the engine should allow environment checks before assuming the
/// environment is unusable.
pub const ENVIRONMENT_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    fn req(name: &str) -> Requirement {
        Requirement {
            id: format!("env-{name}"),
            adapter_id: "environment".into(),
            required: true,
            config: serde_json::json!({ "name": name }),
        }
    }

    #[test]
    fn reports_a_present_variable_without_touching_its_value() {
        // A variable whose value would be instantly recognisable if it leaked.
        std::env::set_var("WC_TEST_SECRET_VALUE", "hunter2-super-secret");

        let results = run_environment_checks(&[req("WC_TEST_SECRET_VALUE")], &[]);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, CheckStatus::ReadyVerified);
        assert!(results[0].evidence.contains("WC_TEST_SECRET_VALUE"));
        assert!(
            !results[0].evidence.contains("hunter2"),
            "the value leaked into evidence: {}",
            results[0].evidence
        );
        assert!(
            !results[0].evidence.contains("super-secret"),
            "the value leaked into evidence: {}",
            results[0].evidence
        );

        std::env::remove_var("WC_TEST_SECRET_VALUE");
    }

    #[test]
    fn reports_a_missing_variable_as_unknown_not_failure() {
        std::env::remove_var("WC_TEST_DEFINITELY_UNSET");

        let results = run_environment_checks(&[req("WC_TEST_DEFINITELY_UNSET")], &[]);

        assert_eq!(results[0].status, CheckStatus::Unknown);
        assert!(results[0].evidence.contains("is not set"));
    }

    #[test]
    fn evidence_across_many_never_contains_any_value() {
        let names = ["WC_A", "WC_B", "WC_C"];
        let values = ["alpha-value", "bravo-value", "charlie-value"];
        for (name, value) in names.iter().zip(values) {
            std::env::set_var(name, value);
        }

        let requirements: Vec<Requirement> = names.iter().map(|n| req(n)).collect();
        let all_evidence = run_environment_checks(&requirements, &[])
            .iter()
            .map(|r| r.evidence.clone())
            .collect::<Vec<_>>()
            .join(" | ");

        for value in values {
            assert!(
                !all_evidence.contains(value),
                "a value leaked: {all_evidence}"
            );
        }
        // All three names should be present though.
        for name in names {
            assert!(all_evidence.contains(name), "missing {name}");
        }

        for name in names {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn finds_a_variable_in_a_configured_env_file() {
        let dir = std::env::temp_dir().join("wc-envfile-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(".env");
        std::fs::write(
            &file,
            "# a comment\n\
             DATABASE_URL=postgres://real:password@host/db\n\
             export EXPORTED_ONE=abc\n\
             QUOTED=\"def\"\n\
             not an assignment\n",
        )
        .unwrap();

        let results = run_environment_checks(
            &[
                req("DATABASE_URL"),
                req("EXPORTED_ONE"),
                req("QUOTED"),
                req("MISSING_FROM_FILE"),
            ],
            &[file.clone()],
        );

        assert_eq!(results[0].status, CheckStatus::ReadyVerified);
        assert_eq!(
            results[1].status,
            CheckStatus::ReadyVerified,
            "an `export ` prefix must not hide the name"
        );
        assert_eq!(
            results[2].status,
            CheckStatus::ReadyVerified,
            "a quoted key must still match"
        );
        assert_eq!(results[3].status, CheckStatus::Unknown);

        // The password in the file must not appear anywhere.
        for result in &results {
            assert!(
                !result.evidence.contains("real:password"),
                "a .env value leaked: {}",
                result.evidence
            );
            assert!(!result.evidence.contains("postgres://"));
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unreadable_env_file_does_not_fail_the_check() {
        let results = run_environment_checks(
            &[req("ANYTHING")],
            &[std::path::PathBuf::from("/definitely/not/here/.env")],
        );
        assert_eq!(results[0].status, CheckStatus::Unknown);
    }

    #[test]
    fn a_requirement_without_a_name_is_reported_not_silently_passed() {
        let requirement = Requirement {
            id: "env-nameless".into(),
            adapter_id: "environment".into(),
            required: true,
            config: serde_json::json!({}),
        };

        let results = run_environment_checks(&[requirement], &[]);
        assert_eq!(results[0].status, CheckStatus::Unknown);
        assert!(results[0].evidence.contains("does not name a variable"));
    }

    #[test]
    fn an_empty_name_is_rejected() {
        let results = run_environment_checks(&[req("   ")], &[]);
        assert_eq!(results[0].status, CheckStatus::Unknown);
        assert!(results[0].evidence.contains("empty variable name"));
    }

    #[test]
    fn a_missing_requirement_offers_a_placeholder_not_a_guess() {
        std::env::remove_var("WC_TEST_MISSING_OFFER");

        let results = run_environment_checks(&[req("WC_TEST_MISSING_OFFER")], &[]);
        let command = results[0].action.as_ref().unwrap().command.as_ref().unwrap();

        assert!(command.contains("WC_TEST_MISSING_OFFER"));
        assert!(
            command.contains("<value>"),
            "the offered command must not invent a value: {command}"
        );
    }
}
