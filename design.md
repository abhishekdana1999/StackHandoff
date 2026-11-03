# StackHandoff Cloud Version Design

## Overview

This document design the **cloud-enabled version** of StackHandoff that allows workspace handoff between two systems **without requiring them to be on the same peer-to-peer network**. The cloud version extends the existing local P2P flow with cloud relay capabilities, user accounts, and paid plan tiers while preserving all existing security, privacy, and trust principles.

The core philosophy remains: **nothing happens without explicit user consent**, **all communication is authenticated and encrypted**, **no plaintext credentials are transferred**, and **the user always controls what is shared**.

---

## Problem Statement

The current MVP only works when both devices are on the **same local network** using mDNS discovery. Real-world developers frequently need to move their workspace between different networks. The cloud version solves this with user accounts and paid plan tiers.

---

## Plan Tiers

### Free Tier
- Local P2P handoff only (same network, mDNS discovery)
- No cloud relay or account required
- All existing local features unchanged
- **Limit**: Cannot use cloud sync across networks

### Basic Plan
- Cloud sync beyond the local network
- **One paired device pair** (two computers registered to the same account)
- **Expanded app support**: Folders, Android Studio, MySQL, VLC, and few more developer applications
- **Review each handoff before restoring** (user approves each restore plan)
- **Limit**: Only two devices per account; basic app restoration only
- **Price**: Entry-level paid tier

### Pro Plan
- **Everything in Basic**
- **Up to three paired devices** registered to the same account
- **Full supported-app catalog**: All means user can completely clone its existing system as it is on another device (we will install things for user on its destination machine)
- **Move between your devices with cloud sync** ( seamless device switching)
- **Limit**: Three devices maximum; full app/runtime installation on restore
- **Price**: Premium tier

---

## User Flow

### 1. Download and Installation
- User downloads StackHandoff as always (free, local P2P MVP)
- On first launch, user sees a **Plan Selection** screen (or is prompted on login)
- Options: **Free (Local P2P only)** → **Basic** → **Pro**

### 2. Login / Account Creation
- User chooses a plan (or starts with Free)
- User logs in or creates an account (email/password or OAuth)
- Account ties all registered devices to a user identity in the cloud backend

### 3. Device Registration (after plan selection)
- User enables cloud sync on a machine
- Machine generates its Noise key pair and registers with the cloud relay **under the user's account**
- User selects trust scopes appropriate for their plan:
  - **Free**: No cloud scopes (local only)
  - **Basic**: Can select `receive` or `send` (but limited to 1 other device)
  - **Pro**: Can select `receive` or `send` (up to 3 devices total)
- Device is now visible in the user's "My Devices" list on the account portal

### 4. Capture and Cloud Sync
- On the **source machine**: User captures a workspace (identical to local P2P capture)
- User sees an option: **"Sync to Cloud"** (available based on plan)
- If Basic/Pro: Workspace manifest + files are uploaded to the cloud relay **encrypted with the source device's storage key**
- The cloud relay stores only ciphertext (never plaintext)

### 5. Select Destination & Restore
- On the **destination machine** (logged into same account): User sees the captured workspace under "Cloud Transfers"
- User selects the workspace and chooses **Restore**
- User can see: source device name, workspace name, size, supported apps detected
- **Basic**: User reviews the restore plan; manually confirms each adaptation needed (path mapping, app availability)
- **Pro**: System suggests app installations; user can choose "Clone as-is" (system will attempt to install missing apps on destination)

### 6. Restore Execution
- The restore flow runs: preflight → prepare → restore plan → execute
- **On success** (Basic or Pro): 
  - Cloud transfer entry is **revoked/removed** from the user's account
  - Workspace is now fully local on the destination machine
  - Transfer history records "completed"
- **On failure** (Basic or Pro):
  - User is shown a **failure report** with reasons (missing apps, path conflicts, etc.)
  - User can choose: `Fix & Retry`, `Download Files Only`, or `Cancel & Revoke`
  - If user chooses `Revise`, the cloud transfer remains active and user can adjust settings and retry
  - If user chooses `Revoke`, the cloud transfer is deleted and no data persists on the destination

### 7. Device Management
- User can view all registered devices in the account settings
- User can **revoke** any device at any time
- Revoking a device deletes its registration and all associated cloud transfer data
- Device count respects plan limits (Free: 0 cloud, Basic: 2 total including local, Pro: 3 total)

---

## Detailed Feature Specification

### Cloud Relay Service (Backend)
- **Dumb carrier** — never sees plaintext, only ciphertext stored with destination's encryption key
- Device registration tied to user account (Noise key + name + plan scope + max devices)
- Transfer relay: routes encrypted manifest+files from source → relay → destination
- TTL/expiry: cloud-stored transfers expire after configurable period (default 24h for Basic, 7d for Pro)
- Payment integration: stripe/paypal integration to upgrade plan status on successful payment

### Authentication & Authorization
- **JWT-based auth** for the cloud backend (user session management)
- Each Tauri command that touches cloud features validates:
  - User is logged in (has valid auth token)
  - Device is registered under user's account
  - Device has appropriate trust scope for the operation (respecting plan limits)
  - Safety number comparison enforced before any transfer completes

### Security (All Plans)
- Device authentication via Noise static keys (unchanged)
- Encryption in transit (Noise_IK per-transfer session)
- Encryption at rest (manifests sealed with capturing device's key, re-sealed with destination's key on arrival)
- No plaintext credentials ever transferred ( passwords, tokens, secrets never captured)
- Revocable devices (user can revoke any paired device; data deleted from cloud)
- Safety number comparison out-of-band before any cloud transfer completes

### Plan Limits Summary

| Limit | Free | Basic | Pro |
|-------|------|-------|-----|
| Cloud sync | ❌ Local P2P only | ✅ Beyond local network | ✅ Beyond local network |
| Max devices (cloud) | 0 | 1 pair (2 devices) | 3 devices |
| Full app cloning | ❌ No | ❌ Review before restoring | ✅ Install apps on destination |
| Review before restore | N/A (local only) | ✅ Yes (per handoff) | ✅ Yes + app suggestions |
| Move between devices | ❌ Not supported | ✅ With manual review | ✅ Seamless cloud sync |
| Max workspace size | Same as local (512 MiB) | Same as local (512 MiB) | Same as local (512 MiB) |
| Per-file cap | 128 MiB | 128 MiB | 128 MiB |
| TTL (cloud-stored) | N/A | 24 hours default | 7 days default |
| Payment required | ❌ No | ✅ Yes (one-time or subscription) | ✅ Yes (subscription) |

---

## Data Model Extensions

### User Account (new)
| Table | Field | Description |
|-------|-------|-------------|
| `accounts` | `email` (string, unique) | User's email address |
| `accounts` | `plan` (enum: free/basic/pro) | Current subscription plan |
| `accounts` | `plan_started_at` (timestamp) | When current plan began |
| `accounts` | `plan_expires_at` (timestamp) | When plan expires (for subscription) |
| `accounts` | `payment_provider` (string) | stripe/paypal/etc |
| `accounts` | `payment_status` (enum: pending/active/cancelled) | Payment lifecycle |

### Device (extended)
| Table | Field | Description |
|-------|-------|-------------|
| `devices` | `account_id` (foreign key) | Links device to user account |
| `devices` | `plan_at_registration` (enum) | Plan in effect when device was registered |
| `devices` | `is_revoked` (boolean) | Whether device has been unregistered |
| `transfers` | `cloud_transfer` (boolean) | Used cloud relay vs. direct P2P |
| `transfers` | `plan_at_transfer` (enum) | Plan that governed this transfer |
| `transfers` | `revoked_by` (string) | Which user revoked it, if applicable |
| `workspaces` | `cloud_relay_id` (string) | Identifier if uploaded to cloud relay |
| `workspaces` | `cloud_relay_expires_at` (timestamp) | When cloud copy expires |
| `workspaces` | `restore_status` (enum: pending/completed/failed/revoked) | Restore lifecycle state |

---

## restore Completion Handling

### On Successful Restore
1. Transfer record status updated to `completed`
2. Cloud relay entry for that workspace is **revoked/deleted** (cipherstone removed)
3. Local database entry for the workspace marked `status: "restored"`
4. User sees: "Workspace restored successfully. Cloud transfer removed."
5. Transfer history shows: completed, cloud transfer revoked

### On Failed Restore
1. Transfer record status updated to `failed`
2. Failure report shown to user with details:
   - Missing applications
   - Path mapping conflicts
   - File size/denylist issues
   - Environment variable mismatches
3. User actions available:
   - **Fix & Retry**: Adjust settings (map paths, select different adapters) and retry restore
   - **Download Files Only**: Download just the project files (no app installation), workspace is available as raw files
   - **Revoke & Delete**: Delete the cloud transfer; no workspace is stored on destination; user starts fresh
4. If user chooses **Revoke**:
   - Cloud transfer entry deleted
   - Any partial files on destination are cleaned up (or left with warning, user choice)
   - Transfer history records: "revoked by user"
5. If user chooses **Fix & Retry**:
   - Cloud transfer remains active
   - User can modify restore plan (new destination folders, different app selections)
   - Retry runs the restore flow again
   - If retry succeeds, same on-success flow applies (revoke cloud entry)

---

## UI/UX Design

### Plan Selection Screen (on first launch or when user clicks "Sync to Cloud")
```
StackHandoff – Choose Your Plan

1. Free – Local P2P only
   Capture and transfer workspaces between two machines on the same Wi-Fi.
   No account needed. No cloud sync.

2. Basic – $9.99/month (or one-time)
   • Cloud sync beyond local network
   • Pair 2 computers under one account
   • Expanded app support (Folders, Android Studio, MySQL, VLC, etc.)
   • Review each handoff before restoring

3. Pro – $19.99/month (or one-time)
   • Everything in Basic
   • Up to 3 paired devices
   • Full app cloning (install on destination)
   • Move between devices with cloud sync

[Continue as Free]   [Upgrade to Basic]   [Upgrade to Pro]
```

### Device Registration Modal
- "Register this device with your account"
- Plan-appropriate scopes are pre-selected (Basic: send+receive for 1 other device; Pro: send+receive for up to 2 more)
- Shows: "You have X/Y devices registered (plan limit)"
- Button: "Register Device"

### Incoming Cloud Transfer Banner
- Same style as local P2P arrivals
- Additional badge: "Cloud Transfer" (different color)
- Shows: source device name, workspace name, plan requirement badge
- Actions: `Accept`, `Reject`, `Details`
- Clicking `Details` shows: safety number comparison prompt, app compatibility info, restore preview

### Account Settings Page
- "My Devices" list (with revoke buttons)
- "Plan: Basic (active until Jan 1, 2027)" or "Plan: Pro (monthly)"
- "Update Plan" button (change between Basic/Pro)
- "Revoke all cloud transfers" button
- Transfer history with filter: "Cloud" / "Local P2P"

---

## Implementation Plan

### Phase 1: Account & Payment Backend
1. Set up user account system (email/login, JWT tokens)
2. Integrate payment provider (Stripe or PayPal) for Basic/Pro plan purchases
3. Plan status management: free/basic/pro states, TTL limits per plan
4. Device-account linking: each device registered under exactly one account

### Phase 2: Cloud Relay Service Enhancements
1. Extend relay API to include `account_id` on registration and transfers
2. Implement plan-enforced limits (max devices, TTL per plan, scope enforcement)
3. TTL/expiry automation (auto-delete after plan-specific TTL)
4. Payment webhook handling (on_success → upgrade plan status)

### Phase 3: Frontend UI
1. Plan selection screen on first launch / when user attempts cloud sync
2. Login / account creation flow
3. Device registration modal (plan-aware scopes)
4. Cloud transfer listing in Workspaces section (distinguish cloud vs. local)
5. Restore completion UI (success: revoke; failure: fix/retry/revoke choices)
6. Account settings page (plan management, device list, transfer history)

### Phase 4: End-to-End Flow Testing
1. Free user: cannot see cloud sync option; local P2P only works
2. Basic user: can register 2 devices; cloud sync works; restore requires manual review per handoff
3. Pro user: can register 3 devices; "clone as-is" works (system installs missing apps); seamless device switching
4. Cross-plan: downgrade from Pro→Basic retains data but limits to 2 devices; new transfers respect new limits

### Phase 5: Security & Compliance
1. Verify no plaintext data in cloud relay (only ciphertext)
2. Verify safety number comparison enforced regardless of plan
3. Verify plan limits enforced at API level (cannot bypass by crafting requests)
4. Verify revocation deletes all cloud-associated data
5. Verify payment webhook correctly upgrades/downgrades plan status
6. Data export/deletion for user privacy (GDPR-friendly)

### Phase 6: Monetization & Documentation
1. Update README: separate "Free (Local P2P)" from "Basic/Pro (Cloud Handoff)"
2. Update docs/PRODUCT_GROWTH_PLAN.md with new plan tiers
3. Update design.md with payment flow and plan details (this document)
4. Pricing page / website copy reflecting Basic/Pro features
5. Free trial period considerations (e.g., 7-day Basic trial before payment required)

---

## Migration Path from MVP

The cloud version is **additive** — it does not replace the local P2P flow:

1. **Local P2P remains fully functional** — all existing commands, UI, and flows continue to work exactly as before, no account needed.
2. **Cloud mode is opt-in** — users must explicitly choose a plan and log in to enable cloud sync.
3. **No breaking changes** to existing commands or data model — new columns/tables are additive only (account_id, plan, cloud_transfer flags).
4. **Backward compatibility** — machines running the old MVP can still pair and transfer locally; only machines upgrading to the cloud version gain cloud capabilities.
5. **Feature gates** — cloud commands are gated behind user-login state and plan checks, so old app versions simply won't have those commands available or visible.

---

## References

- [Product Growth Plan - Cloud Handoff Section](docs/PRODUCT_GROWTH_PLAN.md#5-cloud-handoff)
- [Developer Guide - Local P2P Flow](docs/DEVELOPER_GUIDE.md)
- [Architecture Principles](docs/PRODUCT_GROWTH_PLAN.md#6-cloud-architecture-principles)
- [Current Implementation](src-tauri/) — local P2P networking, crypto, and transfer layer
- [Security Model](docs/PRODUCT_GROWTH_PLAN.md#7-cloud-architecture-principles)
- [Plan Tier Design](design.md#plan-tier-summary) — this section