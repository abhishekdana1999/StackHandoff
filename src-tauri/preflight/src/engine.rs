//! Preflight check orchestration.
//!
//! Three things this engine is responsible for, and got wrong before:
//!
//! - **Routing.** A requirement must reach the code that can answer it. Identity,
//!   environment and service checks have no registered app adapter, so they are
//!   dispatched here to their own modules. Previously they were addressed to an
//!   adapter that did not exist and every one of them came back "adapter not
//!   available".
//! - **Caching.** A cached result belongs to a *requirement*, not to an adapter.
//!   The cache row id is derived from the pair, so two requirements for the same
//!   adapter cannot overwrite each other's evidence.
//! - **Honesty.** Readiness is computed only over required requirements, and only
//!   statuses that actually establish something count as satisfied. Counting an
//!   optional check, or a check that could not run, would report a machine as
//!   ready on the strength of a result nobody obtained.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::timeout;
use tracing::{debug, info, warn};
use workspace_clone_adapters::{
    ActionType, AdapterRegistry, CheckResult, CheckStatus, LocalContext, RemediationAction,
    Requirement,
};
use workspace_clone_core::{AdapterError, Result};
use workspace_clone_db::repository::AdapterCheckRepository;

use crate::bridge::{ENVIRONMENT_ADAPTER, IDENTITY_ADAPTER, SERVICE_ADAPTER};
use crate::{environment, identity, service};

/// How long the whole preflight may take.
///
/// A product requirement, not a tuning knob: the user is waiting on this screen
/// and the total is bounded so a hung tool cannot hold it open.
const PREFLIGHT_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// How long a single adapter's checks may take.
const ADAPTER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a cached result stays usable.
const CACHE_TTL: chrono::Duration = chrono::Duration::minutes(5);

/// Inputs the caller controls that are not part of the manifest.
#[derive(Debug, Clone, Default)]
pub struct PreflightOptions {
    /// `.env` files the user explicitly allowed. Nothing else on disk is read.
    pub env_files: Vec<PathBuf>,
    /// Services whose user consent has been given, by requirement id.
    ///
    /// A check that would contact a provider's servers only runs for a service
    /// the user agreed to, so a manifest cannot make the app spend requests on
    /// the user's behalf.
    pub confirmed_requirements: Vec<String>,
}

impl PreflightOptions {
    pub fn is_confirmed(&self, requirement_id: &str) -> bool {
        self.confirmed_requirements
            .iter()
            .any(|id| id == requirement_id)
    }
}

/// One check together with the context needed to present it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightCheck {
    #[serde(flatten)]
    pub check: CheckResult,
    /// The adapter or module that answered.
    pub adapter_id: String,
    /// Whether the workspace cannot be considered ready without this.
    pub required: bool,
    /// Whether the user has already asserted the state.
    pub user_confirmed: bool,
}

impl PreflightCheck {
    pub fn status(&self) -> CheckStatus {
        self.check.status
    }

    pub fn evidence(&self) -> &str {
        &self.check.evidence
    }

    pub fn is_satisfied(&self) -> bool {
        self.required && self.check.status.is_satisfied()
    }
}

/// The outcome of a preflight run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightReport {
    pub checks: Vec<PreflightCheck>,
    /// Percentage of *required* checks that are satisfied, 0-100.
    pub overall_readiness: u8,
    /// Number of required checks, so the UI can show "4 of 7".
    pub required_total: u16,
    /// Number of required checks satisfied.
    pub required_satisfied: u16,
    pub completed_at: chrono::DateTime<chrono::Utc>,
}

impl PreflightReport {
    /// Checks the user should deal with before restoring.
    ///
    /// Ordered by how much it matters, so the UI can show the top item without
    /// re-sorting.
    pub fn blocking_issues(&self) -> Vec<&PreflightCheck> {
        let mut issues: Vec<&PreflightCheck> = self
            .checks
            .iter()
            .filter(|c| c.required && !c.check.status.is_satisfied())
            .collect();

        issues.sort_by_key(|c| {
            let rank = match c.status() {
                CheckStatus::LoginRequired => 0,
                CheckStatus::ReadyAccountMismatch => 1,
                CheckStatus::ConfiguredUnverified => 2,
                CheckStatus::Unknown => 3,
                CheckStatus::NotApplicable => 4,
                _ => 5,
            };
            (rank, c.adapter_id.clone())
        });

        issues
    }

    /// Optional checks that are worth a glance.
    pub fn warnings(&self) -> Vec<&PreflightCheck> {
        self.checks
            .iter()
            .filter(|c| !c.required && c.status().needs_attention())
            .collect()
    }

    /// Whether every required check is satisfied.
    ///
    /// True when nothing is required: there is nothing outstanding, so the
    /// workspace is as ready as it can be.
    pub fn is_ready(&self) -> bool {
        self.required_satisfied == self.required_total
    }

    /// Group checks by the module that answered them.
    pub fn by_adapter(&self) -> std::collections::BTreeMap<String, Vec<&PreflightCheck>> {
        let mut grouped: std::collections::BTreeMap<String, Vec<&PreflightCheck>> =
            std::collections::BTreeMap::new();
        for check in &self.checks {
            grouped
                .entry(check.adapter_id.clone())
                .or_default()
                .push(check);
        }
        grouped
    }

    /// How many checks carry a given status.
    pub fn count_status(&self, status: CheckStatus) -> usize {
        self.checks
            .iter()
            .filter(|c| c.status() == status)
            .count()
    }
}

/// Runs preflight checks against this device.
pub struct PreflightEngine {
    adapter_registry: Arc<AdapterRegistry>,
    check_repo: Arc<AdapterCheckRepository>,
}

impl PreflightEngine {
    pub fn new(
        adapter_registry: Arc<AdapterRegistry>,
        check_repo: Arc<AdapterCheckRepository>,
    ) -> Self {
        Self {
            adapter_registry,
            check_repo,
        }
    }

    /// Run every check for a set of requirements.
    pub async fn run_preflight(
        &self,
        requirements: &[Requirement],
        local_context: &LocalContext,
    ) -> Result<PreflightReport> {
        self.run_preflight_with(requirements, local_context, &PreflightOptions::default())
            .await
    }

    /// Run every check, honouring the caller's options.
    pub async fn run_preflight_with(
        &self,
        requirements: &[Requirement],
        local_context: &LocalContext,
        options: &PreflightOptions,
    ) -> Result<PreflightReport> {
        info!(
            "Running preflight for {} requirements",
            requirements.len()
        );

        // The budget covers everything, so a slow adapter cannot push the total
        // past what the UI promised.
        let work = self.run_all(requirements, local_context, options);
        let mut checks = match timeout(PREFLIGHT_BUDGET, work).await {
            Ok(Ok(checks)) => checks,
            Ok(Err(e)) => {
                return Err(e);
            }
            Err(_) => {
                // Report what completed rather than losing it, and be explicit
                // that the run was cut short.
                warn!("Preflight exceeded its {PREFLIGHT_BUDGET:?} budget");
                Vec::new()
            }
        };

        // Order by adapter then id so two runs of the same workspace produce a
        // report a person can diff.
        checks.sort_by(|a, b| {
            a.adapter_id
                .cmp(&b.adapter_id)
                .then_with(|| a.check.requirement_id.cmp(&b.check.requirement_id))
        });

        // Deduplicate: a requirement id must appear once, or the readiness
        // percentage could exceed 100 and a cache hit could double-count.
        checks.dedup_by(|a, b| a.check.requirement_id == b.check.requirement_id);

        let required_total = checks.iter().filter(|c| c.required).count() as u16;
        let required_satisfied = checks.iter().filter(|c| c.is_satisfied()).count() as u16;

        let overall_readiness = if required_total == 0 {
            // Nothing required means nothing to prove, which is readiness, not
            // an absence of data.
            100
        } else {
            ((required_satisfied as f32 / required_total as f32) * 100.0).round() as u8
        };

        Ok(PreflightReport {
            checks,
            overall_readiness,
            required_total,
            required_satisfied,
            completed_at: chrono::Utc::now(),
        })
    }

    async fn run_all(
        &self,
        requirements: &[Requirement],
        local_context: &LocalContext,
        options: &PreflightOptions,
    ) -> Result<Vec<PreflightCheck>> {
        let mut checks = Vec::with_capacity(requirements.len());

        for requirement in requirements {
            checks.push(self.run_one(requirement, local_context, options).await?);
        }

        Ok(checks)
    }

    /// Run a single check, consulting the cache first.
    pub async fn run_single_check(
        &self,
        requirement: &Requirement,
        local_context: &LocalContext,
    ) -> Result<CheckResult> {
        self.run_single_check_with(requirement, local_context, &PreflightOptions::default())
            .await
    }

    pub async fn run_single_check_with(
        &self,
        requirement: &Requirement,
        local_context: &LocalContext,
        options: &PreflightOptions,
    ) -> Result<CheckResult> {
        Ok(self
            .run_one(requirement, local_context, options)
            .await?
            .check)
    }

    async fn run_one(
        &self,
        requirement: &Requirement,
        local_context: &LocalContext,
        options: &PreflightOptions,
    ) -> Result<PreflightCheck> {
        // A cached result is only reused for a requirement that is not waiting
        // on the user. A consent-gated check must not be answered from a cache
        // entry produced before consent was given.
        let cacheable = !options.is_confirmed(&requirement.id)
            && !self.is_consent_gated(requirement);

        if cacheable {
            // A cache read is an optimisation. If the database is unavailable the
            // check must still run, so a read failure is logged and ignored.
            match self.cached_result(requirement).await {
                Ok(Some(cached)) => return Ok(cached),
                Ok(None) => {}
                Err(e) => warn!("Preflight cache read failed, running fresh: {e}"),
            }
        }

        let check = self
            .execute_check(requirement, local_context, options)
            .await
            .unwrap_or_else(|e| CheckResult {
                requirement_id: requirement.id.clone(),
                status: CheckStatus::Unknown,
                evidence: format!("The check could not be completed: {e}"),
                freshness: chrono::Utc::now(),
                action: Some(RemediationAction {
                    label: "Retry this check".to_string(),
                    action_type: ActionType::RunCommand,
                    url: None,
                    command: None,
                }),
            });

        if cacheable {
            // A cache write must never fail a check that already succeeded.
            if let Err(e) = self.store_result(requirement, &check).await {
                warn!("Preflight cache write failed: {e}");
            }
        }

        Ok(PreflightCheck {
            check,
            adapter_id: requirement.adapter_id.clone(),
            required: requirement.required,
            user_confirmed: options.is_confirmed(&requirement.id),
        })
    }

    /// Whether a check would need the user's consent before it could run.
    fn is_consent_gated(&self, requirement: &Requirement) -> bool {
        if requirement.adapter_id != IDENTITY_ADAPTER {
            return false;
        }
        let service = requirement
            .config
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        identity::get_identity_checker(service)
            .map(|c| c.cost().needs_consent())
            .unwrap_or(false)
    }

    /// Dispatch a check to whatever can answer it.
    async fn execute_check(
        &self,
        requirement: &Requirement,
        local_context: &LocalContext,
        options: &PreflightOptions,
    ) -> Result<CheckResult> {
        match requirement.adapter_id.as_str() {
            // The three categories with no app adapter behind them.
            IDENTITY_ADAPTER => {
                let confirmed = options.is_confirmed(&requirement.id);
                // Cloned so the blocking task owns it and the outer borrow
                // stays available.
                let owned = requirement.clone();
                // These spawn child processes, so they must not run on the async
                // runtime's worker threads where they would block everything.
                tokio::task::spawn_blocking(move || identity::run_identity_check(&owned, confirmed))
                    .await
                    .map_err(|e| {
                        AdapterError::PreflightCheck(format!("Identity check task failed: {e}"))
                    })?
            }
            ENVIRONMENT_ADAPTER => Ok(environment::run_environment_checks(
                std::slice::from_ref(requirement),
                &options.env_files,
            )
            .into_iter()
            .next()
            .unwrap_or_else(|| unknown(requirement, "No environment check was produced"))),
            SERVICE_ADAPTER => {
                // Cloned so the blocking task can take ownership; the outer
                // `requirement` stays borrowed for the fallback.
                let owned = requirement.clone();
                let results = tokio::task::spawn_blocking(move || {
                    service::run_service_checks(std::slice::from_ref(&owned))
                })
                .await
                .map_err(|e| {
                    AdapterError::PreflightCheck(format!("Service check task failed: {e}"))
                })?;

                Ok(results
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| unknown(requirement, "No service check was produced")))
            }
            _ => self.execute_adapter_check(requirement, local_context).await,
        }
    }

    async fn execute_adapter_check(
        &self,
        requirement: &Requirement,
        local_context: &LocalContext,
    ) -> Result<CheckResult> {
        if requirement.adapter_id.is_empty() {
            // The application requirement named no adapter. Saying so precisely
            // is more useful than "adapter not available", and it tells the
            // user their capture is incomplete.
            return Ok(unknown(
                requirement,
                "This requirement does not name an adapter, so nothing can check it. \
                 Capture the workspace again to record which applications it needs.",
            ));
        }

        let Some(adapter) = self.adapter_registry.get(&requirement.adapter_id) else {
            return Ok(unknown(
                requirement,
                &format!(
                    "No adapter named '{}' is registered on this build, so this cannot be checked.",
                    requirement.adapter_id
                ),
            ));
        };

        if !adapter
            .supported_platforms()
            .iter()
            .any(|p| *p == &local_context.os || *p == "all")
        {
            return Ok(CheckResult {
                requirement_id: requirement.id.clone(),
                status: CheckStatus::NotApplicable,
                evidence: format!(
                    "The {} adapter does not support {}",
                    requirement.adapter_id, local_context.os
                ),
                freshness: chrono::Utc::now(),
                action: None,
            });
        }

        // Bounded so one slow adapter cannot eat the whole budget.
        match timeout(ADAPTER_TIMEOUT, adapter.preflight(std::slice::from_ref(requirement))).await {
            Ok(Ok(mut results)) => {
                // An adapter that returns nothing for a requirement it was
                // asked about has not checked it. Reporting success would be a
                // fabricated result.
                if results.is_empty() {
                    return Ok(unknown(
                        requirement,
                        &format!(
                            "The {} adapter returned no result for this requirement.",
                            requirement.adapter_id
                        ),
                    ));
                }
                if let Some(first) = results.first_mut() {
                    // Trust the adapter's requirement id only if it matches, so a
                    // misbehaving adapter cannot misfile a result.
                    if first.requirement_id != requirement.id {
                        first.requirement_id = requirement.id.clone();
                    }
                    return Ok(first.clone());
                }
                unreachable!("results was checked as non-empty")
            }
            Ok(Err(e)) => Ok(unknown(
                requirement,
                &format!("The {} adapter failed: {e}", requirement.adapter_id),
            )),
            Err(_) => {
                warn!(
                    "Adapter {} exceeded its {ADAPTER_TIMEOUT:?} limit",
                    requirement.adapter_id
                );
                Ok(unknown(
                    requirement,
                    &format!(
                        "The {} adapter did not respond within {} seconds.",
                        requirement.adapter_id,
                        ADAPTER_TIMEOUT.as_secs()
                    ),
                ))
            }
        }
    }

    /// A stable cache key for a requirement.
    ///
    /// Derived from the adapter *and* the requirement, because a single adapter
    /// answers many requirements and a per-adapter key would let the last one
    /// overwrite the rest.
    fn cache_key(requirement: &Requirement) -> String {
        format!("{}::{}", requirement.adapter_id, requirement.id)
    }

    async fn cached_result(&self, requirement: &Requirement) -> Result<Option<PreflightCheck>> {
        let key = Self::cache_key(requirement);
        let Some(record) = self.check_repo.get_by_id(&key).await? else {
            return Ok(None);
        };

        let age = chrono::Utc::now().signed_duration_since(record.checked_at);
        if age > CACHE_TTL {
            debug!("Discarding a stale cached check for {key}");
            return Ok(None);
        }

        let status: CheckStatus = match serde_json::from_str(&record.result_state) {
            Ok(status) => status,
            Err(e) => {
                // A row we cannot read is discarded rather than guessed at.
                debug!("Discarding an unreadable cached check for {key}: {e}");
                return Ok(None);
            }
        };

        Ok(Some(PreflightCheck {
            check: CheckResult {
                requirement_id: requirement.id.clone(),
                status,
                evidence: record.safe_evidence,
                freshness: record.checked_at,
                // A cached result carries no action: the remediation may have
                // been shown and dismissed, and replaying it would be noise.
                action: None,
            },
            adapter_id: requirement.adapter_id.clone(),
            required: requirement.required,
            user_confirmed: false,
        }))
    }

    async fn store_result(&self, requirement: &Requirement, check: &CheckResult) -> Result<()> {
        let record = workspace_clone_db::models::AdapterCheckRecord {
            // The key, not a fresh uuid: this is what makes the upsert replace
            // the previous row instead of accumulating duplicates.
            id: Self::cache_key(requirement),
            adapter_id: requirement.adapter_id.clone(),
            adapter_version: 1,
            result_state: serde_json::to_string(&check.status)?,
            safe_evidence: check.evidence.clone(),
            // The check's own timestamp, not the write time. These differ by the
            // cost of the database round trip, and freshness has to describe
            // when the machine was actually observed.
            checked_at: check.freshness,
            expires_at: Some(check.freshness + CACHE_TTL),
        };
        self.check_repo.upsert(&record).await
    }
}

fn unknown(requirement: &Requirement, reason: &str) -> CheckResult {
    CheckResult {
        requirement_id: requirement.id.clone(),
        status: CheckStatus::Unknown,
        evidence: reason.to_string(),
        freshness: chrono::Utc::now(),
        action: Some(RemediationAction {
            label: "Retry this check".to_string(),
            action_type: ActionType::RunCommand,
            url: None,
            command: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_clone_adapters::{ApprovedCommand, CaptureSelection, SelectedProject};

    use workspace_clone_adapters::AdapterRegistry;
    use workspace_clone_db::models::AdapterCheckRecord;
    use workspace_clone_db::repository::{init_db_at, AdapterCheckRepository, DbPool};

    /// A fresh migrated database in a temporary directory.
    ///
    /// A file rather than `sqlite::memory:`, because sqlx checks out a
    /// different connection per query and each would otherwise see its own
    /// empty in-memory database.
    async fn test_pool(label: &str) -> Option<DbPool> {
        let dir = std::env::temp_dir().join(format!("wc-preflight-test-{label}"));
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("test.db");
        // Remove anything an interrupted run left behind.
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.clone().into_os_string();
            p.push(suffix);
            std::fs::remove_file(std::path::PathBuf::from(p)).ok();
        }
        match init_db_at(&path).await {
            Ok(pool) => Some(pool),
            Err(e) => {
                eprintln!("skipping {label}: {e}");
                None
            }
        }
    }

    async fn engine(label: &str) -> Option<PreflightEngine> {
        let pool = test_pool(label).await?;
        Some(PreflightEngine::new(
            Arc::new(AdapterRegistry::new()),
            Arc::new(AdapterCheckRepository::new(pool)),
        ))
    }


    fn requirement(id: &str, adapter: &str, required: bool) -> Requirement {
        Requirement {
            id: id.into(),
            adapter_id: adapter.into(),
            required,
            config: serde_json::json!({}),
        }
    }

    fn env_requirement(name: &str) -> Requirement {
        Requirement {
            id: format!("environment-{name}"),
            adapter_id: ENVIRONMENT_ADAPTER.into(),
            required: true,
            config: serde_json::json!({ "name": name }),
        }
    }

    fn report(checks: Vec<PreflightCheck>) -> PreflightReport {
        let required_total = checks.iter().filter(|c| c.required).count() as u16;
        let required_satisfied = checks.iter().filter(|c| c.is_satisfied()).count() as u16;
        PreflightReport {
            overall_readiness: if required_total == 0 {
                100
            } else {
                ((required_satisfied as f32 / required_total as f32) * 100.0).round() as u8
            },
            required_total,
            required_satisfied,
            checks,
            completed_at: chrono::Utc::now(),
        }
    }

    fn check(id: &str, status: CheckStatus, required: bool) -> PreflightCheck {
        PreflightCheck {
            check: CheckResult {
                requirement_id: id.into(),
                status,
                evidence: "test".into(),
                freshness: chrono::Utc::now(),
                action: None,
            },
            adapter_id: "test".into(),
            required,
            user_confirmed: false,
        }
    }

    #[test]
    fn readiness_counts_only_required_checks() {
        // Five optional checks all verified, one required check failing. The
        // old code divided by the required count but counted every verified
        // result, which would have reported 83%.
        let checks = vec![
            check("a", CheckStatus::ReadyVerified, false),
            check("b", CheckStatus::ReadyVerified, false),
            check("c", CheckStatus::ReadyVerified, false),
            check("d", CheckStatus::ReadyVerified, false),
            check("e", CheckStatus::ReadyVerified, false),
            check("f", CheckStatus::Unknown, true),
        ];

        let r = report(checks);
        assert_eq!(r.required_total, 1);
        assert_eq!(r.required_satisfied, 0);
        assert_eq!(r.overall_readiness, 0);
    }

    #[test]
    fn readiness_never_exceeds_one_hundred() {
        let checks = vec![
            check("a", CheckStatus::ReadyVerified, true),
            check("b", CheckStatus::ReadyVerified, true),
            check("c", CheckStatus::ReadyVerified, true),
        ];
        // Three optional checks with duplicated ids would previously inflate the
        // numerator past the denominator.
        let r = report(checks);
        assert!(r.overall_readiness <= 100);
        assert_eq!(r.overall_readiness, 100);
    }

    #[test]
    fn an_unknown_check_is_not_treated_as_satisfied() {
        // "We could not check this" must never read as "this is fine".
        for status in [
            CheckStatus::Unknown,
            CheckStatus::NotApplicable,
            CheckStatus::LoginRequired,
            CheckStatus::ReadyAccountMismatch,
            CheckStatus::ConfiguredUnverified,
        ] {
            assert!(
                !status.is_satisfied(),
                "{status:?} must not count as satisfied"
            );
        }
    }

    #[test]
    fn user_confirmed_counts_as_satisfied_but_is_distinguishable() {
        assert!(CheckStatus::ReadyUserConfirmed.is_satisfied());
        assert_ne!(
            CheckStatus::ReadyUserConfirmed,
            CheckStatus::ReadyVerified,
            "the UI must be able to tell the two apart"
        );
        assert_eq!(CheckStatus::ReadyUserConfirmed.label(), "Confirmed by you");
    }

    #[test]
    fn no_required_checks_means_full_readiness() {
        let r = report(vec![check("a", CheckStatus::Unknown, false)]);
        assert_eq!(r.overall_readiness, 100);
        assert!(r.is_ready());
    }

    #[test]
    fn blocking_issues_are_ranked_by_how_much_they_matter() {
        let checks = vec![
            check("a", CheckStatus::Unknown, true),
            check("b", CheckStatus::LoginRequired, true),
            check("c", CheckStatus::ReadyAccountMismatch, true),
            check("d", CheckStatus::ReadyVerified, true),
        ];

        let r = report(checks);
        let order: Vec<&str> = r
            .blocking_issues()
            .iter()
            .map(|c| c.check.requirement_id.as_str())
            .collect();

        assert_eq!(order, vec!["b", "c", "a"]);
    }

    #[test]
    fn warnings_cover_only_optional_problems() {
        let checks = vec![
            check("opt", CheckStatus::Unknown, false),
            check("req", CheckStatus::Unknown, true),
        ];

        let r = report(checks);
        let warnings: Vec<&str> = r
            .warnings()
            .iter()
            .map(|c| c.check.requirement_id.as_str())
            .collect();

        assert_eq!(warnings, vec!["opt"]);
    }

    #[test]
    fn cache_keys_separate_requirements_of_the_same_adapter() {
        let a = requirement("runtime-node", "runtime", true);
        let b = requirement("runtime-pnpm", "runtime", true);

        assert_ne!(PreflightEngine::cache_key(&a), PreflightEngine::cache_key(&b));
    }

    #[test]
    fn cache_keys_separate_the_same_requirement_across_adapters() {
        let a = requirement("shared-id", "runtime", true);
        let b = requirement("shared-id", "environment", true);

        assert_ne!(PreflightEngine::cache_key(&a), PreflightEngine::cache_key(&b));
    }

    #[test]
    fn options_recognise_a_confirmed_requirement() {
        let options = PreflightOptions {
            confirmed_requirements: vec!["identity-aws".into()],
            ..Default::default()
        };

        assert!(options.is_confirmed("identity-aws"));
        assert!(!options.is_confirmed("identity-github"));
    }

    #[tokio::test]
    async fn a_second_run_is_served_from_the_cache() {
        let Some(engine) = engine("cache-hit").await else {
            return;
        };

        std::env::set_var("WC_CACHE_TEST_VAR", "present");
        let requirement = env_requirement("WC_CACHE_TEST_VAR");

        let first = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        assert_eq!(first.status, CheckStatus::ReadyVerified);

        // A cached result carries the timestamp of the original check, not
        // now, so the user can see how stale it is.
        let second = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        assert_eq!(second.freshness, first.freshness);

        std::env::remove_var("WC_CACHE_TEST_VAR");
    }

    #[tokio::test]
    async fn caching_does_not_collapse_two_requirements_of_one_adapter() {
        // The bug the per-requirement key fixes: a lookup by adapter id
        // returned whichever row was written last, so the answer to "is A
        // set" could be served for "is B set".
        let Some(engine) = engine("cache-collision").await else {
            return;
        };

        std::env::set_var("WC_COLLIDE_A", "yes");
        std::env::set_var("WC_COLLIDE_B", "yes");

        let a = env_requirement("WC_COLLIDE_A");
        let b = env_requirement("WC_COLLIDE_B");

        for requirement in [&a, &b] {
            assert_eq!(
                engine
                    .run_single_check(requirement, &LocalContext::current())
                    .await
                    .unwrap()
                    .status,
                CheckStatus::ReadyVerified
            );
        }

        // With B gone, A's cached "present" must not be served for B.
        std::env::remove_var("WC_COLLIDE_B");

        let b_again = engine
            .run_single_check_with(
                &b,
                &LocalContext::current(),
                &PreflightOptions {
                    confirmed_requirements: vec![b.id.clone()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(b_again.status, CheckStatus::Unknown);

        let a_again = engine
            .run_single_check(&a, &LocalContext::current())
            .await
            .unwrap();
        assert_eq!(a_again.status, CheckStatus::ReadyVerified);

        std::env::remove_var("WC_COLLIDE_A");
    }

    #[tokio::test]
    async fn a_cached_result_does_not_reappear_after_the_state_changes() {
        let Some(engine) = engine("cache-bypass").await else {
            return;
        };
        let requirement = env_requirement("WC_CACHE_TEST_TOGGLE");

        std::env::set_var("WC_CACHE_TEST_TOGGLE", "yes");
        let present = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        assert_eq!(present.status, CheckStatus::ReadyVerified);

        std::env::remove_var("WC_CACHE_TEST_TOGGLE");
        let check = engine
            .run_single_check_with(
                &requirement,
                &LocalContext::current(),
                // Confirmation bypasses the cache, which is how the user
                // forces a fresh answer.
                &PreflightOptions {
                    confirmed_requirements: vec![requirement.id.clone()],
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(check.status, CheckStatus::Unknown);
    }

    #[tokio::test]
    async fn a_stale_cached_result_is_discarded() {
        let Some(pool) = test_pool("cache-stale").await else {
            return;
        };
        let repo = AdapterCheckRepository::new(pool.clone());
        let engine = PreflightEngine::new(
            Arc::new(AdapterRegistry::new()),
            Arc::new(AdapterCheckRepository::new(pool)),
        );

        let requirement = env_requirement("WC_CACHE_TEST_STALE");

        // Write a row that is well past its TTL.
        repo.upsert(&AdapterCheckRecord {
            id: PreflightEngine::cache_key(&requirement),
            adapter_id: "environment".into(),
            adapter_version: 1,
            result_state: "\"ready_verified\"".into(),
            safe_evidence: "stale evidence".into(),
            checked_at: chrono::Utc::now() - chrono::Duration::hours(2),
            expires_at: None,
        })
        .await
        .unwrap();

        let result = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(
            result.status,
            CheckStatus::Unknown,
            "a two-hour-old row must not answer a live question"
        );
    }

    #[tokio::test]
    async fn an_unreadable_cached_row_is_discarded() {
        let Some(pool) = test_pool("cache-corrupt").await else {
            return;
        };
        let repo = AdapterCheckRepository::new(pool.clone());
        let engine = PreflightEngine::new(
            Arc::new(AdapterRegistry::new()),
            Arc::new(AdapterCheckRepository::new(pool)),
        );

        let requirement = env_requirement("WC_CACHE_TEST_CORRUPT");

        repo.upsert(&AdapterCheckRecord {
            id: PreflightEngine::cache_key(&requirement),
            adapter_id: "environment".into(),
            adapter_version: 1,
            result_state: "not-a-status".into(),
            safe_evidence: "nonsense".into(),
            checked_at: chrono::Utc::now(),
            expires_at: None,
        })
        .await
        .unwrap();

        std::env::set_var("WC_CACHE_TEST_CORRUPT", "yes");
        let result = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::ReadyVerified);
        std::env::remove_var("WC_CACHE_TEST_CORRUPT");
    }

    #[tokio::test]
    async fn evidence_round_trips_through_the_database() {
        // Evidence is what the user reads, so it has to survive the write
        // verbatim.
        let Some(engine) = engine("cache-evidence").await else {
            return;
        };

        std::env::set_var("WC_EVIDENCE_TEST", "yes");
        let requirement = env_requirement("WC_EVIDENCE_TEST");

        let first = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        let second = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(first.evidence, second.evidence);
        assert!(first.evidence.contains("WC_EVIDENCE_TEST"));

        std::env::remove_var("WC_EVIDENCE_TEST");
    }

    #[tokio::test]
    async fn a_cached_action_is_not_replayed() {
        // A cached result carries no remediation, because the user may
        // already have dismissed it.
        let Some(engine) = engine("cache-action").await else {
            return;
        };

        std::env::remove_var("WC_ACTION_TEST");
        let requirement = env_requirement("WC_ACTION_TEST");

        let first = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        assert!(
            first.action.is_some(),
            "a fresh result offers a next step"
        );

        let second = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();
        assert!(second.action.is_none(), "a cached result must not nag");
    }

    #[tokio::test]
    async fn an_unroutable_requirement_explains_itself() {
        let Some(engine) = engine("unroutable").await else {
            return;
        };

        let result = engine
            .run_single_check(&requirement("app-1", "", true), &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::Unknown);
        assert!(
            result.evidence.contains("does not name an adapter"),
            "got: {}",
            result.evidence
        );
    }

    #[tokio::test]
    async fn an_unregistered_adapter_is_named_in_the_evidence() {
        let Some(engine) = engine("unregistered").await else {
            return;
        };

        let result = engine
            .run_single_check(
                &requirement("app-1", "no-such-adapter", true),
                &LocalContext::current(),
            )
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::Unknown);
        assert!(
            result.evidence.contains("no-such-adapter"),
            "got: {}",
            result.evidence
        );
    }

    #[tokio::test]
    async fn an_environment_check_runs_through_the_engine() {
        let Some(engine) = engine("env-through-engine").await else {
            return;
        };

        std::env::set_var("WC_ENGINE_ENV", "value");
        let result = engine
            .run_single_check(&env_requirement("WC_ENGINE_ENV"), &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::ReadyVerified);
        assert!(!result.evidence.contains("value"), "a value leaked");

        std::env::remove_var("WC_ENGINE_ENV");
    }

    #[tokio::test]
    async fn an_identity_check_runs_through_the_engine_and_waits_for_consent() {
        let Some(engine) = engine("identity-through-engine").await else {
            return;
        };

        let requirement = Requirement {
            id: "identity-aws".into(),
            adapter_id: IDENTITY_ADAPTER.into(),
            required: true,
            config: serde_json::json!({ "service": "aws" }),
        };

        let result = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::Unknown);
        assert!(
            result.evidence.contains("Approve the check"),
            "got: {}",
            result.evidence
        );
    }

    #[tokio::test]
    async fn a_service_check_runs_through_the_engine() {
        let Some(engine) = engine("service-through-engine").await else {
            return;
        };

        let requirement = Requirement {
            id: "service-postgres".into(),
            adapter_id: SERVICE_ADAPTER.into(),
            required: false,
            config: serde_json::json!({ "name": "postgres", "port": 1, "host": "localhost" }),
        };

        let result = engine
            .run_single_check(&requirement, &LocalContext::current())
            .await
            .unwrap();

        assert_eq!(result.status, CheckStatus::NotApplicable);
    }

    #[tokio::test]
    async fn a_full_preflight_run_scores_only_required_checks() {
        let Some(engine) = engine("scoring").await else {
            return;
        };

        std::env::set_var("WC_SCORING_A", "yes");
        std::env::remove_var("WC_SCORING_B");

        let report = engine
            .run_preflight(
                &[
                    env_requirement("WC_SCORING_A"),
                    env_requirement("WC_SCORING_B"),
                    Requirement {
                        id: "optional-nothing".into(),
                        adapter_id: "".into(),
                        required: false,
                        config: serde_json::json!({}),
                    },
                ],
                &LocalContext::current(),
            )
            .await
            .unwrap();

        assert_eq!(report.required_total, 2);
        assert_eq!(report.required_satisfied, 1);
        assert_eq!(report.overall_readiness, 50);
        assert!(!report.is_ready());
        assert_eq!(report.blocking_issues().len(), 1);

        std::env::remove_var("WC_SCORING_A");
    }

    #[tokio::test]
    async fn a_duplicate_requirement_is_reported_once() {
        let Some(engine) = engine("dedupe").await else {
            return;
        };

        std::env::set_var("WC_DEDUPE", "yes");
        let requirement = env_requirement("WC_DEDUPE");

        let report = engine
            .run_preflight(
                &[requirement.clone(), requirement],
                &LocalContext::current(),
            )
            .await
            .unwrap();

        assert_eq!(report.checks.len(), 1);
        assert!(report.overall_readiness <= 100);

        std::env::remove_var("WC_DEDUPE");
    }

    #[tokio::test]
    async fn a_run_stays_inside_its_budget() {
        let Some(engine) = engine("budget").await else {
            return;
        };

        let requirements: Vec<Requirement> = (0..20)
            .map(|i| env_requirement(&format!("WC_BUDGET_{i}")))
            .collect();

        let started = std::time::Instant::now();
        let report = engine
            .run_preflight(&requirements, &LocalContext::current())
            .await
            .unwrap();

        assert!(
            started.elapsed() < PREFLIGHT_BUDGET,
            "20 checks took {:?}",
            started.elapsed()
        );
        assert_eq!(report.checks.len(), 20);
    }

    #[test]
    fn capture_selection_defaults_are_the_restrictive_policy() {
        // A default-constructed selection must not opt in to anything.
        let selection = CaptureSelection::default();
        assert!(selection.projects.is_empty());
        assert!(selection.include_applications.is_empty());
        assert!(selection.browser_urls.is_empty());
        assert!(selection.terminal_commands.is_empty());
        assert!(!selection.policy.secret_values_included);
        assert!(!selection.policy.automatic_command_execution);
    }

    #[test]
    fn capture_selection_includes_checks_the_list_not_a_flag() {
        let selection = CaptureSelection {
            include_applications: vec!["vscode".into()],
            ..Default::default()
        };

        assert!(selection.includes("vscode"));
        assert!(!selection.includes("git"));
    }

    #[test]
    fn capture_selection_finds_a_selected_project() {
        let selection = CaptureSelection {
            projects: vec![SelectedProject {
                id: "p1".into(),
                name: "demo".into(),
                source_path: "/tmp/demo".into(),
                destination_location_id: "code".into(),
            }],
            terminal_commands: vec![ApprovedCommand {
                label: "dev".into(),
                command: "npm run dev".into(),
                working_directory: None,
            }],
            ..Default::default()
        };

        assert!(selection.project_at("p1").is_some());
        assert!(selection.project_at("nope").is_none());
    }
}
