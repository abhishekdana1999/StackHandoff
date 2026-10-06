# StackHandoff

StackHandoff is a local-first desktop app for moving a developer workspace from one machine to another without manually rebuilding the environment.

The current implementation is an MVP focused on the same-network workflow: pair devices, capture a workspace manifest, send it to a peer, review a restore plan, and apply the changes on the destination machine.

## Status

### Shipped in the current codebase

- Local peer-to-peer pairing over the same network
- Device discovery and authenticated pairing with Noise/X25519 keys
- Trust scopes for paired-device permissions
- Workspace capture for project roots and associated metadata
- Workspace transfer and receipt between paired devices
- Incoming-transfer handling and explicit accept/refuse flow
- Preflight, prepare, and restore-preview stages before applying a workspace
- Restore execution tracking and success/failure reporting
- Desktop app built with Tauri + React + Rust
- Cross-platform desktop project setup for macOS, Windows, and Linux targets

### Planned / not yet shipped

These are part of the product roadmap, not the current implementation:

- Cloud relay or internet-based handoff
- Account-based device registration and remote management
- Remote restore requests without a local paired network
- Bulk credential or secret synchronization
- Automatic restoration of every runtime, app, and service on the machine
- Full non-Git folder sync for all developer workloads
- Paid cloud plans, hosted services, or managed account features
- Broader automation beyond the local workspace restore flow

## How the shipped flow works

```text
Pair devices on the same network
        ↓
Capture workspace state
        ↓
Send to a trusted peer
        ↓
Receive on the destination machine
        ↓
Review preflight and restore plan
        ↓
Apply restore and continue working
```

1. Pair a second machine running StackHandoff and compare the safety number out of band.
2. Capture a workspace from the current machine: the current project roots, Git state, and workspace metadata.
3. Send the workspace to a paired device over the local network.
4. On the receiving machine, review the incoming transfer and decide whether to accept it.
5. Inspect the restore plan and preflight checks before applying it.
6. Restore the workspace and continue with the same work session on the other machine.

This is intentionally a local-first MVP. It does not claim remote cloud handoff, zero-configuration credential sync, or fully automated machine recreation.

## What is not included in the MVP

StackHandoff does not currently:

- copy passwords or secrets between machines
- silently grant access to arbitrary devices
- replace the user's entire machine configuration automatically
- provide cloud-hosted workspace relay or account-based handoff
- attempt to restore every possible runtime or service without review

## Getting started

Requirements: [Node.js 20+](https://nodejs.org/) and [Rust](https://rustup.rs).

```bash
# install frontend dependencies
npm install

# build the frontend bundle
npm run build

# run the desktop app from the Rust workspace
cd src-tauri
cargo run -p app
```

For local development and validation, the project also supports the usual frontend test and type-check flow:

```bash
npm run test:run
npx tsc --noEmit
```

For Rust verification:

```bash
cd src-tauri
cargo test --workspace
cargo fmt --all --check
```

## Tech stack

| Layer | What it is |
| --- | --- |
| Shell | [Tauri 2](https://v2.tauri.app/) desktop app |
| Frontend | React 18 + TypeScript + Vite + Tailwind |
| Backend | Rust workspace under `src-tauri/` with crates for core app logic, networking, crypto, database, adapters, preflight, restore, and Tauri commands |
| Data | SQLite via SQLx |
| Transport | mDNS discovery, authenticated device pairing, and encrypted peer transfer |

## Repository layout

```text
├── docs/          ← product plan, developer notes, handoff documentation
├── scripts/       ← repo checks and build automation
├── src/           ← React + TypeScript frontend
├── src-tauri/     ← Rust workspace for the desktop app and core logic
├── LICENSE
├── package.json
├── README.md
└── .gitignore
```

## Open source

The app source is in this repository: [github.com/abhishekdana1999/StackHandoff](https://github.com/abhishekdana1999/StackHandoff).
It is licensed under the [MIT License](LICENSE).

## Notes for contributors

- Frontend-to-backend command names are declared in [src/lib/ipc.ts](src/lib/ipc.ts) and must stay aligned with the Tauri registry in [src-tauri/app/src/lib.rs](src-tauri/app/src/lib.rs).
- The product direction is intentionally split between the shipped local MVP and the future roadmap in [docs/PRODUCT_GROWTH_PLAN.md](docs/PRODUCT_GROWTH_PLAN.md).
- This README describes the current implementation, not aspirational or future product claims.
