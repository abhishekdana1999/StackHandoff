//! Two devices, one transfer, end to end.
//!
//! This is the test the app's reason for existing needs: a workspace captured on
//! one machine, sent over a real socket to a second machine, stored there, and
//! readable on the second machine with the second machine's own key.
//!
//! ## What is real and what is not
//!
//! Real: a TCP listener on a real port, a real Noise_IK handshake, the real
//! chunked framing, the real digest check, and the whole command-layer receive
//! path -- authorization, manifest validation, sealing, the database rows, and
//! the arrival the window polls for.
//!
//! Not real: mDNS, and the two peers' discovery records. Discovery is a way of
//! *finding* a device, and the loopback suite in the network crate already covers
//! the transport against a discovered address; what had no coverage at all was
//! everything after the bytes landed, which is exactly what this covers.
//!
//! Not real: the OS credential store. The sealing key is supplied rather than
//! read from the keychain, because a test binary reading an item belonging to
//! the app blocks on a user prompt. What is asserted is the property that
//! matters -- the stored manifest opens with the key that sealed it, and *not*
//! with the sender's -- which is the bug this path was written to fix.
//!
//! ## Why `#[tokio::test]` with a multi-thread runtime
//!
//! The default is a current-thread runtime, and a test whose body is the send
//! would deadlock: the send needs the accept loop to run, and the accept loop
//! cannot run while the send is blocking the only thread. Every test here is
//! therefore `flavor = "multi_thread"`, and the ones that would deadlock
//! otherwise are marked as such at the call site.

use std::sync::Arc;
use std::time::{Duration, Instant};

use workspace_clone_commands::receive::{serve_forever, IncomingState};
use workspace_clone_core::{
    crypto::EncryptionKey,
    device::{DateTimeUtc, DiscoveredDevice, TrustScope},
    manifest::{
        AppRequirement, Application, CliToolRequirement, DeviceRef, EnvironmentRequirement,
        GitInfo, Portability, Project, Requirements, RuntimeRequirement, WorkspaceManifest,
        WorkspaceMeta,
    },
    Result,
};
use workspace_clone_crypto::noise::KeyPair;
use workspace_clone_db::{
    models::DeviceRecord,
    repository::{
        init_db_at, DeviceRepository, TransferSessionRepository, WorkspaceFilesRepository,
        WorkspaceRepository,
    },
    DbPool,
};
use workspace_clone_network::transfer::{TransferService, TransferStatus};

/// A second machine, as far as the receive path is concerned.
///
/// Holds its own database file, its own identity key, its own listener, and its
/// own sealing key. The keychain is the one thing it cannot have -- see the
/// module comment -- and a real credential store adds nothing to what is being
/// asserted here.
struct Machine {
    key: KeyPair,
    device_id: String,
    pool: DbPool,
    _db: DbFile,
    service: Arc<tokio::sync::Mutex<TransferService>>,
    port: u16,
    /// This machine's own manifest-sealing key.
    storage_key: EncryptionKey,
    arrivals: IncomingState,
}

/// A copy of the arrival fields the assertions read.
///
/// Taken by value rather than read under the lock, so a test cannot hold a tokio
/// mutex across an assertion and deadlock the accept loop.
struct ArrivalSnapshot {
    accepted: bool,
    refusal_reason: Option<String>,
    workspace_id: String,
    workspace_name: String,
    sender_device_id: String,
    sender_device_name: String,
    source_device_id: String,
    transfer_digest: String,
    transfer_id: String,
    bytes_received: u64,
}

struct DbFile(std::path::PathBuf);
impl Drop for DbFile {
    fn drop(&mut self) {
        // WAL mode leaves two sidecar files next to the database.
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(format!("{}-wal", self.0.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.0.display()));
    }
}

impl Machine {
    /// Bring up a machine, with its row in its own database.
    async fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "stackhandoff-e2e-{name}-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let pool = init_db_at(&path).await.expect("migrations");

        let key = KeyPair::generate().expect("a static key");
        let key_b64 = key.public_key().to_base64();
        let device_id =
            workspace_clone_core::crypto::fingerprint_from_connection_key_b64(&key_b64).expect("id");

        DeviceRepository::new(pool.clone())
            .create(&DeviceRecord {
                id: device_id.clone(),
                name: name.to_string(),
                public_key: key_b64.clone(),
                noise_public_key: key_b64,
                fingerprint: device_id.clone(),
                // A local row is granted nothing, exactly as `init` writes it.
                trust_scopes: "[]".to_string(),
                os: "macos".to_string(),
                os_version: "15".to_string(),
                app_version: "0.1.0".to_string(),
                created_at: chrono::Utc::now(),
                last_seen: Some(chrono::Utc::now()),
                revoked: false,
                revoked_at: None,
            })
            .await
            .expect("the local device row");

        let mut service = TransferService::new(
            key.clone(),
            device_id.clone(),
            workspace_clone_network::transfer::TransferConfig::default(),
        );
        // Port 0: the OS picks a free port, so parallel tests cannot collide.
        let port = service.start(0).await.expect("bind the transfer listener");

        Self {
            key,
            device_id,
            pool,
            _db: DbFile(path),
            service: Arc::new(tokio::sync::Mutex::new(service)),
            port,
            storage_key: EncryptionKey::new(rand::random::<[u8; 32]>()),
            arrivals: IncomingState::new(),
        }
    }

    /// Start accepting, sealing with this machine's own key.
    fn start_accepting(&self) {
        let receiver = self
            .service
            .try_lock()
            .expect("the service is not held during setup")
            .receiver()
            .expect("the listener is open");

        serve_forever(
            receiver,
            self.pool.clone(),
            self.arrivals.clone(),
            self.device_id.clone(),
            Arc::new({
                let key = self.storage_key.clone();
                move || Ok(key.clone())
            }),
        );
    }

    /// How this machine would appear in the other machine's discovery.
    fn as_peer(&self) -> workspace_clone_core::device::DiscoveredDevice {
        DiscoveredDevice {
            device_id: self.device_id.clone(),
            name: "peer".to_string(),
            os: "macos".to_string(),
            app_version: "0.1.0".to_string(),
            protocol_version: 1,
            addresses: vec!["127.0.0.1".to_string()],
            port: self.port,
            capabilities: workspace_clone_core::device::DeviceCapabilities::default(),
            static_public_key: self.key.public_key().to_base64(),
            last_seen: chrono::Utc::now().into(),
        }
    }

    /// Pair `peer` here, with the given trust scopes, as `verify_pairing` would.
    async fn trust(&self, peer: &Machine, scopes: Vec<TrustScope>) {
        DeviceRepository::new(self.pool.clone())
            .create(&DeviceRecord {
                id: peer.device_id.clone(),
                name: "The other machine".to_string(),
                public_key: peer.key.public_key().to_base64(),
                noise_public_key: peer.key.public_key().to_base64(),
                fingerprint: peer.device_id.clone(),
                trust_scopes: serde_json::to_string(&scopes).expect("scopes"),
                os: "windows".to_string(),
                os_version: "11".to_string(),
                app_version: "0.1.0".to_string(),
                created_at: chrono::Utc::now(),
                last_seen: None,
                revoked: false,
                revoked_at: None,
            })
            .await
            .expect("the paired device row");
    }

    /// Wait for the newest arrival, so a test never passes on a timing accident.
    ///
    /// Returns a copy of the arrival rather than a tuple. A tuple assembled with
    /// `?` on `refusal_reason` yields `None` for an *accepted* arrival, which is
    /// indistinguishable from the arrival never having happened -- so a working
    /// listener and a broken one would have produced the same result, and the
    /// happy path could not have passed at all.
    async fn wait_for_arrival(&self, within: Duration) -> Option<ArrivalSnapshot> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(arrival) = self.arrivals.arrivals.lock().await.first() {
                return Some(ArrivalSnapshot {
                    accepted: arrival.accepted,
                    refusal_reason: arrival.refusal_reason.clone(),
                    workspace_id: arrival.workspace_id.clone(),
                    workspace_name: arrival.workspace_name.clone(),
                    sender_device_id: arrival.sender_device_id.clone(),
                    sender_device_name: arrival.sender_device_name.clone(),
                    source_device_id: arrival.source_device_id.clone(),
                    transfer_digest: arrival.transfer_digest.clone(),
                    transfer_id: arrival.transfer_id.clone(),
                    bytes_received: arrival.bytes_received,
                });
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        None
    }
}

/// A manifest a workspace captured on `source` would contain.
fn manifest_for(id: &str, name: &str, source: &Machine) -> WorkspaceManifest {
    WorkspaceManifest {
        schema_version: 1,
        workspace: WorkspaceMeta {
            id: id.to_string(),
            name: name.to_string(),
            captured_at: DateTimeUtc::from(chrono::Utc::now()),
            source_device: DeviceRef {
                id: source.device_id.clone(),
                os: "macos".to_string(),
                os_version: "15".to_string(),
            },
            portability: Portability::CrossPlatform,
        },
        ..Default::default()
    }
}

/// Send `payload` from `from` to `to`, and return how the send ended.
async fn send(
    from: &Machine,
    to: &Machine,
    workspace_id: &str,
    payload: Vec<u8>,
) -> workspace_clone_network::transfer::TransferSession {
    let service = from.service.lock().await;
    service
        .send_workspace(&to.as_peer(), workspace_id, &payload, None)
        .await
        .expect("the send should complete")
}

fn unique_id(label: &str) -> String {
    format!(
        "e2e-{label}-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    )
}

fn remove_manifest(id: &str) {
    if let Ok(path) = workspace_clone_commands::capture::manifest_path_for(id) {
        let _ = std::fs::remove_file(path);
    }
}

/// Run git in `cwd`, asserting it succeeded, and return stdout as text.
fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git must be installed to run the transfer test");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("git output must be text")
}

/// `git status --short`, one string per changed path.
fn git_status_short(repo: &std::path::Path) -> Vec<String> {
    git(repo, &["status", "--short"])
        .lines()
        .map(|l| l.to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_sent_from_one_machine_is_stored_and_readable_on_the_other() {
    // The test that covers the whole point of the application.
    let mac = Machine::new("mac").await;
    let windows = Machine::new("windows").await;

    // Pairing happens on both machines, as the blueprint's ceremony requires: each
    // endpoint stores the other. The scopes are the mirror image -- this machine
    // grants the peer `send` because the peer will push here.
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;

    windows.start_accepting();

    let workspace_id = unique_id("happy");
    let manifest = manifest_for(&workspace_id, "Mac Project Setup", &mac);
    let payload = serde_json::to_vec(&manifest).unwrap();

    let session = send(&mac, &windows, &workspace_id, payload.clone()).await;
    assert_eq!(
        session.status,
        TransferStatus::Completed,
        "the send should complete: {:?}",
        session.error
    );

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("an arrival should be reported");
    assert!(
        arrival.accepted,
        "the arrival was refused: {:?}",
        arrival.refusal_reason
    );

    // The workspace is on disk, in the database, and attributed to the machine
    // that captured it.
    let row = WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .expect("the lookup should run")
        .expect("the workspace must be recorded on the receiving machine");
    assert_eq!(row.name, "Mac Project Setup");
    assert_eq!(row.source_device_id, mac.device_id);
    assert_eq!(row.status, "received");

    // And it is readable *on the receiving machine*, with the receiving machine's
    // key, through the same path a locally captured workspace takes.
    let sealed = std::fs::read_to_string(&row.encrypted_manifest_path).expect("the sealed file");
    let opened: WorkspaceManifest =
        workspace_clone_crypto::open_json(&windows.storage_key, &sealed)
            .expect("the receiving machine must be able to open what it stored");
    assert_eq!(opened.workspace.id, workspace_id);
    assert_eq!(serde_json::to_vec(&opened).unwrap(), payload);

    // The receiving machine's own key is what makes that work, and the sender's
    // is not a substitute. This is the specific failure the original send path
    // had: it transmitted the sender's sealed manifest, so the receiving machine
    // stored bytes it could not open.
    assert!(
        workspace_clone_crypto::open_json::<WorkspaceManifest>(&mac.storage_key, &sealed).is_err(),
        "the sender's key must not open a manifest sealed on the receiving machine"
    );

    // The transfer is in the history, from the receiving side.
    let history = TransferSessionRepository::new(windows.pool.clone())
        .list_for_workspace(&workspace_id)
        .await
        .expect("the history lookup should run");
    assert_eq!(history.len(), 1, "the arrival must be recorded once");
    assert_eq!(history[0].source_device_id, mac.device_id);
    assert_eq!(history[0].destination_device_id, windows.device_id);
    assert_eq!(history[0].status, "completed");
    assert_eq!(history[0].progress, 1.0);

    remove_manifest(&workspace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_with_files_arrives_with_its_archive_and_restores() {
    // The user's actual story: a file changed on the Mac, a workspace was
    // captured carrying it, and the Windows side must receive that file and be
    // able to write it out on restore.
    let mac = Machine::new("mac-files").await;
    let windows = Machine::new("windows-files").await;
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    let workspace_id = unique_id("files");
    let manifest = manifest_for(&workspace_id, "Project With Files", &mac);
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();

    // A project folder with a file that "changed" (its content is the payload).
    let source = std::env::temp_dir().join(format!(
        "wc-e2e-src-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let dest = std::env::temp_dir().join(format!(
        "wc-e2e-dest-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::create_dir_all(source.join("src")).unwrap();
    std::fs::write(source.join("src/note.txt"), "the file change from the Mac\n").unwrap();

    let built = workspace_clone_files::snapshot::build_archive(
        &[("proj-1".to_string(), source.clone())],
        Default::default(),
    )
    .expect("the snapshot should build");
    assert!(built.file_count >= 1, "the snapshot must carry the changed file");

    let payload = workspace_clone_files::transit::wrap(&manifest_bytes, Some(&built.tar))
        .expect("the envelope should wrap");

    let session = send(&mac, &windows, &workspace_id, payload.clone()).await;
    assert_eq!(
        session.status,
        TransferStatus::Completed,
        "the file-carrying send should complete: {:?}",
        session.error
    );

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("an arrival should be reported");
    assert!(
        arrival.accepted,
        "the arrival was refused: {:?}",
        arrival.refusal_reason
    );

    // The archive is recorded on the receiving machine, sealed with *its* key.
    let record = WorkspaceFilesRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .expect("the lookup should run")
        .expect("the receiving machine must record the incoming archive");
    assert!(record.file_count >= 1);
    assert!(record.byte_count >= built.byte_count as i64);

    let sealed = std::fs::read(&record.encrypted_files_path).expect("the sealed archive file");
    let opened = workspace_clone_crypto::open_bytes(&windows.storage_key, &sealed)
        .expect("the receiving machine's key must open the archive it stored");
    assert_eq!(
        opened, built.tar,
        "the archive must arrive byte-for-byte identical"
    );
    assert!(
        workspace_clone_crypto::open_bytes(&mac.storage_key, &sealed).is_err(),
        "the sender's key must not open the archive sealed on the receiving machine"
    );

    // Restore: the archive is written into the destination, and the changed
    // file has exactly the content it had on the Mac.
    let report = workspace_clone_files::archive::extract_project(
        &opened,
        "proj-1",
        &dest,
        Default::default(),
    )
    .expect("the extraction should run");
    assert!(report.files_written >= 1);
    assert_eq!(
        std::fs::read_to_string(dest.join("src/note.txt")).unwrap(),
        "the file change from the Mac\n",
        "the restored file must carry the Mac's change"
    );

    remove_manifest(&workspace_id);
    if let Ok(files_path) = workspace_clone_commands::capture::files_path_for(&workspace_id) {
        let _ = std::fs::remove_file(files_path);
    }
    let _ = std::fs::remove_dir_all(&source);
    let _ = std::fs::remove_dir_all(&dest);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dirty_git_repo_arrives_with_its_delta_and_restores_to_the_exact_change() {
    // The story that started this feature: one file changed on the Mac, and a
    // restore on the other machine must leave the destination checkout showing
    // exactly that one change in `git status` -- not every tracked file listed
    // as modified by a whole-tree copy that strips the executable modes.
    let mac = Machine::new("mac-git").await;
    let windows = Machine::new("windows-git").await;
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    // A source repository with one committed file, then one uncommitted change
    // and one new untracked file: the user's delta.
    let root = std::env::temp_dir().join(format!(
        "wc-e2e-git-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let base = root.join("base");
    std::fs::create_dir_all(&base).unwrap();
    git(&base, &["init", "-q", "-b", "main"]);
    git(&base, &["config", "user.email", "e2e@test"]);
    git(&base, &["config", "user.name", "E2E"]);
    std::fs::write(base.join("note.txt"), "one line\n").unwrap();
    git(&base, &["add", "-A"]);
    git(&base, &["commit", "-qm", "base"]);
    std::fs::write(base.join("note.txt"), "one line\nchanged on the Mac\n").unwrap();
    std::fs::write(base.join("new.txt"), "new file\n").unwrap();

    // The delta a capture records: `git diff HEAD` plus a new-file hunk for
    // the untracked file -- the exact recipe the git adapter uses. Its exit
    // code is 1 ("differences found"), so this path accepts that.
    let patch = git(&base, &["diff", "HEAD", "--binary", "--no-color", "--no-ext-diff"])
        + &(|| {
            let out = std::process::Command::new("git")
                .args([
                    "diff",
                    "--no-index",
                    "--binary",
                    "--no-color",
                    "--no-ext-diff",
                    "/dev/null",
                    "new.txt",
                ])
                .current_dir(&base)
                .output()
                .expect("git must run");
            assert!(
                out.status.code().is_some_and(|c| c == 0 || c == 1),
                "the file diff must run: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).expect("git output must be text")
        })();

    let workspace_id = unique_id("git-delta");
    let mut manifest = manifest_for(&workspace_id, "Git Delta", &mac);
    manifest.projects = vec![Project {
        id: "p1".to_string(),
        name: "delta".to_string(),
        source_path_hint: "~/code/delta".to_string(),
        destination_location_id: "code".to_string(),
        git: Some(GitInfo {
            remote_hint: None,
            branch: "main".to_string(),
            commit: None,
            dirty_worktree: true,
            dirty_state_captured: true,
            patch: Some(patch.clone()),
        }),
    }];

    let payload = serde_json::to_vec(&manifest).unwrap();
    let session = send(&mac, &windows, &workspace_id, payload.clone()).await;
    assert_eq!(
        session.status,
        TransferStatus::Completed,
        "the delta-carrying send should complete: {:?}",
        session.error
    );
    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("an arrival should be reported");
    assert!(
        arrival.accepted,
        "the arrival was refused: {:?}",
        arrival.refusal_reason
    );

    // The patch travels inside the manifest to the receiving machine's store,
    // intact, and opens with the receiving machine's key.
    let row = WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .expect("the lookup should run")
        .expect("the workspace must be recorded on the receiving machine");
    let sealed = std::fs::read_to_string(&row.encrypted_manifest_path).expect("the sealed file");
    let opened: WorkspaceManifest =
        workspace_clone_crypto::open_json(&windows.storage_key, &sealed)
            .expect("the receiving machine must open what it stored");
    assert_eq!(
        opened.projects[0]
            .git
            .as_ref()
            .and_then(|g| g.patch.as_deref()),
        Some(patch.as_str()),
        "the captured delta must arrive intact inside the manifest"
    );

    // Restore the way the app does on this machine: a checkout at the captured
    // state, with the captured delta applied to it. `git status` must show the
    // Mac's change and the new file, and nothing else -- the one-change story,
    // not the hundred-and-forty-six-file rewrite.
    let dest = root.join("dest");
    git(
        &root,
        &[
            "clone",
            "-q",
            base.to_string_lossy().as_ref(),
            dest.to_string_lossy().as_ref(),
        ],
    );
    let patch_path = root.join("delta.patch");
    std::fs::write(&patch_path, &patch).unwrap();
    git(
        &dest,
        &[
            "apply",
            "--binary",
            "--whitespace=nowarn",
            patch_path.to_string_lossy().as_ref(),
        ],
    );

    let mut status = git_status_short(&dest);
    status.sort();
    assert_eq!(
        status,
        vec![" M note.txt", "?? new.txt"],
        "the restored checkout must show exactly the source's uncommitted state"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("note.txt")).unwrap(),
        "one line\nchanged on the Mac\n"
    );
    assert_eq!(std::fs::read_to_string(dest.join("new.txt")).unwrap(), "new file\n");

    remove_manifest(&workspace_id);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_with_projects_and_requirements_arrives_intact() {
    // A byte-for-byte check on a manifest that is actually worth sending. The
    // happy-path test above uses a nearly empty manifest, and an empty one cannot
    // tell "arrived intact" from "arrived, minus the parts that were dropped".
    let mac = Machine::new("mac-rich").await;
    let windows = Machine::new("windows-rich").await;
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    let mut manifest = manifest_for(&unique_id("rich"), "Full Workspace", &mac);
    manifest.projects = vec![Project {
        id: "p1".to_string(),
        name: "stackhandoff".to_string(),
        source_path_hint: "projects/stackhandoff".to_string(),
        destination_location_id: "work".to_string(),
        git: Some(GitInfo {
            remote_hint: Some("github.com/example/stackhandoff".to_string()),
            branch: "main".to_string(),
            commit: Some("0".repeat(40)),
            dirty_worktree: true,
            dirty_state_captured: false,
            patch: None,
        }),
    }];
    manifest.applications = vec![Application {
        id: "a1".to_string(),
        adapter: "vscode".to_string(),
        project_id: Some("p1".to_string()),
        required: true,
        config: serde_json::json!({ "name": "Visual Studio Code" }),
    }];
    manifest.requirements = Requirements {
        applications: vec![AppRequirement::new("a1", "vscode", true)],
        runtimes: vec![RuntimeRequirement {
            name: "node".to_string(),
            version: ">=20".to_string(),
            required: true,
        }],
        cli_tools: vec![CliToolRequirement {
            name: "cargo".to_string(),
            version: None,
            required: true,
        }],
        identities: vec![],
        // Names only. A value would be rejected by `validate`, and the rejection
        // is the point of the check.
        environment: EnvironmentRequirement {
            presence_only: vec!["JAVA_HOME".to_string()],
            values_included: false,
        },
        services: vec![],
    };

    let payload = serde_json::to_vec(&manifest).unwrap();
    let workspace_id = manifest.workspace.id.clone();

    let session = send(&mac, &windows, &workspace_id, payload.clone()).await;
    assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);

    assert!(windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .map(|a| a.accepted)
        .unwrap_or(false));

    let row = WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .unwrap()
        .expect("recorded");
    let sealed = std::fs::read_to_string(&row.encrypted_manifest_path).unwrap();
    let opened: WorkspaceManifest =
        workspace_clone_crypto::open_json(&windows.storage_key, &sealed).unwrap();

    assert_eq!(
        serde_json::to_vec(&opened).unwrap(),
        payload,
        "every project, application and requirement must survive the transfer"
    );
    assert_eq!(opened.projects[0].git.as_ref().unwrap().branch, "main");
    assert_eq!(opened.requirements.environment.presence_only, vec!["JAVA_HOME"]);

    remove_manifest(&workspace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_from_a_device_that_was_never_paired_is_refused() {
    // The two machines complete a real handshake over a real socket, and the
    // payload still does not land: authorization is this device's decision about
    // a device it has a row for, not a property of the connection.
    let mac = Machine::new("mac-stranger").await;
    let windows = Machine::new("windows-stranger").await;
    // Paired on the sending side only. The receiving machine has never heard of
    // this peer, which is the case a user hits by forgetting to pair in both
    // directions.
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.start_accepting();

    let workspace_id = unique_id("stranger");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "Not Welcome", &mac)).unwrap();

    let session = send(&mac, &windows, &workspace_id, payload).await;
    // The transport itself succeeds -- the bytes were delivered. It is the
    // receiver's decision to refuse them, and it says so.
    assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("the refusal should still be reported");
    assert!(!arrival.accepted, "an unpaired device must not store anything");
    let reason = arrival.refusal_reason.unwrap_or_default();
    assert!(reason.contains("not been paired"), "got: {reason}");

    assert!(WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paired_device_trusted_only_to_receive_cannot_push_a_workspace() {
    let mac = Machine::new("mac-one-way").await;
    let windows = Machine::new("windows-one-way").await;
    // The scope is granted on the sending side, so the sender is happy. The
    // receiving machine granted only `receive`, which is the wrong half of the
    // pair, and must refuse the push.
    mac.trust(&windows, vec![TrustScope::SendWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.start_accepting();

    let workspace_id = unique_id("one-way");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "One Way", &mac)).unwrap();

    send(&mac, &windows, &workspace_id, payload).await;

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("the refusal should be reported");
    assert!(!arrival.accepted, "a peer without the send scope must not store anything");
    let reason = arrival.refusal_reason.unwrap_or_default();
    assert!(reason.contains("not to send"), "got: {reason}");

    assert!(WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revoked_device_cannot_push_a_workspace() {
    let mac = Machine::new("mac-revoked").await;
    let windows = Machine::new("windows-revoked").await;
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    DeviceRepository::new(windows.pool.clone())
        .revoke(&mac.device_id)
        .await
        .unwrap();

    let workspace_id = unique_id("revoked");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "Revoked", &mac)).unwrap();
    send(&mac, &windows, &workspace_id, payload).await;

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("the refusal should be reported");
    assert!(!arrival.accepted, "a revoked device must not store anything");
    let reason = arrival.refusal_reason.unwrap_or_default();
    assert!(reason.contains("revoked"), "got: {reason}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_refused_workspace_does_not_stop_the_next_one_from_arriving() {
    // The loop has to survive. A machine that stopped listening after one
    // unauthorised peer would be unreachable for every peer afterwards, and the
    // symptom -- a workspace that never appears, with no error anywhere -- is
    // indistinguishable from a network problem.
    let mac = Machine::new("mac-resilient").await;
    let windows = Machine::new("windows-resilient").await;
    // Deliberately not paired yet.
    windows.start_accepting();

    // First: the sender is not paired here, so this is refused.
    let refused_id = unique_id("resilient-refused");
    let refused = serde_json::to_vec(&manifest_for(&refused_id, "Refused", &mac)).unwrap();
    send(&mac, &windows, &refused_id, refused).await;
    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("the refusal should be reported");
    assert!(!arrival.accepted);

    // Now pair, and the very next transfer must land on the same listener.
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;

    let accepted_id = unique_id("resilient-accepted");
    let payload = serde_json::to_vec(&manifest_for(&accepted_id, "Accepted", &mac)).unwrap();
    let session = send(&mac, &windows, &accepted_id, payload).await;
    assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut landed = false;
    while Instant::now() < deadline {
        if let Some(arrival) = windows.arrivals.arrivals.lock().await.first() {
            if arrival.accepted {
                landed = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(landed, "the listener stopped accepting after one refusal");

    assert!(WorkspaceRepository::new(windows.pool.clone())
        .get(&accepted_id)
        .await
        .unwrap()
        .is_some());
    remove_manifest(&accepted_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_workspace_captured_on_the_receiving_machine_can_be_forwarded_to_a_third() {
    // Forwarding is why the capture-device check allows a device this machine
    // already has a row for, rather than only the sender.
    let mac = Machine::new("mac-origin").await;
    let windows = Machine::new("windows-relay").await;
    let linux = Machine::new("linux-third").await;

    // mac -> windows
    mac.trust(&windows, vec![TrustScope::ReceiveWorkspaces]).await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    // windows -> linux, and mac is known to windows so its provenance checks out.
    windows.trust(&linux, vec![TrustScope::ReceiveWorkspaces]).await;
    linux.trust(&windows, vec![TrustScope::SendWorkspaces]).await;
    // The third machine also needs a row for the *capture* device, not for the
    // machine it received from. `workspaces.source_device_id` is a foreign key, and
    // a manifest naming a device this machine has never seen is an unverifiable
    // claim -- so a relayed workspace needs all three machines paired, which is
    // what a real three-machine setup looks like.
    //
    // Note the direction: `linux.trust(&mac)` writes a row for the Mac into the
    // Linux machine's database. `mac.trust(&linux)` would write a row for Linux
    // into the Mac's, which is a different pairing entirely.
    linux.trust(&mac, vec![TrustScope::SendWorkspaces]).await;

    windows.start_accepting();
    linux.start_accepting();

    let workspace_id = unique_id("relayed");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "Relayed Workspace", &mac)).unwrap();
    send(&mac, &windows, &workspace_id, payload.clone()).await;

    assert!(windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .map(|a| a.accepted)
        .unwrap_or(false));

    // windows forwards what it stored. The bytes it sends are its own reading of
    // the manifest, not the file it received -- the same step a capture's send
    // takes, which is why the provenance still points at mac.
    let row = WorkspaceRepository::new(windows.pool.clone())
        .get(&workspace_id)
        .await
        .unwrap()
        .expect("recorded on the relay");
    let sealed = std::fs::read_to_string(&row.encrypted_manifest_path).unwrap();
    let relayed: WorkspaceManifest =
        workspace_clone_crypto::open_json(&windows.storage_key, &sealed).unwrap();
    let forwarded = serde_json::to_vec(&relayed).unwrap();

    let session = send(&windows, &linux, &workspace_id, forwarded).await;
    assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut landed = false;
    while Instant::now() < deadline {
        if let Some(arrival) = linux.arrivals.arrivals.lock().await.first() {
            if arrival.accepted {
                landed = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        landed,
        "the third machine should have accepted the forwarded workspace; its newest arrival was {:?}",
        linux.arrivals.arrivals.lock().await.first().map(|a| (
            a.accepted,
            a.refusal_reason.clone(),
            a.workspace_id.clone()
        ))
    );

    let final_row = WorkspaceRepository::new(linux.pool.clone())
        .get(&workspace_id)
        .await
        .unwrap()
        .expect("recorded on the third machine");
    assert_eq!(
        final_row.source_device_id, mac.device_id,
        "provenance must survive a relay: the workspace was captured on the Mac"
    );

    remove_manifest(&workspace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_never_speaks_noise_does_not_stall_the_loop() {
    // A peer that connects and says nothing would otherwise hold the loop for
    // the handshake timeout. The next legitimate transfer must not have to wait
    // for it.
    let mac = Machine::new("mac-silent").await;
    let windows = Machine::new("windows-silent").await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    // Connect and hold the socket open without sending a byte.
    let silent = tokio::net::TcpStream::connect(("127.0.0.1", windows.port))
        .await
        .expect("connect");
    let _silent = silent;

    let workspace_id = unique_id("after-silence");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "After Silence", &mac)).unwrap();
    let started = Instant::now();
    let session = send(&mac, &windows, &workspace_id, payload).await;
    assert_eq!(session.status, TransferStatus::Completed, "{:?}", session.error);

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("an arrival should be reported");
    assert!(arrival.accepted, "refused: {:?}", arrival.refusal_reason);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a silent peer delayed the transfer by {:?}",
        started.elapsed()
    );

    remove_manifest(&workspace_id);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_arrival_the_window_polls_for_says_what_happened() {
    // The shape the UI depends on: newest first, with a reason on a refusal and
    // the sender's real name on both.
    let mac = Machine::new("mac-arrival").await;
    let windows = Machine::new("windows-arrival").await;
    windows.trust(&mac, vec![TrustScope::SendWorkspaces]).await;
    windows.start_accepting();

    let workspace_id = unique_id("arrival-shape");
    let payload = serde_json::to_vec(&manifest_for(&workspace_id, "Shape Check", &mac)).unwrap();
    send(&mac, &windows, &workspace_id, payload).await;

    let arrival = windows
        .wait_for_arrival(Duration::from_secs(5))
        .await
        .expect("an arrival should be reported");
    assert!(arrival.accepted, "refused: {:?}", arrival.refusal_reason);

    assert_eq!(arrival.workspace_id, workspace_id);
    assert_eq!(arrival.workspace_name, "Shape Check");
    assert_eq!(arrival.sender_device_id, mac.device_id);
    assert_eq!(
        arrival.sender_device_name, "The other machine",
        "the arrival must name the paired device, not the id"
    );
    assert_eq!(arrival.source_device_id, mac.device_id);
    assert!(!arrival.transfer_digest.is_empty(), "the payload digest is reported");
    assert!(arrival.bytes_received > 0);
    assert!(
        arrival.refusal_reason.is_none(),
        "an accepted arrival has no reason: {:?}",
        arrival.refusal_reason
    );
    assert!(!arrival.transfer_id.is_empty());

    remove_manifest(&workspace_id);
}

/// A convenience the other tests hand-roll: confirming that a helper's signature
/// still matches the `Result` alias the crate exports. Cheap, and it fails loudly
/// if that alias ever changes shape.
#[test]
fn the_error_type_is_the_crate_alias() {
    fn takes_core_result(_: Result<()>) {}
    let ok: Result<()> = Ok(());
    takes_core_result(ok);
}

/// The accept loop has to be startable from Tauri's `setup` hook.
///
/// `setup` runs on the main thread with no Tokio runtime entered, and
/// `tokio::spawn` panics there — "there is no reactor running". Because `setup`
/// is on the startup path, that panic killed the app on launch, *after* the
/// listener had bound and this device had already announced itself over mDNS: a
/// peer could find it, connect to it, and the window would never open. Every
/// test that caught it would have to be one nobody wrote, because the app simply
/// stopped.
///
/// Every other test in this file is `#[tokio::test]`, so it enters a runtime and
/// the bug is invisible to all of them. This one is a plain `#[test]`, and the
/// listener is deliberately bound on *Tauri's* global runtime rather than a local
/// one. That is the point: the call to `serve_forever` below therefore happens
/// with no reactor in scope, exactly as it does on the startup path, while the
/// listener it hands over stays alive — the same relationship the real app has,
/// where the listener is bound on Tauri's runtime and the loop is spawned onto
/// it.
#[test]
fn the_accept_loop_can_be_started_without_a_tokio_runtime_entered() {
    let (receiver, pool, key, port) = tauri::async_runtime::block_on(async {
        let key = KeyPair::generate().expect("a static key");
        let device_id = workspace_clone_core::crypto::fingerprint_from_connection_key_b64(
            &key.public_key().to_base64(),
        )
        .expect("id");
        let mut service = TransferService::new(
            key,
            device_id,
            workspace_clone_network::transfer::TransferConfig::default(),
        );
        let port = service.start(0).await.expect("bind");
        let receiver = service.receiver().expect("the listener is open");
        let pool = init_db_at(&std::env::temp_dir().join(format!(
            "stackhandoff-noruntime-{}.db",
            std::process::id()
        )))
        .await
        .expect("migrations");
        (receiver, pool, EncryptionKey::new(rand::random::<[u8; 32]>()), port)
    });

    // The assertion is that this returns. On the buggy version it panicked on
    // this line rather than failing an assert, so the test binary aborts and the
    // failure is unmistakable.
    serve_forever(
        receiver,
        pool,
        IncomingState::new(),
        "local-device".to_string(),
        Arc::new(move || Ok(key.clone())),
    );

    // And the loop is genuinely running afterwards, not merely un-panicked: a
    // spawn that silently did nothing would sail past the line above.
    tauri::async_runtime::block_on(async move {
        let _ = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the listener the loop owns should still be bound");
    });

    let _ = std::fs::remove_file(std::env::temp_dir().join(format!(
        "stackhandoff-noruntime-{}.db",
        std::process::id()
    )));
}
