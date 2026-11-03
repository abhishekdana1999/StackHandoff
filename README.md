# StackHandoff

StackHandoff is a local-first desktop app for moving a developer workspace from one machine to another without manually rebuilding the environment.

The current implementation is an MVP focused on the same-network workflow: pair devices, capture a workspace manifest, send it to a peer, review a restore plan, and apply the changes on the destination machine.

## Status

### Shipped in the current codebase

- Local peer-to-peer pairing over the same network
- Device discovery and authenticated pairing with Noise/X25519 keys
- Trust scopes for paired-device permissions
- Workspace capture for Git repositories and recognizable non-Git project folders, with filtered project-file transfer
- Workspace transfer and receipt between paired devices
- Incoming-transfer handling and explicit accept/refuse flow
- Preflight, prepare, and restore-preview stages before applying a workspace
- Restore execution tracking and success/failure reporting
- Desktop app built with Tauri + React + Rust
- Cross-platform desktop project setup for macOS, Windows, and Linux targets
- A Cloud screen that connects to the StackHandoff cloud API: email/password
  sign-in, plan tiers and limits, cloud device registration, and relay transfer
  listing (additive — the local P2P flow needs no account)
- PayPal checkout for the paid plans (Orders v2). The app asks the API to open
  an order — no PayPal credential, not even the public client id, ever reaches
  the frontend — redirects the buyer to PayPal, and the API captures the
  approved order before granting the plan (sandbox credentials in `.env.dev`
  today, live swap is a credentials-only change)

### Planned / not yet shipped

These are part of the product roadmap, not the current implementation:

- The internet relay data plane itself (transfers are currently cloud-side
  records; the encrypted payload still moves over the local network)
- Remote restore requests without a local paired network
- Bulk credential or secret synchronization
- Automatic restoration of every runtime, app, and service on the machine
- Automatic discovery of arbitrary folders that do not contain recognizable project files
- Recurring/billing-webhook support for the paid cloud plans (each PayPal
  approval currently captures a one-time payment; auto-renewal needs a live
  business account and PayPal subscription webhooks)
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
2. Capture a workspace from the current machine: selected project folders (Git or non-Git), Git state when available, and workspace metadata.
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
- provide a cloud-hosted workspace relay (cloud accounts and plan management exist, but the encrypted payload still moves over the local network)
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

## Cloud API

The Cloud screen talks to a separate NestJS service (the
`stackhandoff-backend` project). It is optional: the local peer-to-peer flow
works with it stopped. To run the two together:

```bash
# 1. start the cloud API (listens on :3000)
#    its .env.dev points at the dev Supabase project, so sign-up/login work
cd ../stackhandoff-backend && npm run dev

# 2. start the app dev server (listens on :1420)
npm run dev

# 3. open the app and visit the Cloud section
#    http://localhost:1420/cloud
```

The client's base URL defaults to `http://localhost:3000` in development and is
overridable with `VITE_CLOUD_API_URL`. Production builds read `.env.production`,
which points at `https://api.stackhandoff.com`. The transport lives in
`src/lib/cloudApi.ts`; the session is in `src/store/useCloudStore.ts`; the screen
is `src/screens/CloudScreen.tsx`.

To verify the full journey end to end — a real browser, the live API, sign-up →
PayPal checkout → cancelled-return handling → device registration → sign-out —
run both servers and then:

```bash
node scripts/verify-cloud.mjs
```

## Tech stack

| Layer | What it is |
| --- | --- |
| Shell | [Tauri 2](https://v2.tauri.app/) desktop app |
| Frontend | React 18 + TypeScript + Vite + Tailwind |
| Backend | Rust workspace under `src-tauri/` with crates for core app logic, networking, crypto, database, adapters, preflight, restore, and Tauri commands |
| Cloud API | Separate NestJS service for accounts, plans, devices, and transfers (optional; see above) |
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
