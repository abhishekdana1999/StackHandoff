//! Tauri commands for Workspace Clone

pub mod app;
pub mod capture;
pub mod device;
pub mod preflight;
pub mod projects;
pub mod receive;
pub mod restore;
pub mod settings;
pub mod transfer;
pub mod workspace;

use std::sync::Arc;
use tauri::Manager;
use tracing::{info, warn};
use workspace_clone_crypto::keys::KeyStorage;
use workspace_clone_db::repository::DeviceRepository;

/// Initialise every long-lived service the commands need.
///
/// Ordering matters here and is not incidental:
///
/// 1. The database, because everything else records into it.
/// 2. This device's own key bundle, because the network services are constructed
///    from a Noise key that has to exist before the first connection, and because
///    `workspaces.source_device_id` is a foreign key -- a capture cannot be
///    persisted unless this device is already a row.
/// 3. The transfer listener, before discovery, so the port discovery advertises
///    is the port something is actually listening on. The other order advertises a
///    port nobody is on.
pub fn init(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let db_pool = tauri::async_runtime::block_on(workspace_clone_db::init_db())?;
    app.manage(workspace_clone_db::DbPool::clone(&db_pool));

    // Created on first run. Failing here is fatal on purpose: an app that can
    // capture but cannot seal, or cannot be identified by a peer, has no useful
    // mode.
    let bundle = KeyStorage::load_or_create_local_keys()?;
    let noise_key = bundle.noise_key()?;
    let noise_key_b64 = noise_key.public_key_b64();

    // This device's id is a fingerprint over its *Noise* key, and that is
    // deliberate rather than incidental. An id has to be computable by a peer,
    // and a peer's only key is the one discovery advertises in order to
    // complete a handshake. Deriving the id from the Ed25519 identity key
    // instead would make it something no peer can ever reproduce, so
    // `send_workspace`'s lookup of a discovered device by id could never match
    // -- every send would fail with "no device is reachable" however paired the
    // two machines were.
    //
    // The Ed25519 key is still kept on the row, and still signed with where
    // signing happens; it just is not the identity a peer looks up.
    let device_id =
        workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&noise_key_b64)?;

    let local_device = tauri::async_runtime::block_on(async {
        DeviceRepository::new(workspace_clone_db::DbPool::clone(&db_pool))
            .ensure_local(&workspace_clone_db::models::DeviceRecord {
                id: device_id.clone(),
                name: hostname(),
                public_key: bundle.public_key_b64()?,
                noise_public_key: noise_key_b64.clone(),
                // Same value as the id. A fingerprint that differed from the id
                // would be a second name for the same device, and one of the two
                // would be wrong.
                fingerprint: device_id.clone(),
                // This device is not a peer, so it grants itself nothing. Trust
                // scopes describe what a *paired* device may do, and a self-row
                // with a scope would be a claim nobody granted.
                trust_scopes: "[]".to_string(),
                os: std::env::consts::OS.to_string(),
                os_version: os_version(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                created_at: chrono::Utc::now(),
                last_seen: Some(chrono::Utc::now()),
                revoked: false,
                revoked_at: None,
            })
            .await
    })?;
    info!("This device is {device_id} on {}", local_device.name);

    // Initialize adapter registry
    let mut adapter_registry = workspace_clone_adapters::AdapterRegistry::new();
    adapter_registry.register(Box::new(workspace_clone_adapters::GitAdapter::new()));
    adapter_registry.register(Box::new(workspace_clone_adapters::VSCodeAdapter::new()));
    adapter_registry.register(Box::new(workspace_clone_adapters::BrowserAdapter::new()));
    adapter_registry.register(Box::new(workspace_clone_adapters::TerminalAdapter::new()));
    adapter_registry.register(Box::new(workspace_clone_adapters::RuntimeAdapter::new()));
    let adapter_registry = Arc::new(adapter_registry);
    app.manage(adapter_registry.clone());

    // Initialize preflight engine
    let check_repo = Arc::new(
        workspace_clone_db::repository::AdapterCheckRepository::new(db_pool.clone()),
    );
    let preflight_engine = Arc::new(workspace_clone_preflight::engine::PreflightEngine::new(
        adapter_registry.clone(),
        check_repo,
    ));
    app.manage(preflight_engine);

    // Initialize restore planner
    let restore_planner = Arc::new(
        workspace_clone_restore::planner::RestorePlanner::new(adapter_registry.clone()),
    );
    app.manage(restore_planner);

    // Initialize restore executor
    let run_repo = Arc::new(workspace_clone_db::repository::RestoreRunRepository::new(
        db_pool.clone(),
    ));
    let restore_executor = Arc::new(
        workspace_clone_restore::executor::RestoreExecutor::new(adapter_registry.clone(), run_repo),
    );
    app.manage(restore_executor);

    // Network services. The listener is bound before discovery advertises a port,
    // so the port peers are told about is one that answers.
    let mut transfer = workspace_clone_network::transfer::TransferService::new(
        noise_key.clone(),
        device_id.clone(),
        workspace_clone_network::transfer::TransferConfig::default(),
    );
    // A failure to bind is reported but not fatal. The app is still useful
    // offline -- capture, preflight and restore of a workspace already on this
    // machine all work without a network -- and a hard failure here would leave
    // the user with a blank window and no way to reach a workspace they already
    // have.
    let bound_port = match tauri::async_runtime::block_on(transfer.start(0)) {
        Ok(port) => {
            info!("Listening for incoming transfers on port {port}");
            port
        }
        Err(e) => {
            warn!("Could not open the transfer listener; transfers are unavailable: {e}");
            0
        }
    };

    // Taken before the service is moved into the shared state, because a
    // `TransferReceiver` is what the accept loop needs and the only way to get
    // one is from the service that owns the bound listener.
    let receiver = transfer.receiver();

    let discovery = workspace_clone_network::DiscoveryService::new(
        device_id.clone(),
        local_device.name.clone(),
        &noise_key_b64,
    );

    let state = transfer::NetworkState {
        discovery: tokio::sync::Mutex::new(Some(discovery)),
        transfer: tokio::sync::Mutex::new(Some(Arc::new(tokio::sync::Mutex::new(transfer)))),
        bound_port,
    };
    app.manage(state);

    // Accept incoming transfers for as long as the app runs. Started here, after
    // the listener is bound and before discovery announces this device -- so a
    // peer that finds this device connects to a listener that is already being
    // served, rather than to a port that accepts and then goes unanswered.
    //
    // Without this the app could send and never receive. A connection sat in the
    // kernel's queue until the sender's frame timeout, which is the worst
    // possible failure for the thing this app exists to do: it looks like a
    // network problem, and nothing on either machine says what went wrong.
    //
    // A `TransferReceiver` holds a handle to the same bound socket and a clone of
    // the local static key, so this task accepts while commands keep sending
    // over the same port. It is not the service behind the send lock: holding
    // that for the length of a transfer would block every send, every
    // discovered-device lookup and every cancel request until it finished.
    let incoming = receive::IncomingState::new();
    match receiver {
        Ok(receiver) => receive::spawn_accept_loop(
            receiver,
            workspace_clone_db::DbPool::clone(&db_pool),
            incoming.clone(),
            device_id.clone(),
        ),
        Err(e) => warn!("Not accepting incoming transfers: {e}"),
    }
    app.manage(incoming);

    Ok(())
}

/// The machine's name, for display.
///
/// Falls back to something stable rather than "unknown device" so two machines
/// are distinguishable in a device list.
fn hostname() -> String {
    // `uname` does not exist on Windows, so it is only ever a macOS/Linux path.
    let uname = std::process::Command::new("uname")
        .arg("-n")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    resolve_hostname(
        std::env::var("HOSTNAME").ok().as_deref(),
        uname.as_deref(),
        std::env::var("COMPUTERNAME").ok().as_deref(),
    )
}

/// The platform-independent half of [`hostname`].
///
/// Split out so the fallbacks can be tested directly. Reading the process
/// environment is global mutable state, and a test that sets `HOSTNAME` to
/// prove the Windows path is denied it would race every other test in the
/// binary -- which is exactly the class of bug that hides behind a green suite.
fn resolve_hostname(
    hostname_env: Option<&str>,
    uname_output: Option<&str>,
    computer_name_env: Option<&str>,
) -> String {
    // A nested fn rather than a closure: a closure capturing nothing still
    // infers a single region for its argument and its return, and the borrow
    // checker rejects handing a short-lived `&str` through it. A function item
    // is generic over the lifetime, so each call site is its own instantiation.
    fn clean(v: Option<&str>) -> Option<&str> {
        v.map(str::trim).filter(|s| !s.is_empty())
    }

    // Order matters only in the impossible case where both are set; the Unix
    // variable first means a Unix host never picks up a stale Windows-style
    // name from its environment.
    clean(hostname_env)
        .or_else(|| clean(computer_name_env))
        .or_else(|| clean(uname_output))
        // Deliberately not "This Mac": a Windows laptop reaching this fallback
        // would be listed as a Mac, and two identically-named machines in the
        // device list is the thing this function exists to prevent.
        .unwrap_or("This device")
        .to_string()
}

/// A best-effort OS version string.
///
/// Deliberately never a failure. It is display metadata, and a machine whose
/// version cannot be read still captures and restores workspaces.
fn os_version() -> String {
    platform_os_version()
}

/// The one platform branch, as its own function per OS.
///
/// These are separate functions rather than stacked `#[cfg]` blocks in one
/// body: with several blocks in a single function, `cfg` stripping happens
/// before type checking, and which block is left in tail position is decided by
/// parse order rather than by intent. One function per platform cannot get that
/// wrong, and each is independently checkable.
#[cfg(target_os = "macos")]
fn platform_os_version() -> String {
    if let Ok(output) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                return version;
            }
        }
    }
    std::env::consts::OS.to_string()
}

/// Windows has no `sw_vers`, and the usual `cmd /c ver` trick yields the
/// badly-stripped "10.0.19045" -- no service pack, nothing a person recognises
/// as a Windows version, and it is a display string in a device list.
///
/// `RUSTVERSION` is read from the environment rather than baked in at compile
/// time, because the OS is upgraded underneath a running app and a constant
/// would go stale.
#[cfg(target_os = "windows")]
fn platform_os_version() -> String {
    match std::env::var("RUSTVERSION") {
        Ok(build) if !build.trim().is_empty() => {
            format!("Windows (build {})", build.trim())
        }
        _ => "Windows".to_string(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_os_version() -> String {
    if let Ok(output) = std::process::Command::new("uname").arg("-r").output() {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !version.is_empty() {
                return version;
            }
        }
    }
    std::env::consts::OS.to_string()
}

/// Re-export command modules
pub use app::*;
pub use capture::*;
pub use device::*;
pub use preflight::*;
pub use receive::*;
pub use restore::*;
pub use settings::*;
pub use transfer::*;
pub use workspace::*;

#[cfg(test)]
mod platform_tests {
    use super::resolve_hostname;

    /// The Windows laptop must name itself, not fall through to a Mac-only
    /// string. `HOSTNAME` is a bash export and does not exist on Windows, and
    /// `uname` is not a Windows executable, so before this was fixed a Windows
    /// machine reached the fallback and was listed as "This Mac" -- which
    /// defeats the stated purpose of the function, since pairing two machines
    /// and telling them apart is the entire task.
    #[test]
    fn windows_machine_reports_its_computer_name() {
        let name = resolve_hostname(
            None,          // HOSTNAME unset on Windows
            None,          // `uname` does not exist there
            Some("ALEX-LAPTOP"),
        );
        assert_eq!(name, "ALEX-LAPTOP");
    }

    #[test]
    fn unix_prefers_the_hostname_variable() {
        let name = resolve_hostname(Some("abhishek-mac"), Some("ignored"), None);
        assert_eq!(name, "abhishek-mac");
    }

    /// Guards the "unreachable in practice, so pick deliberately" case: if
    /// something ever does export both, the Unix name wins, because that is
    /// the host whose native variable it is.
    #[test]
    fn unix_name_wins_when_both_variables_are_present() {
        let name = resolve_hostname(Some("mac-name"), None, Some("PC-NAME"));
        assert_eq!(name, "mac-name");
    }

    #[test]
    fn uname_is_used_when_no_variable_is_set() {
        let name = resolve_hostname(None, Some("mini.local"), None);
        assert_eq!(name, "mini.local");
    }

    #[test]
    fn surrounding_whitespace_is_not_part_of_the_name() {
        let name = resolve_hostname(Some("  padded  \n"), None, None);
        assert_eq!(name, "padded");
    }

    /// A blank variable must be treated as absent, not as a name. An empty
    /// device name is worse than a generic one: it is invisible in a list and
    /// impossible to tell apart from a rendering bug.
    #[test]
    fn empty_and_whitespace_values_are_skipped() {
        assert_eq!(resolve_hostname(Some("   "), Some("real"), None), "real");
        assert_eq!(resolve_hostname(None, Some(""), Some("PC")), "PC");
    }

    /// The last-resort string must not claim a platform, or the fallback
    /// re-introduces exactly the bug these tests exist to prevent.
    #[test]
    fn the_final_fallback_does_not_claim_to_be_a_mac() {
        let name = resolve_hostname(None, None, None);
        assert_eq!(name, "This device");
        assert!(
            !name.to_lowercase().contains("mac"),
            "fallback must not name an OS: got {name:?}"
        );
    }
}
