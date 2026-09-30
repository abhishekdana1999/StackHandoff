//! Local service availability checks.
//!
//! Blueprint: "Local service availability and port status. Do not scan unrelated
//! hosts or networks."
//!
//! That restriction is enforced structurally rather than by convention: every
//! address in this module is resolved to loopback, so a manifest cannot use this
//! check as a port scanner pointed at a LAN, a public host, or a private address
//! on another machine.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;
use tracing::debug;
use workspace_clone_adapters::{CheckResult, CheckStatus, RemediationAction, Requirement};

/// Per-connection timeout. Short enough that ten checks stay well inside the
/// preflight budget even when every port is closed.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(750);

/// Ports a user may reasonably want running locally, with friendly names.
const WELL_KNOWN: &[(u16, &str)] = &[
    (3000, "development server"),
    (4200, "Angular development server"),
    (5173, "Vite development server"),
    (5432, "PostgreSQL"),
    (6379, "Redis"),
    (8000, "Python / API development server"),
    (8080, "local web service"),
    (8443, "local HTTPS service"),
    (9000, "PHP development server"),
    (27017, "MongoDB"),
    (54321, "Supabase local stack"),
];

/// Reduce a host to loopback, or refuse it.
///
/// A hostname is resolved and its addresses are filtered down to loopback
/// only. If nothing loopback remains the host is refused outright. This is the
/// single place a host name is interpreted, so there is no second path that
/// could reach a remote address.
fn loopback_only(host: &str, port: u16) -> Option<SocketAddr> {
    let ip: IpAddr = match host.parse() {
        Ok(ip) => ip,
        Err(_) => {
            // A name. Accept only names that are unambiguously local.
            let lowered = host.to_ascii_lowercase();
            let resolves_to_loopback = matches!(
                lowered.as_str(),
                "localhost" | "localhost.localdomain" | "ip6-localhost" | "ip6-loopback"
            );
            if !resolves_to_loopback {
                debug!("Refusing non-local host for a service check: {host}");
                return None;
            }
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        }
    };

    // Anything that is not loopback is out of scope, whatever produced it.
    if !ip.is_loopback() {
        debug!("Refusing non-loopback address for a service check: {ip}");
        return None;
    }

    Some(SocketAddr::new(
        // Prefer IPv4 so a check against `localhost` behaves the same on a
        // machine that has no IPv6 route.
        match ip {
            IpAddr::V6(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            other => other,
        },
        port,
    ))
}

/// Describe a port in a way that helps the user act.
fn label_for(port: u16) -> Option<&'static str> {
    WELL_KNOWN
        .iter()
        .find(|(p, _)| *p == port)
        .map(|(_, name)| *name)
}

/// Run the service checks in `requirements`.
pub fn run_service_checks(requirements: &[Requirement]) -> Vec<CheckResult> {
    requirements.iter().map(check_one).collect()
}

fn check_one(requirement: &Requirement) -> CheckResult {
    let now = chrono::Utc::now();

    let config = &requirement.config;
    let host = config
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("localhost");

    let Some(port) = config.get("port").and_then(|v| v.as_u64()) else {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: "The requirement does not name a port, so availability cannot be checked."
                .to_string(),
            freshness: now,
            action: None,
        };
    };

    if port == 0 || port > u16::MAX as u64 {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::Unknown,
            evidence: format!("Port {port} is not a valid port number"),
            freshness: now,
            action: None,
        };
    }
    let port = port as u16;

    let Some(addr) = loopback_only(host, port) else {
        return CheckResult {
            requirement_id: requirement.id.clone(),
            status: CheckStatus::NotApplicable,
            evidence: format!(
                "{host} is not a local address, so this check does not apply. \
                 StackHandoff checks local services only and never contacts remote hosts."
            ),
            freshness: now,
            action: None,
        };
    };

    let reachable = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok();
    let label = label_for(port);

    let (status, evidence) = if reachable {
        (
            CheckStatus::ReadyVerified,
            match label {
                Some(name) => format!("Something is listening on {addr} ({name})"),
                None => format!("Something is listening on {addr}"),
            },
        )
    } else {
        (
            CheckStatus::NotApplicable,
            match label {
                Some(name) => format!("Nothing is listening on {addr} ({name} is not running)"),
                None => format!("Nothing is listening on {addr}"),
            },
        )
    };

    CheckResult {
        requirement_id: requirement.id.clone(),
        status,
        // A TCP connect proves a listener exists, nothing more. Saying "a
        // listener is accepting connections" rather than "the service is
        // healthy" keeps the claim no stronger than the evidence.
        evidence,
        freshness: now,
        action: (!reachable).then(|| RemediationAction {
            label: match label {
                Some(name) => format!("Start the {name}"),
                None => format!("Start the service on port {port}"),
            },
            action_type: workspace_clone_adapters::ActionType::RunCommand,
            url: None,
            command: None,
        }),
    }
}

/// Whether an adapter id is handled by this module.
pub fn is_service_adapter(adapter_id: &str) -> bool {
    adapter_id == "service"
}

/// The `::1` form, exposed so the engine can document its scope.
pub const LOOPBACK_V6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);

#[cfg(test)]
mod tests {
    use super::*;
    
    use std::net::TcpListener;

    fn req(config: serde_json::Value) -> Requirement {
        Requirement {
            id: "svc-1".into(),
            adapter_id: "service".into(),
            required: false,
            config,
        }
    }

    #[test]
    fn finds_a_listening_local_service() {
        // Bind an ephemeral port, so the test cannot collide with a real one.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let results = run_service_checks(&[req(serde_json::json!({
            "host": "localhost",
            "port": port
        }))]);

        assert_eq!(results[0].status, CheckStatus::ReadyVerified);
        assert!(results[0].evidence.contains(&port.to_string()));
    }

    #[test]
    fn reports_an_absent_local_service_as_not_applicable() {
        // Port 1 is privileged and unbound on a normal machine.
        let results = run_service_checks(&[req(serde_json::json!({ "port": 1 }))]);

        assert_eq!(results[0].status, CheckStatus::NotApplicable);
        assert!(results[0].evidence.contains("Nothing is listening"));
    }

    #[test]
    fn refuses_to_probe_a_remote_host() {
        // The important safety property: a manifest cannot point this check at
        // someone else's machine.
        for host in [
            "example.com",
            "192.168.1.10",
            "10.0.0.1",
            "8.8.8.8",
            "169.254.169.254", // cloud metadata
            "[::1]",
        ] {
            let results = run_service_checks(&[req(serde_json::json!({ "host": host, "port": 80 }))]);
            assert_eq!(
                results[0].status,
                CheckStatus::NotApplicable,
                "{host} should have been refused"
            );
            assert!(
                results[0].evidence.contains("never contacts remote hosts"),
                "{host}: {}",
                results[0].evidence
            );
        }
    }

    #[test]
    fn accepts_the_standard_loopback_names() {
        for host in ["localhost", "LOCALHOST", "127.0.0.1", "127.0.0.2"] {
            let addr = loopback_only(host, 3000);
            assert!(addr.is_some(), "{host} should be accepted");
            assert!(addr.unwrap().ip().is_loopback());
        }
    }

    #[test]
    fn rejects_an_invalid_port() {
        for port in [serde_json::json!(0), serde_json::json!(70000)] {
            let results = run_service_checks(&[req(serde_json::json!({ "port": port }))]);
            assert_eq!(results[0].status, CheckStatus::Unknown);
        }
    }

    #[test]
    fn rejects_a_requirement_with_no_port() {
        let results = run_service_checks(&[req(serde_json::json!({}))]);
        assert_eq!(results[0].status, CheckStatus::Unknown);
        assert!(results[0].evidence.contains("does not name a port"));
    }

    #[test]
    fn names_well_known_services_when_it_can() {
        let results = run_service_checks(&[req(serde_json::json!({ "port": 5432 }))]);
        assert!(
            results[0].evidence.contains("PostgreSQL"),
            "got {}",
            results[0].evidence
        );
    }

    #[test]
    fn checks_complete_within_the_preflight_budget() {
        // Ten closed ports must not take anywhere near the 30s preflight limit.
        let started = std::time::Instant::now();
        let requirements: Vec<Requirement> = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10]
            .iter()
            .map(|p| req(serde_json::json!({ "port": p })))
            .collect();

        let results = run_service_checks(&requirements);

        assert_eq!(results.len(), 10);
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "ten checks took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_refused_port_still_offers_a_next_step() {
        let results = run_service_checks(&[req(serde_json::json!({ "port": 6379 }))]);
        let action = results[0].action.as_ref().unwrap();
        assert!(action.label.contains("Redis"), "got {}", action.label);
    }

    #[test]
    fn an_open_port_offers_nothing_to_do() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let results = run_service_checks(&[req(serde_json::json!({ "port": port }))]);
        assert!(results[0].action.is_none());
    }

    #[test]
    fn loopback_v6_is_loopback() {
        assert!(LOOPBACK_V6.is_loopback());
    }
}
