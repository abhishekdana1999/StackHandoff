//! Restore Tauri commands.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{command, State};
use workspace_clone_adapters::LocalContext;
use workspace_clone_core::manifest::WorkspaceManifest;
use workspace_clone_core::Result;
use workspace_clone_db::DbPool;
use workspace_clone_restore::{
    executor::{RestoreExecutionReport, RestoreExecutor},
    planner::{PlanRequest, RestorePlan, RestorePlanner},
};

/// Turn a manifest into an ordered plan for this machine.
///
/// `destination_roots_json` maps each logical bucket to a folder on this
/// machine, for example `{"code": "/Users/me/code"}`. A project whose bucket is
/// missing is planned without a destination and reports that no location has
/// been chosen, rather than being mapped somewhere the user never picked.
#[command]
pub async fn generate_restore_plan(
    planner: State<'_, Arc<RestorePlanner>>,
    manifest_json: String,
    destination_roots_json: Option<String>,
) -> Result<RestorePlan> {
    let manifest: WorkspaceManifest = serde_json::from_str(&manifest_json)?;

    // A plan built from a manifest that fails validation is not worth building:
    // the commands would be a rendering of data the app has already decided not
    // to trust.
    manifest.validate()?;

    let destination_roots: BTreeMap<String, PathBuf> = destination_roots_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();

    tracing::info!(
        "Planning restore for '{}' with {} destination root(s)",
        manifest.workspace.name,
        destination_roots.len()
    );

    let local_context = LocalContext::current();
    let request = PlanRequest {
        manifest: &manifest,
        local_context: &local_context,
        destination_roots,
    };

    planner.generate_plan(&request).await
}

/// Run a plan.
///
/// `approvals_json` is a map of step id to the user's decision. A step the user
/// left out follows the planner's default, which is declined for anything that
/// opens a window.
#[command]
pub async fn execute_restore(
    executor: State<'_, Arc<RestoreExecutor>>,
    pool: State<'_, DbPool>,
    run_id: String,
    plan_json: String,
    approvals_json: Option<String>,
) -> Result<RestoreExecutionReport> {
    let plan: RestorePlan = serde_json::from_str(&plan_json)?;
    let approvals: std::collections::HashMap<String, bool> = approvals_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?
        .unwrap_or_default();

    // The workspace's file archive, when it has one. Loaded here -- the command
    // layer owns the database and the storage key -- and handed to the executor
    // as bytes, so the executor never needs either.
    let files = crate::capture::sealed_files(pool.inner(), &plan.workspace_id).await?;

    executor
        .execute_plan(&run_id, &plan, &approvals, files.as_deref())
        .await
}

/// Summarise a plan for the confirmation screen, without executing anything.
///
/// Split out so the UI can show what a restore will do before the user commits
/// to it. The summary is derived from the plan, so it cannot disagree with what
/// execution would actually do.
#[command]
pub fn summarize_restore_plan(plan_json: String) -> Result<PlanSummary> {
    let plan: RestorePlan = serde_json::from_str(&plan_json)?;

    let opens_something = plan
        .steps
        .iter()
        .filter(|a| {
            matches!(
                a.action_type,
                workspace_clone_adapters::RestoreActionType::OpenApplication
                    | workspace_clone_adapters::RestoreActionType::OpenUrls
            )
        })
        .count();

    Ok(PlanSummary {
        total_steps: plan.steps.len() as u32,
        already_approved: plan.approved_step_ids().len() as u32,
        need_consent: plan.steps_needing_consent().len() as u32,
        opens_something: opens_something as u32,
        notes: plan.notes.clone(),
    })
}

/// What a restore is about to do.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanSummary {
    pub total_steps: u32,
    /// Steps that will run without asking.
    pub already_approved: u32,
    /// Steps the user still has to tick.
    pub need_consent: u32,
    /// Steps that put something on screen. The number a user most wants to see.
    pub opens_something: u32,
    /// Anything the planner could not express.
    pub notes: Vec<String>,
}
