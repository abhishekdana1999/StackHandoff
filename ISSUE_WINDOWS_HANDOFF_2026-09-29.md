# Windows laptop handoff: fixing "sends an empty workspace to the Mac"

**Date:** 2026-09-29 · **Target:** the Windows laptop (BISWAJITA)
**Work on the laptop, in order, then report back.**

---

## 1. What the user sees on the Mac

- The transfer "arrives" (green banner) but **restore/preflight shows nothing** — no
  projects, no files to restore, nothing the user selected.
- Confirmed against the **Mac's own database and decrypted manifests**: every
  Windows→Mac workspace arrived with
  `projects = 0`, `applications = [terminal-session]` only, and **no file archive**
  (`no workspace_files row`, no `.files.sealed` on the receiving side).

## 2. Root cause (proven, receiver-side data)

The MAC is not at fault — the **capture on this laptop** produced a near-empty
workspace. The user ticks **vscode / git / terminal** on the capture screen, but:

- Those are **adapter** ticks. They only enhance **projects**.
- **Projects and files come from the Projects section**, which lists repositories
  found by the project scan.
- The project scan only looks in `~/projects` and `~/Documents` by default. The
  user's code lives elsewhere → **no repositories were found** → nothing was
  tickable → the capture had zero projects → no files were snapshotted → nothing
  reached the Mac to restore.

## 3. Fix on this laptop (two parts)

### Part A — add your code folder so the scan finds the project

1. Open the app → **Settings** → **Project locations** (add as many as you like):
   the folder(s) that contain your git repositories, e.g. the parent of the
   VSCode project you want to send.
2. Go to **Capture** → the **Projects** list should now show your repository(s).

### Part B — make sure the laptop runs the current build

1. `git pull` in the workspace clone repo (get the latest `main`,
   incl. the network + frontend fixes).
2. Rebuild and reinstall the `.exe`:

   ```powershell
   bash scripts/build_windows.sh
   ```

   (no bash? then: `npm ci` then
   `npx tauri build --config src-tauri/app/tauri.conf.json`.)
   Install the produced NSIS `.exe` from
   `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/`.
3. **Allow the firewall prompt** (Private networks) on first launch.

## 4. Create a GOOD workspace and send it

1. Capture:
   - give the workspace a name;
   - in **Projects**, **tick the repository** you want to send;
   - tick adapters as desired (git / vscode / terminal);
   - **confirm the capture summary shows the project count and file count**
     (e.g. "1 project", "N files") — if it says no projects/files, stop and fix
     Part A.
2. Send that workspace to the Mac.

## 5. Verify on the Mac

- Preflight/restore for the received workspace should now list your **project**
  and a **file step** (your uncommitted git changes carried over).
- The green banner should read `'<workspace>' arrived from BISWAJITA`.

## 6. Report back / check details

If the capture still shows 0 projects after Part A:
- what does Settings → Project locations contain?
- what does the Capture screen Projects section say (found count / "no
  repositories found…" message / "these folders do not exist…" list)?
- the app version shown in the app (Settings or About).

Also useful from this laptop:
- In **Devices**, tell me the exact name stored for the MAC (ABHISHEKs-MacBook-Air.local?)
- Whether the green `/'' arrived from/` **empty sender-name alert** appears on
  THIS laptop (when the Mac sends TO Windows) — that symptom is so far
  unexplained on the Mac side and may be a Windows-side name issue.