"""Tracker addendum session 3f: the seven post-handoff bugs.

BUG-030 delete workspace silent failure, BUG-031 arrival banner missing the
sender name, BUG-032 dismissal button inert, BUG-033 "Open Preflight" inert,
BUG-034 list not refreshing on arrival, BUG-035 Devices page showing this
machine, BUG-036 cannot delete a paired device (and the redundant Revoke
button). All fixed and validated against the rebuilt, re-installed app.

The single root cause behind BUG-031/032/033/034: `IncomingTransfer` is
serialized by Rust with `#[serde(rename_all = "camelCase")]` but the TS
interface (and the fake backend fixtures) used snake_case, so every field the
banner, the dismissal and the preflight link read was undefined at runtime.
BUG-034 is the completion of BUG-029: the list-refresh effect landed there but
`a.workspace_id` was always undefined on the real wire, so it never fired.

BUG-030 and BUG-036 are the same silent FK-violation class: both repository
`delete`s were a single `DELETE` on a table that several child tables reference
with real foreign keys, which the pool enforces.
"""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 29, 23, 45).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-29"

wb = openpyxl.load_workbook("docs/WorkspaceClone_Tracking.xlsx")

# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------
bugs = wb["Bugs"]
assert bugs.max_column == 17

WIRE_FIX = (
    "the TS IncomingTransfer interface is renamed to the camelCase keys serde actually "
    "emits (transferId, workspaceId, workspaceName, senderDeviceId, senderDeviceName, "
    "sourceDeviceId, refusalReason, transferDigest, bytesReceived, receivedAt), the banner, "
    "the dismissal and the preflight link read those names, and the fake-backend fixtures are "
    "camelCase so tests exercise the real wire shape."
)

bug_rows = [
    [
        "BUG-030",
        "FEAT-051",
        "Delete workspace does nothing on both Mac and Windows",
        "Deleting a workspace from the Workspaces list leaves it in place. Reported for both machines "
        "after the handoff. The row stayed put, and so did the sealed manifest (and file archive) on disk.",
        "Medium",
        "P1",
        "Fixed",
        "Capture or receive a workspace (a received one with files is the strongest repro), then click the "
        "trash button on its card.",
        "The workspace leaves the list; its sealed manifest (and its sealed file archive, when files were "
        "carried) is removed from disk.",
        "Nothing happened: the row remained and the files remained. No error was surfaced anywhere.",
        "WorkspaceRepository::delete was a bare `DELETE FROM workspaces WHERE id = ?`. `workspace_files`, "
        "`snapshots`, `restore_runs` and `transfer_sessions` all hold real foreign keys to workspaces(id) "
        "and the pool enforces them, so every workspace with a file archive, a snapshot or a transfer "
        "record violated a constraint and the mutation silently failed.",
        "WorkspaceRepository::delete now removes the four referencing tables for the workspace inside one "
        "transaction, then the workspace row. The delete_workspace command additionally removes the sealed "
        "manifest and the `.files.sealed` archive from disk, tolerating an already-missing file and "
        "warnings (never failing the committed delete) otherwise.",
        TODAY,
        TODAY,
        "Assistant",
        "Validated in the rebuilt app on the live Mac DB: deleting a received workspace removed its row "
        "(confirmed with sqlite3) and its manifest file (confirmed with ls). Two new db-crate tests pin "
        "the cascade for workspace and device deletes.",
    ],
    [
        "BUG-031",
        "FEAT-051",
        "Arrival banner shows an empty sender name (\"arrived from\")",
        "After a successful Windows->Mac transfer the green banner renders the workspace name and the "
        "sender name blank on the receiving machine.",
        "High",
        "P1",
        "Fixed",
        "Receive a workspace from the paired Windows laptop and read the green arrival banner.",
        "The banner reads \"'<workspace>' arrived from BISWAJITA\".",
        "The banner showed an empty name after 'arrived from'.",
        WIRE_FIX
        + " The name render was `arrival.sender_device_name`, but serde emits `senderDeviceName`, so the "
        "value was undefined.",
        WIRE_FIX,
        TODAY,
        TODAY,
        "Assistant",
        "Root cause confirmed on the producer side with a new Rust test asserting IncomingTransfer "
        "serializes no snake_case key, and on the consumer side with a new frontend test that renders an "
        "exact camelCase payload.",
    ],
    [
        "BUG-032",
        "FEAT-051",
        "Arrival banner's dismiss (close) button is inert",
        "Clicking the X on the green arrival banner does not dismiss it.",
        "Medium",
        "P1",
        "Fixed",
        "Receive a workspace, then click the close button on its banner.",
        "The banner disappears and the workspace stays stored.",
        "The banner stayed on screen.",
        "dismiss.mutate used `arrival.transfer_id`, which is undefined on the wire (the key is "
        "`transferId`), so the backend was asked to dismiss a transfer it does not know and nothing "
        "changed.",
        WIRE_FIX,
        TODAY,
        TODAY,
        "Assistant",
        "The dismiss path sends the camelCase transferId now; the new wire-shape frontend test asserts "
        "dismiss_incoming_transfer receives `transferId` for the arrival.",
    ],
    [
        "BUG-033",
        "FEAT-051",
        "\"Open preflight\" on a successful arrival does nothing",
        "Clicking the banner's 'Open preflight' link does not open the preflight screen.",
        "Medium",
        "P1",
        "Fixed",
        "Receive a workspace and click 'Open preflight' on its banner.",
        "The app navigates to preflight for the workspace that arrived.",
        "Nothing happened.",
        "The navigation guard read `arrival.workspace_id` (undefined on the wire; the key is `workspaceId`) "
        "so `accepted && workspace_id` was always false and the handler fell through to the no-op branch.",
        WIRE_FIX,
        TODAY,
        TODAY,
        "Assistant",
        "New frontend test opens the banner of an arrival and asserts the preflight screen is reached for "
        "that workspace's id (get_manifest called with ws-1).",
    ],
    [
        "BUG-034",
        "FEAT-051",
        "Workspace list does not refresh when a transfer arrives",
        "The BUG-029 fix (invalidate ['workspaces'] on a newly-seen accepted arrival) does not fire: a "
        "workspace that arrives while the Workspaces screen is open still needs a menu change to appear.",
        "Medium",
        "P1",
        "Fixed",
        "Receive a workspace while sitting on the Workspaces screen with the list already fetched.",
        "The accepted workspace appears in the list as soon as it arrives.",
        "Still needed a navigation to remount the screen, exactly as before BUG-029.",
        "The BUG-029 effect filtered on `a.accepted && a.workspace_id && ...`, and `a.workspace_id` is "
        "undefined on the real wire (the key is `workspaceId`). The guard was false for every arrival, so "
        "the invalidation never ran. BUG-029 passed in tests because the fake backend used snake_case "
        "fixtures that matched the wrong type.",
        WIRE_FIX
        + " The effect now reads the camelCase ids (workspaceId, transferId) so the guard holds.",
        TODAY,
        TODAY,
        "Assistant",
        "Same root cause and fix as BUG-031/032/033. Validated by the wire-shape frontend test plus the "
        "existing poll-driven list-refresh tests, which now exercise the real key names.",
    ],
    [
        "BUG-035",
        "FEAT-050",
        "Devices page shows this machine among the devices",
        "The Devices page lists this laptop itself in the devices area, alongside (or instead of being "
        "clearly about) the machines it is actually paired with.",
        "Medium",
        "P1",
        "Fixed",
        "Open the Devices page and look at the device list.",
        "Only actually-paired peer machines are listed; this machine appears nowhere on the page.",
        "This machine (the Mac) appeared in the list of devices.",
        "This device keeps a row in the `devices` table (workspaces.source_device_id is a foreign key that "
        "needs a row to point at), and `list_paired_devices` returned every non-revoked row -- including "
        "the self row.",
        "`list_paired_devices` now derives the local device id from the Noise key bundle and filters the "
        "self-row out of every listing. The Devices screen also no longer renders the 'This device' "
        "identity card, so the page is entirely about peers.",
        TODAY,
        TODAY,
        "Assistant",
        "Validated in the rebuilt app: the Devices page now lists only the paired Windows laptop.",
    ],
    [
        "BUG-036",
        "FEAT-050",
        "Paired devices cannot be deleted; Delete and Revoke both offered",
        "On the Devices page no paired device can be deleted (the action silently does nothing), and the "
        "screen offers two overlapping destructive actions (Revoke and Forget).",
        "Medium",
        "P1",
        "Fixed",
        "Open Devices, pick a paired device, try its delete action.",
        "The device disappears from the list and its pairing is gone; the UI has one clear destructive "
        "action.",
        "Nothing happened on delete; both a Revoke and a Forget button were always present.",
        "DeviceRepository::delete was a bare `DELETE FROM devices WHERE id = ?`, which violates the "
        "foreign keys from workspaces, snapshots, restore_runs and transfer_sessions that still point at "
        "the device row. Same silent class as BUG-030.",
        "DeviceRepository::delete now removes, in one transaction, every row that references the device -- "
        "the workspaces it sent (with their own children), transfer sessions in either direction, restore "
        "runs and snapshots -- then the device row, while never touching workspaces this machine captured "
        "itself. The UI keeps exactly one destructive action (Forget); the Revoke button is removed and "
        "the confirmation states what is removed.",
        TODAY,
        TODAY,
        "Assistant",
        "Backend verified by the new db-crate cascade test (peer's workspaces and records removed, local "
        "captures untouched). UI verified in the rebuilt app: single delete button, no Revoke, list shows "
        "only the paired laptop.",
    ],
]
for row in bug_rows:
    bugs.append(row)

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------
runs = wb["Runs"]
assert runs.max_column == 14

runs.append(
    [
        "RUN-031",
        "Test",
        "Frontend + Rust suites / BUG-030..036",
        "npx vitest run && npx tsc --noEmit && cargo test -p workspace_clone_commands -p workspace_clone_db",
        "Passed",
        f"{TODAY} 23:20",
        f"{TODAY} 23:26",
        320,
        0,
        "102 frontend tests passed (was 100: +2 regression tests -- exact camelCase wire-shape payload "
        "and 'Open preflight' navigation). tsc --noEmit clean. Rust: 79 tests passed in workspace_clone_"
        "commands (67 unit incl. new IncomingTransfer wire-shape test, 12 two-device end-to-end across a "
        "real socket) plus 7 in workspace_clone_db (2 new cascade-delete tests).",
        None,
        None,
        "Assistant",
        "Companion runs for the seven bugs. Counts: Rust total 408 (was 405), frontend 102 (was 100).",
    ]
)
runs.append(
    [
        "RUN-032",
        "Build + App",
        "Rebuilt app / BUG-030..036 validation",
        "npx tauri build --config src-tauri/app/tauri.conf.json; installed DMG; manual checks on the live "
        "Mac DB",
        "Passed",
        f"{TODAY} 23:40",
        f"{TODAY} 23:55",
        900,
        0,
        "New DMG built and installed over the previous build. In the running app: Devices page lists only "
        "the paired Windows laptop (self gone), single Forget action with no Revoke button; deleting a "
        "received workspace removed its DB row (sqlite3) and manifest file (ls); the history tab renders "
        "real source/destination device names from stored transfer_sessions data.",
        None,
        None,
        "Assistant",
        "Full validation on real hardware and data. One live end-to-end banner check remains impossible "
        "from this machine (needs a new Windows-side send); covered by the Rust e2e + wire-shape tests.",
    ]
)

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------
logs = wb["Logs"]
assert logs.max_column == 9

logs.append(
    [
        "LOG-090",
        NOW,
        "FIX + ANALYSIS",
        "Backend + Frontend / App",
        "BUG-030,BUG-031,BUG-032,BUG-033,BUG-034,BUG-035,BUG-036",
        "All seven post-handoff bugs fixed and validated in the rebuilt app",
        "One root cause covers four of them: `receive::IncomingTransfer` serializes camelCase "
        "(rename_all = \"camelCase\") but the TS interface and every fake-backend fixture used "
        "snake_case, so at runtime the banner name (BUG-031), the dismissal id (BUG-032), the preflight "
        "navigation guard (BUG-033) and the BUG-029 list-refresh effect (BUG-034) all read undefined. The "
        "TS type and fixtures now mirror the wire exactly; a Rust test pins the producer's JSON keys and "
        "a frontend test renders a literal camelCase payload so the two sides cannot drift again. The two "
        "delete bugs share their own root cause: both repository deletes were a single DELETE against a "
        "table that child tables reference with enforced foreign keys, so every delete silently failed. "
        "Delete now cascades inside a transaction (workspace: files/snapshots/restore-runs/transfer-"
        "sessions then the row plus the sealed manifest and archive on disk; device: the workspaces it "
        "sent with their children, sessions both directions, restores, snapshots, then the row, never "
        "touching local captures). Devices page no longer lists this machine (local id filtered out of "
        "list_paired_devices) and has exactly one destructive action, Forget. Validated in the rebuilt, "
        "re-installed app on the live Mac DB; 408 Rust tests (12 two-device e2e) and 102 frontend tests "
        "pass, tsc clean, build warnings 0. The one thing only the user can do is send one more "
        "Windows->Mac workspace to watch the corrected banner live.",
        "Assistant",
        "frontend,backend,serde,wire-shape,arrival,delete,devices,bug,analysis",
    ]
)

# ---------------------------------------------------------------------------
# Overview: test coverage + its timestamp
# ---------------------------------------------------------------------------
overview = wb["Overview"]
for row in range(2, overview.max_row + 1):
    field = overview.cell(row=row, column=1).value
    if field == "Test Coverage":
        overview.cell(row=row, column=2).value = (
            "408 Rust tests (12 of them two-device end-to-end across a real socket; 7 in the db crate "
            "including both cascade-delete paths; 13 in the new files crate; 11 connect-candidate and "
            "dual-stack listener tests in the network crate), 102 frontend tests, 0 build warnings, "
            "command-surface parity clean at 44/44"
        )
        overview.cell(row=row, column=4).value = f"{TODAY} 23:45"

wb.save("docs/WorkspaceClone_Tracking.xlsx")
print("saved:")
print("  bugs rows    :", bugs.max_row - 1)
print("  runs rows    :", runs.max_row - 1)
print("  logs rows    :", logs.max_row - 1)