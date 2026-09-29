# Workspace Clone — Developer Guide

**For someone new to this codebase, with one Windows laptop and one Mac laptop.**

This guide assumes you have never used this tool before and have never written
Rust. It walks the whole path — install on both machines, pair them, capture a
workspace on one, send it, receive it on the other, and restore it — and then
explains what the code is doing underneath so you can change it.

Read Part 1 straight through the first time. It is the part that gets you a
workspace onto your other machine.

---

## Table of contents

**Part 1 — Using it (no code knowledge needed)**

1. [What this app actually does](#1-what-this-app-actually-does)
2. [What you need before you start](#2-what-you-need-before-you-start)
3. [Install it on both machines](#3-install-it-on-both-machines)
4. [Pair the two machines](#4-pair-the-two-machines)
5. [Choose what can travel between them](#5-choose-what-can-travel-between-them)
6. [Capture a workspace on the Mac](#6-capture-a-workspace-on-the-mac)
7. [Send it to the Windows laptop](#7-send-it-to-the-windows-laptop)
8. [Restore it on the Windows laptop](#8-restore-it-on-the-windows-laptop)
9. [When something goes wrong](#9-when-something-goes-wrong)
10. [Where your data lives](#10-where-your-data-lives)

**Part 2 — Working on the code**

11. [How the codebase is laid out](#11-how-the-codebase-is-laid-out)
12. [Getting the code to compile and run](#12-getting-the-code-to-compile-and-run)
13. [The tests, and what each layer covers](#13-the-tests-and-what-each-layer-covers)
14. [How a workspace actually travels](#14-how-a-workspace-actually-travels)
15. [Conventions that will bite you if you break them](#15-conventions-that-will-bite-you-if-you-break-them)
16. [Known gaps](#16-known-gaps)

---

# Part 1 — Using it

## 1. What this app actually does

You have two laptops. You start work on one — a handful of project folders open
in your editor, a few tools installed, some environment variables set, a list of
internal URLs to remember. Then you travel, and you want that setup on the other
machine.

**Workspace Clone captures that setup on one machine and restores it on another.**

It does *not* copy your files. It captures a description of your working
environment — a **manifest** — and rebuilds that environment on the other
machine:

- which project folders you use, and which git branch each is on
- which applications were open, and for which projects
- which runtimes, command-line tools and environment variables you need
- which internal accounts you were signed in to

You then review what it found, decide what to skip, and apply it.

Two ideas carry the whole design:

**It is peer-to-peer.** There is no server, no cloud account, no sign-up. Your two
machines find each other on your local network and talk directly, over an
encrypted connection that is authenticated by keys you confirm by hand.

**Nothing happens without you saying so.** Every step — accepting a workspace
from another machine, running a command, opening an application — is shown to you
first, with a plain-English explanation.

### The window

This is a Mac application, not a web page in a browser, and the interface is
built for that: a sidebar with the three sections, a toolbar that names where you
are, and a status bar along the bottom showing the app version, how many devices
are paired, and this machine's fingerprint.

The sidebar can be collapsed to a narrow icon rail with the button at its bottom —
handy when you want the content area wide. The choice is remembered.

**Appearance** is at the bottom of the sidebar, and again in **Settings**. There
are three choices: **Light**, **Dark**, and **Auto**, which follows whatever your
Mac is set to. Auto is the default and the one to leave alone unless you have a
reason; it tracks your system if you switch to dark mode in macOS at lunchtime.
The choice is remembered across launches, and applied before the window appears,
so there is no white flash first.

---

## 2. What you need before you start

- A Mac and a Windows laptop.
- **Both on the same Wi-Fi network**, on the same subnet. This is the single most
  common reason pairing fails; see [§9](#9-when-something-goes-wrong).
- A firewall that permits the app to accept connections *on both machines*.
- About 30 minutes for the first run.

You do **not** need the same Apple/iCloud/Microsoft account. The machines do not
need to be able to log into each other.

---

## 3. Install it on both machines

### The Mac

You need the [Rust toolchain](https://rustup.rs) and [Node.js 20+](https://nodejs.org).

```bash
# 1. Rust (the backend is written in Rust)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 2. Verify
cargo --version    # 1.7x or newer
```

Then, in the project folder:

```bash
npm install
npm run tauri dev
```

The first run takes several minutes because it compiles ~10 Rust crates from
scratch. When it finishes, a **Workspace Clone** window opens.

> **On Apple silicon (M1/M2/M3/M4):** nothing special to do. The project no
> longer pins a build target, so cargo builds for whatever your host is. An old
> copy of this guide told Intel users to delete `.cargo/config.toml`; that file
> now only sets `SQLX_OFFLINE` and a linker for an explicit
> `aarch64-apple-darwin` target, neither of which affects an Intel host.

### The Windows laptop

Everything above, in **PowerShell** (not `cmd`):

```powershell
# 1. Rust: download and run https://rustup.rs, then restart PowerShell
cargo --version

# 2. Node.js 20+ from https://nodejs.org
node --version

# 3. In the project folder
npm install
npm run tauri dev
```

Rust alone is not enough on Windows. Tauri links against the MSVC runtime, and
cargo's default Windows toolchain is GNU, which fails at link time with errors
that never mention the real cause. You also need Visual Studio Build Tools with
the **Desktop development with C++** workload:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools
winget install Rustlang.Rustup
winget install OpenJS.NodeJS.LTS
```

Restart PowerShell afterwards — `rustup` and the build tools only reach `PATH`
in a new session.

#### Making a shareable .exe

You cannot cross-compile this app to Windows from the Mac. Tauri needs the MSVC
toolchain, the Windows SDK and NSIS, and while `cargo-xwin` can fetch enough of
those to *compile* the Rust, the bundling step still needs a real Windows host.
There are two supported routes:

**On the laptop** — the simplest, no accounts needed. Copy the project across
(Git, a zip, a USB stick) and run, in PowerShell:

```powershell
bash scripts/build_windows.sh
```

or, if you have no bash on the laptop, the equivalent by hand:

```powershell
npm ci
npx tauri build --config src-tauri/app/tauri.conf.json
```

The installer lands in
`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/`. You get an NSIS
`.exe` and an `.msi`; the `.exe` installs without admin rights.

The `--config` flag is not optional. `tauri.conf.json` sits beside
`src-tauri/app/Cargo.toml`, not at the repo root or in `src-tauri/`, and the CLI
searches only those two places.

**Via GitHub Actions** — for a machine you do not want to install a toolchain
on. `.github/workflows/windows-installer.yml` builds the same installers on
`windows-latest` and uploads them as artifacts. Push a `v*` tag or dispatch the
workflow by hand. It needs the project in a Git repository; this tree is not
currently one, and `git init` should come with a `node_modules`-free, 17 GB-free
`.gitignore` (there is now one).

The Windows build is the **least-tested platform in this project**, and the CI
workflow has never been executed. See [§16](#16-known-gaps) before you rely on
it.

### Allow the firewall

The first time each machine launches, the OS asks whether to accept incoming
network connections. **Say yes.**

- **macOS:** System Settings → Network → Firewall → Options… → allow
  `Workspace Clone` for incoming connections.
- **Windows:** when Windows Defender Firewall prompts, tick **Private networks**
  and untick **Public networks**, then Allow access. If you are on a network
  Windows considers public, tick that too.

If you decline, everything on that machine still works locally — capture,
preflight and restore. The machine just cannot *receive* from a peer, and it
cannot be found. See [§9](#9-when-something-goes-wrong).

---

## 4. Pair the two machines

"Pairing" is how each machine learns the other's identity key, so that
transfers can be authenticated. It happens **on both machines separately**, and
both halves must be done — this trips up nearly everyone.

### Why both halves

The pairing is not a handshake that completes once. Each machine keeps its own
list of who it trusts, with its own permissions. The Mac pairing the laptop says
"this is the laptop, and it may send me workspaces." The laptop pairing the Mac
says "this is the Mac, and it may push to me." Neither machine can grant the
other anything on its behalf.

### The ceremony, step by step

Leave **both** apps open, on the **same Wi-Fi**.

**On the Mac:**

1. Click **Devices** in the sidebar.
2. Click **Pair a device**.
3. Type a name for **this** machine, e.g. `MacBook Pro`. This is the name the
   person at the other machine will see.
4. Under **Devices on the network**, your Windows laptop should appear within a
   few seconds. Click it.

   *If it does not appear,* skip to the "If the laptop is not listed" box below.
5. The app shows a **safety number** — a long string of digits.
6. **On the Windows laptop, do the same thing choosing the Mac** (below). Both
   screens now show a number.
7. **Compare the two screens.** If they match, press **The numbers match — pair**
   on the Mac. If they differ, **stop** and start over — that means something is
   intercepting the connection, and you should not pair.

**On the Windows laptop:** Devices → **Pair a device** → type a name for the
laptop → under **Devices on the network** click the Mac. It shows the *same*
number.

### Reading the safety number

This is the step that makes the whole thing safe, and it is easy to want to skip.

Both screens show a number derived from the two machines' keys. You **compare
the two screens** and press one button. There is nothing to type.

Why is that still a real check? Because the number travels between the two
machines **by you reading it aloud**, not by the app sending it over the
network. That is the entire point. A number that appeared on your screen and
was accepted automatically would prove only that the app computed a number — and
a man-in-the-middle on the same Wi-Fi could compute a matching one on both
sides, which is exactly the attack this is meant to catch.

The app also **recomputes** the number from the key rather than trusting the
screen, so the key it pairs is provably the key the number came from. It cannot
pair one device while displaying another's.

An earlier version made you retype all 45 digits. That bought no security — any
caller of the command could pass the number the app displayed — and it made
correct pairings fail whenever a single digit was mistyped.

If the numbers differ, do not pair. Either a machine is impersonating another,
or you paired the wrong device.

### If the laptop is not listed

Auto-discovery uses mDNS, which **routers routinely block** — especially guest
Wi-Fi, and many corporate and campus networks.

Workarounds, in order of preference:

1. **Both machines on the same non-guest network.** Guest networks commonly
   isolate clients from each other entirely.
2. **Turn the firewall off temporarily** on both machines to test. If pairing
   then works, it was the firewall.
3. **Pair by key instead.** This always works and needs no network at all:

   - On the machine that will *invite*, click **Show my pairing code instead**
     in the pairing dialog. It shows a `workspace-clone://pair?...` link.
   - Copy it and send it to the other machine however you like — email, Slack,
     a text message.
   - On the other machine, click **Pair a device**, paste it into the
     **"Or paste the other machine's key"** box, and press **Check the safety
     number**.
   - Then complete the ceremony in [§4](#the-ceremony-step-by-step) from step 5.

   Paste the link over a channel you already trust. It is not a secret, but it
   *is* the thing the safety number is there to confirm.

---

## 5. Choose what can travel between them

While pairing, you tick what that device is allowed to do **on the machine you
are pairing on**:

| Tick box                                               | Means                                                       |
| ------------------------------------------------------ | ----------------------------------------------------------- |
| **Receive** — "May be sent workspaces"          | The other machine may send workspaces**to this one**. |
| **Send** — "May send workspaces to this device" | The other machine may push workspaces**to this one**. |

Both labels are written from the perspective of the machine you are standing at.
It is the single most confusing part of the UI, so here is the concrete recipe:

> **You want to send from the Mac to the Windows laptop.**
>
> - **On the Mac**, pair the laptop and tick **Receive** only. (This machine may
>   be sent to.)
> - **On the Windows laptop**, pair the Mac and tick **Send** only. (This machine
>   may send to the Mac.)
>
> Then it works. Reversed, the send is refused and the laptop tells you exactly
> why.

If you want both machines to send to each other, tick **both** boxes on both
machines.

Both directions are enforced independently, and refusals say which gate failed:

- The Mac checks that the laptop is paired, not revoked, and has **Receive**.
- The laptop checks that the Mac is paired, not revoked, and has **Send**.

You can change a device's permissions later by pairing it again, or by
revoking it entirely with the shield button.

---

## 6. Capture a workspace on the Mac

### Tell it where your projects are

The first time, the app needs to know which folders to look in.

1. Go to **Settings**.
2. Under **Project folders**, click **Add a folder** and choose the folder your
   projects live in — for example `~/code`. You can add several.
3. The **What the scan found** list should populate within a few seconds. If it
   says the folders could not be scanned, the path does not exist or you do not
   have permission to read it.

### Capture

1. Go to **Workspaces** → **Capture New Workspace**.
2. Give it a name you will recognise on the other machine, e.g.
   `Welcome Rewards`. Click **Continue**.
3. On **Capture Workspace**, tick what to include:
   - **Project folders** — the checkboxes come from the scan in Settings.
   - **Applications** — which tools you had open.
   - **URLs** — internal links, one per line.
   - **Environment variables** — names only, one per line.
4. Read the two boxed notices. They are the privacy contract, and they are
   accurate:
   - **What is never captured** — environment variable *values*, tokens, cookies
     and private keys; **absolute paths**, which are replaced with a redacted hint
     like `~/code/thing`; credentials in repository remotes; and uncommitted
     **git state** — a dirty worktree is reported as dirty, and the `.git`
     internals never travel (the working-tree files themselves do; see the next
     bullet). Shell history is never read at all.
   - **Project files travel too.** Every selected project's files are copied into
     the workspace — that is the point of a "clone". The copy is made at capture
     time, and the denylist keeps it safe: version-control internals
     (`.git`), build output (`node_modules`, `target`, `dist`, `build`, `.next`,
     caches), Python virtualenvs, `Pods`, and secrets (`.env`, `.pem`/`.key`,
     `id_rsa` & friends, databases, logs) are never included, nothing bigger
     than 128 MiB ships, and the whole snapshot stops at 512 MiB with a warning
     saying so. A file 1 MiB over the cap or a database sitting in the tree is
     reported in the capture warnings, not silently dropped.
   - The manifest itself is shown on the next screen, sealed, before anything is
     sent. It is the actual document that will travel, so you can read exactly
     what is in it.
5. Click **Capture Workspace**. The log records how many files and bytes the
   workspace carried; the receiving machine's restore report tells you on the
   other end. If the snapshot overflowed the 512 MiB cap, a warning says so and
   the archive is truncated to what fit; a file over the 128 MiB per-file cap is
   skipped and named in the warnings rather than silently dropped.

The workspace now appears under **Workspaces** with a green **Captured** badge.
Its manifest is stored **encrypted on this machine**, sealed with a key that
never leaves the Mac, and so is its file archive.

---

## 7. Send it to the Windows laptop

1. Go to **Workspaces**.
2. Click the workspace you just captured. This opens **Send Workspace**.
3. Under **Destinations**, your paired Windows laptop should be listed. If it
   is not, it has not been paired, has been revoked, or lacks the **Receive**
   scope — go back to [§5](#5-choose-what-can-travel-between-them).
4. Click **Send**.

### What happens

1. The Mac checks the laptop is allowed to receive. If not, it refuses **before**
   sending, and tells you why.
2. Mac and laptop complete a **Noise_IK handshake** over a direct TCP connection
   on your local network. This is mutually authenticated: both ends prove they
   hold the private key matching the public key you confirmed with the safety
   number. Traffic is encrypted from the first byte.
3. The manifest travels in framed chunks, with a SHA-256 digest checked at the
   end. A corrupted or truncated transfer is rejected rather than stored.
4. The laptop validates the manifest, checks the Mac is paired and has **Send**,
   and **re-seals the manifest with its own key**.
5. The laptop shows an **arrival banner** within a few seconds.

### The step that catches everyone

> **Sending is not the same as arriving.**
>
> The Mac reports "sent" as soon as the bytes are delivered over an encrypted
> channel. It cannot know whether the laptop *accepted* them, because the
> acceptance decision is made on the laptop, after the handshake ends.
>
> **Always check the Windows laptop's screen.** That is where the truth is. If
> the laptop refused the workspace, it says so, and gives the reason.

For the record, a workspace that arrived is badged **Received** rather than
**Captured** — it was not captured on that machine.

---

## 8. Restore it on the Windows laptop

This is the half of the job most people never reach, because they assume
transferring a workspace *is* the product. It is not. Transferring hands over a
description; restoring turns that description back into a working setup.

The restore runs as four screens. **Continue is never blocked** — you can always
press on, and the outstanding requirements stay visible and recorded either way.
The screens exist to let you decide, not to gatekeep you.

### Step 1 — Preflight: "what is missing here?"

Open **Workspaces → Welcome Rewards → Preflight**.

The app compares the manifest against this machine and reports, requirement by
requirement, whether it is satisfied. The statuses it can show:

| Status                           | What it means                                                                                         |
| -------------------------------- | ----------------------------------------------------------------------------------------------------- |
| **Ready**                  | Checked on this machine and satisfied. Usually shows its evidence —`node 22.1.0`, not just "yes".  |
| **Ready — you confirmed** | You ticked it off by hand rather than the app checking it.                                            |
| **Present, unverified**    | The tool is installed, but the app could not confirm it is the right version or configured correctly. |
| **Sign-in required**       | Installed, but you are not signed in. Suggests a command, stating the permission it needs.            |
| **Account mismatch**       | Signed in, but as a different account than the one the workspace expects.                             |
| **Not applicable**         | Deliberately skipped on this platform.                                                                |
| **unknown**                | The app could not check this one.                                                                     |

That last one matters. `unknown` is **not** counted as satisfied — a check that
could not run has established nothing, and treating it as a pass would let a
workspace claim readiness on the strength of a check that never happened. A
requirement with no adapter to check it is honestly reported as unknown rather
than quietly passing.

Nothing is changed. This screen only *tells you* where you stand. It is safe to
read and re-read.

The header shows a readiness percentage and, below it, `N of M required` — how
many **required** requirements are satisfied. When any are outstanding it says
how many, and names them. Optional requirements never hold readiness back.

### Step 2 — Prepare: "what will I have to do myself?"

Continue to **Prepare this machine**.

This screen lists everything preflight found that is **not** ready, and for each
one, the action recorded by the tool's own adapter — a documentation link or a
command that tool's author published, not something this app invented.

You tick off anything you have handled yourself, and each tick re-runs preflight
for that requirement. A requirement you confirm becomes **Ready — you confirmed**,
which *does* count towards readiness. That is the difference between "checked
and known good" and "the user says so", and the report keeps them distinct.

The screen carries an explicit notice: **this app installs and signs in to
nothing.** Where it has no recorded action, it says *No automatic action
available* rather than inventing an install method.

### Step 3 — Restore Plan: "where do things go, and what will happen?"

Continue to **Restore Plan**. Two things happen here.

**Destination folders.** The manifest records each project under a *bucket* label
(`code`, `work`, and so on) rather than an absolute path — the Mac's home
directory is not the Windows laptop's home directory. This screen asks you to
name a real folder on **this** machine for each bucket.

> A bucket holding **one** project uses the folder exactly as you write it —
> typing `E:/workspace-clone` restores the code *into* `E:/workspace-clone`, not
> into a subfolder under it. A bucket holding **several** projects nests each in
> its own folder beneath yours, so they cannot overwrite one another. The step
> table always shows the exact final path of every project before anything runs.

> **Leave a bucket empty and its projects are planned without a destination and
> reported as unplaced.** They are never written somewhere you did not name. This
> is deliberate, and it is the most common cause of a restore that "succeeded"
> while putting nothing where you expected — fill these in.

**The step table.** Every action, in order, each marked **Required** or
**Optional**, each showing its dependencies. A required step runs; an optional
step runs **unless you switch it off**. Nothing runs until you choose.

Each project with a destination gets a required **file-restore step**
(`extract-files`) that writes the project's captured files into that folder,
and any step that inspects or opens the project runs after it. The report tells
you how many files landed, how many bytes, and whether anything was skipped
(unsafe names, symlinks, oversized files) or truncated by the size cap. A
workspace captured before files existed reports *"No files were captured"* and
restores everything else.

### Step 4 — Restore and read the report

Run the restore, then read **Restore Report**: what ran, what succeeded, what did
not, and what — if anything — is left for you to do by hand.

Every step's result is recorded permanently under the workspace's **History** tab,
alongside any earlier restore attempts. That tab also has a **Transfers** card
listing every transfer of that workspace in both directions with its status — and
so a transfer the other machine *refused*, with the reason it was refused.

> **The History tab only appears once you have selected a workspace.** Its
> trigger is labelled `History: <workspace name>`, so if you cannot see it, click
> the workspace in the **Workspaces** tab first. That is also the tab an arrival
> banner sends you to when something was refused, since a refused workspace has
> no workspace to open.

### One thing this app will not do

It will not install software or sign you in to an account on your behalf. If a
step needs `gh auth login`, it tells you the command and asks you to run it
yourself. That is a deliberate limit, not a missing feature.

---

## 9. When something goes wrong

Work down this list; it is ordered by how often each problem is the cause.

### The other machine is not listed under "Devices on the network"

**Most common cause: they are not on the same network segment.** Guest Wi-Fi and
many corporate networks put clients on isolated networks that cannot see each
other, and mDNS does not cross a router.

- Confirm both are on the same network and neither is a guest network.
- Temporarily disable both firewalls and retry — that isolates the cause in one
  step.
- If it still fails, **pair by key** instead. See
  [§4](#if-the-laptop-is-not-listed). This bypasses discovery entirely.

### The Mac says "not paired" or "not allowed to send"

The pairing is one-sided, or the scope is wrong. Check **both** machines against
the table in [§5](#5-choose-what-can-travel-between-them) — the fix is almost
always to pair the other machine with the opposite scope.

### The Mac says "sent" but nothing appears on the laptop

In order:

1. **Look at the laptop.** A refusal is reported on the receiving machine, with
   the reason. This is the answer; read it.
2. **Is the laptop running?** Receiving works while the window is closed, but
   only while the *app* is running. Quit the app and nothing can arrive.
3. **Is the laptop on the same network as the Mac right now?** Moving between
   networks invalidates the address the Mac would use to reach it.
4. **Was the send recent enough to still be running?** A send is a live
   connection. Closing a laptop mid-transfer fails it; there is no queue to retry
   into.

### The send fails after ~30 seconds with a timeout

The Mac connected to a port and nothing answered. Usually a firewall on the
receiving machine.

- Confirm the OS firewall prompt was allowed on the **receiving** machine.
- Confirm both are on the same subnet.

### The Mac says "No route to host" and names a `fe80::…` address

That address is an IPv6 link-local address. It names a *link*, not a
destination, and the OS refuses to guess which of your interfaces the peer is
on, so the attempted connect fails before it reaches the network. Current
builds do not hit this: the send tries every advertised address with IPv4
first (which almost always just works on a LAN), and when the peer is only
reachable over IPv6 the address is scoped to the local interface that can host
it. Update the app on the Mac and send again.

### "Not accepted" on the laptop, with a reason about a device it has never seen

This happens when a workspace is **forwarded** — sent from A to B, then B sends
it to C. C needs a record of the machine the workspace was *captured on*, not
just the machine that forwarded it. Pair all three machines with each other.

### A workspace will not restore

Go back to **Preflight** and read the failing requirement. It names the tool, the
version, and what it actually found. The common ones:

- **Present, unverified** — the right tool is there but the app could not confirm
  the version or configuration. Often a version the manifest did not expect.
- **Sign-in required** — installed but not signed in. The screen gives you the
  command; run it yourself.
- **Account mismatch** — signed in as somebody else. Sign in as the account the
  work belongs to.
- **unknown** — nothing on this machine can check this requirement. Common for
  applications this build has no adapter for. It is reported as unknown, not as
  a pass, so it will show as outstanding.

### A received workspace restores as "nothing selected"

Preflight on this machine shows no projects and no files to restore, but the
receiver expected the sender's selected content.

This is almost always the **capture**, not the transfer: the workspace the
sender shipped contained no project entries. A capture with only the Terminal
adapter ticked (or an empty project selection) produces a manifest with zero
projects, and zero projects means no file archive is carried at all — the
restore plan then legitimately has nothing but the adapter steps.

Confirm on the **sending** machine:

1. Open the capture screen and check **Projects**: it must show repositories it
   found. The scan looks in `~/projects` and `~/Documents` by default. If your
   code lives elsewhere, add the folder in **Settings → Project locations**, then
   rescan.
2. Tick the projects you want **and** the adapters. Terminal working directories
   come from the selected projects, so a terminal session with no projects has no
   folders.
3. Re-capture, then send again. The receiving machine's preflight will now list
   the projects and a file step.

The arrival banner is the receiving side's own reading of the manifest, so this
also applies when a banner looks emptier than expected — check the sender's
capture first.

### Both machines are on the same Wi-Fi but discovery still fails

Test the network directly. `ping` the Mac from the laptop and vice versa. If
that fails, it is the network, not this app.

---

## 10. Where your data lives

On **each** machine, under the app's own folder:

| Platform | Path                                                                 |
| -------- | -------------------------------------------------------------------- |
| macOS    | `~/Library/Application Support/com.workspaceclone.WorkspaceClone/` |
| Windows  | `%APPDATA%\com.workspaceclone.WorkspaceClone\`                     |

Inside:

- **`workspace-clone.db`** — SQLite. Workspaces, devices, transfer history,
  restore runs, settings.
- **`manifests/`** — one file per workspace: the **encrypted** manifest.

### The keys, and why they are separate

The app generates three independent keys per device:

- **Noise static key** — your device's identity on the network. It is *public*
  (that is how peers address you) and it is the basis of your fingerprint and your
  safety numbers. Never leaves the device.
- **Storage key** — seals manifests **at rest**. It is what makes
  `manifests/*.json` unreadable to anyone who copies the file off your disk. This
  key never leaves the device and is never transmitted.
- **Signing key** — reserved for future manifest signing. Generated, not yet used
  in the transfer path.

All three live in the **OS credential store** — macOS Keychain, Windows
Credential Manager. Not in the database, and not in any file in that folder.

> **Consequence worth knowing:** the database and `manifests/` are only readable
> on the machine that created them. Copying the app folder to a new machine gets
> you a list of workspaces you cannot open. The sending machine re-sends the
> manifest as plaintext inside the encrypted channel for exactly this reason, and
> the receiving machine seals it afresh with its own key.

### What is never read, and what is never captured

The app makes two different promises, and the **Welcome** screen (the first thing
you see) lists both. They are worth reading in the app rather than trusting a
summary, but here they are:

**Never read by any capture adapter:**

- Passwords, tokens, cookies, passkeys, recovery codes
- Private keys, SSH agent state, and the contents of any `.env` file
- Browser tabs, history, profiles, or session databases
- Clipboard contents — there is no opt-in to accept them yet
- The OS credential store, where this app keeps its own keys
- Terminal scrollback, shell history, or command output

(The **file snapshot** is a separate promise, described above in §6: it copies
the selected projects' *files* — the point of a "clone" — but only what survives
the denylist: no `.git`, no build output or caches, no `.env` or `.env.*`, no
`*.pem`/`*.key`/`id_rsa`, no databases or logs, no symlinks, nothing over
128 MiB, and a hard 512 MiB total. What the snapshot skips is listed in the
capture's warnings.)

**Recorded as a fact, but its contents never leave the machine:**

- Environment variable **values** — only names are captured
- **Absolute paths** — replaced with a redacted hint such as `~/code/thing`,
  because your home directory name is not the other machine's business
- **Credentials in repository remotes**
- **A dirty worktree as a git report** — the git adapter records *that* the
  worktree is dirty, which is what a recipient reads as "the owner had
  uncommitted work". The *files those changes live in* do travel when file
  transfer is on, because that is what the recipient needs to do the work.

If you want to check rather than trust: capture a workspace, and the manifest is
shown on the next screen before it is sealed. The sealed manifest and file
archive are stored on this machine and re-sealed with the receiving machine's
own key when they arrive there.

---

# Part 2 — Working on the code

## 11. How the codebase is laid out

```
openshorts/                     ← frontend + Rust workspace + tracker
├── docs/                       ← documentation + the tracking workbook
│   ├── AI_HANDOFF.md           ← handoff notes for the next agent session
│   ├── DEVELOPER_GUIDE.md      ← this file
│   ├── ISSUE_WINDOWS_HANDOFF_2026-09-29.md
│   └── WorkspaceClone_Tracking.xlsx
├── scripts/                    ← tracker updater, parity + token checkers
├── src/                        ← React + TypeScript
│   ├── lib/ipc.ts              ← THE ONLY FILE THAT NAMES A TAURI COMMAND
│   ├── types/index.ts          ← TypeScript mirror of every Rust payload
│   ├── styles/index.css        ← THE ONLY FILE THAT DEFINES A COLOUR
│   ├── store/useThemeStore.ts  ← light/dark/auto + sidebar collapse
│   ├── screens/                ← one file per route
│   ├── components/             ← the app shell + shared pieces
│   │   ├── Layout.tsx          ← sidebar, toolbar, status bar; owns the <h1>
│   │   ├── AppearanceControl   ← the three-state theme selector
│   │   ├── EmptyState.tsx      ← shared empty state
│   │   └── ui/                 ← Button, Card, Badge, Dialog, Tabs
│   └── test/                   ← fake backend + route, theme and receive tests
├── tailwind.config.js          ← maps tokens to utilities; darkMode: 'class'
└── src-tauri/                  ← Rust workspace
    ├── Cargo.toml              ← virtual manifest, no [package]
    ├── core/                   ← types: manifest, device, crypto primitives
    ├── crypto/                 ← Noise, AEAD, key storage, fingerprints
    ├── db/                     ← schema, migrations, repositories
    ├── network/                ← Noise_IK transport, mDNS discovery
    ├── adapters/               ← per-tool detectors (node, git, gh, …)
    ├── preflight/              ← "what is missing here?"
    ├── restore/                ← planning and execution
    ├── commands/               ← Tauri commands = the API surface
    └── app/                    ← the binary
        ├── src/lib.rs          ← the command registry (generate_handler!)
        ├── tauri.conf.json     ← must be HERE, not in src-tauri/ (see §15)
        └── capabilities/       ← must be HERE, not in src-tauri/ (see §15)
```

> **Dead files, still on disk:** `src-tauri/build.rs` and `src-tauri/src/`
> (`lib.rs`, `main.rs`) are left over from when this was a single crate at
> `src-tauri/`. `src-tauri/Cargo.toml` is a virtual workspace manifest with no
> `[package]` section, so Cargo never builds them — `cargo metadata` lists nine
> packages and the root is not one of them. `src-tauri/src/main.rs` even calls
> `app_lib::run()`, a crate that does not exist, so it could not compile if
> anything tried. They are harmless but misleading; delete them if you want the
> tree to reflect what is actually built.

### The three rules that matter most

**1. `src/lib/ipc.ts` is the only place in the frontend that names a Tauri
command.** Every call is a named function with a declared return type, so a
signature change in Rust becomes a TypeScript error at that line rather than
`undefined` rendered into a screen three components away.

**2. `src-tauri/app/src/lib.rs` is the authority for what exists.** Its
`generate_handler![…]` block is the registry. `scripts/check_command_parity.py`
compares it against `ipc.ts` in both directions and fails on any mismatch.

Run it after adding a command:

```bash
python3 scripts/check_command_parity.py
```

It catches the one class of bug TypeScript cannot: a wrapper calling a command
Rust never registered, which compiles perfectly and fails at runtime with
"command not found".

**3. `src/styles/index.css` is the only file that defines a colour.** Never
write a raw palette value in a component — no `bg-yellow-50`, no `text-slate-600`,
no `#1a1a2e`. Every colour is a token (`bg-surface`, `text-fg-muted`,
`bg-warning-bg`, `border-hairline`) defined once per mode, so light and dark stay
in step and neither can drift.

```bash
python3 scripts/apply_design_tokens.py --check   # exits non-zero on any raw colour
```

`green`, `amber` and `red` are **reserved for status** — ready, needs you, failed.
The accent is indigo and nothing else should use it. Spending a status hue on
decoration is how a warning badge stops reading as a warning.

### The theme

`src/store/useThemeStore.ts` holds a three-state mode — `light` / `dark` /
`system` — in `localStorage` under `wc.theme`, plus the sidebar's collapsed flag
under `wc.sidebar.collapsed`. `applyTheme()` toggles `.dark` on `<html>` and sets
`colorScheme` so macOS scrollbars match the content.

> **Why three states and not a light/dark switch.** A boolean toggle makes
> "follow the system" unreachable the moment it is touched, and a user who wants
> the OS default back cannot get it. For a desktop app that may be handed to
> someone else, `Auto` has to be reachable.

`index.html` inlines a deliberate one-line duplicate of the storage logic. It is
the only way to run **before first paint**; without it the window flashes the
wrong appearance on every launch. Keep the two in step — the store's
initialiser calls an exported `readStoredMode()` so a test can assert the round
trip, and the inline copy is checked for agreement.

### Adding a command, end to end

1. Write it in the right `src-tauri/commands/src/*.rs` module with
   `#[tauri::command]`.
2. Return `core::Result<T>`. Note: `Result` here is a **one-generic** alias. When
   you need two generics, spell out `std::result::Result`.
3. Register it in `src-tauri/app/src/lib.rs`.
4. Add a wrapper to `src/lib/ipc.ts` with an explicit return type.
5. Mirror the payload in `src/types/index.ts`.
6. Add a handler to `fullBackend()` in `src/test/routes.test.tsx`, or an existing
   route test fails with "no fake handler" — which is the intended behaviour.
7. Run the parity script.

---

## 12. Getting the code to compile and run

```bash
source "$HOME/.cargo/env"   # required in every new shell on macOS/Linux
```

### Development

```bash
npm run tauri dev
```

Starts Vite, compiles the Rust, opens the window, and hot-reloads both. The first
build takes minutes; later ones take seconds.

> **If you see `No package info in the config file`,** the Tauri CLI cannot find
> the app. It requires `tauri.conf.json` to sit beside a `Cargo.toml` that has a
> `[package]` section — that is `src-tauri/app/`, not `src-tauri/`. If you have
> moved the config, put it back and drop the `config_path` override in
> `app/build.rs`. This is documented further in [§15](#15-conventions-that-will-bite-you-if-you-break-them).

### Tests

```bash
# Rust — 405 tests
cd src-tauri
cargo test --workspace -- --test-threads=2

# Frontend — 99 tests, run once and exit (npm test alone starts a watcher)
npx vitest run

# Type checking
npx tsc --noEmit

# Production frontend build (runs tsc first, so it also type-checks)
npm run build

# Command-surface parity
python3 scripts/check_command_parity.py

# No raw palette colours in src/ (exits non-zero if any remain)
python3 scripts/apply_design_tokens.py --check
```

A full green run is: `cargo build --workspace` with no warnings, then all five of
the above. The token check matters as much as the tests — it is the only thing
standing between you and an unreadable status colour in dark mode.

`--test-threads=2` is not optional on this project. Default parallelism causes
binary contention on the several test binaries that bind real sockets, which
presents as a hang rather than a failure.

### The binary

```
src-tauri/target/debug/app
```

`.cargo/config.toml` at the project root deliberately has **no `[build] target`**. It used to pin `build.target = "aarch64-apple-darwin"`, which was
harmless on an Apple silicon Mac and fatal everywhere else: Cargo applies
`[build] target` to every invocation from anywhere in the repo on any host, so
`cargo build` on the Windows laptop tried to compile for Apple Silicon and
failed before compiling anything.

If you want a specific triple, pass it: `cargo build --target aarch64-apple-darwin`.
The binary then appears under `target/<triple>/debug/`. A stale
`target/aarch64-apple-darwin/debug/app` from the pinned era may still be sitting
in your tree; it is a leftover and will not match current source.

---

## 13. The tests, and what each layer covers

| Layer                                                          | Count         | Covers                                                                                                                |
| -------------------------------------------------------------- | ------------- | --------------------------------------------------------------------------------------------------------------------- |
| `core`, `crypto`, `db`, `preflight`, `restore` units | 159           | pure logic: manifest validation, crypto, repositories, requirement checks, restore execution                          |
| `files` units                                                | 13            | snapshot denylist + caps, archive round-trips, hostile extraction, the wire envelope                                  |
| `adapters` units                                             | 62            | capture, detection, path redaction, the safety predicates                                                             |
| `network` units                                              | 79            | handshake, framing, digest, cancellation, discovery and naming, connect-address selection                             |
| `network/tests/loopback_transfer.rs`                         | 8             | real TCP, real Noise_IK, two services in one process                                                                  |
| `commands` unit tests                                        | 66            | authorization gates, refusals, validation, platform naming, destination resolution                                    |
| **`commands/tests/two_device_transfer.rs`**            | **12**  | **the whole two-machine path, files included**                                                                  |
| **`adapters/tests/ipc_selection_contract.rs`**         | **6**   | **TypeScript and Rust agree on field names**                                                                    |
| **Total Rust**                                           | **405** |                                                                                                                       |
| Frontend (`vitest`)                                          | 99            | routes resolve the URL id; receive UI; IPC wiring; theme + shell; one`<h1>` per route; status bar reads the backend |

### `two_device_transfer.rs` is the important one

It is the only test that exercises what the app exists to do. Two real
`TransferService` instances, each with its own database, its own identity key,
its own bound listener and its own sealing key, transferring over a real socket
with a real Noise_IK handshake and the real command-layer receive path.

It covers: a workspace arriving and being readable with the receiving machine's
key; a manifest with projects and requirements surviving byte-for-byte; a
workspace carrying its **file archive** arriving, being re-sealed with the
receiver's key, and extracting the changed file the Mac captured; an unpaired
sender refused; a peer trusted only to receive being unable to push; a revoked
peer refused; one refusal not stopping the next transfer; forwarding through a
third machine; a silent peer not stalling the listener; the arrival shape the
window polls for; and the accept loop being startable with no Tokio runtime
entered.

Every one of those tests is a bug that was found and fixed. They are written to
fail loudly if the behaviour regresses — for example the "sender's key must not
open what the receiving machine sealed" assertion exists because the original
send path transmitted the *sender's* sealed manifest, leaving the receiver
holding bytes it could not open.

### What is deliberately *not* covered, and why

**The OS credential store.** A test binary reading a keychain item that belongs
to the app blocks on a user prompt nobody can see. So the sealing key is passed
into the receive path as a parameter, and the tests supply their own. What is
asserted is the property that matters — the stored manifest opens with the key
that sealed it and not with the sender's — which is the thing a real keychain
would change nothing about.

**The GUI.** No assistive access and no screen recording in the build
environment, so the window cannot be driven or captured. The backend is verified
by its own tests plus launching the real app and asserting on its logs. UI
behaviour is covered by jsdom tests that go through the real IPC path. A change
that only shows up on screen — a CSS problem, an interaction a mouse would
trigger — will not be caught automatically. **Try it by hand after frontend
changes.**

> **jsdom cannot see the UI.** It has no layout engine and no CSS cascade, so it
> cannot tell you that a colour resolved, a sidebar is 208px wide, or two
> elements overlap. It will happily pass a screen that is visually broken.
>
> This is not theoretical. The sidebar was invisible at the shipped window size
> for the whole life of the project while the suite was green, because every
> test renders at jsdom's 1024px default — precisely the width where the
> Tailwind `lg` breakpoint makes the bug disappear.
>
> For layout and colour, the substitute is to serve the app (`npm run dev`) and
> read `getComputedStyle` and `getBoundingClientRect` in a real browser, and to
> measure contrast ratios rather than eyeball them. Two traps when doing that:
> **a hidden tab does not advance the CSS animation clock**, so every
> `transition-colors` property reports its pre-transition value forever and looks
> exactly like a broken token system — call
> `document.getAnimations().forEach(a => a.finish())` before reading anything;
> and **jsdom implements neither `localStorage` nor `matchMedia`**, so
> `src/test/setup.ts` has to fake both, with a *mutable* `matches` read at
> listener time.

**No automated test covers contrast.** Dark mode was verified by measuring it
(fg-on-bg 5.79–9.48 dark, 5.48–6.63 light, all clearing WCAG AA's 4.5), not by a
test that would fail if a future colour change broke it. A `jest-axe` or
`vitest-axe` pass would close that; it is not installed.

### The seam between TypeScript and Rust

`src-tauri/adapters/tests/ipc_selection_contract.rs` closes the one gap the other
layers cannot. The TypeScript and the Rust command structs are hand-mirrored in
two languages, and nothing in the build used to compare them: `tsc` checks
TypeScript against its own types, `cargo test` builds the structs in Rust where
field naming is irrelevant, and `check_command_parity.py` compares command
*names*. A field renamed on one side only is invisible to all three.

So the test reads the real `src/types/index.ts` and compares its field names
against the names serde will actually accept. Rename a field in either language
and the build fails. `deny_unknown_fields` on the capture-selection structs is
the other half: serde's default is to ignore an unrecognised key, which turns
any such mismatch into a **successful capture that silently omits what the user
selected**.

> This cost a real live-test failure, and the error message pointed somewhere
> unhelpful. A capture failed with `missing field `file_transfer`` — a field the
> UI *was* sending, under the name it was also sending. Rust read the selection
> as `snake_case`, the UI wrote it as `camelCase`, and the only reason anything
> failed at all is that the nested `Policy` happened to be checked first. Had
> that gone the other way, the capture would have "succeeded" with every list
> empty: the URLs the user pasted would have vanished and the browser adapter
> would have looked like the culprit, four functions from the actual cause.

Two things worth knowing if you touch that test. `Policy` is the exception to
`rename_all` and takes per-field `alias`es instead, because it is also the
manifest's policy and the manifest is `snake_case` on the wire (DEC-029) — a
blanket rename there would appear to work while rewriting the wire format for
every manifest already stored. And the field-name comparison on its own does
**not** catch removal of the `rename_all` attribute, because it converts
`snake_case` to `camelCase` first and therefore agrees either way; a separate
test pins the convention directly. Both facts were established by reverting the
fix and re-running, not by inspection.

---

## 14. How a workspace actually travels

```
MAC                                     WINDOWS
────                                     ───────
capture_workspace
  └ manifest built in memory
  └ sealed with the MAC's storage key  → manifests/<id>.json
  └ when policy.file_transfer is set: project files walked (denylist +
    size caps), tarred into one archive, sealed with the MAC's key
      → files/<id>.files.sealed   +  workspace_files row
  └ workspace row written

send_workspace(workspace_id, destination)
  │
  ├─ authorise: destination paired, not revoked, has `receive`
  │     └ refuse here → nothing leaves the machine
  │
  ├─ open the manifest with the MAC's key, re-serialise as JSON
  │     └ NOT the sealed file: sealed with a key the receiver does not have
  ├─ wrap: MAGIC ‖ len ‖ manifest ‖ (archive if the workspace has one)
  │
  ├─ TCP connect → Noise_IK handshake (authenticated, encrypted)
  ├─ frame: header ‖ SHA-256(payload)
  ├─ send frames, awaiting an ack per frame
  └─ record transfer_sessions: completed / failed / cancelled
                                            accept loop (always running)
                                              ├─ accept, then handle on its
                                              │  own task  ← see §15
                                              ├─ authorise sender: paired,
                                              │  not revoked, has `send`
                                              ├─ size limit: 544 MiB
                                              │  (512 MiB files + headroom)
                                              ├─ split the envelope: manifest
                                              │  (+ archive when carried)
                                              ├─ parse + validate()
                                              ├─ workspace id must agree
                                              ├─ capture device must be known
                                              ├─ seal with the WINDOWS key
                                              │  (manifest and archive)
                                              └─ record + show an arrival
restore (windows)
  └ generate_restore_plan: each project with a destination gets an
    `extract-files-<id>` step, after its map-path step
  └ execute_restore: the executor opens files/<id>.files.sealed with the
    WINDOWS key and extracts each project's entries into its mapped folder
```

### Why the payload is the readable manifest plus its archive, inside the envelope

Sealing is protection **at rest**, and the key that does it is the *device's own*
storage key. No other machine has it. So transmitting the sealed file would
arrive as bytes the receiver cannot open — a workspace that appears in the list
and then fails to restore, with the real cause several steps away from the
symptom.

Confidentiality in flight does not need the seal: the Noise channel is
authenticated and encrypted for its whole duration. So the manifest travels as
JSON, and — when the capture included files — so does the file archive, packed
behind it in a versioned envelope (`WCFB1` magic, u32 manifest length, manifest,
archive). Payloads without the magic (every transfer made before this feature)
read back as manifest-only, so old senders still arrive. The receiver seals the
manifest *and* the archive with its own key on arrival. The stored result is
indistinguishable from a locally captured workspace, and the archive's byte and
file counts are recorded next to it so restore can say what a workspace carries.

The file archive is a plain tar, not a zip: no compression, so the side that
signed the payload can predict its exact size (that is what the caps and the
announced totals are for), and a deterministic entry order, so equal folders
produce byte-identical archives. Extraction treats every entry as hostile —
traversal names, absolute paths, backslashes, drive-letter prefixes, symlinks,
reserved Windows device names and oversized entries are all refused, and the
same 512 MiB total cap applies on the way out.

One consequence: `workspaces.manifest_digest` is a digest of the **locally
sealed** bytes, not of what arrived — the two are different documents. A
receive-side check comparing them would fail every time. The arrival reports the
arrived digest separately, for exactly this reason.

### Why authorization runs in both directions

Neither side trusts the other on the strength of a successful handshake:

- **Sending** requires the destination to be paired, not revoked, and to hold the
  **receive** scope.
- **Receiving** requires the sender to be paired on *this* device, not revoked,
  and to hold the **send** scope.

The identity checked is always the **Noise static key the handshake
authenticated** — never the device id in a frame header, which is an
unauthenticated string anything on the network could put there.

### The connect uses every advertised address, and the listener accepts both families

A device discovered over mDNS may list a link-local IPv6 address (`fe80::…`)
before its IPv4 address, and connecting to a bare `fe80::…` fails before the
network is even consulted: the address names the *link*, and the OS refuses to
guess which interface the peer is on. The send therefore does not take the
first entry. It expands every advertised address into connect candidates —
IPv4 first, each link-local IPv6 scoped to a local interface that can host it
(`fe80::x%en0`) — and races them against one connect budget, taking the first
that answers, so a peer reached over IPv4 connects even when its record lists
IPv6 first. The transfer listener is bound dual-stack (`[::]` with
`IPV6_V6ONLY` off) where the platform allows it, which is what lets a peer
connecting over its link-local IPv6 address be accepted at all; where that is
impossible the IPv4-only bind the listener has always used remains the
fallback. Manual pairing accepts a `%zone` suffix (`fe80::1%en0`) and scopes a
bare link-local address itself.

### The manifest has no signature field

Integrity in transit is the Noise AEAD's job, and it is a stronger guarantee than
a detached signature over the same bytes. There is no "verify signature" step
because there is nothing to verify. Do not add a claim about one.

---

## 15. Conventions that will bite you if you break them

### The toolbar owns the page title

`Layout.tsx` renders the app's only `<h1>`, on every route. Screens do not
render their own heading — they keep the explanatory line where the heading used
to be. This is a desktop window, so the title is pinned in the chrome, present at
every scroll position, next to the back button on a step. A heading in the
scrollable content is a web layout.

> This was not always so, and it is a genuine trap. With a heading in *both* the
> toolbar and the content, every route showed the same words twice, and two
> elements sharing an accessible name broke real `getByRole('heading')` queries.
> `routes.test.tsx` now has parameterized tests over all nine routes asserting
> exactly one `<h1>` and that nothing in `<main>` repeats the toolbar's title. If
> you add a screen, that test is what will tell you.

### The window is a fixed size, not a viewport

It opens 1180×780 centered, with `minWidth: 900` / `minHeight: 600`. Do not reach for responsive breakpoints
(`hidden lg:flex` and friends) — they are a browser idiom and they break here:
Tailwind's `lg` is 1024px, which is *above* the old 800px default window, so
`lg:flex` meant the entire sidebar was invisible and the navigation was
unreachable. Every test renders at jsdom's 1024px default, which is exactly the
width where that class of bug does not show up, so a green suite will not catch
it. Verify geometry at 900px by hand.

### Body text is 13px

Set on `<body>` in `index.css` so markup added without a size class lands on
the dense default. 16px gave roughly 34 lines in the content
area; 13px gives about 45, which is the difference between a paired-device list
fitting on screen and needing a scrollbar. The type scale is 11 / 12 / 13 / 15,
with 20–24px reserved for figures the user reads character by character — a
fingerprint, a pairing code. Forms inherit the size explicitly; they do not by
default.

### A `TcpListener` belongs to the runtime that created it

`tokio::net::TcpListener` is tied to its runtime. Binding it in one runtime and
using it in another is not a bug you can debug later — it is a closed socket. In
tests, create the listener inside the runtime that will serve it.

### `tokio::spawn` needs a runtime; Tauri's `setup` hook has none

Tauri's `setup` runs on the main thread with no Tokio runtime entered.
`tokio::spawn` there **panics** — "there is no reactor running" — and because
`setup` is on the startup path, that panic kills the app on launch.

Use `tauri::async_runtime::spawn` for anything started from `init`. Spawns
*inside* a running task can use plain `tokio::spawn`.

This one is covered by
`the_accept_loop_can_be_started_without_a_tokio_runtime_entered`, which is a
plain `#[test]` for exactly this reason: every other test enters a runtime and
would not see the bug.

### Each connection is handled on its own task

`serve_forever` accepts a connection, then spawns handling of it and goes
straight back to accepting. Handling is bounded by a 15-second handshake timeout,
so handling inline meant one peer that connected and said nothing delayed *every
other peer* by that long. That was measured, not theorised: a test connected a
silent socket and watched a legitimate transfer take 15.03 seconds behind it.

### `INSERT OR REPLACE` is delete-then-insert

`WorkspaceRepository::upsert` uses `INSERT … ON CONFLICT(id) DO UPDATE`
deliberately. `INSERT OR REPLACE` would delete the existing row and insert a new
one, taking `snapshots`, `transfer_sessions` and `restore_runs` with it through
their foreign keys. Do not "simplify" it.

### `DateTimeUtc` silently becomes `now()` on a parse failure

`From<DateTimeUtc> for DateTime<Utc>` falls back to `Utc::now()` when the
timestamp will not parse. A corrupt timestamp therefore becomes *plausible
rather than obviously wrong*, and a bug like that is very hard to find later.
Always pass timestamps explicitly; never rely on the conversion.

### Tauri config paths resolve against the config file's directory

`tauri.conf.json` lives in `src-tauri/app/`, beside the crate that has a
`[package]` section. So `frontendDist` is `../../dist` and the icons are
`../icons/…`.

Two things depend on this and break silently if it changes:

- **The Tauri CLI** requires the config's directory to have a sibling
  `Cargo.toml` with a `[package]`. `src-tauri/Cargo.toml` is a virtual workspace
  manifest with no package, which is why `tauri dev` failed with "No package info
  in the config file" when the config lived there.
- **`capabilities/` must sit next to the build script.** `tauri-build` globs
  `./capabilities/**/*` against the *build script's* directory. When the
  capabilities were in `src-tauri/capabilities/`, the glob matched nothing and
  the build produced **zero capabilities, silently** — `app/gen/schemas/ capabilities.json` was `{}`, so the app shipped declaring no permissions at
  all. Nothing errors on a glob that matches nothing.

Check that generated file after touching this area. An empty `{}` there is a bug,
not a neutral state.

### Never pin a build target in `.cargo/config.toml`

Cargo applies `[build] target` to **every** invocation from anywhere inside the
repo, on **every** host. It is not scoped to the machine it was written on, and
it is not scoped to the directory it sits in — it applies to the whole tree
below and above it. So a pin that is correct on your laptop silently breaks
every `cargo` command for every other developer and for CI.

Pass the triple explicitly instead: `cargo build --target aarch64-apple-darwin`.
`scripts/build_windows.sh` checks for a reintroduced pin and refuses to run, but
a check in one script is not a guarantee — leave the file honest.

### `transfer_sessions` is written by both ends

It exists because "did that actually get there?" used to have no answer: the send
result lived in the window until it closed, and an arriving transfer left no
trace at all. Any new transfer path must record its outcome.

---

## 16. Known gaps

Honest list of what is not finished. None of these are hidden; each is a
decision or a limitation, not a surprise.

**The Windows build works; the Windows *app* is only partly proven.** It has
been built, installed and launched on a real Windows machine, and the two
machines now discover each other over mDNS. What is proven, and what is not:

Proven on real hardware:

- `scripts/build_windows.sh` produces a working installer. It needed two fixes
  on the first real run: the Visual Studio lookup only knew about "2022" while
  the installed Build Tools live under a directory named for the compiler
  version, and Smart App Control blocks the unsigned build scripts cargo has to
  execute (`os error 4551`).
- The Windows credential store works. This was inferred from a feature table and
  is now observed: the laptop's mDNS record carries a `device_id` and
  `static_key`, which can only exist if it read its Noise key from Credential
  Manager.
- `hostname()` reports the real machine name — the laptop advertises itself as
  `BISWAJITA`, from `COMPUTERNAME`, not the `"This Mac"` the old fallback
  produced.
- Discovery, name resolution and the firewall are all fine: a TCP connect from
  the Mac to the laptop's advertised transfer port succeeded in 0.01s.

Now proven, on the two real machines:

- The Noise handshake and the pairing ceremony. The user paired from both sides
  and the safety numbers matched. The device id stored on the Mac,
  `hnODGDVs/oYDiOhuQO941A==`, is byte-identical to the one the laptop
  advertises — derived independently on two separately-built machines, which is
  the strongest evidence available that the keychain and Credential Manager
  paths agree.
- The app serves its transfer listener. A probe that distinguishes a *bound*
  port from a *served* one passes against the running app and fails against a
  deliberately unserved listener (see §13).

Still unproven:

- Any actual capture → transfer → restore. The first attempt failed on a
  command-argument bug (BUG-024, since fixed); the end-to-end path has not yet
  been run to completion.
- The adapter probes. A successful build says nothing about whether the VS Code
  probe finds a per-user install; that only shows up on a capture. The old
  Windows list contained a literal `C:\Users\USERNAME\...` path that nothing
  expands, so per-user installs were reported as absent and preflight told users
  to install an editor they already had. **Run a capture on the laptop and check
  what the manifest says about its editor.**
- `.github/workflows/windows-installer.yml` has still never been executed.

The failure mode to expect more of: detection code that reports *absence*
rather than failing loudly, so a wrong answer becomes a wrong instruction.

**There is no incoming-pairing prompt yet.** Pairing is two-sided and both
people drive it: each machine independently initiates, compares the two safety
numbers by eye, and confirms. So when you start pairing from the Mac, the
laptop does not light up — you have to go and start it from there too. The
instruction in the dialog says so, but a real notification with Accept/Decline
is the missing piece. Adding it means a new frame kind on the wire and a branch
in the accept loop, which is a real change to the trust boundary rather than a
screen.

**The macOS bundle is ad-hoc signed, and `spctl` will still say "rejected".**
`bundle.macOS.signingIdentity` is set to `"-"` in `tauri.conf.json`, so
`tauri build` signs the bundle itself and `codesign -v` passes. That is enough
for Gatekeeper to load it on the machine that built it. The remaining
`rejected` is the *distribution* verdict for an app with no Developer ID and no
notarisation, and it does not affect local launch.

A DMG copied to a **different** Mac is a different story: it arrives
quarantined, and an ad-hoc signature cannot satisfy Gatekeeper, so it needs
right-click → Open. Only a Developer ID plus notarisation fixes that, and it
becomes worth doing when you distribute beyond your own machines.

**Linux, relay and multi-user are not started.** Phase 5 of the blueprint. The
data model anticipates them — the `transfer_sessions` table has a source and a
destination, and the three-machine forwarding test passes — but there is no UI,
and relaying through an always-on machine is not built.

**No Content Security Policy.** `csp` is `null` in `tauri.conf.json`. This is
left deliberately: a CSP that is wrong produces a blank window with no error,
and the GUI cannot be loaded in this environment to test one. Setting a CSP
without being able to verify the window still renders would trade a documented
gap for an invisible one. **Do this on a machine where you can see the window.**

**npm audit reports 7 findings** (1 critical, 1 high, 5 moderate), all in the
dev toolchain: `vitest`/`vite`/`esbuild` and `react-router`. Fixing them requires
major version bumps that the test suite has not been validated against. None
affect a shipped binary. Worth resolving before this goes anywhere real.

**No automated GUI tests.** See [§13](#13-the-tests-and-what-each-layer-covers).
Backend behaviour is covered; anything only observable on screen is not — and
this has already hidden a total loss of navigation, see the note there.

**The Tauri window frame is unverified.** The window opens and renders the
redesigned UI, but in this environment the Rust startup path blocks on a macOS
keychain authorisation prompt that cannot be dismissed, so the transfer listener
never binds and the window cannot be captured. All UI verification was done
against the same dev server in a browser, which exercises the styling and layout
but not the native frame. **Launch it on a machine where you can see it.**

**Single device per install.** There is no multi-user or shared-installation
model. Each user has their own app folder and their own keys.

Remove Old Content
