"""
Second tracker update: the receive path, and three bugs that only running the
app could find.

This session set out to finish the receive half of the transfer feature, which
the workbook recorded as Done. It was not done. There was no code on either
machine that accepted a connection: the listener was bound and advertised over
mDNS and nothing ever called `accept`. A workspace sent between two machines
succeeded at the protocol level and left no trace on the receiving machine. The
transfer features were marked 100% because every test that existed covered the
sending half.

That is the lesson the corrections below record. A feature marked Done is a
claim, and three of these claims were wrong. Two of the bugs in this batch were
invisible to every test in the suite, because every test entered a Tokio runtime
or ran in a process where the Tauri CLI was never involved. The only thing that
found them was starting the application and connecting to its port.

So this update does three things the previous one could not:

  1. Appends BUG-005..BUG-011, including the three that no test could catch.
  2. Corrects EPIC-006 and its features from Done back to what was true, and
     logs the correction, so the board and the log agree about when the board
     was wrong.
  3. Records the run that actually found the launch-time bugs: a real `tauri dev`
     with a socket connected to the advertised port.

Append-only, as before. Nothing is deleted; a wrong row is corrected in place
with the change logged.
"""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 27, 17, 40).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-27"

wb = openpyxl.load_workbook("WorkspaceClone_Tracking.xlsx")


def append(sheet, rows):
    ws = wb[sheet]
    for row in rows:
        assert len(row) == ws.max_column, f"{len(row)} cells for a {ws.max_column}-column sheet: {row[0]}"
        ws.append(row)
    return len(rows)


def log(log_id, kind, component, item, message, details, tags):
    return [log_id, NOW, kind, component, item, message, details, "Assistant", tags]


# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------

bugs = [
    [
        "BUG-005",
        "FEAT-028,FEAT-030",
        "There was no receive path at all: the listener was bound and advertised, and nothing ever accepted a connection",
        "The transfer listener was started in init and its port published over mDNS, and no task ever called accept_once. "
        "A peer that connected had its socket sit in the kernel accept queue, unauthenticated and unhandled, until the "
        "sender's 30-second frame timeout. No payload was read, no workspace row written, and no indication on the receiving "
        "machine that anything had happened. Sending a workspace from one machine to another did not work at all, while "
        "every existing test passed because all of them drove the sending side.",
        "Critical",
        "P0",
        "Fixed",
        "1. Start the app. Note the port in the log: 'Transfer listener bound on port NNNNN'. "
        "2. On another machine, or with a socket client, connect to that port and send a workspace. "
        "3. Observe: the sender eventually reports a frame timeout, and the receiving machine stores nothing.",
        "The receiving machine authenticates the peer, stores the manifest, and shows that a workspace arrived.",
        "The sender times out after about 30 seconds. The receiving machine has no new workspace and no notification.",
        "init bound the listener and advertised the port but never started a task that accepted from it. The receive side "
        "of the transfer had never been written; only the send side had.",
        "Added commands/src/receive.rs with a background accept loop started from init. Each arrival is authenticated, "
        "authorized, validated, stored sealed with this device's own key, recorded in transfer_sessions, and surfaced to the "
        "window through get_incoming_transfers. Covered by commands/tests/two_device_transfer.rs and verified against the "
        "running app by RUN-010.",
        "Yes - 11 two-device tests, plus a raw socket connected to the running application was accepted and logged",
        TODAY,
        TODAY,
        "Assistant",
        "transfer,network,receive,critical,found-by-running-the-app",
    ],
    [
        "BUG-006",
        "FEAT-028",
        "The payload sent was the sender's sealed manifest, which the receiver holds no key to open",
        "send_workspace transmitted the sealed manifest file, which is encrypted with the SENDING device's storage key. "
        "That key never leaves the sender and no peer has it, so the receiver stored bytes it could not read: the workspace "
        "appeared in the list and then failed at preflight, with the real cause several steps away from the symptom. A "
        "receive path that had existed would have turned a transfer that failed at the protocol level into a failure that "
        "looked like data corruption.",
        "Critical",
        "P0",
        "Fixed",
        "1. Capture a workspace on device A. "
        "2. Send it to device B. "
        "3. On B, open the workspace and run preflight. Attempt the same read on A, using B's storage key.",
        "The manifest is readable on whichever device stored it, and each device's stored copy opens with that device's own key.",
        "Readable on the sender. On the receiver, the manifest is the sender's ciphertext, and B's storage key cannot open it.",
        "Sealing is protection at rest using the device's own storage key. Sending the sealed file therefore sent bytes no "
        "peer can open. Confidentiality in transit is the Noise channel's job, and the receiver re-seals with its own key on "
        "arrival.",
        "send_workspace now opens the manifest locally, re-serialises it as JSON, and sends that inside the encrypted channel. "
        "The receiver seals it with its own key. See DEC-012. The two-device suite asserts the stored manifest opens with the "
        "receiver's key and NOT with the sender's.",
        "Yes - asserted explicitly in two_device_transfer.rs, including a negative assertion on the sender's key",
        TODAY,
        TODAY,
        "Assistant",
        "transfer,crypto,critical,design-error",
    ],
    [
        "BUG-007",
        "FEAT-028,FEAT-030",
        "No authorization on either direction: any peer that completed a handshake could send, and any paired device could be sent to",
        "Neither end checked that the other was permitted. A successful Noise handshake established that the peer held a "
        "private key; it did not establish that this device had decided to trust that key, or that the peer had been granted "
        "the right to push. Likewise send_workspace offered any device, paired or not. The pair row carried trust_scopes and "
        "nothing read them.",
        "Major",
        "P0",
        "Fixed",
        "1. Pair a device, then clear its scopes. "
        "2. Send it a workspace: the send proceeds. "
        "3. From that device, send a workspace back: it is accepted and stored.",
        "Sending requires the destination to be paired, not revoked, and to hold the receive scope. Receiving requires the "
        "sender to be paired on this device, not revoked, and to hold the send scope.",
        "Both directions proceed. A device trusted for nothing can both receive and push.",
        "The trust model was carried in the database but never enforced at either gate. The identity checked is the Noise "
        "static key the handshake authenticated, never the device id in a frame header, which is an unauthenticated string "
        "anything on the network could set.",
        "Added authorize_destination on the send side and a paired/not-revoked/scope gate in accept_or_refuse on the receive "
        "side. Both are covered: an unpaired sender, a sender trusted only to receive, and a revoked sender are each refused "
        "for a distinct reason, and the loop survives the refusal.",
        "Yes - four two-device tests, one per gate plus one proving the loop survives",
        TODAY,
        TODAY,
        "Assistant",
        "transfer,security,authorization",
    ],
    [
        "BUG-008",
        "FEAT-028",
        "One silent peer delayed every later peer by the full 15-second handshake timeout",
        "The accept loop handled each connection inline, on the listener's own task. Handshake handling is bounded by a 15-"
        "second timeout, so a peer that connected and then said nothing held the loop for that whole period. Every other "
        "peer that arrived in the meantime queued behind it. Measured, not theorised: the test that exposes this connected a "
        "silent socket and watched a legitimate transfer behind it complete in 15.03 seconds instead of immediately.",
        "Major",
        "P1",
        "Fixed",
        "1. Connect a socket to the listening port and send nothing. "
        "2. Immediately complete a real transfer from a second peer. "
        "3. Observe it waits for the first connection to time out.",
        "A peer that connects and says nothing affects only its own connection.",
        "Every later peer waits behind the silent one for the handshake timeout.",
        "accept was awaited inline in the loop, so a single slow connection serialised the whole listener.",
        "TransferReceiver was made Clone and gained accept_connection(); the loop now accepts, then hands the connection to "
        "its own task and immediately returns to accepting. accept_once and accept_once_with_progress delegate to it, so the "
        "single-connection entry points behave the same. See DEC-014.",
        "Yes - a_connection_that_never_speaks_noise_does_not_stall_the_loop, which measured 15.03s before the fix",
        TODAY,
        TODAY,
        "Assistant",
        "transfer,network,concurrency,found-by-a-test",
    ],
    [
        "BUG-009",
        "FEAT-001,FEAT-049",
        "The Tauri CLI could not find the application: tauri dev failed with 'No package info in the config file'",
        "tauri.conf.json lived in src-tauri/, whose sibling Cargo.toml is a virtual workspace manifest with no [package] "
        "section. The Tauri CLI requires the config's directory to have a sibling Cargo.toml that does have one, so it "
        "aborted. The Rust build script disagreed with the CLI: it had an explicit config_path pointing one level up, so "
        "cargo build found the config and succeeded. The application therefore compiled and could not be run in development, "
        "and the two tools had different ideas about the project layout.",
        "Critical",
        "P0",
        "Fixed",
        "1. cd to the project root. 2. Run npx tauri dev. 3. Observe: 'Error: No package info in the config file'.",
        "npx tauri dev starts Vite, compiles the app, and opens the window.",
        "The CLI exits immediately. cargo build works, so the failure looks like a CLI problem rather than a layout one.",
        "The config was in the virtual workspace root while the app crate was in src-tauri/app/. An explicit config_path in "
        "build.rs masked the disagreement from the Rust side.",
        "Moved tauri.conf.json to src-tauri/app/, beside the Cargo.toml that has [package]. Removed the config_path override "
        "from build.rs, and the path argument from generate_context!, so all three agree on one location. Re-expressed the "
        "config's relative paths (frontendDist ../../dist, icons ../icons/...). See DEC-017.",
        "Yes - npx tauri dev starts, compiles, and opens the window",
        TODAY,
        TODAY,
        "Assistant",
        "build,tooling,critical,found-by-running-the-app",
    ],
    [
        "BUG-010",
        "FEAT-001,FEAT-049",
        "The application compiled with zero capabilities: the declared permission set was silently discarded",
        "capabilities/default.json existed and declared core:default, but tauri-build globs ./capabilities/**/* relative to "
        "the BUILD SCRIPT's directory, which was src-tauri/app/. The file was in src-tauri/capabilities/, so the glob "
        "matched nothing. A glob that matches nothing is not an error. The generated app/gen/schemas/capabilities.json was "
        "an empty object, so the app shipped declaring no frontend permissions at all, and nothing in the build, the type "
        "checker or the test suite said so.",
        "Critical",
        "P0",
        "Fixed",
        "1. Read src-tauri/app/gen/schemas/capabilities.json, the file the build writes from the parsed capabilities. "
        "2. Observe: {}. 3. Note that src-tauri/capabilities/default.json declares core:default.",
        "The generated file contains the 'default' capability with core:default.",
        "{} - the app is built with no capabilities.",
        "Same layout divergence as BUG-009: the build script's directory and the directories the other Tauri tooling assumes "
        "were different, and a glob silently accepts a directory that does not exist.",
        "Moved capabilities/ to src-tauri/app/capabilities/, beside the build script. The generated file now contains the "
        "default capability. The DEVELOPER_GUIDE records that an empty {} there is a bug, not a neutral state.",
        "Yes - app/gen/schemas/capabilities.json contains the default capability with core:default",
        TODAY,
        TODAY,
        "Assistant",
        "build,security,permissions,silent-failure,found-by-running-the-app",
    ],
    [
        "BUG-011",
        "FEAT-001,FEAT-030",
        "The application panicked on launch and never opened its window, after advertising itself on the network",
        "serve_forever called tokio::spawn. It is called from Tauri's setup hook, which runs on the main thread with no Tokio "
        "runtime entered, and tokio::spawn panics there: 'there is no reactor running, must be called from the context of a "
        "Tokio 1.x runtime'. setup is on the startup path, so the panic killed the application on every launch - and after "
        "the listener had bound and this device had already announced itself over mDNS, so a peer could discover it, connect "
        "to it, and get nothing. Every test in the suite enters a runtime via #[tokio::test], so all of them passed.",
        "Critical",
        "P0",
        "Fixed",
        "1. Fix BUG-009 so the app can start at all. 2. Run npx tauri dev. 3. Observe the process exit with the panic above, "
        "and no window.",
        "The application starts, opens its window, and logs 'Listening for incoming transfers on port NNNNN'.",
        "Panic during setup. No window. The transfer port was already advertised, so the device looked reachable and was not.",
        "tokio::spawn requires an entered runtime. Tauri's setup hook has none; tauri::async_runtime::spawn enters Tauri's "
        "global runtime and works from either context.",
        "Changed the outer spawn to tauri::async_runtime::spawn. The inner per-connection spawn stays tokio::spawn, with a "
        "comment recording why the difference is deliberate. Added a plain #[test] that reproduces the startup condition by "
        "binding the listener on Tauri's runtime and calling serve_forever with no reactor in scope.",
        "Yes - the regression test panics with the exact production message when the fix is reverted (verified), and the "
        "running app starts clean",
        TODAY,
        TODAY,
        "Assistant",
        "startup,async,critical,found-by-running-the-app,regression-tested",
    ],
]

# ---------------------------------------------------------------------------
# Errors
# ---------------------------------------------------------------------------

errors = [
    [
        "ERR-003",
        f"{NOW}",
        "Tauri App Shell",
        "FEAT-001",
        "Panic",
        "there is no reactor running, must be called from the context of a Tokio 1.x runtime",
        "panicked at commands/src/receive.rs:314:5:\n"
        "there is no reactor running, must be called from the context of a Tokio 1.x runtime\n"
        "note: run with RUST_BACKTRACE=1 to display a backtrace",
        "Start the application with tauri dev. The panic is in the accept loop's spawn, reached from init during Tauri's "
        "setup hook. The listener had already bound and the device had already announced itself.",
        "Critical",
        "Resolved",
        "tokio::spawn called from a thread with no Tokio runtime entered. Tauri's setup hook runs on the main thread "
        "outside any runtime.",
        "Use tauri::async_runtime::spawn, which enters Tauri's global runtime. Added a plain #[test] that reproduces the "
        "condition, because every other test in the suite enters a runtime and would never have caught it.",
        "Yes",
        "Yes - commands/tests/two_device_transfer.rs::the_accept_loop_can_be_started_without_a_tokio_runtime_entered, "
        "verified to fail with the same panic when the fix is reverted",
        "When starting background work from init, use tauri::async_runtime::spawn. tokio::spawn is only correct from inside a "
        "task that is already running. The startup path is the one place this is easy to get wrong and impossible to unit "
        "test by accident.",
        "BUG-011",
        "Found only by starting the application. No test could have found it: all of them are #[tokio::test].",
    ],
    [
        "ERR-004",
        f"{NOW}",
        "Tauri App Shell",
        "FEAT-001",
        "CLI Error",
        "Error: No package info in the config file",
        "Error No package info in the config file",
        "npx tauri dev from the project root. Raised by tauri-cli 2.11.5 at interface/rust.rs: RustAppSettings::new loads "
        "CargoSettings for the config's directory and returns this when there is no [package] section.",
        "Critical",
        "Resolved",
        "tauri.conf.json was in src-tauri/, whose Cargo.toml is a virtual workspace manifest with no [package]. The app crate "
        "is src-tauri/app/. A config_path override in build.rs let cargo build succeed while the CLI could not find the app.",
        "Move tauri.conf.json beside the crate that has [package], drop the config_path override and the generate_context! "
        "path argument, and re-express the config's relative paths from the new location.",
        "Yes",
        "Yes - npx tauri dev starts the app",
        "Keep tauri.conf.json and capabilities/ inside src-tauri/app/, beside the crate that has [package] and the build "
        "script. All three Tauri tools resolve paths from there; the virtual workspace root is not one of them.",
        "BUG-009",
        "The same layout divergence caused BUG-010. Both were invisible to cargo build, which succeeded throughout.",
    ],
    [
        "ERR-005",
        f"{NOW}",
        "UI - Screens",
        "FEAT-030",
        "Type Error",
        "TS2345: Argument of type 'Element' is not assignable to parameter of type 'HTMLElement'",
        "src/test/receive.test.tsx(297,19): error TS2345\n"
        "src/test/receive.test.tsx(298,19): error TS2345",
        "A new test scoped a query with within() on the result of .closest(), which is typed Element | null rather than "
        "HTMLElement. Reached by two assertions, so it surfaced twice.",
        "Low",
        "Resolved",
        "Reaching for a CSS class selector on a component's rendered output. The element was found by its visible text, whose "
        "closest() returns a general Element.",
        "Put a data-testid on the workspace Card, which forwards its props to the div, and select the row by test id rather "
        "than by traversing the DOM for a class name.",
        "Yes",
        "Yes - npx tsc --noEmit is clean",
        "Select by data-testid, not by a CSS class. The Card components spread HTMLAttributes onto their div, so a test id is "
        "always available, and a test that depends on a styling class breaks when the styling changes.",
        "",
        "",
    ],
    [
        "ERR-006",
        f"{NOW}",
        "Commands Crate",
        "FEAT-028",
        "Test Failure",
        "the listener the loop owns should still be bound: ConnectionRefused (os error 61)",
        "thread 'the_accept_loop_can_be_started_without_a_tokio_runtime_entered' panicked at commands/tests/"
        "two_device_transfer.rs:839:14:\n"
        "the listener the loop owns should still be bound: Os { code: 61, kind: ConnectionRefused }",
        "Writing the regression test for BUG-011. The test built the listener inside a locally created Tokio runtime and then "
        "dropped that runtime to make the spawn call happen with no reactor in scope. The spawn panic was fixed, and the test "
        "then failed on its second assertion.",
        "Low",
        "Resolved",
        "A tokio::net::TcpListener belongs to the runtime that created it. Dropping the runtime closed the socket, so the "
        "listener the loop was supposed to serve was already gone.",
        "Bind the listener on Tauri's global runtime instead, which outlives the call. That is also a truer reproduction of "
        "the real startup path, where the listener and the loop share Tauri's runtime.",
        "Yes",
        "Yes - the test passes, and fails with the production panic when the BUG-011 fix is reverted",
        "A TcpListener is owned by its runtime. In tests, create and serve it on the same runtime, or the test measures the "
        "runtime being dropped rather than the code under test.",
        "BUG-011",
        "A wrong first attempt at the regression test, recorded because the failure was informative rather than confusing.",
    ],
]

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------

runs = [
    [
        "RUN-006",
        "Test",
        "Rust workspace (all crates)",
        "cd src-tauri && cargo test --workspace -- --test-threads=2",
        "Success",
        f"{TODAY} 17:05",
        f"{TODAY} 17:12",
        420,
        0,
        "348 passed, 0 failed, 0 ignored. Includes commands/tests/two_device_transfer.rs at 11 tests.",
        "",
        "src-tauri/target/aarch64-apple-darwin/debug/deps/",
        "Manual",
        "Whole-workspace regression run after the receive path landed. --test-threads=2 is required: default parallelism "
        "causes binary contention on the test binaries that bind real sockets, which presents as a hang rather than a failure.",
    ],
    [
        "RUN-007",
        "Test",
        "Frontend (vitest)",
        "npx vitest run",
        "Success",
        f"{TODAY} 17:06",
        f"{TODAY} 17:12",
        6,
        0,
        "37 passed across 3 files: ipc.test.ts (10), routes.test.tsx (9), receive.test.tsx (18).",
        "",
        "",
        "Manual",
        "receive.test.tsx is new: 18 tests covering the arrivals banner, refusals, dismissal, the Received badge and the "
        "transfer history. The polling test takes a real 4.0s because it waits for the actual interval rather than forcing "
        "an invalidation, which would have made it pass whether or not refetchInterval was set.",
    ],
    [
        "RUN-008",
        "Test",
        "TypeScript",
        "npx tsc --noEmit",
        "Success",
        f"{TODAY} 17:06",
        f"{TODAY} 17:07",
        40,
        0,
        "Clean. No errors, no output.",
        "",
        "",
        "Manual",
        "Caught ERR-005, two errors from a test reaching for a CSS class instead of a test id.",
    ],
    [
        "RUN-009",
        "Build",
        "Frontend",
        "npm run build",
        "Success",
        f"{TODAY} 17:12",
        f"{TODAY} 17:13",
        1,
        0,
        "Vite production build, 1655 modules. dist/index.html 1.09 kB, CSS 30.51 kB, JS bundles 301.98 kB raw / 107.18 kB gzipped.",
        "",
        "dist/",
        "Manual",
        "Runs tsc first, so this also type-checks.",
    ],
    [
        "RUN-010",
        "E2E",
        "Running application",
        "npx tauri dev, then a raw TCP connect to the port from the log",
        "Success",
        f"{TODAY} 17:23",
        f"{TODAY} 17:24",
        60,
        0,
        "App started with no panic. Log: 'This device is 6APnKHTwEwIWXA/Yg6xBdA== on ABHISHEKs-Air.lan', 'Transfer "
        "listener bound on port 53828', 'Listening for incoming transfers on port 53828'. Vite served HTTP 200. A raw "
        "socket connected to 127.0.0.1:53828 and the running app logged 'Accepted a connection from 127.0.0.1:53841'.",
        "",
        "run2.log",
        "Manual",
        "The decisive run. Before this, no test had ever started the application, which is why BUG-009, BUG-010 and BUG-011 "
        "were all live. A connection to the advertised port being accepted and logged is the proof that BUG-005 is fixed in "
        "the running program and not merely in the test harness.",
    ],
    [
        "RUN-011",
        "Test",
        "Command surface parity",
        "python3 scripts/check_command_parity.py",
        "Success",
        f"{TODAY} 17:11",
        f"{TODAY} 17:12",
        1,
        0,
        "PARITY OK. 44 registered in Rust, 44 distinct names, 44 called from TypeScript, zero orphans in either direction, "
        "no duplicate registrations.",
        "",
        "",
        "Manual",
        "Compares the generate_handler! registry against the invoke() names in src/lib/ipc.ts. Catches the one class of bug "
        "TypeScript cannot: a wrapper naming a command Rust never registered compiles perfectly and fails at runtime. Also "
        "reports duplicate registrations, where the second silently shadows the first.",
    ],
    [
        "RUN-012",
        "Test",
        "Test quality (mutation check)",
        "Three deliberate regressions injected into WorkspacesScreen.tsx, reverted after each",
        "Success",
        f"{TODAY} 22:50",
        TODAY and " 23:05",
        900,
        0,
        "Each mutation was caught by exactly the intended test and by no other. 1. Hiding the refusal reason: 1 failure, "
        "'shows the reason the workspace was not stored'. 2. Removing refetchInterval: 1 failure, the polling test, after "
        "waiting the full 12s timeout. 3. Badging 'received' as something else: 1 failure, 'badges it Received, not "
        "Captured'.",
        "",
        "",
        "Manual",
        "A test that passes is not evidence the code works. Three tests were rewritten during this session because they "
        "asserted nothing real - one spied on QueryClient.prototype.mount and then forced the refetch it claimed to be "
        "testing, one asserted the absence of an alert that was never rendered, one rendered twice in a single test. "
        "Reintroducing BUG-011 in Rust was checked the same way: the regression test panics with the exact production message.",
    ],
]

# ---------------------------------------------------------------------------
# Decisions
# ---------------------------------------------------------------------------

decisions = [
    [
        "DEC-012",
        TODAY,
        "Send the manifest as re-serialised JSON inside the encrypted channel, not as the sealed file",
        "A manifest is sealed at rest with the device's own storage key so that copying the file off the disk yields nothing "
        "readable. The transfer path was sending that sealed file, which is encrypted with a key no other machine has. The "
        "choice had to be made before the receive path existed, and it determined what a receiving machine could possibly do "
        "with what it was sent.",
        "(a) Send the sealed file, as the code did. (b) Send the plaintext manifest inside the already-encrypted Noise "
        "channel, and have the receiver seal it with its own key. (c) Encrypt the manifest to the destination's public key, "
        "adding a second key exchange to the protocol.",
        "(b). Send re-serialised manifest JSON; the receiver seals it with its own storage key on arrival.",
        "Sealing is protection AT REST and the key is per-device, so (a) sends bytes no peer can open - a workspace that "
        "appears in the list and then fails, with the real cause several steps from the symptom. Confidentiality in transit "
        "does not need the seal: the Noise channel is authenticated and encrypted for its whole duration, so (b) loses "
        "nothing. (c) was rejected as a second, weaker key exchange layered under one that is already proven.",
        "A stored received workspace is indistinguishable from a locally captured one, and opens with the local key. One "
        "consequence: workspaces.manifest_digest is a digest of the LOCALLY SEALED bytes, not of what arrived, so the arrival "
        "reports the arrived digest separately. A check comparing the two would fail every time. The two-device suite asserts "
        "the stored manifest opens with the receiver's key and NOT the sender's.",
        "Would be revisited if a future transport is not confidential end to end, or if manifests are ever large enough that "
        "re-serialising them is a problem.",
        "Assistant",
        "Accepted",
        "BUG-006, FEAT-028, commands/src/transfer.rs, commands/src/receive.rs",
    ],
    [
        "DEC-013",
        TODAY,
        "Authorize in both directions, and check the key the handshake authenticated rather than an announced id",
        "The device rows carried trust_scopes, and nothing read them. The send side offered any device; the receive side, once "
        "written, would have accepted anything that completed a handshake. Identity also had two possible sources - the Noise "
        "static key the handshake proved, and a device id in a frame header - and only one of them is authenticated.",
        "(a) Keep the single-scope, send-time-only check. (b) Enforce pairing, revocation and scope on both sides, deriving "
        "identity from the authenticated Noise key. (c) Enforce on both sides but trust the frame header's device id.",
        "(b). Both gates check paired, then not revoked, then the scope. Identity comes from the authenticated Noise key.",
        "A scope checked on one side only is a suggestion. And a device id in a frame header is an unauthenticated string that "
        "anything on the network could set, so trusting it would let an unpaired machine claim to be a paired one. (c) is "
        "the failure mode the whole pairing ceremony exists to prevent.",
        "Sending requires the destination to be paired, not revoked, and to hold `receive`. Receiving requires the sender to be "
        "paired ON THIS DEVICE, not revoked, and to hold `send`. Matches the DevicesScreen labels, which are both written "
        "from the perspective of the machine you are standing at. Covered by four tests, one per gate plus one proving a "
        "refusal does not stop the next transfer.",
        "Would be revisited if scopes ever need to differ per workspace rather than per device pair.",
        "Assistant",
        "Accepted",
        "BUG-007, FEAT-028, commands/src/transfer.rs, commands/src/receive.rs",
    ],
    [
        "DEC-014",
        TODAY,
        "Handle each accepted connection on its own task rather than inline in the accept loop",
        "Writing the loop as a plain accept-handle-accept loop serialised the listener behind per-connection work. Handshake "
        "handling is bounded by a 15-second timeout, so one peer that connected and said nothing delayed every other peer by "
        "that long. The test that measured it recorded 15.03 seconds behind a silent socket.",
        "(a) Handle inline. Simple, and one task. (b) Handle each connection on its own task. (c) Set the handshake timeout "
        "low, and handle inline.",
        "(b). TransferReceiver became Clone with accept_connection(); the loop accepts, spawns handling, and returns to "
        "accepting immediately.",
        "A silent peer is a normal event on a network, and must cost only its own connection. (c) would trade a real stall for "
        "a real class of failure: a legitimate slow handshake would be cut off. Holding the service mutex across a whole "
        "transfer was rejected too, for the same reason at a larger scale - it would block sends for the duration.",
        "A peer that connects and says nothing now affects only itself. accept_once and accept_once_with_progress delegate to "
        "the same accept_connection, so the single-connection entry points are unchanged.",
        "Would be revisited if a connection-count limit is ever needed, which would make a semaphore the right shape.",
        "Assistant",
        "Accepted",
        "BUG-008, FEAT-028, network/src/transfer.rs",
    ],
    [
        "DEC-015",
        TODAY,
        "Pass the storage key and the local device id into the receive path as parameters",
        "accept_or_refuse needs the local storage key to seal an arrival, and the local device id to verify that a manifest's "
        "capture device is one this device knows. Both were to be read from the OS credential store. A test binary reading a "
        "keychain item that belongs to the application blocks on a user prompt that nobody is present to answer, so every test "
        "would hang rather than fail.",
        "(a) Read the key inside the receive path. (b) Pass it in as a parameter, with the KeyStorage-backed caller supplying "
        "a closure. (c) Introduce a key-provider trait.",
        "(b). serve_forever takes a StorageKeySource = Arc<dyn Fn() -> Result<EncryptionKey> + Send + Sync>. The application "
        "passes a closure that reads the keychain on demand; the tests pass a fixed key.",
        "A closure keeps the key out of memory for the application's lifetime - it is fetched when an arrival is actually "
        "sealed and not held resident - and it makes the loop testable without touching the credential store. (c) is a larger "
        "interface than one call site justifies.",
        "Unit tests can exercise the whole receive path. capture::read_manifest gained read_manifest_with_key for the same "
        "reason. What the tests assert is the property that actually matters - the stored manifest opens with the key that "
        "sealed it and not with the sender's - which a real keychain would change nothing about.",
        "Would be revisited if key access ever needs to be scheduled, cached or reported on separately.",
        "Assistant",
        "Accepted",
        "commands/src/receive.rs, commands/src/capture.rs, commands/tests/two_device_transfer.rs",
    ],
    [
        "DEC-016",
        TODAY,
        "Record a received workspace with status 'received', not 'captured'",
        "workspaces.status is free text and the capture path wrote 'captured'. A workspace that arrived on this machine was "
        "not captured here, and the badge is the only place on the row that says where a workspace came from.",
        "(a) Reuse 'captured', since the manifest is the same shape. (b) Write 'received'. (c) Add a source column and leave "
        "status alone.",
        "(b). status = 'received', with the capture device's id in the existing source_device_id.",
        "A status is a claim about what happened on this machine. Writing 'captured' for a workspace that arrived would make "
        "the provenance invisible in the one place a user looks for it, and source_device_id is a foreign key to a device that "
        "may not be paired here. (c) duplicates information the column already carries.",
        "The Workspaces list badges it Received, in a different colour from Transferred, and a test asserts the badge is not "
        "'Captured'.",
        "",
        "Assistant",
        "Accepted",
        "BUG-005, FEAT-030, src/screens/WorkspacesScreen.tsx",
    ],
    [
        "DEC-017",
        TODAY,
        "Keep tauri.conf.json and capabilities/ inside src-tauri/app/, beside the crate that has [package]",
        "The Tauri CLI requires the config's directory to have a sibling Cargo.toml with a [package] section, because that is "
        "how it decides which crate is the application. tauri-build globs ./capabilities/**/* against the build script's "
        "directory. Both resolve from src-tauri/app/. Neither resolves from src-tauri/, whose Cargo.toml is a virtual "
        "workspace manifest.",
        "(a) Leave both in src-tauri/ and pass an explicit config_path from build.rs, which is what the project did. (b) Move "
        "both into the app crate directory and let every tool use the default. (c) Collapse the Rust workspace back into a "
        "single crate at src-tauri/ to match the default layout.",
        "(b). Config and capabilities live in src-tauri/app/. Relative paths re-expressed: frontendDist ../../dist, icons "
        "../icons/...; build.rs drops its config_path; generate_context! drops its path argument.",
        "(a) is what produced two live bugs: the CLI could not find the app at all, and the capabilities glob matched nothing "
        "so the app shipped declaring no frontend permissions. Both were silent - cargo build succeeded throughout - because "
        "an explicit config_path lets the build script and the CLI disagree about the project layout. (c) would undo the crate "
        "separation the whole design depends on.",
        "npx tauri dev works from the project root. The generated app/gen/schemas/capabilities.json is no longer empty. The "
        "DEVELOPER_GUIDE records that an empty {} there is a bug, not a neutral state.",
        "Would be revisited if a second Tauri application were ever added to the workspace, which would need per-app config "
        "paths anyway.",
        "Assistant",
        "Accepted",
        "BUG-009, BUG-010, FEAT-001, src-tauri/app/build.rs, src-tauri/app/src/lib.rs",
    ],
    [
        "DEC-018",
        TODAY,
        "The window polls for arrivals rather than subscribing to an event",
        "The accept loop is a backend task started at init, not something the UI starts, so receiving works with the window "
        "closed. The window therefore has no natural event to subscribe to without adding a backend-to-frontend event channel.",
        "(a) Poll get_incoming_transfers on an interval. (b) Emit a Tauri event from the accept loop and subscribe in the "
        "window. (c) Fetch once on mount and refresh after every action.",
        "(a). A 4-second interval, exported as ARRIVALS_POLL_MS so the test asserts against the real value.",
        "(b) is the better design and is the right thing to build if this grows; it was not chosen now because it needs a "
        "new event channel and a subscription lifecycle for a screen that is not the only consumer of an arrival. (c) is "
        "visibly wrong: a transfer that completes while the window is open would never appear, and a workspace that arrived "
        "thirty seconds ago and is not yet listed is indistinguishable from a transfer that failed. Four seconds bounds that "
        "confusion, and the poll is not load-bearing - the transfer is stored whether or not the window ever asks.",
        "ARRIVALS_POLL_MS = 4000 in src/screens/WorkspacesScreen.tsx. The polling test waits the real interval and fails if "
        "refetchInterval is removed; it was rewritten after a first version that forced the refetch it claimed to be testing.",
        "Would be revisited if arrivals became frequent enough to matter, or if a second surface needed them.",
        "Assistant",
        "Accepted",
        "FEAT-030, src/screens/WorkspacesScreen.tsx",
    ],
]

# ---------------------------------------------------------------------------
# Features added this session
# ---------------------------------------------------------------------------

features = [
    [
        "FEAT-064",
        "EPIC-006",
        "Receive Path (Accept, Authorize, Store, Report)",
        "The receiving half of a transfer: an accept loop started at init, per-connection handling, authorization against the "
        "pairing record, manifest validation, re-sealing with the local storage key, a transfer_sessions record, and an "
        "arrival the window can read. Commands: get_incoming_transfers, dismiss_incoming_transfer, get_transfer_history.",
        "Tauri Commands / Network Crate",
        "Critical",
        "Done",
        100,
        3,
        2,
        TODAY,
        NOW,
        "Assistant",
        "FEAT-027, FEAT-028",
        "A workspace sent from one machine is stored on the other, readable with the receiver's own key, and reported to the "
        "window with the sender's name and, on refusal, the reason it was refused. 11 two-device tests pass.",
        "This was the missing half of EPIC-006, which was recorded as Done while nothing accepted a connection. See BUG-005.",
    ],
    [
        "FEAT-065",
        "EPIC-006",
        "Transfer History",
        "A durable record of every transfer this device took part in, written by both ends, surfaced per workspace on the "
        "History tab. Answers 'did that actually get there?' after the sending window has closed, and shows a refusal with "
        "its reason.",
        "Tauri Commands / DB Crate",
        "High",
        "Done",
        100,
        1,
        1,
        TODAY,
        NOW,
        "Assistant",
        "FEAT-064",
        "transfer_sessions is written by the sender and the receiver, and the History tab shows both directions of a "
        "workspace with status and any recorded error.",
        "transfer_sessions existed since migration 001 with nothing writing to it, so an arriving transfer left no trace. "
        "Both ends now record their outcome.",
    ],
    [
        "FEAT-066",
        "EPIC-009",
        "Arrivals Banner and Refusal Reporting",
        "An arrivals banner on the Workspaces screen showing each arrival with the sender's NAME, not a fingerprint; a "
        "refusal shown as prominently as an acceptance and carrying the reason; a dismiss action that clears the "
        "notification without touching the stored workspace; and a 'Received' badge distinguishing an arrived workspace from "
        "a captured one.",
        "UI - Screens",
        "High",
        "Done",
        100,
        1,
        1,
        TODAY,
        NOW,
        "Assistant",
        "FEAT-064",
        "18 tests. Both outcomes are rendered, the sender is named, a refusal announces assertively and an acceptance "
        "politely, dismissal sends the transfer id rather than the workspace id, and a failure to read the arrivals list "
        "leaves the workspace list intact.",
        "The receiving machine is where the truth about a transfer lives: the sender reports success as soon as the bytes are "
        "delivered, so a refusal here is the only place a user can learn the workspace did not land.",
    ],
]

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------

logs = [
    log(
        "LOG-033",
        "Bug Found",
        "Tauri Commands",
        "BUG-005",
        "The receive path did not exist. The listener was bound and advertised; nothing accepted.",
        "init started a listener and published its port over mDNS, and no task ever called accept_once. A peer's connection "
        "sat in the kernel accept queue until the sender's 30-second frame timeout. Fixed with commands/src/receive.rs and an "
        "accept loop started from init, each connection handled on its own task.",
        "transfer,receive,critical",
    ),
    log(
        "LOG-034",
        "Bug Found",
        "Tauri Commands",
        "BUG-006",
        "The payload was the sender's sealed manifest, which the receiver has no key to open.",
        "send_workspace transmitted the file sealed with the sender's storage key. A key that never leaves the sender. The "
        "receiver would have stored bytes it could not read. Fixed by sending re-serialised JSON inside the encrypted channel "
        "and having the receiver seal it with its own key. See DEC-012.",
        "transfer,crypto,design-error",
    ),
    log(
        "LOG-035",
        "Bug Found",
        "Tauri Commands",
        "BUG-007",
        "No authorization on either direction. trust_scopes was stored and never read.",
        "Added a paired / not-revoked / scope gate to both send and receive, with identity taken from the Noise key the "
        "handshake authenticated rather than from a frame header. Four tests, one per gate.",
        "transfer,security",
    ),
    log(
        "LOG-036",
        "Bug Found",
        "Tauri Network",
        "BUG-008",
        "One silent peer delayed every later peer by 15.03 seconds.",
        "accept was awaited inline, so a peer that connected and said nothing held the listener for the whole handshake "
        "timeout. Measured by a new test rather than assumed. TransferReceiver is now Clone and the loop spawns per "
        "connection. See DEC-014.",
        "transfer,concurrency",
    ),
    log(
        "LOG-037",
        "Test Run",
        "Tauri Commands",
        "RUN-006",
        "Whole-workspace regression run: 348 passed, 0 failed.",
        "cargo test --workspace -- --test-threads=2. Up from 347 after the BUG-011 regression test was added.",
        "testing,regression",
    ),
    log(
        "LOG-038",
        "Test Run",
        "Frontend",
        "RUN-007",
        "Frontend suite: 37 passed across 3 files, including 18 new receive-UI tests.",
        "npx vitest run. Three of the new tests were rewritten because they asserted nothing real - see LOG-042.",
        "testing,frontend",
    ),
    log(
        "LOG-039",
        "Bug Found",
        "Tauri App Shell",
        "BUG-009, BUG-010",
        "Running the app for the first time surfaced two more bugs, both silent to every test.",
        "tauri dev failed with 'No package info in the config file' because the config sat beside a virtual workspace "
        "manifest (BUG-009), and once started, the app compiled with ZERO capabilities because the capabilities glob resolved "
        "to a directory that did not exist (BUG-010). cargo build had succeeded throughout, so nothing was ever wrong as far "
        "as the build was concerned. See DEC-017.",
        "build,tooling,security,found-by-running-the-app",
    ),
    log(
        "LOG-040",
        "Bug Found",
        "Tauri App Shell",
        "BUG-011",
        "The app panicked on launch: tokio::spawn in the accept loop, called from Tauri's setup hook with no runtime entered.",
        "'there is no reactor running' - on the startup path, after the listener had bound and the device had advertised "
        "itself. Every test in the suite is #[tokio::test] and enters a runtime, so none of them could have caught it. Fixed "
        "with tauri::async_runtime::spawn, plus a plain #[test] that reproduces the condition.",
        "startup,async,critical,found-by-running-the-app",
    ),
    log(
        "LOG-041",
        "Test Run",
        "Running application",
        "RUN-010",
        "The running app accepts a connection on its advertised port. BUG-005 verified in the real program.",
        "npx tauri dev, then a raw socket to the port from the log: 'Accepted a connection from 127.0.0.1:53841'. Before "
        "this, no test had ever started the application, which is why three live bugs survived a fully green suite.",
        "e2e,verification,found-by-running-the-app",
    ),
    log(
        "LOG-042",
        "Improvement",
        "Frontend",
        "RUN-012",
        "Verified the new tests catch real regressions, by breaking the code on purpose three times.",
        "Each injected regression was caught by exactly the intended test and by no other. Before that, three of the new "
        "tests were rewritten: one spied on QueryClient.prototype.mount and then forced the refetch it claimed to be "
        "testing, one asserted the absence of an alert that was never rendered, and one rendered two component trees in a "
        "single test, making screen queries ambiguous. Reintroducing the BUG-011 fix in Rust was checked the same way.",
        "testing,quality,mutation-testing",
    ),
    log(
        "LOG-043",
        "Improvement",
        "Tooling",
        "FEAT-001",
        "Added scripts/check_command_parity.py as a repeatable check on the command surface.",
        "Compares the generate_handler! registry against the invoke() names in src/lib/ipc.ts in both directions, and "
        "reports duplicate registrations where the second shadows the first. Exits non-zero, so it can run in CI. Currently: "
        "44 registered, 44 called, zero orphans either way.",
        "tooling,parity,ci",
    ),
    log(
        "LOG-044",
        "Documentation",
        "Project",
        "All",
        "Wrote DEVELOPER_GUIDE.md for a newcomer with one Windows laptop and one Mac laptop.",
        "Part 1 is the user path end to end: install on both machines, pair (including the by-key fallback when mDNS is "
        "blocked by the router), choose scopes, capture, send, restore through preflight / prepare / restore plan / report, "
        "and a troubleshooting section ordered by how often each cause is the answer. Part 2 is the code: layout, how to run "
        "and test, how a workspace actually travels, the conventions that will bite, and the known gaps. Every factual claim "
        "about the UI was checked against the source; three were wrong on the first pass and were corrected.",
        "documentation,guide,handoff",
    ),
    log(
        "LOG-045",
        "Correction",
        "Project",
        "EPIC-006, FEAT-028, FEAT-030",
        "EPIC-006 and its transfer features were recorded as Done at 100% while the receive path did not exist.",
        "Every test that existed drove the sending half. A feature marked Done is a claim, and these claims were wrong for "
        "weeks. The three bugs fixed in this session were all invisible to the suite: two were on the launch path and one "
        "was the absence of the thing the feature existed to do. Correcting the board in place, and logging the correction, "
        "so the board and the log agree about when the board was wrong.",
        "correction,process,status",
    ),
    log(
        "LOG-046",
        "Note",
        "Project",
        "SECURITY",
        "No Content Security Policy is set. csp remains null in tauri.conf.json, deliberately.",
        "A CSP that is wrong produces a blank window and no error, and the GUI cannot be loaded in this environment to test "
        "one - there is no assistive access and no screen recording. Setting a CSP without being able to verify the window "
        "still renders would trade a documented gap for an invisible one. This is EPIC-011 work and should be done on a "
        "machine where the window is visible.",
        "security,deferred,known-gap",
    ),
    log(
        "LOG-047",
        "Note",
        "Project",
        "DEPENDENCIES",
        "npm audit reports 7 findings: 1 critical, 1 high, 5 moderate.",
        "All in the dev toolchain - vitest/vite/esbuild, and react-router. Fixing them requires major version bumps the test "
        "suite has not been validated against, and none affect a shipped binary. Worth resolving before this goes anywhere "
        "real; not done in this session because a forced major bump across the test runner is a larger change than the "
        "findings justify mid-session.",
        "dependencies,security,deferred",
    ),
]

# ---------------------------------------------------------------------------
# Append everything
# ---------------------------------------------------------------------------

print("Bugs appended:     ", append("Bugs", bugs))
print("Errors appended:   ", append("Errors", errors))
print("Runs appended:     ", append("Runs", runs))
print("Decisions appended:", append("Decisions", decisions))
print("Features appended: ", append("Features", features))
print("Logs appended:     ", append("Logs", logs))

# ---------------------------------------------------------------------------
# Corrections. The board was wrong; correcting it in place and saying so is the
# point. Leaving a feature at 100% when its receive half did not exist would make
# the tracker worse than useless.
# ---------------------------------------------------------------------------

corrections = [
    (
        "Epics",
        lambda r: r[0].value == "EPIC-006",
        {
            5: "In Progress",
            6: 90,
            11: "Send, discover, authorize, record and the receive path all work and are covered by 11 two-device tests. "
               "Held at 90 rather than Done because no Windows build has been exercised end to end. Was recorded as Done at "
               "100% on 2026-09-26 while the receive path did not exist at all - see BUG-005 and LOG-045.",
        },
        "EPIC-006 was marked Done at 100% with no code that accepted a connection. Corrected on 2026-09-27.",
    ),
    (
        "Features",
        lambda r: r[0].value == "FEAT-028",
        {
            6: "Done",
            7: 100,
            11: NOW,
            15: "Verified 2026-09-27. Was already recorded Done at 100% on 2026-09-26, but the payload was the SENDER's "
                "sealed manifest, which no peer can open (BUG-006), and the receive path did not exist (BUG-005). Both ends "
                "now work and are covered by commands/tests/two_device_transfer.rs. See DEC-012.",
        },
        "FEAT-028 was marked Done while the protocol as shipped could not produce a usable result on the receiving machine.",
    ),
    (
        "Features",
        lambda r: r[0].value == "FEAT-030",
        {
            6: "Done",
            7: 100,
            11: NOW,
            15: "Verified 2026-09-27. The send button was wired but there was no arrival UI and no refusal reporting, so a "
                "transfer that failed produced no message on either machine. Added the arrivals banner, refusal reporting "
                "with reasons, transfer history and the Received badge (FEAT-064 to FEAT-066). See BUG-005 and BUG-007.",
        },
        "FEAT-030 was marked Done with no way for a user to learn that a transfer had arrived or been refused.",
    ),
    (
        "Features",
        lambda r: r[0].value == "FEAT-029",
        {
            6: "Done",
            7: 100,
            11: NOW,
            15: "Verified 2026-09-27 by RUN-010: a raw socket connected to the running app's advertised port was accepted and "
                "logged. Discovery and the manual fallback were already Done; this confirms the advertised port is one "
                "something actually listens on, which is the first time that has been true.",
        },
        "FEAT-029 was marked Done, but the port it advertised was bound and never accepted from.",
    ),
    (
        "Features",
        lambda r: r[0].value == "FEAT-001",
        {
            6: "In Progress",
            7: 95,
            11: NOW,
            15: "2026-09-27: three bugs in this area were found only by starting the application. tauri dev could not find "
                "the app at all (BUG-009), the app shipped with zero capabilities (BUG-010), and the app panicked on launch "
                "(BUG-011). All three were silent to a fully green cargo build and test suite. npx tauri dev now works from "
                "the project root. See DEC-017.",
        },
        "FEAT-001 was marked Done, but the project could not be run in development and the built app could not start.",
    ),
]

for sheet, predicate, updates, why in corrections:
    ws = wb[sheet]
    for row in ws.iter_rows(min_row=2):
        if predicate(row):
            for col, value in updates.items():
                row[col].value = value
            break
    else:
        raise SystemExit(f"correction target not found in {sheet}: {why}")

# Overview
ws = wb["Overview"]
overview = {
    "Test Coverage": (
        "348 Rust tests (11 of them two-device end-to-end across a real socket), 37 frontend tests, 0 build warnings, "
        "command-surface parity clean at 44/44",
        "Done",
        NOW,
        "Up from 325/19. The two-device suite is the only layer that exercises what the app exists to do, and the two "
        "launch-path bugs in this batch (BUG-010, BUG-011) were found by running the application, not by any test.",
    ),
    "Target Platforms": (
        "macOS 13+ (built, launched and verified end to end). Windows 11 implemented but unverified - the build, the "
        "credential store and mDNS behind a Windows firewall have not been exercised. Linux not started.",
        "Partial",
        NOW,
        "Changed from 'Done' to 'Partial'. The platform claim was stronger than the evidence: the app had never been run on "
        "either platform before 2026-09-27.",
    ),
    "Current Phase": (
        "Phase 4 - LAN Transfer: receive path complete, verified in the running application",
        "In Progress",
        NOW,
        "EPIC-006 corrected from Done to In Progress. Phase 5 (Linux, relay, team features) is Not Started.",
    ),
    "Security Review": (
        "Required before Public Beta. No CSP is set. 7 npm audit findings in the dev toolchain. Windows unverified.",
        "Not Started",
        NOW,
        "Changed from 'Done' to 'Not Started'. Review has not happened; these are the things it would have to cover.",
    ),
}

for row in ws.iter_rows(min_row=2):
    field = row[0].value
    if field in overview:
        value, status, updated, notes = overview[field]
        row[1].value = value
        row[2].value = status
        row[3].value = updated
        row[4].value = notes

append(
    "Logs",
    [
        log(
            "LOG-048",
            "Correction",
            "Project",
            "EPIC-006, FEAT-001, FEAT-028, FEAT-029, FEAT-030",
            "Corrected five feature and epic rows from Done at 100% to what the code actually did, and four Overview rows.",
            "EPIC-006 and FEAT-001/028/029/030 were recorded as complete on 2026-09-26 with the receive path absent, the "
            "Tauri CLI unable to find the app, the app building with no capabilities, and the app panicking on launch. "
            "Overview: Test Coverage was 325/19 and is now 348/37; Target Platforms, Current Phase and Security Review moved "
            "to Partial / In Progress / Not Started because each was claiming more than the evidence supported. The log and "
            "the board now agree about when the board was wrong.",
            "correction,process,status",
        )
    ],
)

wb.save("WorkspaceClone_Tracking.xlsx")

print()
for sheet in wb.sheetnames:
    print(f"  {sheet:12} {wb[sheet].max_row - 1:4} rows")
print("saved: WorkspaceClone_Tracking.xlsx")
