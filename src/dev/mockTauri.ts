/// <reference types="vite/client" />

/**
 * A browser-only stand-in for the Tauri IPC layer, for product screenshots.
 *
 * This module is imported first from `main.tsx`. It does nothing unless all of
 * these are true:
 *
 *  * the bundle was built for development (`import.meta.env.DEV`), and
 *  * we are in a plain browser (no `window.__TAURI_INTERNALS__` injected by a
 *    Tauri webview).
 *
 * Inside the real desktop app both conditions fail, so this file is inert there
 * and cannot intercept real IPC.
 *
 * When it does activate it installs a `__TAURI_INTERNALS__` whose `invoke`
 * answers every command the screens call with canned fixtures. The fixtures are
 * the app's own wire shapes (camelCase results, snake_case records, exactly as
 * `src/types` describes) so the React tree renders the true product UI — the
 * same components, CSS and layout the desktop build shows — rather than a
 * hand-built approximation.
 *
 * Two knobs are read from the URL so a screenshot driver can point at a
 * specific story:
 *
 *  * `?theme=light|dark` — forces the appearance before `initTheme()` runs.
 *  * `?story=transfer|report` — seeds the zustand store with the exact state a
 *    finished capture/restore would leave behind (see `seedStory`), so a screen
 *    that normally only has data mid-flow renders that way on load.
 *
 * Device names are masked ("Office PC" instead of a real machine name) so the
 * captures never expose the user's actual device names. Everything else is
 * plausible product data for this repository.
 */

import type {
  CaptureResult,
  DiscoveredDevice,
  DeviceIdentity,
  IncomingTransfer,
  PairedDevice,
  PlanSummary,
  ProjectScan,
  RestoreExecutionReport,
  RestorePlan,
  RestoreRunRecord,
  SendOutcome,
  SnapshotRecord,
  TransferHistoryEntry,
  WorkspaceManifest,
  WorkspaceRecord,
} from '../types';
import { useAppStore } from '../store/useAppStore';

/**
 * The minimal surface of the real `__TAURI_INTERNALS__` that the app touches.
 * Kept local (rather than merged into the global `Window`) because the test
 * suite already owns that type with its own narrower shape.
 */
interface TauriInternals {
  invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
  transformCallback: (callback: (value: unknown) => void, once?: boolean) => number;
  unregisterCallback: (id: number) => void;
  convertFileSrc: (filePath: string) => string;
  metadata: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Fixtures — real wire shapes, masked device names
// ---------------------------------------------------------------------------

const NOW = '2026-09-30T09:12:00.000Z';

const SOURCE_DEVICE_NAME = 'MacBook Pro'; // masked
const TARGET_DEVICE_NAME = 'Office PC'; // masked
const TARGET_DEVICE_ID = 'pc-1';
const FINGERPRINT = 'A1B2C3D4E5F60718';

const MANIFEST: WorkspaceManifest = {
  schema_version: 1,
  workspace: {
    id: 'ws-1',
    name: 'Workspace Clone - Win to Mac Transfer',
    captured_at: NOW,
    source_device: { id: 'mac-1', os: 'macos', os_version: 'macOS 15.2' },
    portability: 'cross_platform',
  },
  projects: [
    {
      id: 'p1',
      name: 'openshorts',
      source_path_hint: '~/Documents/openshorts',
      destination_location_id: 'code',
      git: {
        remote_hint: 'github.com/developer/workspace-clone',
        branch: 'main',
        commit: 'a1b2c3d',
        dirty_worktree: true,
        dirty_state_captured: true,
      },
    },
    {
      id: 'p2',
      name: 'docs',
      source_path_hint: '~/Documents/docs',
      destination_location_id: 'code',
      git: {
        remote_hint: null,
        branch: 'main',
        commit: 'f9e8d7c',
        dirty_worktree: false,
        dirty_state_captured: false,
      },
    },
  ],
  applications: [
    { id: 'a1', adapter: 'vscode', project_id: 'p1', required: false, extensions: ['rust-analyzer'] },
    { id: 'a2', adapter: 'terminal', project_id: null, required: false },
  ],
  requirements: {
    applications: [{ id: 'a1', adapter: 'vscode', required: false }],
    runtimes: [
      { id: 'rt-node', name: 'node', version: '22.13.1', source: 'nvm', required: true, check_command: 'node --version' },
    ],
    cli_tools: [{ id: 'cli-git', name: 'git', required: true }],
    identities: [
      {
        id: 'id-gh',
        service: 'github',
        required: false,
        check_command: 'gh auth status',
        needs_consent: true,
        expected_account: 'developer',
      },
    ],
    environment: { presence_only: ['SUPABASE_URL', 'API_BASE_URL'], values_included: false },
    services: [],
  },
  restore: { steps: [], notes: [] },
  policy: {
    file_transfer: 'all',
    clipboard: 'excluded',
    automatic_command_execution: false,
    secret_values_included: false,
  },
};

const PLAN: RestorePlan = {
  workspace_id: 'ws-1',
  steps: [
    {
      id: 'step-open',
      action_type: 'open_project',
      adapter_id: 'vscode',
      description: 'Open openshorts in VS Code',
      required: true,
      approved: true,
      config: { project_id: 'p1' },
      dependencies: [],
    },
    {
      id: 'step-git',
      action_type: 'check_git',
      adapter_id: 'git',
      description: 'Verify git state at ~/code/openshorts',
      required: true,
      approved: true,
      config: { branch: 'main', commit: 'a1b2c3d' },
      dependencies: [],
    },
    {
      id: 'step-patch',
      action_type: 'extract_files',
      adapter_id: 'git',
      description: 'Apply the 14-file uncommitted patch',
      required: true,
      approved: true,
      config: { files: '14', mode: 'patch' },
      dependencies: [],
    },
    {
      id: 'step-dev',
      action_type: 'offer_command',
      adapter_id: 'terminal',
      description: 'Offer dev server: pnpm dev',
      required: false,
      approved: true,
      config: { command: 'pnpm dev' },
      dependencies: [],
    },
  ],
  notes: [],
};

const REPORT: RestoreExecutionReport = {
  run_id: 'restore-ws-1-001',
  workspace_id: 'ws-1',
  results: [
    {
      action_id: 'step-open',
      status: 'success',
      message: 'Opened openshorts in VS Code',
      duration_ms: 612,
      details: null,
    },
    {
      action_id: 'step-git',
      status: 'success',
      message: 'Git verified at main a1b2c3d — worktree dirty',
      duration_ms: 3218,
      details: null,
    },
    {
      action_id: 'step-patch',
      status: 'success',
      message: 'Applied the patch to the destination checkout',
      duration_ms: 9204,
      details: 'patch: 14 files, +312 −78',
    },
    {
      action_id: 'step-dev',
      status: 'manual',
      message: 'Printed in a terminal for you to run',
      duration_ms: 0,
      details: null,
    },
  ],
  notes: ['SUPABASE_URL and API_BASE_URL are not set on this machine.'],
  completed_at: '2026-09-30T09:14:42.000Z',
};

const PLAN_SUMMARY: PlanSummary = {
  total_steps: 4,
  already_approved: 4,
  need_consent: 0,
  opens_something: 2,
  notes: [],
};

const WORKSPACE: WorkspaceRecord = {
  id: 'ws-1',
  name: 'Workspace Clone - Win to Mac Transfer',
  schema_version: 1,
  captured_at: NOW,
  source_device_id: 'mac-1',
  manifest_digest: 'd'.repeat(64),
  encrypted_manifest_path: '…/workspaces/ws-1.enc',
  status: 'captured',
};

const SNAPSHOT: SnapshotRecord = {
  id: 'snap-1',
  workspace_id: 'ws-1',
  captured_at: NOW,
  source_device_id: 'mac-1',
  size_bytes: 48123412,
  transfer_status: 'completed',
  transfer_id: 'tr-1',
};

const RESTORE_RUN: RestoreRunRecord = {
  id: 'restore-ws-1-001',
  workspace_id: 'ws-1',
  started_at: '2026-09-30T09:14:30.000Z',
  completed_at: '2026-09-30T09:14:42.000Z',
  status: 'completed',
  summary: 'Restored 4 steps, 1 for you to run',
};

const TRANSFER_HISTORY: TransferHistoryEntry = {
  id: 'tr-1',
  workspaceId: 'ws-1',
  sourceDeviceId: 'mac-1',
  sourceDeviceName: SOURCE_DEVICE_NAME,
  destinationDeviceId: TARGET_DEVICE_ID,
  destinationDeviceName: TARGET_DEVICE_NAME,
  status: 'completed',
  progress: 1,
  startedAt: '2026-09-30T09:13:00.000Z',
  completedAt: '2026-09-30T09:13:09.000Z',
  error: null,
};

const PAIRED: PairedDevice = {
  id: TARGET_DEVICE_ID,
  name: TARGET_DEVICE_NAME,
  public_key: 'x25519-noise-key-office-pc',
  fingerprint: 'XyZq9Bc2Df8Gh7Jk',
  trust_scopes: ['receive-workspaces'],
  os: 'windows',
  os_version: 'Windows 11',
  app_version: '0.1.0',
  created_at: '2026-09-01T00:00:00.000Z',
  last_seen: '2026-09-30T09:00:00.000Z',
  revoked: false,
  revoked_at: null,
};

const DISCOVERED: DiscoveredDevice = {
  device_id: TARGET_DEVICE_ID,
  name: TARGET_DEVICE_NAME,
  os: 'windows',
  app_version: '0.1.0',
  protocol_version: 1,
  addresses: ['192.168.1.12'],
  port: 47890,
  capabilities: {},
  static_public_key: 'x25519-noise-key-office-pc',
  last_seen: NOW,
};

const PROJECT_SCAN: ProjectScan = {
  projects: [
    {
      id: 'p1',
      name: 'openshorts',
      path: '/Users/developer/Documents/openshorts',
      isGitRepo: true,
      git: {
        branch: 'main',
        commit: 'a1b2c3d',
        dirty: true,
        remoteHint: 'github.com/developer/workspace-clone',
      },
    },
    {
      id: 'p2',
      name: 'docs',
      path: '/Users/developer/Documents/docs',
      isGitRepo: true,
      git: { branch: 'main', commit: 'f9e8d7c', dirty: false, remoteHint: null },
    },
  ],
  roots: ['/Users/developer/Documents'],
  missingRoots: [],
  unreadableRoots: [],
};

const DEVICE_IDENTITY: DeviceIdentity = {
  signingPublicKeyB64: 'ed25519-b64',
  noisePublicKeyB64: 'noise-b64',
  fingerprint: FINGERPRINT,
};

// ---------------------------------------------------------------------------
// invoke dispatcher
// ---------------------------------------------------------------------------

type Handler = (args: Record<string, unknown>) => unknown;

const handlers: Record<string, Handler> = {
  get_app_version: () => '0.1.0',
  get_platform: () => 'macos',
  get_device_key_exists: () => true,
  get_device_identity: () => DEVICE_IDENTITY,
  get_device_fingerprint: () => FINGERPRINT,
  list_workspaces: () => [WORKSPACE],
  get_workspace: () => WORKSPACE,
  get_workspace_snapshots: () => [SNAPSHOT],
  get_workspace_restore_runs: () => [RESTORE_RUN],
  get_transfer_history: () => [TRANSFER_HISTORY],
  get_incoming_transfers: () => [] as IncomingTransfer[],
  list_project_roots: () => PROJECT_SCAN,
  get_manifest: () => MANIFEST,
  list_paired_devices: () => [PAIRED],
  get_paired_device: () => PAIRED,
  start_discovery: () => [DISCOVERED],
  get_discovered_devices: () => [DISCOVERED],
  capture_workspace: (): CaptureResult => ({
    manifest: MANIFEST,
    sealed: 'sealed-ws-1',
    warnings: ['The .env file was not read — secret values are never captured.'],
  }),
  send_workspace: (): SendOutcome => ({
    succeeded: true,
    bytesSent: 48123412,
    transfer: {
      id: 'tr-1',
      workspace_id: 'ws-1',
      source_device_id: 'mac-1',
      destination_device_id: TARGET_DEVICE_ID,
      status: 'completed',
      progress: 1,
      bytes_transferred: 48123412,
      total_bytes: 48123412,
      started_at: '2026-09-30T09:13:00.000Z',
      completed_at: '2026-09-30T09:13:09.000Z',
      error: null,
    },
  }),
  generate_restore_plan: () => PLAN,
  summarize_restore_plan: () => PLAN_SUMMARY,
  execute_restore: (args): RestoreExecutionReport => {
    const runId = typeof args.runId === 'string' ? args.runId : REPORT.run_id;
    return { ...REPORT, run_id: runId };
  },
  create_pairing_invitation: () => ({
    code: 'WC-1234-5678-9ABC',
    qr_data: 'workspace-clone://pair?code=WC-1234-5678-9ABC',
    expires_at: '2026-09-30T09:17:00.000Z',
    inviting_device_id: 'mac-1',
    inviting_device_name: SOURCE_DEVICE_NAME,
    inviting_device_public_key: 'ed25519-b64',
  }),
  get_safety_number: () => ({ number: '12345 67890', note: 'compare on both screens' }),
  verify_pairing: () => PAIRED,
  list_settings: () => [['project_roots', '["/Users/abhishek/Documents"]']],
  get_setting: (args) => {
    if (args.key === 'project_roots') return '["/Users/abhishek/Documents"]';
    return null;
  },
};

function invoke(cmd: string, args?: unknown, _options?: unknown): Promise<unknown> {
  const received = (args ?? {}) as Record<string, unknown>;
  const handler = handlers[cmd];
  if (!handler) {
    // A screen in the current UI should never need a command this file lacks.
    console.warn(`[mockTauri] no handler for '${cmd}'`);
    return Promise.reject(new Error(`mock backend: unknown command '${cmd}'`));
  }
  return Promise.resolve().then(() => handler(received));
}

// ---------------------------------------------------------------------------
// Registration + story seeding
// ---------------------------------------------------------------------------

function installMock(): void {
  if (!import.meta.env.DEV) return;
  // Tauri webviews inject this before any bundle code runs — never override it.
  const holder = window as unknown as { __TAURI_INTERNALS__?: unknown; isTauri?: boolean };
  if (typeof holder.__TAURI_INTERNALS__ !== 'undefined') return;

  const params = new URLSearchParams(window.location.search);
  const theme = params.get('theme');
  if (theme === 'light' || theme === 'dark') {
    try {
      window.localStorage.setItem('wc.theme', theme);
    } catch {
      // Not being able to remember the appearance is fine for a screenshot.
    }
  }

  let callbackId = 0;
  const callbacks = new Map<number, (value: unknown) => void>();

  const internals: TauriInternals = {
    invoke,
    transformCallback: (callback: (value: unknown) => void, once?: boolean) => {
      const id = callbackId;
      callbackId += 1;
      callbacks.set(id, (value: unknown) => {
        if (once) callbacks.delete(id);
        callback(value);
      });
      return id;
    },
    unregisterCallback: (id: number) => {
      callbacks.delete(id);
    },
    convertFileSrc: (filePath: string) => filePath,
    metadata: {
      currentWindow: { label: 'main' },
      currentMonitor: null,
      app: { name: 'Workspace Clone' },
    },
  };

  holder.__TAURI_INTERNALS__ = internals;
  holder.isTauri = true;

  seedStory(params.get('story'));
}

function seedStory(story: string | null): void {
  const { setCapture, setRestorePlan, setRestoreReport } = useAppStore.getState();
  const CAPTURE_WARNINGS = [
    'The .env file was not read — secret values are never captured.',
  ];
  if (story === 'transfer') {
    setCapture({ workspaceId: 'ws-1', manifest: MANIFEST, warnings: CAPTURE_WARNINGS });
  }
  if (story === 'report') {
    setRestorePlan({ runId: 'restore-ws-1-001', plan: PLAN, summary: PLAN_SUMMARY });
    setRestoreReport(REPORT);
  }
}

installMock();