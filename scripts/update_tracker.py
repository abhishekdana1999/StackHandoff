"""Update the tracking workbook with this session's findings.

Appends rather than rewrites: the epics, features and decisions already in the
file are still accurate, and a tracker that loses its history on every update
cannot be used to see what changed or when. Where a prior row is now wrong --
the UI features were marked Done while the screens were still on mock data --
the status is corrected in place and the change is logged, so the board and the
log agree about when the board was wrong.
"""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 27, 16, 20).strftime("%Y-%m-%d %H:%M")

wb = openpyxl.load_workbook("WorkspaceClone_Tracking.xlsx")


def append(sheet, rows):
    ws = wb[sheet]
    for row in rows:
        ws.append(row)
    return len(rows)


def log(log_id, kind, component, item, message, details, tags):
    return [
        log_id,
        NOW,
        kind,
        component,
        item,
        message,
        details,
        "Assistant",
        tags,
    ]


# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------

bugs = [
    [
        "BUG-001",
        "FEAT-054,FEAT-055,FEAT-056",
        "Route parameter name did not match the name the screens read, so three screens could not load a workspace",
        "App.tsx declared preflight/:transferId, prepare/:transferId and restore-preview/:transferId, while "
        "PreflightScreen, PrepareScreen and RestorePreviewScreen all read useParams<{ workspaceId }>(). "
        "useParams is generic, so params.workspaceId typed as string and compiled cleanly while resolving to "
        "undefined at runtime. All three screens opened showing 'the workspace could not be read'.",
        "Critical",
        "P0",
        "Fixed",
        "Run the app, open any workspace, and choose Preflight, Prepare or Review restore plan.",
        "Each screen loads the workspace named in the address bar.",
        "The screens rendered their error state: get_manifest was called with workspaceId = undefined.",
        "The param is the workspace id, not a transfer id. The transfer is complete by the time any of these "
        "screens is reachable, and on the sending machine none of them apply. A mismatch between a route pattern "
        "and a param name is invisible to the type system, to the linter, and to any unit test of a screen in "
        "isolation - a screen test mounts the screen directly, so the router is never involved.",
        "Renamed the three route patterns to :workspaceId and documented why the param is a workspace id. Added "
        "src/test/routes.test.tsx, which loads the real <App /> at a real URL and asserts the id in the address "
        "bar reached the backend. Confirmed the test has teeth by reverting one route pattern: exactly the two "
        "preflight tests failed and the prepare and restore-preview tests, whose patterns were untouched, passed.",
        "Yes - 9 route tests pass; tsc --noEmit clean",
        "2026-09-26",
        NOW,
        "Assistant",
        "Found while rewriting the three screens off mock data.",
    ],
    [
        "BUG-002",
        "FEAT-053",
        "Cancelling a transfer could take fifteen seconds to take effect",
        "The cancel flag was an AtomicBool that the send loop only read at a frame boundary. A send spends "
        "nearly all of its time blocked in a read, so cancelling one that was waiting on a silent peer was not "
        "noticed until the handshake timeout expired - fifteen seconds - and up to thirty if a frame stalled "
        "mid-transfer. The Cancel button appeared to do nothing for as long as the user could bear to look at it.",
        "Major",
        "P0",
        "Fixed",
        "Start a send to a host that accepts the TCP connection but never answers, then call cancel() on the "
        "service after 2ms. Measured how long the send takes to return.",
        "The send returns promptly, reporting Cancelled with a reason.",
        "The send returned after 15.005s, the full HANDSHAKE_TIMEOUT. The loopback test that found this took "
        "15.05s; the other seven took 0.08s combined.",
        "Cancellation was only observed where the code happened to check it, which was between frames, not where "
        "the send actually spent its time. A poll on a flag cannot interrupt a blocking read.",
        "Added a tokio::sync::watch channel carrying the cancel flag, and raced every blocking operation in "
        "send_inner against it: the connect, the handshake reply, each per-frame acknowledgement, and the final "
        "confirmation. A watch rather than a Notify because the flag is resettable and a service is reused for a "
        "second workspace - a Notify stores a permit that reset_cancel cannot take back, which made the *next* "
        "transfer cancel itself and hung resetting_the_cancel_flag_allows_another_transfer. The test now asserts "
        "a latency budget of under two seconds, verified to fail at 15.005s when the notification is removed.",
        "Yes - 64 network lib tests and 8 loopback tests pass; the cancel test reports the old 15s when the fix is reverted",
        "2026-09-27",
        NOW,
        "Assistant",
        "Found by the new loopback transfer test, which is what it was written to find.",
    ],
    [
        "BUG-003",
        "EPIC-005",
        "Key storage wrote all three secrets under all three credential names, contradicting its own documentation",
        "store_local_keys documented that the three secrets were kept 'each under its own credential so that "
        "reading one does not imply access to another', then serialised the whole LocalKeyBundle and wrote that "
        "same JSON to device-identity, device-noise and storage-key. Reading any one of the three returned the "
        "Ed25519 identity key, the Noise transport key and the manifest storage key together. It also made a "
        "partial write - one of the three set_password calls failing - look like a success and yield a device "
        "whose identity key was also its transport key.",
        "Minor",
        "P1",
        "Fixed",
        "Inspect the three credentials named under the workspace-clone service and compare their contents.",
        "Each credential contains one secret, and reading one does not reveal the other two.",
        "All three credentials held byte-identical JSON containing all three secrets.",
        "The implementation was written for an earlier layout and the comment was never updated to match it. The "
        "practical exposure on macOS is limited - all three sit in the same Keychain under the same ACL, so a "
        "process able to read one can read all three anyway - but the code claimed a separation it did not have, "
        "and a comment that is wrong about security is worse than none.",
        "store_local_keys now writes each field to its own credential under its own username and to no other. "
        "load_local_keys tolerates the old all-three-identical layout by detecting a leading '{' and parsing the "
        "bundle, so an existing installation keeps its identity and does not have to pair every peer again. "
        "delete_local_keys was already correct for the new layout.",
        "Yes - 23 crypto tests pass; verified against the live Keychain, which kept the same device id across a "
        "relaunch",
        "2026-09-27",
        NOW,
        "Assistant",
        "Found while rewriting the Settings screen, which asserts to the user where their keys are.",
    ],
    [
        "BUG-004",
        "EPIC-011",
        "Application identifier was the default com.tauri.dev while the database path used a different one",
        "tauri.conf.json carried the default identifier from scaffolding. The database is located through "
        "ProjectDirs::from(\"com\", \"workspaceclone\", \"WorkspaceClone\"), so the app's data directory and its "
        "macOS bundle identity did not match. Nothing failed, because the path is hardcoded rather than derived "
        "from the identifier - which is why the mismatch was invisible until something needed the two to agree.",
        "Minor",
        "P1",
        "Fixed",
        "Compare tauri.conf.json's identifier with the string passed to ProjectDirs.",
        "The two agree, so a bundled or signed build has the identity its data and credentials are filed under.",
        "identifier = com.tauri.dev, data directory com.workspaceclone.WorkspaceClone.",
        "The scaffolded default was never changed, and nothing in the build compared it with the hardcoded path.",
        "Set identifier to com.workspaceclone.WorkspaceClone. Verified the change is safe for an existing "
        "installation: the device id was identical before and after, because the Keychain credential is keyed on "
        "the service name workspace-clone and not on the bundle id.",
        "Yes - rebuilt and relaunched; same device id, migrations applied, transfer listener bound",
        "2026-09-27",
        NOW,
        "Assistant",
        "Surfaces at signing or notarisation time rather than in development, so it is cheap now and not later.",
    ],
]

append("Bugs", bugs)

# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------

errors = [
    [
        "ERR-001",
        NOW,
        "Frontend / App.tsx",
        "FEAT-054,FEAT-055,FEAT-056",
        "Runtime",
        "get_manifest invoked with workspaceId = undefined; all three workspace-scoped screens rendered "
        "'The workspace could not be read'.",
        "",
        "Reached by clicking Preflight, Prepare or Review restore plan from a workspace.",
        "Critical",
        "Resolved",
        "Route patterns declared :transferId while the screens read useParams<{ workspaceId }>(). The generic "
        "on useParams is an unchecked assertion, so the mismatch compiled.",
        "Renamed the patterns to :workspaceId. Added src/test/routes.test.tsx, which mounts the real <App /> "
        "behind a MemoryRouter at each route and asserts the id in the URL reached get_manifest - the only level "
        "at which a route pattern is observable. Reverted one pattern to confirm the test fails.",
        "Yes",
        "A screen test cannot catch this: mounting a screen directly bypasses the router entirely. Test the route, "
        "not the screen, when the screen's input is a URL.",
        "BUG-001",
    ],
    [
        "ERR-002",
        NOW,
        "Rust / network/src/transfer.rs",
        "FEAT-053",
        "Performance / Correctness",
        "A cancelled send took 15.005s to return, the full HANDSHAKE_TIMEOUT.",
        "",
        "Loopback transfer test: send 4 MiB to a listening-but-silent receiver, call cancel() after 2ms.",
        "Major",
        "Resolved",
        "The cancel flag was read only at a frame boundary, and a send blocked in a read never reaches one. The "
        "three blocking points - connect, handshake reply, per-frame ack - were not cancellable at all.",
        "Added a watch channel carrying the flag and raced each blocking operation against it. Test duration for "
        "the cancel case went from 15.05s to under 0.08s, and the whole loopback file from 15.16s to 0.08s.",
        "Yes",
        "A cancellation path needs a latency assertion, not just a status assertion. The status was already "
        "correct; only the timing was wrong, and nothing tested the timing.",
        "BUG-002",
    ],
]

append("Errors", errors)

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------

runs = [
    [
        "RUN-001",
        "Test",
        "Rust / whole workspace",
        "cargo test --workspace -- --test-threads=2",
        "Passed",
        NOW,
        NOW,
        "300",
        "0",
        "325 tests passed, 0 failed. Up from 317: 8 new loopback transfer tests. 7 binaries, no failures.",
        "None",
        "",
        "Manual",
        "Test-threads=2 avoids the parallel-binary contention that makes a default run look slow.",
    ],
    [
        "RUN-002",
        "Build",
        "Rust / whole workspace",
        "cargo build --workspace",
        "Passed",
        NOW,
        NOW,
        "40",
        "0",
        "Clean build, 0 warnings.",
        "None",
        "",
        "Manual",
        "Warnings are treated as failures here; no #[allow] was added to silence anything.",
    ],
    [
        "RUN-003",
        "Test",
        "Frontend / whole project",
        "npx vitest run",
        "Passed",
        NOW,
        NOW,
        "1.4",
        "0",
        "2 files, 19 tests passed. 10 pre-existing IPC contract tests plus 9 new route tests.",
        "None",
        "",
        "Manual",
        "",
    ],
    [
        "RUN-004",
        "Build",
        "Frontend",
        "npm run build",
        "Passed",
        NOW,
        NOW,
        "4",
        "0",
        "tsc then vite build. 1655 modules, 363 KB JS (127 KB gzipped across chunks).",
        "None",
        "dist/",
        "Manual",
        "tsc runs as part of the build, so the type check is not a separate step that can be skipped.",
    ],
    [
        "RUN-005",
        "E2E",
        "Desktop app / startup",
        "./target/aarch64-apple-darwin/debug/app",
        "Passed",
        NOW,
        NOW,
        "45",
        "0",
        "Migrations applied, Keychain keys loaded, device identity derived, mDNS discovery started, transfer "
        "listener bound on an ephemeral port. No errors or warnings from any workspace crate.",
        "None",
        "",
        "Manual",
        "Binary is under target/aarch64-apple-darwin/debug/, not target/debug/, because .cargo/config.toml pins "
        "build.target. Worth knowing before concluding the build produced nothing.",
    ],
]

append("Runs", runs)

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------

logs = [
    log(
        "LOG-026",
        "BUGFIX",
        "Frontend",
        "BUG-001",
        "Corrected the route parameter on three screens that could not load a workspace",
        "Renamed :transferId to :workspaceId in App.tsx. Added src/test/routes.test.tsx with 9 tests that mount "
        "the real App behind a MemoryRouter and assert the URL's id reaches the backend. Verified the test fails "
        "when one pattern is reverted.",
        "bugfix,routing,test",
    ),
    log(
        "LOG-027",
        "BUGFIX",
        "Rust / network",
        "BUG-002",
        "Made cancellation interrupt the operation in progress rather than wait for a frame boundary",
        "Added a watch channel and raced connect, handshake, per-frame ack and final confirmation against it. "
        "Cancel latency 15.005s -> under 0.08s. Used watch rather than Notify because a Notify's stored permit "
        "survives reset_cancel and cancelled the following transfer, hanging an existing test.",
        "bugfix,cancellation,transfer",
    ),
    log(
        "LOG-028",
        "BUGFIX",
        "Rust / crypto",
        "BUG-003",
        "Split the three local secrets into three credentials, as the documentation always claimed",
        "store_local_keys writes each field to its own username. load_local_keys detects and parses the old "
        "all-three-identical layout so existing installations keep their device identity.",
        "bugfix,keychain,security",
    ),
    log(
        "LOG-029",
        "BUGFIX",
        "Rust / app",
        "BUG-004",
        "Set the application identifier to match the data directory",
        "tauri.conf.json identifier com.tauri.dev -> com.workspaceclone.WorkspaceClone. Confirmed the device id "
        "was unchanged across a relaunch, because the Keychain credential is keyed on the service name.",
        "bugfix,packaging",
    ),
    log(
        "LOG-030",
        "REFACTOR",
        "Frontend / screens",
        "FEAT-050,FEAT-055,FEAT-058,FEAT-049",
        "Rebuilt four screens against the commands that exist",
        "DevicesScreen: pairing now uses the real flow - paste or pick a key, compute the safety number from both "
        "Noise keys, type back what the other screen shows. The previous flow was a UI fiction: it asked for a "
        "pairing code, but verify_pairing takes a Noise key, and no implemented command accepts a code. "
        "PrepareScreen: now the unmet subset of the real preflight report, grouped by adapter, with the action the "
        "adapter recorded. It had 9 invented actions including 'Login to Supabase'. SettingsScreen: 22 toggles "
        "that changed nothing and 5 alert() stubs replaced by the real persisted surface, with the absent settings "
        "listed as absent. WelcomeScreen: removed claims of QR codes, browser tab capture and a Phase 0 status, "
        "none of which this build does.",
        "refactor,ui,honesty",
    ),
    log(
        "LOG-031",
        "TEST",
        "Rust / network",
        "FEAT-053,FEAT-006",
        "Added the loopback transfer integration test",
        "network/tests/loopback_transfer.rs, 8 tests. Two TransferServices with real generated X25519 statics, "
        "real TCP over the loopback interface, a real Noise_IK handshake: byte-for-byte round trip, digest, the "
        "receiver learning the sender's authenticated key, progress shape, cancellation latency, a device with no "
        "advertised key, a wrong key, two transfers in a row, and an empty workspace. Multi-threaded runtime on "
        "purpose: a current-thread runtime deadlocks a test whose body is the send. Loopback rather than a mock "
        "socket, because a mock that correctly modelled the 65535-byte message ceiling, backpressure and partial "
        "reads would be a second implementation of the transport with its own bugs.",
        "test,integration,network",
    ),
    log(
        "LOG-032",
        "VERIFY",
        "Desktop app",
        "EPIC-005",
        "Verified the device identity derivation against a live database",
        "The app's own key was read out of the Keychain-backed devices row and its id recomputed in Python: "
        "sha256('workspace-clone/connection-fingerprint/v1' || key)[:16] base64 matches the stored id exactly. "
        "This is the invariant whose violation made every send report 'no device is reachable'. Also confirmed the "
        "id is unchanged across a relaunch, which is what makes pairing durable.",
        "verify,e2e,identity",
    ),
]

append("Logs", logs)

# ---------------------------------------------------------------------------
# Decisions
# ---------------------------------------------------------------------------

decisions = [
    [
        "DEC-010",
        "2026-09-27",
        "Cancellation is a watch channel rather than a polled flag",
        "A send spends nearly all of its time blocked in a read. A cancel flag read only at a frame boundary "
        "therefore cannot interrupt it, and cancelling a transfer waiting on a silent peer took the full "
        "15-second handshake timeout.",
        "Poll the flag more often; tokio::sync::Notify; tokio::sync::watch; give up cancelling mid-transfer",
        "tokio::sync::watch carrying the cancel flag, raced against every blocking operation",
        "The flag stays the source of truth; the channel is only what makes noticing it prompt. A watch rather than "
        "a Notify specifically because the flag is resettable and the service is reused: a Notify stores a permit "
        "when nobody is listening and offers no way to take it back, so after a cancel and a reset the *next* "
        "transfer consumed the stale permit and cancelled itself. That was found by an existing test hanging, not "
        "by reasoning, which is the argument for having run it.",
        "Cancellation latency drops from 15.005s to under 0.08s. Adds a field to TransferService. A reset now "
        "propagates to waiters, which is harmless because a reset is never issued while a transfer is in flight.",
        "If a transfer ever needs to be paused and resumed rather than stopped, the latch semantics would have to "
        "change.",
        "Assistant",
        "Decided",
        "BUG-002, ERR-002",
    ],
    [
        "DEC-011",
        "2026-09-27",
        "The pairing flow is key-based with a typed-back safety number, not code-based",
        "DevicesScreen presented a two-step flow built around a pairing code: generate a code, paste it on the "
        "other machine, verify. No implemented command accepts a code. verify_pairing takes a peer's Noise public "
        "key, a name, the safety number the user read out loud, and the trust scopes; get_safety_number computes "
        "that number from the local and remote keys.",
        "Keep the code flow and add a code command; make the UI key-based on the commands that exist",
        "Key-based: pick a discovered device or paste a key, compute the safety number, require the user to type "
        "back what the other screen shows, then verify",
        "The code flow was a fiction with no backend behind it. create_pairing_invitation is real and is still "
        "offered, but as a way to hand over this device's keys - an invitation, not a pairing. PairingInvitation "
        "carries no safety number, because it is derived from both Noise keys and the inviter does not know the "
        "other side's yet; a type suggesting otherwise would assert a binding that was never established.",
        "The Devices screen now matches the backend. A pair requires both machines and an out-loud comparison, so "
        "there is no purely local pairing path in the UI.",
        "If a deep link for workspace-clone://pair is ever handled, the paste box should accept the whole link and "
        "parse out the key.",
        "Assistant",
        "Decided",
        "FEAT-050",
    ],
]

append("Decisions", decisions)

# ---------------------------------------------------------------------------
# Overview: correct the rows that are now wrong
# ---------------------------------------------------------------------------

ws = wb["Overview"]
corrected = {
    "Key Storage": (
        "OS credential store (macOS Keychain / Windows Credential Manager), three separate entries",
        "Each secret is written only to its own entry. The database is not encrypted; manifests are sealed "
        "individually with a local key.",
    ),
    "Protocol": (
        "Noise_IK over direct TCP, with mDNS discovery",
        "Direct device-to-device only. There is no relay, so two machines on different networks cannot reach each "
        "other yet.",
    ),
    "Target Platforms": (
        "macOS 13+ (built and verified). Windows 11 implemented but unverified. Linux not started.",
        "The blueprint's initial platforms are Windows 11 and macOS 13+; only macOS has been run end to end.",
    ),
}
for row in ws.iter_rows(min_row=2):
    field = row[0].value
    if field in corrected:
        row[1].value, row[4].value = corrected[field]
    row[2].value = "Done"
    row[3].value = NOW

ws.append(
    [
        "Test Coverage",
        "325 Rust tests, 19 frontend tests",
        "Done",
        NOW,
        "8 of the Rust tests are a new loopback transfer suite: two Noise endpoints over real TCP. 9 of the "
        "frontend tests mount the real router, which is the only level at which a route pattern is observable.",
    ]
)

# ---------------------------------------------------------------------------
# Epics: EPIC-009 was Done while the screens were on mock data. It is now
# genuinely done, so the status stands, but EPIC-011 has a new known finding and
# EPIC-009 gains a start date that reflects when it was actually finished.
# ---------------------------------------------------------------------------

ws = wb["Epics"]
for row in ws.iter_rows(min_row=2):
    if row[0].value == "EPIC-009":
        row[8].value = "2026-09-27"
        row[9].value = NOW
        row[10].value = (
            "All ten screens are driven by real commands. Devices, Prepare, Settings and Welcome were previously "
            "on mock data with stubbed handlers and were rebuilt this session; the other six were rebuilt earlier. "
            "The mock flows were not cosmetic: Devices asked for a pairing code that no command accepts, Prepare "
            "listed nine invented actions, and Settings offered 22 toggles that changed nothing."
        )
    if row[0].value == "EPIC-011":
        row[10].value = (
            "Not started, but three findings already exist: BUG-003 (the key storage layout contradicted its own "
            "documentation), BUG-004 (the identifier mismatch, fixed), and tauri.conf.json still sets "
            "security.csp to null, which disables the Content-Security-Policy header entirely."
        )
    if row[0].value == "EPIC-006":
        row[10].value = (
            "Verified end to end by the loopback suite: a real Noise_IK handshake, a byte-for-byte round trip, a "
            "digest checked on both sides, a refused handshake against the wrong key, and a refusal to connect to "
            "a device that advertised no key. Cancellation was 15 seconds late and is now prompt (BUG-002)."
        )

# ---------------------------------------------------------------------------
# Features: the ten UI features were marked Done while the screens ran on mock
# data. They are done now, so the status is correct, but the end date and notes
# need to say when and what changed.
# ---------------------------------------------------------------------------

ui_notes = {
    "FEAT-049": "Rebuilt 2026-09-27. Removed claims of QR codes, browser tab capture and a Phase 0 development "
    "status; this build does none of those. Now reports the real version, platform and paired-device count.",
    "FEAT-050": "Rebuilt 2026-09-27 from mockDevices (6 hardcoded refs) to listPairedDevices, "
    "createPairingInvitation, getSafetyNumber, verifyPairing, revokePairedDevice, deletePairedDevice and "
    "updatePairedDevice. See DEC-011: the previous code-based flow had no backend behind it.",
    "FEAT-055": "Rebuilt 2026-09-27 from 9 mockActions to the unmet subset of the real preflight report, grouped "
    "by adapter, each row offering the action the adapter recorded and a rerun.",
    "FEAT-058": "Rebuilt 2026-09-27. 22 toggles that called a command which does not exist, and 5 alert() stubs "
    "saying 'will be implemented in Phase 1', replaced by the real persisted surface: identity, project roots, "
    "settings export/import.",
    "FEAT-054": "Verified 2026-09-27 by BUG-001's route test. Preflight categories derive from check.adapter_id; "
    "unrecognised statuses render as themselves rather than as ready.",
    "FEAT-056": "Verified 2026-09-27 by BUG-001's route test.",
}

ws = wb["Features"]
for row in ws.iter_rows(min_row=2):
    fid = row[0].value
    if fid in ui_notes:
        row[10].value = "2026-09-27"
        row[11].value = NOW
        notes_col = row[len(row) - 1]
        existing = notes_col.value or ""
        notes_col.value = f"{existing} | {ui_notes[fid]}" if existing else ui_notes[fid]

wb.save("WorkspaceClone_Tracking.xlsx")

print("Bugs appended:     ", len(bugs))
print("Errors appended:   ", len(errors))
print("Runs appended:     ", len(runs))
print("Logs appended:     ", len(logs))
print("Decisions appended:", len(decisions))
print("Overview rows:     ", wb["Overview"].max_row)
print("saved: WorkspaceClone_Tracking.xlsx")
