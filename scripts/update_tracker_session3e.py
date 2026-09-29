"""Tracker addendum session 3e: BUG-029 (received workspace invisible in the
Workspaces list until a menu change) fixed in the frontend, RUN-030 recorded,
LOG-089 logged, frontend test coverage updated to 100."""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 29, 10, 30).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-29"

wb = openpyxl.load_workbook("WorkspaceClone_Tracking.xlsx")

# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------
bugs = wb["Bugs"]
assert bugs.max_column == 17
bug = [
    "BUG-029",
    "FEAT-051",
    "A workspace that arrives while the Workspaces screen is open stays invisible in the list until the user switches menus",
    "Post-BUG-028 retest on hardware. Sending 'Testing Window - Mac' from the paired Windows "
    "laptop (BISWAJITA) to the Mac completed: the green arrival banner appeared at the top of "
    "the Workspaces screen, but the new workspace was not in the list below. Navigating to "
    "another menu and back made it appear. The workspace is stored correctly by the backend; "
    "the window just never asked for the list again.",
    "Medium",
    "P1",
    "Fixed",
    "Receive a workspace (any direction) while sitting on the Workspaces screen with the list "
    "already fetched, then look at the list without navigating.",
    "The accepted workspace appears in the list as soon as it arrives.",
    "The green banner appeared but the workspace only materialised in the list after a manual "
    "navigation that remounted the screen.",
    "The workspaces query (['workspaces']) is fetched on mount and was never invalidated. The "
    "arrivals poll (['incoming-transfers'], ARRIVALS_POLL_MS) is the channel that proves "
    "something arrived, so a UI with no listener meant the list kept serving its pre-arrival "
    "snapshot until something forced a refetch.",
    "WorkspacesScreen now watches the arrivals poll for a previously unseen *accepted* "
    "arrival with a workspace id and invalidates ['workspaces'] at that moment, so the list "
    "refetches right away. Seen transfer ids are tracked in a ref so a restart or re-render "
    "does not re-invalidate; refusals do not invalidate (nothing was stored).",
    "Yes, at source. New vitest 'puts a workspace that arrived after load into the list "
    "without a remount' drives the poll to flip from empty to a landed arrival and asserts "
    "list_workspaces is invoked again and the new workspace renders with no navigation. "
    "Suite: 100 frontend tests (was 99), tsc --noEmit clean.",
    TODAY,
    TODAY,
    "Assistant",
    "Found by inspecting the live Mac DB after the retest: workspace bbb38119 (name 'Testing "
    "Window - Mac') was stored correctly with source BISWAJITA, so the data was always there "
    "and only the window's cache was stale. Same fix covers the reverse direction.",
]
bugs.append(bug)

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------
runs = wb["Runs"]
assert runs.max_column == 14
run = [
    "RUN-030",
    "Test",
    "Frontend / BUG-029",
    "npx vitest run && npx tsc --noEmit",
    "Passed",
    f"{TODAY} 10:02",
    f"{TODAY} 10:03",
    20,
    0,
    "100 frontend tests passed across 6 files, including the new arrival-refreshes-the-list "
    "test. tsc --noEmit is clean. receive.test.tsx grew 19 -> 20.",
    None,
    None,
    "Assistant",
    "Companion screen for BUG-029: the two 'polling' tests together pin both halves of an "
    "arrival - the banner appears (already covered) and now the list refetches without a "
    "remount. No Rust changes in this session.",
]
runs.append(run)

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------
logs = wb["Logs"]
assert logs.max_column == 9
log_entry = [
    "LOG-089",
    NOW,
    "FIX + ANALYSIS",
    "Frontend / Workspaces Screen",
    "BUG-029,FEAT-051",
    "BUG-029 fixed; Windows->Mac retest analysed: arrival correct, capture was near-empty",
    "BUG-029 resolved in WorkspacesScreen.tsx: a newly-seen accepted arrival in the "
    "incoming-transfers poll now invalidates ['workspaces'], so a workspace that arrives "
    "while the list is open appears without a menu change. New vitest pins it; suite is now "
    "100 frontend tests, tsc clean. On the hardware retest, the green banner rendered "
    "'Testing Window - Mac' and the Mac DB stores it with source BISWAJITA - so the reported "
    "empty-looking banner was not reproducible from current data and needs the screenshot. "
    "The 'restore shows nothing' report was root-caused on the sender side: the unsealed "
    "received manifest for bbb38119 has projects=0 and one optional terminal-session "
    "application, and no workspace_files row exists - the Windows capture selected no "
    "projects, so no file archive was ever carried and the restore plan legitimately has "
    "nothing. DEVELOPER_GUIDE gains a 'received workspace restores as nothing selected' "
    "entry pointing at the sender's project scan (default roots ~/projects, ~/Documents) and "
    "Settings -> Project locations. Next: user to reattach the two screenshots and on "
    "Windows to set project roots, tick projects, re-capture and re-send.",
    "Assistant",
    "frontend,arrival,workspaces-list,bug,analysis,restore,capture",
]
logs.append(log_entry)

# ---------------------------------------------------------------------------
# Overview: test coverage + its timestamp
# ---------------------------------------------------------------------------
overview = wb["Overview"]
for row in range(2, overview.max_row + 1):
    field = overview.cell(row=row, column=1).value
    if field == "Test Coverage":
        overview.cell(row=row, column=2).value = (
            "405 Rust tests (12 of them two-device end-to-end across a real socket; 13 in the "
            "new files crate; 11 connect-candidate and dual-stack listener tests in the "
            "network crate), 100 frontend tests, 0 build warnings, command-surface parity "
            "clean at 44/44"
        )
        overview.cell(row=row, column=4).value = f"{TODAY} 10:30"

wb.save("WorkspaceClone_Tracking.xlsx")
print("saved:")
print("  bugs rows    :", bugs.max_row - 1)
print("  runs rows    :", runs.max_row - 1)
print("  logs rows    :", logs.max_row - 1)