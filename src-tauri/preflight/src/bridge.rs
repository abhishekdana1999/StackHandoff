//! Translation from the manifest's requirement model to the check model.
//!
//! The manifest describes *what a workspace needs*; the preflight engine works
//! in terms of `Requirement`, which additionally says *which adapter answers
//! it*. Bridging the two in one place matters because the previous conversion
//! lived in a Tauri command and got the routing wrong: it used each
//! application's opaque id as the adapter id, so every application check was
//! dispatched to an adapter named after the application and none of them were
//! found.

use std::collections::BTreeSet;
use workspace_clone_adapters::Requirement;
use workspace_clone_core::manifest::{
    AppRequirement, IdentityRequirement, Requirements, ServiceRequirement,
};

/// The adapter id that routes identity checks.
///
/// Identity checks are not app adapters, so they get their own id and the engine
/// sends them to the identity module. Previously they were routed to an adapter
/// named after the service, which was never registered.
pub const IDENTITY_ADAPTER: &str = "identity";

/// The adapter id that routes environment-presence checks.
pub const ENVIRONMENT_ADAPTER: &str = "environment";

/// The adapter id that routes local service checks.
pub const SERVICE_ADAPTER: &str = "service";

/// Convert a manifest's requirements into engine requirements.
///
/// Deterministic ordering: the report is shown to a human comparing it against a
/// second run, and a stable order is what makes that comparison possible.
pub fn to_check_requirements(requirements: &Requirements) -> Vec<Requirement> {
    let mut out = Vec::new();

    out.extend(requirements.applications.iter().map(app_requirement));
    out.extend(
        requirements
            .runtimes
            .iter()
            .map(|r| runtime_requirement("runtime", &r.name, &r.version, r.required)),
    );
    out.extend(requirements.cli_tools.iter().map(|r| {
        runtime_requirement(
            "runtime",
            &r.name,
            r.version.as_deref().unwrap_or(""),
            r.required,
        )
    }));
    out.extend(requirements.identities.iter().map(identity_requirement));
    out.extend(requirements.services.iter().map(service_requirement));
    out.extend(environment_requirements(&requirements.environment.presence_only));

    out
}

fn app_requirement(app: &AppRequirement) -> Requirement {
    Requirement {
        id: app.id.clone(),
        // The adapter comes from the requirement itself. Falling back to the id
        // would reproduce the original bug, so an app with no adapter keeps an
        // empty id and is reported as unroutable rather than misrouted.
        adapter_id: app.adapter.clone(),
        required: app.required,
        config: app.config.clone(),
    }
}

fn runtime_requirement(
    adapter_id: &str,
    name: &str,
    version: &str,
    required: bool,
) -> Requirement {
    Requirement {
        id: format!("{adapter_id}-{name}"),
        adapter_id: adapter_id.to_string(),
        required,
        config: serde_json::json!({ "name": name, "version": version }),
    }
}

fn identity_requirement(identity: &IdentityRequirement) -> Requirement {
    Requirement {
        id: format!("{IDENTITY_ADAPTER}-{}", identity.service),
        adapter_id: IDENTITY_ADAPTER.to_string(),
        required: identity.required,
        config: serde_json::json!({
            // `service` is what the identity module dispatches on, so it has to
            // be in the config. It used to be only in `adapter_id`, where the
            // identity module never looked.
            "service": identity.service,
            "account_hint": identity.account_hint,
            "project_hint": identity.project_hint,
            "verification": identity.verification,
        }),
    }
}

fn service_requirement(service: &ServiceRequirement) -> Requirement {
    Requirement {
        id: format!("{SERVICE_ADAPTER}-{}", service.name),
        adapter_id: SERVICE_ADAPTER.to_string(),
        required: service.required,
        config: serde_json::json!({
            "name": service.name,
            "host": service.host,
            "port": service.port,
        }),
    }
}

/// One check per named variable, and never a value.
fn environment_requirements(names: &[String]) -> impl Iterator<Item = Requirement> + '_ {
    // Sorted so the report is stable regardless of manifest ordering.
    let unique: BTreeSet<&String> = names.iter().collect();
    unique.into_iter().map(|name| Requirement {
        id: format!("{ENVIRONMENT_ADAPTER}-{name}"),
        adapter_id: ENVIRONMENT_ADAPTER.to_string(),
        required: true,
        config: serde_json::json!({ "name": name }),
    })
}

/// Every distinct adapter id a set of requirements refers to.
///
/// Used by the report so the UI can group by the real owner of a check rather
/// than guessing a category from the requirement id's prefix.
pub fn referenced_adapters(requirements: &[Requirement]) -> BTreeSet<String> {
    requirements
        .iter()
        .map(|r| r.adapter_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_clone_core::manifest::{
        CliToolRequirement, EnvironmentRequirement, RuntimeRequirement,
    };

    fn full_requirements() -> Requirements {
        Requirements {
            applications: vec![
                AppRequirement::new("app-1", "vscode", false),
                AppRequirement::new("app-2", "", true), // no adapter
            ],
            runtimes: vec![RuntimeRequirement {
                name: "node".into(),
                version: ">=18.0.0".into(),
                required: true,
            }],
            cli_tools: vec![CliToolRequirement {
                name: "pnpm".into(),
                version: Some(">=9.0.0".into()),
                required: false,
            }],
            identities: vec![IdentityRequirement {
                service: "github".into(),
                account_hint: Some("octocat".into()),
                project_hint: None,
                verification: "account".into(),
                required: true,
            }],
            environment: EnvironmentRequirement {
                presence_only: vec!["DATABASE_URL".into(), "API_KEY".into()],
                values_included: false,
            },
            services: vec![ServiceRequirement::new("postgres", 5432, false)],
        }
    }

    fn empty_requirements() -> Requirements {
        Requirements {
            applications: Vec::new(),
            runtimes: Vec::new(),
            cli_tools: Vec::new(),
            identities: Vec::new(),
            environment: EnvironmentRequirement::default(),
            services: Vec::new(),
        }
    }

    #[test]
    fn every_requirement_lands_on_a_real_adapter_id() {
        let checks = to_check_requirements(&full_requirements());

        // The bug being fixed: an application check addressed to an adapter
        // named after the application.
        let vscode = checks.iter().find(|c| c.id == "app-1").unwrap();
        assert_eq!(vscode.adapter_id, "vscode");
        assert_ne!(vscode.adapter_id, vscode.id);
    }

    #[test]
    fn an_application_with_no_adapter_is_not_misrouted() {
        let checks = to_check_requirements(&full_requirements());
        let unroutable = checks.iter().find(|c| c.id == "app-2").unwrap();

        assert_eq!(unroutable.adapter_id, "");
        assert!(unroutable.adapter_id != unroutable.id);
    }

    #[test]
    fn identity_checks_route_to_the_identity_module_and_carry_the_service() {
        let checks = to_check_requirements(&full_requirements());
        let identity = checks
            .iter()
            .find(|c| c.adapter_id == IDENTITY_ADAPTER)
            .unwrap();

        assert_eq!(identity.config["service"], "github");
        assert_eq!(identity.config["account_hint"], "octocat");
    }

    #[test]
    fn service_checks_carry_the_port() {
        let checks = to_check_requirements(&full_requirements());
        let service = checks
            .iter()
            .find(|c| c.adapter_id == SERVICE_ADAPTER)
            .unwrap();

        assert_eq!(service.config["port"], 5432);
        assert_eq!(service.config["host"], "localhost");
    }

    #[test]
    fn environment_names_become_one_check_each() {
        let checks = to_check_requirements(&full_requirements());
        let env: Vec<_> = checks
            .iter()
            .filter(|c| c.adapter_id == ENVIRONMENT_ADAPTER)
            .collect();

        assert_eq!(env.len(), 2);
        let names: Vec<&str> = env
            .iter()
            .map(|c| c.config["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"DATABASE_URL"));
        assert!(names.contains(&"API_KEY"));
    }

    #[test]
    fn duplicate_environment_names_collapse() {
        let requirements = Requirements {
            environment: EnvironmentRequirement {
                presence_only: vec!["A".into(), "A".into(), "B".into()],
                values_included: false,
            },
            ..empty_requirements()
        };

        let env = to_check_requirements(&requirements)
            .into_iter()
            .filter(|c| c.adapter_id == ENVIRONMENT_ADAPTER)
            .count();

        assert_eq!(env, 2);
    }

    #[test]
    fn conversion_is_deterministic() {
        let requirements = full_requirements();
        let first = to_check_requirements(&requirements);
        let second = to_check_requirements(&requirements);

        let ids = |checks: &[Requirement]| -> Vec<String> {
            checks.iter().map(|c| c.id.clone()).collect()
        };
        assert_eq!(ids(&first), ids(&second));
    }

    #[test]
    fn check_ids_are_unique() {
        // Two requirements sharing an id would make the cache return one
        // result for both.
        let checks = to_check_requirements(&full_requirements());
        let mut ids: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();

        assert_eq!(before, ids.len(), "duplicate requirement ids: {ids:?}");
    }

    #[test]
    fn runtime_and_cli_tool_ids_do_not_collide() {
        // Both go to the runtime adapter, so identical names would collide.
        let requirements = Requirements {
            runtimes: vec![RuntimeRequirement {
                name: "node".into(),
                version: ">=18".into(),
                required: true,
            }],
            cli_tools: vec![CliToolRequirement {
                name: "node".into(),
                version: None,
                required: false,
            }],
            ..empty_requirements()
        };

        let checks = to_check_requirements(&requirements);
        let ids: Vec<&str> = checks.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["runtime-node", "runtime-node"]);
        assert_eq!(checks[0].config["version"], ">=18");
        assert_eq!(checks[1].config["version"], "");
    }

    #[test]
    fn an_empty_requirement_set_produces_nothing() {
        assert!(to_check_requirements(&empty_requirements()).is_empty());
    }

    #[test]
    fn an_old_manifest_without_ports_still_loads() {
        // Backwards compatibility for manifests written before `port` existed.
        let requirements: Requirements = serde_json::from_value(serde_json::json!({
            "applications": [{ "id": "app-1", "required": true }],
            "runtimes": [], "cli_tools": [], "identities": [],
            "environment": { "presence_only": ["A"] },
            "services": [{ "name": "postgres", "required": false }]
        }))
        .unwrap();

        assert_eq!(requirements.applications[0].adapter, "");
        assert!(!requirements.applications[0].is_checkable());
        assert!(!requirements.services[0].is_checkable());
        assert!(!requirements.environment.values_included);
    }
}
