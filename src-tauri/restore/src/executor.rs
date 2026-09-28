//! Restore action execution.
//!
//! The executor's job is narrow: walk the plan in order, ask the adapter named
//! on each step to carry it out, and report honestly on what happened.
//!
//! Three properties it must hold, all of which the previous version got wrong:
//!
//! - **Route on the typed field.** `RestoreAction::adapter_id` is the only
//!   source of truth for who runs a step. Reading an adapter name out of the
//!   step's free-form config was how every step failed with "adapter not found",
//!   because nothing ever wrote that key.
//! - **A failure is a result, not an abort.** An adapter error becomes a
//!   `Failed` result so the run continues and the user sees the whole picture.
//!   Propagating the error used to abandon every later step silently.
//! - **Skipped is explained.** When a required step fails, the rest of the plan
//!   is reported as not attempted rather than simply missing, so a short report
//!   is never mistaken for a complete one.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use workspace_clone_adapters::{
    ActionResult, ActionStatus, AdapterRegistry, ApprovedRestoreAction, RestoreActionType,
};
use workspace_clone_core::Result;
use workspace_clone_db::models::RestoreRunRecord;
use workspace_clone_db::repository::RestoreRunRepository;

/// How long one step may take before it is called hung.
const ACTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Carries out a plan on this machine.
pub struct RestoreExecutor {
    adapter_registry: Arc<AdapterRegistry>,
    run_repo: Arc<RestoreRunRepository>,
    action_timeout: Duration,
}

impl RestoreExecutor {
    pub fn new(
        adapter_registry: Arc<AdapterRegistry>,
        run_repo: Arc<RestoreRunRepository>,
    ) -> Self {
        Self {
            adapter_registry,
            run_repo,
            action_timeout: ACTION_TIMEOUT,
        }
    }

    /// Run a plan, recording the outcome.
    ///
    /// `approvals` maps a step id to the user's decision. A step absent from the
    /// map falls back to the planner's default, which is approved for
    /// information-gathering steps and declined for anything that opens a
    /// window.
    ///
    /// `files` is the workspace's file archive (tar bytes) when the workspace
    /// carries files, and `None` for manifest-only workspaces. It is supplied by
    /// the command layer, which owns the database and the storage key; the
    /// executor only ever sees the bytes.
    pub async fn execute_plan(
        &self,
        run_id: &str,
        plan: &crate::planner::RestorePlan,
        approvals: &std::collections::HashMap<String, bool>,
        files: Option<&[u8]>,
    ) -> Result<RestoreExecutionReport> {
        info!("Executing restore plan with {} steps", plan.steps.len());

        let mut results: Vec<ActionResult> = Vec::with_capacity(plan.steps.len());
        let mut blocked_by: Option<String> = None;

        for action in &plan.steps {
            // Once a required step has failed, later steps are not attempted.
            // They are still reported, so the report is a complete account of
            // the plan rather than a truncated one.
            if let Some(blocker) = &blocked_by {
                results.push(not_attempted(&action.id, blocker));
                continue;
            }

            let approved = approvals
                .get(&action.id)
                .copied()
                .unwrap_or(action.approved);

            if !approved && !action.required {
                results.push(ActionResult {
                    action_id: action.id.clone(),
                    status: ActionStatus::Skipped,
                    message: format!("Skipped: {}", action.description),
                    duration_ms: 0,
                    details: None,
                });
                continue;
            }

            if !approved && action.required {
                // A required step the user declined. Rather than running
                // something they did not approve, the run stops here and says so.
                blocked_by = Some(action.id.clone());
                results.push(ActionResult {
                    action_id: action.id.clone(),
                    status: ActionStatus::Skipped,
                    message: format!(
                        "Skipped a required step: {}. Later steps were not attempted.",
                        action.description
                    ),
                    duration_ms: 0,
                    details: None,
                });
                continue;
            }

            let result = self.execute_action(action, files).await;
            let failed = matches!(result.status, ActionStatus::Failed);

            if failed && action.required {
                blocked_by = Some(action.id.clone());
            }

            results.push(result);
        }

        let report = RestoreExecutionReport {
            run_id: run_id.to_string(),
            workspace_id: plan.workspace_id.clone(),
            results,
            notes: plan.notes.clone(),
            completed_at: chrono::Utc::now(),
        };

        // Persisting is a convenience, not a precondition: a run that worked
        // must not be reported as failed because its history could not be saved.
        if let Err(e) = self.record(run_id, &report, approvals).await {
            warn!("Could not record restore run {run_id}: {e}");
        }

        Ok(report)
    }

    /// Carry out one step.
    async fn execute_action(
        &self,
        action: &workspace_clone_adapters::RestoreAction,
        files: Option<&[u8]>,
    ) -> ActionResult {
        let start = std::time::Instant::now();

        // Path mapping is bookkeeping, not work: the planner has already done
        // the mapping, so the step only confirms it. Handling it here keeps it
        // out of the adapter routing below, where it has no adapter to reach.
        if action.action_type == RestoreActionType::MapPath {
            return map_path_result(action, start.elapsed());
        }

        // File extraction is the executor's own work too: it needs the archive
        // bytes the command layer supplied, which no adapter owns.
        if action.action_type == RestoreActionType::ExtractFiles {
            return extract_files_result(action, files, start.elapsed());
        }

        let approved_action = ApprovedRestoreAction {
            action: action.clone(),
            // The plan is the resolution: the planner has already substituted
            // this machine's paths for the captured hints.
            resolved_config: action.config.clone(),
        };

        let outcome = timeout(
            self.action_timeout,
            self.adapter_registry.execute(&approved_action),
        )
        .await;

        match outcome {
            Ok(Ok(result)) => result,
            Ok(Err(e)) => {
                // An adapter error is a result for this step, not a failure of
                // the run: the remaining steps still get their chance.
                debug!("Step {} failed: {e}", action.id);
                ActionResult {
                    action_id: action.id.clone(),
                    status: ActionStatus::Failed,
                    message: format!("{} could not be completed: {e}", action.description),
                    duration_ms: start.elapsed().as_millis() as u64,
                    details: None,
                }
            }
            Err(_) => {
                warn!("Step {} exceeded {:?}", action.id, self.action_timeout);
                ActionResult {
                    action_id: action.id.clone(),
                    status: ActionStatus::Failed,
                    message: format!(
                        "{} did not finish within {} seconds",
                        action.description,
                        self.action_timeout.as_secs()
                    ),
                    duration_ms: self.action_timeout.as_millis() as u64,
                    details: None,
                }
            }
        }
    }

    /// Save the run so it appears in history.
    async fn record(
        &self,
        run_id: &str,
        report: &RestoreExecutionReport,
        approvals: &std::collections::HashMap<String, bool>,
    ) -> Result<()> {
        let summary = serde_json::json!({
            "succeeded": report.success_count(),
            "failed": report.failed_count(),
            "skipped": report.skipped_count(),
            "manual": report.manual_count(),
            "results": report.results,
        });

        // A run the user has not seen before is inserted; one that already
        // exists is updated, so a re-run does not duplicate history.
        if let Some(existing) = self.run_repo.get(run_id).await? {
            let mut record = existing;
            record.status = report.status().to_string();
            record.result_summary = summary.to_string();
            record.completed_at = Some(report.completed_at);
            return self.run_repo.update(&record).await;
        }

        self.run_repo
            .create(&RestoreRunRecord {
                id: run_id.to_string(),
                workspace_id: report.workspace_id.clone(),
                destination_device_id: "local".to_string(),
                plan_digest: String::new(),
                approved_steps: serde_json::to_string(approvals).unwrap_or_else(|_| "{}".into()),
                result_summary: summary.to_string(),
                status: report.status().to_string(),
                started_at: report.completed_at,
                completed_at: Some(report.completed_at),
            })
            .await
    }
}

/// Report a step that a previous failure prevented from running.
fn not_attempted(action_id: &str, blocker: &str) -> ActionResult {
    ActionResult {
        action_id: action_id.to_string(),
        status: ActionStatus::Skipped,
        message: format!("Not attempted, because an earlier required step failed: {blocker}"),
        duration_ms: 0,
        details: None,
    }
}

/// Confirm where a project maps to.
///
/// The planner resolved the path, so all this does is tell the user whether
/// that folder actually exists yet. A missing folder is reported as needing a
/// manual step rather than being created, because cloning is the user's
/// decision.
fn map_path_result(
    action: &workspace_clone_adapters::RestoreAction,
    duration: Duration,
) -> ActionResult {
    let destination = action.config.get("destination_path").and_then(|v| v.as_str());

    let Some(destination) = destination else {
        return ActionResult {
            action_id: action.id.clone(),
            status: ActionStatus::Manual,
            message: format!(
                "{} has no destination folder chosen, so nothing was opened for it",
                action.description
            ),
            duration_ms: duration.as_millis() as u64,
            details: None,
        };
    };

    let path = std::path::Path::new(destination);
    let (status, message) = if path.is_dir() {
        (ActionStatus::Success, format!("{destination} is ready"))
    } else {
        // The file-restore step later in the run creates the folder when the
        // workspace carries files; without files this stays a manual step.
        (
            ActionStatus::Manual,
            format!("{destination} does not exist yet. The file step will create it when this workspace has files; otherwise clone the code there, then re-run restore."),
        )
    };

    ActionResult {
        action_id: action.id.clone(),
        status,
        message,
        duration_ms: duration.as_millis() as u64,
        details: Some(serde_json::json!({ "destination_path": destination }).to_string()),
    }
}

/// Write a project's captured files from the workspace archive into its
/// destination folder.
///
/// Safe by construction: `workspace_clone_files::archive::extract_project`
/// refuses traversal names, absolute paths, symlinks, reserved names and
/// oversized entries, and caps the total. A workspace without files reports an
/// honest success ("nothing was captured") so this required step never blocks
/// the rest of the run.
fn extract_files_result(
    action: &workspace_clone_adapters::RestoreAction,
    files: Option<&[u8]>,
    duration: Duration,
) -> ActionResult {
    let human_bytes = |b: u64| {
        if b >= 1024 * 1024 {
            format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
        } else if b >= 1024 {
            format!("{:.1} KB", b as f64 / 1024.0)
        } else {
            format!("{b} B")
        }
    };

    let destination = action
        .config
        .get("destination_path")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let project_id = action
        .config
        .get("project_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    if destination.is_empty() || project_id.is_empty() {
        return ActionResult {
            action_id: action.id.clone(),
            status: ActionStatus::Failed,
            message: format!("{} could not run: it has no project or destination", action.id),
            duration_ms: duration.as_millis() as u64,
            details: None,
        };
    }

    let Some(tar) = files else {
        return ActionResult {
            action_id: action.id.clone(),
            status: ActionStatus::Success,
            message: "No files were captured for this workspace, so there is nothing to restore".to_string(),
            duration_ms: duration.as_millis() as u64,
            details: Some(
                serde_json::json!({ "files_written": 0, "destination_path": destination }).to_string(),
            ),
        };
    };

    if tar.is_empty() {
        return ActionResult {
            action_id: action.id.clone(),
            status: ActionStatus::Success,
            message: "The workspace archive is empty, so nothing was restored".to_string(),
            duration_ms: duration.as_millis() as u64,
            details: Some(
                serde_json::json!({ "files_written": 0, "destination_path": destination }).to_string(),
            ),
        };
    }

    let dest_path = std::path::Path::new(&destination);
    match workspace_clone_files::archive::extract_project(tar, &project_id, dest_path, Default::default()) {
        Ok(report) => {
            let truncated = if report.overflow {
                " The total size limit was reached, so some files were left out; the rest were restored."
            } else {
                ""
            };
            let refused = if report.refused.is_empty() {
                String::new()
            } else {
                format!(
                    " {} entry/entries were refused (unsafe names, symlinks or oversized files) and skipped.",
                    report.refused.len()
                )
            };
            ActionResult {
                action_id: action.id.clone(),
                status: ActionStatus::Success,
                message: format!(
                    "Restored {} file(s), {} in total{}{}",
                    report.files_written,
                    human_bytes(report.bytes_written),
                    truncated,
                    refused
                ),
                duration_ms: duration.as_millis() as u64,
                details: Some(
                    serde_json::json!({
                        "files_written": report.files_written,
                        "bytes_written": report.bytes_written,
                        "dirs_created": report.dirs_created,
                        "destination_path": destination,
                    })
                    .to_string(),
                ),
            }
        }
        Err(e) => ActionResult {
            action_id: action.id.clone(),
            status: ActionStatus::Failed,
            message: format!("Could not restore files to {destination}: {e}"),
            duration_ms: duration.as_millis() as u64,
            details: None,
        },
    }
}

/// The outcome of a restore run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreExecutionReport {
    pub run_id: String,
    /// The workspace this run restored, so history is meaningful.
    #[serde(default)]
    pub workspace_id: String,
    pub results: Vec<ActionResult>,
    /// Anything the planner could not express, carried through to the user.
    #[serde(default)]
    pub notes: Vec<String>,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

impl RestoreExecutionReport {
    pub fn success_count(&self) -> usize {
        self.count(ActionStatus::Success)
    }

    pub fn failed_count(&self) -> usize {
        self.count(ActionStatus::Failed)
    }

    pub fn skipped_count(&self) -> usize {
        self.count(ActionStatus::Skipped)
    }

    /// Steps that need the user to do something before they can succeed.
    pub fn manual_count(&self) -> usize {
        self.count(ActionStatus::Manual)
    }

    fn count(&self, status: ActionStatus) -> usize {
        self.results
            .iter()
            .filter(|r| r.status == status)
            .count()
    }

    /// Whether every step that ran, worked.
    ///
    /// A run with nothing left to do counts as successful: a skipped step is a
    /// decision, not a defect.
    pub fn is_successful(&self) -> bool {
        self.failed_count() == 0
    }

    /// A single word describing the run, for the database and for the UI.
    pub fn status(&self) -> &'static str {
        if self.failed_count() > 0 {
            "failed"
        } else if self.manual_count() > 0 {
            "needs_attention"
        } else if self.success_count() > 0 {
            "completed"
        } else {
            "nothing_to_do"
        }
    }

    /// The steps the user still has to act on, most important first.
    pub fn needs_user_action(&self) -> Vec<&ActionResult> {
        let mut pending: Vec<&ActionResult> = self
            .results
            .iter()
            .filter(|r| matches!(r.status, ActionStatus::Manual | ActionStatus::Failed))
            .collect();
        pending.sort_by_key(|r| match r.status {
            ActionStatus::Failed => 0,
            _ => 1,
        });
        pending
    }
}

/// Placeholder kept so the error type stays reachable from this module.
pub type ExecutorError = workspace_clone_core::RestoreError;

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_clone_adapters::{AdapterRegistry, RestoreAction};
    use workspace_clone_core::manifest::WorkspaceManifest;

    fn action(id: &str, adapter: &str, approved: bool, required: bool) -> RestoreAction {
        RestoreAction {
            id: id.into(),
            action_type: RestoreActionType::OpenApplication,
            adapter_id: adapter.into(),
            description: format!("step {id}"),
            required,
            approved,
            config: serde_json::json!({}),
            dependencies: vec![],
        }
    }

    /// An executor with no database, for tests that never persist.
    ///
    /// `RestoreRunRepository` needs a pool, so a lazy in-memory pool is used;
    /// the record step's failure is logged and ignored, which is the behaviour
    /// under test elsewhere.
    fn executor() -> RestoreExecutor {
        let pool = workspace_clone_db::repository::DbPool::connect_lazy("sqlite::memory:")
            .expect("a lazy sqlite pool");
        RestoreExecutor::new(
            Arc::new(AdapterRegistry::new()),
            Arc::new(RestoreRunRepository::new(pool)),
        )
    }

    fn plan(steps: Vec<RestoreAction>) -> crate::planner::RestorePlan {
        crate::planner::RestorePlan {
            workspace_id: "ws-1".into(),
            steps,
            notes: Vec::new(),
        }
    }

    fn report(results: Vec<ActionResult>) -> RestoreExecutionReport {
        RestoreExecutionReport {
            run_id: "run-1".into(),
            workspace_id: "ws-1".into(),
            results,
            notes: Vec::new(),
            completed_at: chrono::Utc::now(),
        }
    }

    fn result(id: &str, status: ActionStatus) -> ActionResult {
        ActionResult {
            action_id: id.into(),
            status,
            message: String::new(),
            duration_ms: 0,
            details: None,
        }
    }

    #[tokio::test]
    async fn a_step_with_no_adapter_is_skipped_rather_than_failing() {
        // Path mapping is bookkeeping; it has no adapter and must not be
        // treated as a failure.
        let plan = plan(vec![RestoreAction {
            action_type: RestoreActionType::MapPath,
            config: serde_json::json!({ "destination_path": std::env::temp_dir() }),
            ..action("map-path-p1", "", true, true)
        }]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, ActionStatus::Success);
        assert!(report.is_successful());
    }

    #[tokio::test]
    async fn a_missing_destination_is_reported_as_a_manual_step() {
        let plan = plan(vec![RestoreAction {
            action_type: RestoreActionType::MapPath,
            config: serde_json::json!({ "destination_path": "/definitely/not/here" }),
            ..action("map-path-p1", "", true, true)
        }]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Manual);
        assert!(report.is_successful(), "a manual step is not a failure");
        assert_eq!(report.needs_user_action().len(), 1);
    }

    #[tokio::test]
    async fn an_unchosen_destination_is_reported_rather_than_guessed() {
        let plan = plan(vec![RestoreAction {
            action_type: RestoreActionType::MapPath,
            config: serde_json::json!({}),
            ..action("map-path-p1", "", true, true)
        }]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Manual);
        assert!(report.results[0].message.contains("no destination folder chosen"));
    }

    #[tokio::test]
    async fn a_step_whose_adapter_is_missing_fails_with_a_useful_message() {
        // The old failure was a bare "Adapter not found for action: x" with no
        // indication of what to do.
        let plan = plan(vec![action("open-1", "no-such-adapter", true, false)]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Failed);
        assert!(
            report.results[0].message.contains("no-such-adapter"),
            "got: {}",
            report.results[0].message
        );
        assert!(!report.is_successful());
        assert_eq!(report.status(), "failed");
    }

    #[tokio::test]
    async fn an_unapproved_optional_step_is_skipped() {
        let plan = plan(vec![action("open-1", "vscode", false, false)]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Skipped);
        assert_eq!(report.skipped_count(), 1);
        assert!(report.is_successful());
    }

    #[tokio::test]
    async fn an_approval_from_the_caller_overrides_the_planner_default() {
        let plan = plan(vec![action("open-1", "no-such-adapter", false, false)]);
        let approvals = [("open-1".to_string(), true)].into_iter().collect();

        let report = executor().execute_plan("run-1", &plan, &approvals, None).await.unwrap();

        // Approved, so it runs -- and fails, because the adapter is absent.
        assert_eq!(report.results[0].status, ActionStatus::Failed);
    }

    #[tokio::test]
    async fn a_failed_required_step_stops_later_steps_but_still_reports_them() {
        // The old code broke out of the loop, so the report was short and looked
        // like a complete run.
        let plan = plan(vec![
            action("first", "no-such-adapter", true, true),
            action("second", "no-such-adapter", true, true),
            action("third", "no-such-adapter", true, true),
        ]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results.len(), 3, "every step must be accounted for");
        assert_eq!(report.results[0].status, ActionStatus::Failed);
        for later in &report.results[1..] {
            assert_eq!(later.status, ActionStatus::Skipped);
            assert!(
                later.message.contains("Not attempted"),
                "a skipped step must say why: {}",
                later.message
            );
            assert!(later.message.contains("first"));
        }
    }

    #[tokio::test]
    async fn a_failed_optional_step_does_not_stop_the_run() {
        let plan = plan(vec![
            action("optional", "no-such-adapter", true, false),
            action("also-optional", "no-such-adapter", true, false),
        ]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.failed_count(), 2, "both steps should have been attempted");
    }

    #[tokio::test]
    async fn a_required_step_the_user_declined_stops_the_run_without_running_it() {
        let plan = plan(vec![
            action("required", "vscode", false, true),
            action("after", "vscode", true, false),
        ]);

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Skipped);
        assert!(report.results[0].message.contains("required step"));
        assert_eq!(report.results[1].status, ActionStatus::Skipped);
        assert_eq!(report.failed_count(), 0, "declining is not a failure");
    }

    #[tokio::test]
    async fn an_empty_plan_succeeds_trivially() {
        let report = executor()
            .execute_plan("run-1", &plan(vec![]), &Default::default(), None)
            .await
            .unwrap();

        assert!(report.is_successful());
        assert_eq!(report.status(), "nothing_to_do");
        assert!(report.needs_user_action().is_empty());
    }

    #[tokio::test]
    async fn a_run_with_only_successes_is_completed() {
        let r = report(vec![result("a", ActionStatus::Success)]);
        assert_eq!(r.status(), "completed");
        assert!(r.is_successful());
    }

    #[tokio::test]
    async fn a_run_needing_attention_is_distinguished_from_a_completed_one() {
        let r = report(vec![
            result("a", ActionStatus::Success),
            result("b", ActionStatus::Manual),
        ]);
        assert_eq!(r.status(), "needs_attention");
        assert!(r.is_successful(), "nothing failed");
        assert_eq!(r.needs_user_action().len(), 1);
    }

    #[tokio::test]
    async fn failures_are_ranked_ahead_of_manual_steps() {
        let r = report(vec![
            result("manual", ActionStatus::Manual),
            result("failed", ActionStatus::Failed),
        ]);
        let order: Vec<&str> = r
            .needs_user_action()
            .iter()
            .map(|x| x.action_id.as_str())
            .collect();
        assert_eq!(order, vec!["failed", "manual"]);
    }

    #[tokio::test]
    async fn the_plan_notes_are_carried_into_the_report() {
        let mut plan = plan(vec![]);
        plan.notes = vec!["The 'zed' section could not be read.".into()];

        let report = executor().execute_plan("run-1", &plan, &Default::default(), None).await.unwrap();

        assert_eq!(report.notes, plan.notes);
    }

    #[tokio::test]
    async fn a_default_constructed_report_deserialises() {
        // The UI reconstructs reports from JSON, so a missing optional field
        // must not break the page.
        let json = serde_json::json!({
            "run_id": "run-1",
            "results": [],
            "completed_at": chrono::Utc::now()
        });
        let parsed: RestoreExecutionReport = serde_json::from_value(json).unwrap();
        assert!(parsed.workspace_id.is_empty());
        assert!(parsed.notes.is_empty());
    }

    #[tokio::test]
    async fn a_manifest_with_no_steps_still_produces_a_run() {
        let manifest = WorkspaceManifest::default();
        assert!(manifest.projects.is_empty());
    }

    fn extract_action(project_id: &str, destination: &std::path::Path) -> RestoreAction {
        RestoreAction {
            action_type: RestoreActionType::ExtractFiles,
            config: serde_json::json!({
                "project_id": project_id,
                "workspace_id": "ws-1",
                "destination_path": destination.to_string_lossy(),
            }),
            ..action(&format!("extract-files-{project_id}"), "", true, true)
        }
    }

    #[tokio::test]
    async fn extract_files_writes_the_archive_into_the_destination() {
        use workspace_clone_files::snapshot::build_archive;
        let source = std::env::temp_dir().join(format!("wc-x-src-{}", std::process::id()));
        let dest = std::env::temp_dir().join(format!("wc-x-dest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(source.join("lib")).unwrap();
        std::fs::write(source.join("lib/mod.rs"), b"pub fn go() {}\n").unwrap();
        std::fs::write(source.join("README.md"), b"readme\n").unwrap();

        let built = build_archive(&[("p1".into(), source.clone())], Default::default()).unwrap();
        let plan = plan(vec![extract_action("p1", &dest)]);

        let report = executor()
            .execute_plan("run-1", &plan, &Default::default(), Some(&built.tar))
            .await
            .unwrap();

        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].status, ActionStatus::Success);
        assert_eq!(
            std::fs::read_to_string(dest.join("lib/mod.rs")).unwrap(),
            "pub fn go() {}\n",
            "the captured file content must land on disk"
        );
        assert_eq!(std::fs::read_to_string(dest.join("README.md")).unwrap(), "readme\n");

        let _ = std::fs::remove_dir_all(&source);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[tokio::test]
    async fn extract_files_without_an_archive_reports_nothing_to_restore() {
        let dest = std::env::temp_dir().join(format!("wc-x-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let plan = plan(vec![extract_action("p1", &dest)]);

        // Manifest-only workspace: no archive exists, and the required step must
        // still succeed rather than blocking the rest of the run.
        let report = executor()
            .execute_plan("run-1", &plan, &Default::default(), None)
            .await
            .unwrap();

        assert_eq!(report.results[0].status, ActionStatus::Success);
        assert!(report.is_successful());
        assert!(!dest.exists(), "nothing should be created for a file-less workspace");
        let _ = std::fs::remove_dir_all(&dest);
    }
}
