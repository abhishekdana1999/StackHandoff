# StackHandoff

**Pick up right where you left off.**

StackHandoff is a peer-to-peer desktop app for cross-device work-session
continuity. Start work on one machine — project folders open in your editor,
tools installed, environment variables set, accounts signed in — and when you
travel, it captures that setup and rebuilds it on the other machine.

It does **not** copy your files. It captures a *description* of your working
environment — a **manifest** — and restores that environment on the other side:
open projects and their git branches, running applications, runtimes and
command-line tools, environment variables, and the accounts you were signed in
to. Git working-tree changes travel along as a patch, so your in-progress edits
land on the other machine exactly as you left them.

- **Peer-to-peer, no accounts.** Your two machines find each other on the local
  network (mDNS) and talk directly over an encrypted, key-authenticated
  connection (Noise protocol + X25519 + AEAD). There is no server, no cloud
  account, no sign-up.
- **Nothing happens without you saying so.** Every step — accepting a
  workspace, running a command, opening an application — is shown first with a
  plain-English explanation. You review what was captured, decide what to skip,
  and apply it.
- **Mac ↔ Windows.** Ships as a Mac app; builds Windows installers (`.exe` /
  `.msi`) on the laptop or via GitHub Actions.
- **Your git history survives the trip.** Cloned repositories are rebuilt on
  the destination with the working-tree delta re-applied as a patch — `git
  status` on the other machine shows exactly your changes, not a rewritten tree.

## How it works

```
macOS laptop                          Windows laptop
┌──────────────────┐                  ┌──────────────────┐
│ Capture workspace│ ── encrypted ──▶ │  Review + apply  │
│   (the manifest) │   mDNS found     │  (restore plan)  │
└──────────────────┘                  └──────────────────┘
```

1. **Pair** the two machines on the same network and confirm each other's
   fingerprints by hand.
2. **Capture** a workspace: which project folders you use and the git branch
   each is on, which apps were open and for which projects, which runtimes,
   tools and environment variables you need, which accounts you were signed in
   to — plus your uncommitted git changes as a patch.
3. **Send** it. The workspace travels directly between the machines, sealed
   and encrypted.
4. **Restore** on the other laptop: review what was found, skip what you don't
   want, and apply the rest — projects cloned or re-synced, apps opened, tools
   verified, git delta re-applied.

## Getting started

Requirements: [Rust](https://rustup.rs) and [Node.js 20+](https://nodejs.org).

```bash
# install dependencies
npm install

# run the desktop app (first build compiles ~10 Rust crates — give it a few minutes)
npm run tauri dev
```

On Windows, the same flow works from PowerShell; you also need Visual Studio
Build Tools with the **Desktop development with C++** workload. For a
shareable installer, run `bash scripts/build_windows.sh` (or push a `v*` tag
to trigger `.github/workflows/windows-installer.yml`).

See [docs/DEVELOPER_GUIDE.md](docs/DEVELOPER_GUIDE.md) for the full walkthrough —
including pairing, firewall setup, and troubleshooting.

## Open source

The desktop application source is in this repository:
[github.com/abhishekdana1999/StackHandoff](https://github.com/abhishekdana1999/StackHandoff).
The standalone website source lives at
[github.com/abhishekdana1999/StackHandoff-site](https://github.com/abhishekdana1999/StackHandoff-site).
Both are licensed under the [MIT License](LICENSE).

## Tech stack

| Layer | What it is |
| --- | --- |
| Shell | [Tauri 2](https://v2.tauri.app/) desktop app |
| Frontend | React 18 + TypeScript + Vite + Tailwind |
| Backend | Rust workspace under `src-tauri/` — crates for types, cryptography, database, networking, per-tool adapters, preflight checks, restore planner/executor, and the Tauri command layer |
| Data | SQLite via SQLx (offline migrations) |
| Transport | Noise_IK encryption, mDNS discovery, AEAD-sealed workspace archives |

## Repository layout

```
├── docs/          ← developer guide, handoff notes, tracking workbook
├── scripts/       ← tracker updaters, Windows build, parity + design-token checks
├── src/           ← React + TypeScript frontend
├── src-tauri/     ← Rust workspace (core, crypto, db, network, adapters, restore, app)
└── README.md
```

The frontend-to-backend boundary lives in exactly one file:
`src/lib/ipc.ts` names the Tauri commands, and
`src-tauri/app/src/lib.rs` is the registry that must match it —
`scripts/check_command_parity.py` enforces the two never drift.

## Testing

The Rust workspace has 419 tests across adapters, cryptography, the restore
planner/executor, and end-to-end transfer scenarios; the frontend has 102
tests over routing, theming, and the receive flow. `tsc --noEmit` passes with
zero errors and the workspace builds with no warnings.

```bash
# frontend
npm run test:run
npx tsc --noEmit

# backend (from src-tauri/)
cargo test --workspace
cargo fmt --all --check
```
