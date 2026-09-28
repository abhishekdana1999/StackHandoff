//! Preflight Tauri commands.

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{command, State};
use workspace_clone_adapters::{CheckResult, LocalContext};
use workspace_clone_core::{manifest::Requirements, Result};
use workspace_clone_preflight::engine::{PreflightEngine, PreflightOptions, PreflightReport};
use workspace_clone_preflight::to_check_requirements;

/// Run every check a workspace needs before restoring.
///
/// The manifest carries the requirements; this command only bridges them into
/// the engine's check model and passes through the caller's options. The
/// conversion used to live here and routed each application to an adapter named
/// after the application, so every application check came back "adapter not
/// available". It now lives in `workspace_clone_preflight::bridge`, where it is
/// covered by tests.
#[command]
pub async fn run_preflight(
    engine: State<'_, Arc<PreflightEngine>>,
    requirements_json: String,
    env_files: Option<Vec<String>>,
    confirmed_requirements: Option<Vec<String>>,
) -> Result<PreflightReport> {
    let requirements: Requirements = serde_json::from_str(&requirements_json)?;

    let options = PreflightOptions {
        env_files: env_files
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            // Only files that exist are passed on, so a typo cannot make a check
            // claim it consulted a nonexistent source.
            .filter(|path| path.is_file())
            .collect(),
        confirmed_requirements: confirmed_requirements.unwrap_or_default(),
    };

    let checks = to_check_requirements(&requirements);

    tracing::info!(
        "Preflight requested for {} checks across {} applications, {} runtimes, {} identities",
        checks.len(),
        requirements.applications.len(),
        requirements.runtimes.len(),
        requirements.identities.len()
    );

    engine
        .run_preflight_with(&checks, &LocalContext::current(), &options)
        .await
}

/// Re-run one check, bypassing the cache.
///
/// The user pressed "check again", so the previous answer is by definition not
/// what they want, and a consent-gated check gets the chance it was waiting for.
#[command]
pub async fn rerun_preflight_check(
    engine: State<'_, Arc<PreflightEngine>>,
    requirement_json: String,
    confirmed: Option<bool>,
) -> Result<CheckResult> {
    let requirement: workspace_clone_adapters::Requirement =
        serde_json::from_str(&requirement_json)?;

    let options = PreflightOptions {
        // Naming the requirement as confirmed is what both bypasses the cache
        // and grants consent, which is the correct reading of an explicit
        // "check again".
        confirmed_requirements: if confirmed.unwrap_or(true) {
            vec![requirement.id.clone()]
        } else {
            Vec::new()
        },
        ..Default::default()
    };

    engine
        .run_single_check_with(&requirement, &LocalContext::current(), &options)
        .await
}
