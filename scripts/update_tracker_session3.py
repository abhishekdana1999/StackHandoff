"""
Third tracker update: real file transfer end to end.

This session made a captured workspace carry the actual project files, not just
the manifest. Everything below it stood on was real: the manifest already
travelled inside an encrypted, chunked, acked channel, and the receive path
already sealed arrivals with its own key. What was missing was the point of a
"clone" -- the files. A file changed on the Mac had to exist, identical, on the
Windows laptop after send + restore.

The work split into four pieces and one correction:

  1. A new `files` crate: snapshot (sorted deterministic tar, denylist at any
     depth, per-file 128 MiB cap and a 512 MiB total, symlinks skipped, every
     skip reported), archive (extraction that treats every entry as hostile),
     and transit (the wire envelope). 13 tests, zero warnings.
  2. Capture: at capture time the selected projects are walked, tarred, sealed
     with the storage key and stored at files/<id>.files.sealed, with counts in
     a new workspace_files table. Re-sending re-sends the same copy.
  3. The wire: the manifest is now followed by the archive inside an opaque
     envelope (WCFB1 magic, u32 LE manifest length, manifest, archive). Payloads
     without the magic read back as manifest-only, so every transfer made before
     this feature still arrives. The receive side unseals the archive with its
     own key and stores it exactly as a local capture would.
  4. Restore: the planner emits a required extract-files step per placed project,
     ordered after map-path; the executor restores each project's entries into
     its mapped folder, overwriting what is there (the snapshot is
     authoritative), refusing unsafe entries, and reporting counts and bytes.
     The two-device e2e now proves a changed file restores on the other machine.
  5. Correction: FEAT-045 was marked Done at 100% for "file transfer" while only
     the manifest *policy flag* existed. The transport is the four features
     added below (FEAT-076..080); FEAT-045's note now says exactly what it was.

Append-only, as before. Nothing is deleted; the one row that now means
something narrower is corrected in place, and the correction is logged.

Full gates at the end: 394 Rust (12 two-device, 13 files), 99 frontend, tsc
clean, parity and design-token checks clean.
"""

import datetime as dt

import openpyxl

NOW = dt.datetime(2026, 9, 28, 21, 45).strftime("%Y-%m-%d %H:%M")
TODAY = "2026-09-28"

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
# Features
# ---------------------------------------------------------------------------

features = [
    [
        "FEAT-076",
        "EPIC-008",
        "File snapshot at capture (denylist + size caps)",
        "At capture time every selected project's folder is walked and built into one sorted tar "
        "containing everything minus the denylist: version-control internals (.git, .svn), dependency and "
        "build output (node_modules, target, dist, build, .next, .nuxt, .output, .turbo, .parcel-cache), "
        "caches (.cache, __pycache__), virtualenvs (.venv, venv), Pods, DerivedData, coverage, plus the "
        "secret files: .env and .env.*, *.pem, *.key, id_rsa/id_ed25519/id_ecdsa/id_dsa, Thumbs.db, "
        "desktop.ini, databases (*.db/-shm/-wal, *.sqlite/*.sqlite3) and logs (*.log), all matched at any "
        "depth. Symlinks are skipped. A single file over 128 MiB is skipped with a reason; once the total "
        "reaches 512 MiB the walk stops and reports the overflow. The snapshot is the copy that will be "
        "re-sent, so re-sending is idempotent.",
        "Core / Files",
        "P0",
        "Done",
        100,
        1,
        1,
        TODAY,
        f"{TODAY} 21:30",
        "Assistant",
        "DEC-031",
        "Equal folders produce byte-identical archives (sorted entry order, no compression). Every "
        "denylisted name is excluded at any depth. An over-cap file is skipped and named. The total cap "
        "stops the walk and sets overflow. Every skip and the overflow surface as capture warnings.",
        "New crate src-tauri/files. The capture screen's 'what is never captured' notice and the developer "
        "guide now state the denylist explicitly; what is excluded is regenerable build output or secrets.",
    ],
    [
        "FEAT-077",
        "EPIC-006",
        "File archive on the wire (opaque envelope, chunking unchanged)",
        "send_workspace now wraps the payload: MAGIC (WCFB1) then a u32 LE manifest length, then the "
        "manifest JSON, then the archive tar, sent inside the existing framed, acked, encrypted channel. "
        "The wire envelope is opaque to transport; nothing about chunk framing or the Noise channel "
        "changed. Payloads without the magic read back as manifest-only, so every pre-feature transfer "
        "still arrives exactly as before. The accept side's incoming budget is raised to the 512 MiB total "
        "cap plus 32 MiB of headroom.",
        "Network / Transfer",
        "P0",
        "Done",
        100,
        1,
        1,
        TODAY,
        f"{TODAY} 21:30",
        "Assistant",
        "DEC-032",
        "A workspace with files arrives and opens with the receiver's key; a legacy manifest-only payload "
        "arrives byte-for-byte as before. The e2e suite proves both directions.",
        "The receive side unseals the archive with its own key, so a received workspace is "
        "indistinguishable from a local capture. No relay or additional key exchange was introduced.",
    ],
    [
        "FEAT-078",
        "EPIC-006",
        "Sealed archive storage + workspace_files table",
        "The sealed archive lives at <data_dir>/files/{id}.files.sealed, sealed with the storage key like "
        "the manifest. Migration 003 adds the workspace_files table; the repository records the sealed "
        "path, file count, byte count and archive format. The receive path unseals the arrived archive "
        "with the receiving machine's own key and stores it the same way, and restore reads it back by "
        "workspace id through the same repository.",
        "Tauri Commands / DB",
        "P0",
        "Done",
        100,
        1,
        1,
        TODAY,
        f"{TODAY} 21:30",
        "Assistant",
        "DEC-032",
        "A captured workspace and a received workspace are stored identically; the files restore by "
        "workspace id with no extra configuration.",
        "The archive for a workspace the denylist emptied or that overflowed is still recorded, so the "
        "board and the restore report can say what a workspace carries.",
    ],
    [
        "FEAT-079",
        "EPIC-008",
        "Restore extracts project files",
        "The restore planner adds a required extract-files step for every project with a destination, "
        "dependent on its map-path step and ordered before anything that inspects or opens the project. "
        "The executor hands the unsealed tar to the archive extractor, which writes each project's entries "
        "into its mapped folder overwriting what is there (the snapshot is authoritative), refuses "
        "traversal names, absolute paths, backslashes, drive letters, symlinks, reserved Windows device "
        "names and oversized entries, and caps the total. The step message reports files written, bytes, "
        "refusals and overflow; a workspace captured without files reports an honest success.",
        "Core / Restore",
        "P0",
        "Done",
        100,
        1,
        1,
        TODAY,
        f"{TODAY} 21:30",
        "Assistant",
        "DEC-032",
        "A project's changed file restores over an existing file. Unsafe entries are refused, not "
        "followed or written. 43 restore tests incl. 2 extract tests; the two-device e2e asserts the "
        "changed file's content on the receiving machine.",
        "ExtractFiles is a distinct RestoreActionType, so the preview and report render and gate it like "
        "any other step instead of it being a hidden side effect.",
    ],
    [
        "FEAT-080",
        "EPIC-009",
        "File transfer defaulted on; restore steps rendered",
        "New captures default to file transfer policy 'all' -- the capture screen is the single place the "
        "decision is made, and a fresh capture that silently reverted to manifest-only would defeat the "
        "feature no matter what the screen said. The restore preview and restore report render the "
        "extract-files step with its own icon and status, and classify it as an execution step so the "
        "'commands will run' warning stays honest. The capture screen's 'What is never captured' notice "
        "now states the file snapshot's denylist and corrects the stale claim that working-tree files are "
        "never copied.",
        "Frontend / Screens",
        "P0",
        "Done",
        100,
        1,
        1,
        TODAY,
        f"{TODAY} 21:30",
        "Assistant",
        "FEAT-076,FEAT-079",
        "A capture built from a fresh selection carries files by default. RestoreActionType includes "
        "extract_files; preview and report render it; 'commands will run' covers it.",
        "The size caps and the denylist are what keep 'all' safe; manifest-only remains available in the "
        "policy type for when per-project selection or a no-files choice is built into the UI.",
    ],
]

# FEAT-045 was marked Done for 'file transfer' when only the manifest policy flag
# existed. Correct in place (append-only: nothing deleted), and log the correction.
for row in wb["Features"].iter_rows(min_row=2):
    if row[0].value == "FEAT-045":
        current = row[15].value or ""
        row[15].value = (
            current
            + " [Corrected 2026-09-28] This row stood for the manifest *policy flag* only. The real "
            "transport is FEAT-076 (snapshot), FEAT-077 (wire), FEAT-078 (storage) and FEAT-079 "
            "(restore); the UI wiring is FEAT-080. See LOG-085."
        )
        break

# ---------------------------------------------------------------------------
# Runs
# ---------------------------------------------------------------------------

runs = [
    [
        "RUN-024",
        "Test",
        "Rust / whole workspace",
        "cargo test --workspace -- --test-threads=2",
        "Passed",
        f"{TODAY} 21:20",
        f"{TODAY} 21:22",
        90,
        0,
        "394 passed, 0 failed, 0 build warnings. Per layer: adapters 62, ipc_selection_contract 6, "
        "commands 66, two_device_transfer 12, core 16, crypto 24, db 5, files 13, network 68, loopback "
        "8, preflight 71, restore 43. Up from 377 by 17: the files crate (13) plus the files e2e in the "
        "two-device suite (1) plus restore's two extract tests and one further command test.",
        "",
        "",
        "Manual",
        "The new e2e a_workspace_with_files_arrives_with_its_archive_and_restores sends a workspace "
        "whose file was changed on the sending side, and asserts the changed content is present after "
        "receive + restore.",
    ],
    [
        "RUN-025",
        "Test",
        "Frontend",
        "npx vitest run",
        "Passed",
        f"{TODAY} 21:18",
        f"{TODAY} 21:19",
        7,
        0,
        "6 test files, 99 passed, 0 failed.",
        "",
        "",
        "Manual",
        "No test asserted a manifest-only default, so the policy default change to 'all' stayed green.",
    ],
    [
        "RUN-026",
        "Test",
        "TypeScript",
        "npx tsc --noEmit",
        "Passed",
        f"{TODAY} 21:18",
        f"{TODAY} 21:18",
        30,
        0,
        "Clean, no output, no errors.",
        "",
        "",
        "Manual",
        "Runs over the edited RestoreActionType union and the new screen icons/imports.",
    ],
    [
        "RUN-027",
        "Test",
        "Tooling gates",
        "python3 scripts/check_command_parity.py && python3 scripts/apply_design_tokens.py --check",
        "Passed",
        f"{TODAY} 21:46",
        f"{TODAY} 21:46",
        5,
        0,
        "PARITY OK: every command Rust registers is called and every call resolves. No raw palette "
        "colours remain in src/.",
        "",
        "",
        "Manual",
        "The full green run also needs these two, plus the DMG build (RUN-028).",
    ],
]

# ---------------------------------------------------------------------------
# Decisions
# ---------------------------------------------------------------------------

decisions = [
    [
        "DEC-031",
        TODAY,
        "File scope is 'everything minus junk', snapshotted at capture time",
        "A captured workspace must carry the actual project files, so a file changed on the Mac exists "
        "on the Windows laptop after send + restore. The manifest policy column file_transfer already "
        "existed; nothing implemented it. Two coupled choices had to be made first: what counts as 'the "
        "files', and when the copy is made.",
        "(a) Keep manifest-only. (b) Snapshot at capture time. (c) Snapshot at send time. Within (b), "
        "scope could be every selected project's folder wholesale, or a curated allowlist, or 'everything "
        "minus a denylist'.",
        "(b) with 'everything minus two denylists and two caps'. At capture time: denied directories "
        "(.git, node_modules, target, dist, build, .next, .nuxt, .output, caches, virtualenvs, Pods, "
        "DerivedData, coverage) and denied files (.env/.env.*, keys, databases, logs, *.db, *.sqlite, "
        "Thumbs.db, desktop.ini) at any depth; symlinks skipped; one file over 128 MiB skipped and "
        "named; total capped at 512 MiB. Explicit policy is treated as All with a warning, since "
        "per-project selection is not built yet.",
        "(c) means the copy changes between capture and send, so re-sending would re-send a different "
        "workspace -- the one property a re-send must not have. An allowlist is unmaintainable and would "
        "miss real projects; 'everything minus junk' is safe precisely because every excluded name is "
        "either regenerable build output or a secret. The caps keep the payload honest against the wire "
        "and the receiver's budget; the denylist keeps 'everything' safe.",
        "A 13-test files crate; capture stores a sealed tar plus counts; re-send re-sends the same copy; "
        "legacy manifest-only payloads and the policy value 'none' still work. The capture screen "
        "defaults the policy to 'all' (FEAT-080).",
        "Revisit when per-project file selection is wanted, which turns the Explicit policy from an "
        "alias into a real feature.",
        "Assistant",
        "Decided",
        "FEAT-076,FEAT-077",
    ],
    [
        "DEC-032",
        TODAY,
        "The archive travels in an opaque envelope; restore runs as an ExtractFiles step",
        "The wire already carries chunked, acked, encrypted opaque bytes, so carrying the archive "
        "required no change to framing or transport -- what had to change was the receive side's size "
        "budget and what the receiver does with the bytes. Storing and restoring also had two shapes "
        "available: formal storage plus a planner step, or a side file the restore command reads ad hoc.",
        "(a) Extend the existing manifest payload with fields. (b) Wrap payload as MAGIC + u32 LE "
        "manifest length + manifest + archive, treating anything without the magic as legacy "
        "manifest-only. (c) Side-channel the archive entirely (separate transfer, separate storage). "
        "Restore: (d) a special-cased ExtractFiles RestoreActionType planned after map-path, or (e) a "
        "hidden file copy inside restore with no step and no preview.",
        "(b) and (d). Envelope WCFB1 | u32 LE manifest length | manifest | archive. The receiver splits "
        "the envelope, unseals the archive with its own key, stores it at files/{id}.files.sealed, and "
        "records counts in the workspace_files table (migration 003). The planner emits extract-files-"
        "{proj} (required, dependent on map-path-{proj}); the executor takes files: Option<&[u8]> and "
        "extracts per project into its mapped destination, overwriting existing files (snapshot "
        "authoritative). The accept-side incoming budget rises to the 512 MiB total plus 32 MiB "
        "headroom.",
        "(a) would deny unknown-field-free manifests and break every stored payload. (c) splits one "
        "logical transfer into two atomicity domains. (e) hides the one thing a restore does that "
        "touches the disk -- the preview and the 'commands will run' warning would lie. (b)+(d) keep the "
        "transport untouched, the received workspace indistinguishable from a local capture, the report "
        "telling the truth about refusals and overflow, and a no-files workspace reporting an honest "
        "success.",
        "Legacy payloads read back manifest-only, so pre-feature senders still arrive. 43 restore tests "
        "incl. the two extract tests; the two-device e2e proves the changed file restores. The restore "
        "report's step message carries counts, refusals and overflow.",
        "Revisit if compression ever becomes worthwhile: the size math the signature and the caps rely on "
        "would change.",
        "Assistant",
        "Decided",
        "FEAT-077,FEAT-078,FEAT-079",
    ],
]

# ---------------------------------------------------------------------------
# Logs
# ---------------------------------------------------------------------------

logs = [
    log(
        "LOG-079",
        "IMPLEMENTATION",
        "Core / Files",
        "FEAT-076",
        "Built the files crate: snapshot, archive, transit",
        "New crate src-tauri/files. snapshot.rs: sorted deterministic tar built at capture time -- denylists "
        "for directories and files matched at any depth, per-file 128 MiB cap, 512 MiB total cap, symlinks "
        "skipped, every skip and the overflow surfaced. archive.rs: extraction treats every entry as "
        "hostile -- refuses traversal names, absolute paths, backslashes, drive letters, symlinks, reserved "
        "Windows device names and oversized entries, and caps the total. transit.rs: the WCFB1 envelope "
        "wrap/unwrap. 13 tests, 0 warnings.",
        "files,archive,snapshot,denylist,caps",
    ),
    log(
        "LOG-080",
        "IMPLEMENTATION",
        "IPC / Capture",
        "FEAT-076",
        "Capture now snapshots, seals and stores the file archive at capture time",
        "snapshot_and_store walks the selected projects, seals the tar with the storage key, writes it to "
        "files/{id}.files.sealed, and upserts the workspace_files row (migration 003). CaptureResult "
        "carries file_count and byte_count; every skip and the overflow become capture warnings. The exact "
        "same bytes are what a re-send puts on the wire.",
        "capture,files,snapshot,sealed",
    ),
    log(
        "LOG-081",
        "IMPLEMENTATION",
        "Network / Transfer",
        "FEAT-077",
        "The wire now carries the archive behind the manifest, in an opaque envelope",
        "transfer_payload replaced manifest_payload at the send site: WCFB1 magic, u32 LE manifest length, "
        "manifest JSON, archive -- inside the existing encrypted chunked frames, with no change to framing. "
        "The receive side splits the envelope; a payload without the magic reads back as manifest-only, so "
        "every pre-feature sender still arrives. The accept-side budget rose to 512 MiB + 32 MiB headroom, "
        "and the arrived archive is unsealed with the receiver's own key and stored exactly as a local "
        "capture stores one.",
        "transfer,wire,envelope,backward-compatible",
    ),
    log(
        "LOG-082",
        "IMPLEMENTATION",
        "Restore / Planner",
        "FEAT-079",
        "Restore gained an extract-files step per placed project",
        "The planner adds extract-files-{proj} (required, dep map-path-{proj}); the executor receives "
        "files: Option<&[u8]> and calls the hostile extractor per project into its mapped folder, "
        "overwriting what is there. The step message reports files written and bytes, refusals and "
        "overflow; a workspace with no files reports an honest success. 43 restore tests incl. two new "
        "extract tests; the two-device e2e asserts that a file changed on the sending machine has the "
        "changed content on the receiving machine after send + restore.",
        "restore,extract,planner,executor,e2e",
    ),
    log(
        "LOG-083",
        "IMPLEMENTATION",
        "Frontend",
        "FEAT-080",
        "Frontend wired for the file feature",
        "extract_files joined RestoreActionType; the restore preview and report render the step with its "
        "own icon and the preview classifies it as an execution step, so the 'commands will run' warning "
        "stays honest. The capture policy defaults to 'all' -- the UI's only choice for a fresh capture is "
        "to carry files, which is the point of a clone; 'none' and 'explicit' remain in the type. The "
        "capture screen's 'What is never captured' notice states the snapshot's denylist and corrects the "
        "stale claim that working-tree files are never copied.",
        "frontend,restore-preview,restore-report,capture-policy",
    ),
    log(
        "LOG-084",
        "TEST",
        "Project",
        "RUN-024,RUN-025,RUN-026",
        "Full gates green: 394 Rust, 99 frontend, tsc clean",
        "cargo test --workspace -- --test-threads=2: 394 passed, 0 failed -- up from 377 by 17 (files "
        "crate 13, the two-device files e2e 1, restore extracts 2, one further command unit). npx vitest "
        "run: 99/99 across 6 files. npx tsc --noEmit: clean. Command-surface parity and the design-token "
        "check are clean too. No test asserted a manifest-only capture default, so defaulting the policy "
        "to 'all' stayed green.",
        "testing,regression,full-run",
    ),
    log(
        "LOG-085",
        "IMPROVEMENT",
        "Project",
        "All",
        "FEAT-045 corrected, guide and tracker aligned to the file feature",
        "FEAT-045 was Done at 100% for 'file transfer' while only the manifest policy flag existed; its "
        "row now says exactly that, and the transport is FEAT-076..080. DEVELOPER_GUIDE updated: the "
        "capture section states the denylist and caps, restore step 3 documents the extract step, the "
        "wire diagram shows the envelope and the extract step, the privacy section distinguishes the "
        "adapters' promises from the file snapshot's, and the test counts are 394/99.",
        "correction,documentation,guide",
    ),
]

# ---------------------------------------------------------------------------
# Overview
# ---------------------------------------------------------------------------

overview = {
    "Test Coverage": (
        "394 Rust tests (12 of them two-device end-to-end across a real socket; 13 in the new files "
        "crate), 99 frontend tests, 0 build warnings, command-surface parity clean at 44/44",
        "Done",
        NOW,
        "Up from 377/99. The files crate, the wire envelope, and the restore extract path are fully "
        "covered; the two-device suite now proves a file changed on the sending machine restores with "
        "the changed content on the receiving machine.",
    ),
    "Current Phase": (
        "Phase 4 - LAN Transfer: receive path complete and workspace files travel and restore; Phase 5 "
        "(Linux, relay, team features) not started",
        "In Progress",
        NOW,
        "Workspace files are now captured, carried and restored end to end. Windows is still implemented "
        "but unverified on the hardware; the DMG is built on the Mac.",
    ),
}

for row in wb["Overview"].iter_rows(min_row=2):
    field = row[0].value
    if field in overview:
        value, status, updated, notes = overview[field]
        row[1].value = value
        row[2].value = status
        row[3].value = updated
        row[4].value = notes

# ---------------------------------------------------------------------------
# Append everything
# ---------------------------------------------------------------------------

print("Features appended: ", append("Features", features))
print("Runs appended:     ", append("Runs", runs))
print("Decisions appended:", append("Decisions", decisions))
print("Logs appended:     ", append("Logs", logs))

wb.save("WorkspaceClone_Tracking.xlsx")

print()
for sheet in wb.sheetnames:
    print(f"  {sheet:12} {wb[sheet].max_row - 1:4} rows")
print("saved: WorkspaceClone_Tracking.xlsx")