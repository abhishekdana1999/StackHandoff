"""Tracker addendum session 3g: git working-tree changes travel as a patch.

BUG-037: restoring a workspace whose project is a dirty git clone re-wrote the
whole tree over the destination checkout, so `git status` there listed every
tracked file as modified (146 files for the user's one change). Root cause: the
archive does not carry file modes, and the restore overwrote the checkout
wholesale, so on a same-commit baseline every executable file looked changed.

Fix: the capture now records the dirty worktree's delta as a `git apply`-able
patch (`git diff HEAD` -- staged and unstaged together -- plus one new-file
hunk per untracked, non-ignored file; the snapshot denylist is applied to the
untracked list and the user's index is never touched). The patch travels inside
the manifest (new optional `GitInfo.patch`, backward compatible). On restore the
planner emits a `git-apply-<id>` step instead of whole-tree extraction for
patched projects, and the git adapter applies the delta with `git apply
--binary`; a patch that does not apply (different base commit) reports the
remedy (`git checkout <commit>`) as a Manual step. The destination's `git
status` therefore shows exactly the delta the user made.
"""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 30, 10, 30).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-30"

wb = openpyxl.load_workbook("WorkspaceClone_Tracking.xlsx")

# ---------------------------------------------------------------------------
# Bugs
# ---------------------------------------------------------------------------
bugs = wb["Bugs"]
assert bugs.max_column == 17

bug_rows = [
    [
        "BUG-037",
        "FEAT-014,FEAT-042,FEAT-079",
        "Restore of a dirty git clone rewrote the whole tree: 146 files for one change",
        "A workspace whose project is a git checkout with uncommitted changes was "
        "restored by overwriting the whole tree at the destination, so `git status` "
        "there listed every tracked file as modified. The user's real delta was one "
        "file; the restore produced 146. Reported as the transfer/restore follow-up: "
        "'make sure to take git changes using the patch'.",
        "High",
        "P1",
        "Fixed",
        "Capture a workspace that includes a dirty git repo (one modified tracked "
        "file is enough), restore it into a same-commit clone, and run `git status "
        "--short` there.",
        "`git status --short` at the destination shows exactly the source's "
        "uncommitted delta (one file), nothing more.",
        "`git status --short` listed every tracked file as modified (146 files): "
        "the whole tree had been extracted over the checkout.",
        "The file archive does not carry file modes, and restore's extract_project "
        "writes every file with default permissions, so executable files in a "
        "same-commit checkout reported mode changes en masse. The dirty state had "
        "explicitly never been captured (dirty_state_captured was always false), so "
        "there was no way to reproduce the delta on the other side.",
        "Capture now records the worktree delta as a git-applyable patch: `git diff "
        "HEAD --binary` (staged + unstaged net) plus one `git diff --no-index "
        "/dev/null <file>` new-file hunk per untracked, non-ignored file, with the "
        "snapshot denylist applied and the source index never mutated (no `git add "
        "-N`). The patch travels in new optional `GitInfo.patch`. For patched "
        "projects the planner plans `git-apply-<id>` (adapter git, required "
        "false) and skips whole-tree extraction; the adapter applies via `git apply "
        "--binary --whitespace=nowarn` from a temp patch file, reporting `git "
        "checkout <source_commit>` as the remedy when the patch does not apply. A "
        "clean repo -- or one whose delta could not be captured -- keeps the old "
        "whole-tree copy.",
        TODAY,
        TODAY,
        "Assistant",
        "All suites green: 419 Rust tests (6 new adapter round-trip tests on real "
        "repos -- capture-to-apply gives exact `git status`, denylist respected, "
        "clean repo has no patch, mismatched base reports the remedy -- plus 2 new "
        "planner tests for the extract-skip and 1 new two-device e2e proving the "
        "patch survives the real socket transfer and restores to the exact change), "
        "102 frontend tests, tsc --noEmit clean, 0 build warnings.",
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
        "RUN-033",
        "Test",
        "Rust + Frontend suites / BUG-037 git patch transport",
        "cargo test && npx tsc --noEmit && npx vitest run",
        "Passed",
        f"{TODAY} 10:10",
        f"{TODAY} 10:30",
        480,
        0,
        "419 Rust tests passed (was 408): 6 new adapter tests capture a dirty "
        "worktree as a patch, apply it to an identical clone and assert `git status "
        "` shows exactly the source delta; a mismatched-base clone reports 'git "
        "checkout <commit>' as a Manual remedy; the denylist keeps .env and "
        "node_modules out of the patch; a clean worktree captures no patch; "
        "plan_restore emits git-apply only when a delta exists. 2 new planner tests "
        "(patched project restores via git-apply with no extract-files step; clean "
        "project keeps the tree copy). 1 new two-device e2e: a dirty repo's delta "
        "survives the real socket + sealing round trip and restores to exactly "
        "[' M note.txt', '?? new.txt']. tsc --noEmit clean, 102 frontend tests "
        "passed, 0 build warnings.",
        None,
        None,
        "Assistant",
        "One follow-up remains out of reach from this machine: a true live transfer "
        "with the Windows laptop (requires the user to send). Covered by the Rust "
        "e2e + adapter round-trip tests.",
    ]
)

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------
logs = wb["Logs"]
assert logs.max_column == 9

logs.append(
    [
        "LOG-091",
        NOW,
        "FIX + ANALYSIS",
        "Backend / Adapters + Restore",
        "BUG-037",
        "Git working-tree changes now travel as a patch, so restore changes one file instead of 146",
        "Capture records the dirty worktree's delta: `git diff HEAD --binary` "
        "(staged+unstaged net) plus a `git diff --no-index /dev/null <file>` "
        "new-file hunk per untracked, non-ignored file, filtering through the "
        "snapshot denylist and never mutating the user's index (no `git add -N`). "
        "The delta rides in new optional `GitInfo.patch` (serde default, "
        "backward compatible; the old honest lies -- dirty_state_captured always "
        "false -- are retired). Restore: patched projects skip whole-tree "
        "extraction and get a `git-apply-<id>` step run by the git adapter with "
        "`git apply --binary --whitespace=nowarn` from a temp patch file; a patch "
        "that cannot apply (base mismatch) is a Manual step reading 'git checkout "
        "<source_commit>', never a silent drop. Three bugs worth remembering: "
        "`git diff --no-index` exits 1 when files differ, so the capture must "
        "accept exit 1 or drop every untracked hunk; `str::trim` on the patch "
        "strips the final hunk line's newline and git rejects the patch as "
        "corrupt at that line; `ls-files` must use `-z` or the whole listing "
        "comes back as one newline-swallowing entry. Verified end to end: adapter "
        "capture-to-apply round trips, planner extract-skip, and a real two-device "
        "socket transfer whose restored checkout shows exactly the source's delta.",
        "Assistant",
        "bug,git,patch,restore,delta,transfer,analysis",
    ]
)

# ---------------------------------------------------------------------------
# Decisions
# ---------------------------------------------------------------------------
decisions = wb["Decisions"]
assert decisions.max_column == 12

decisions.append(
    [
        "DEC-034",
        TODAY,
        "A dirty git worktree travels as a `git apply`-able patch; restore applies it instead of overwriting the tree",
        "Capturing a workspace whose project is a dirty git clone and extracting "
        "the whole tree at restore made `git status` on the destination list every "
        "tracked file as modified (146 files for a one-file change): the archive "
        "carries no file modes, and the baseline at the destination may differ.",
        "Overwrite the tree as before; capture only the dirty flag (status quo); "
        "ship the patch and apply it at restore",
        "Record `git diff HEAD --binary` plus one new-file hunk per untracked, "
        "non-ignored file in the manifest (`GitInfo.patch`); planner plans "
        "`git-apply-<id>` and skips extraction for patched projects; the git "
        "adapter applies with `git apply --binary --whitespace=nowarn`",
        "The delta is the honest thing to move: it is small, it is exactly what "
        "the user changed, and git apply's path validation keeps every write "
        "inside the chosen destination. Untracked files travel as hunks rather "
        "than via `git add -N` because a capture must never rewrite the user's "
        "index, and the snapshot denylist filters the untracked list so a secret "
        "cannot leak through a hunk the archive would refuse. A clean worktree "
        "captures no patch and restores exactly as before, so the change is "
        "strictly additive.",
        "Dirty git repos restore to the exact uncommitted state instead of a "
        "mass re-overwrite. Adds an optional manifest field; manifests from older "
        "builds are unchanged. A patch that cannot apply (different base commit) "
        "is reported with the remedy, not silently dropped.",
        "If restore ever needs to land local, uncommitted work on a checkout "
        "whose history diverged, the patch apply would need a 3-way merge "
        "strategy rather than the current apply-only.",
        "Assistant",
        "Decided",
        "BUG-037",
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
            "419 Rust tests (was 408). The +11: 6 git-adapter tests proving a "
            "dirty worktree captures as an appliable patch and restore applies "
            "exactly that delta (`git status` matches), a mismatched base reports "
            "the checkout remedy, the denylist keeps secrets out of the patch, "
            "and a clean worktree carries no patch; 2 planner tests for the "
            "extract-skip on patched projects; 1 two-device e2e sending a dirty "
            "repo's delta over a real socket and restoring it to exactly "
            "'M note.txt' + '?? new.txt'; 102 frontend tests; 0 build warnings."
        )
        overview.cell(row=row, column=4).value = f"{TODAY} 10:30"

# ---------------------------------------------------------------------------
# Features: note the changed restore behavior where it lives
# ---------------------------------------------------------------------------
features = wb["Features"]
notes = {
    "FEAT-014": "Git Adapter now captures the dirty worktree as a git-applyable "
    "patch (GitInfo.patch) and applies it on restore via `git apply --binary`; "
    "the whole-tree overwrite for dirty repos is gone. 2026-09-30, BUG-037.",
    "FEAT-042": "Git Safe Restore: patched projects restore via a git-apply step "
    "instead of whole-tree extraction, so the destination's `git status` shows "
    "exactly the source's uncommitted delta. 2026-09-30, BUG-037.",
    "FEAT-079": "Restore extracts only for projects without a captured git patch "
    "(clean repos keep the archive copy); dirty repos apply their delta instead. "
    "2026-09-30, BUG-037.",
}
for row in range(2, features.max_row + 1):
    fid = features.cell(row=row, column=1).value
    if fid in notes:
        existing = features.cell(row=row, column=16).value or ""
        features.cell(row=row, column=16).value = (
            f"{existing} | {notes[fid]}" if existing else notes[fid]
        )

wb.save("WorkspaceClone_Tracking.xlsx")
print("saved:")
print("  bugs rows    :", bugs.max_row - 1)
print("  runs rows    :", runs.max_row - 1)
print("  logs rows    :", logs.max_row - 1)
print("  decisions    :", decisions.max_row - 1)