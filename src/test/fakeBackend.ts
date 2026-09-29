/**
 * A fake backend for frontend tests.
 *
 * It holds a handler per command name, exactly as the real invoke handler does,
 * so a test drives the same argument shape the app does. Handlers are declared
 * per test rather than shipped as canned fixtures, because a fixture that does
 * not match a command's real signature will typecheck in the test and fail at
 * runtime in the app.
 *
 * `calls` records everything, which is what makes assertions like "the capture
 * sent the adapter list sorted and deduped" testable at all — the sorting
 * decision lives in Rust, so the test can only observe what went over the wire.
 */

import type {
  DiscoveredDevice,
  IncomingTransfer,
  PairedDevice,
  PreflightReport,
  TransferHistoryEntry,
  WorkspaceManifest,
} from '../types';

export interface Call {
  cmd: string;
  args: Record<string, unknown>;
}

export interface Backend {
  calls: Call[];
  handlers: Record<string, (args: Record<string, unknown>) => unknown>;
  /** Names of commands invoked, in order. */
  commandNames(): string[];
  /** Arguments of the most recent call to `cmd`. Throws if it was never called. */
  lastArgs(cmd: string): Record<string, unknown>;
  callsTo(cmd: string): Call[];
  reset(): void;
}

export const backend: Backend = {
  calls: [],
  handlers: {},
  commandNames() {
    return this.calls.map((c) => c.cmd);
  },
  lastArgs(cmd: string) {
    for (let i = this.calls.length - 1; i >= 0; i -= 1) {
      if (this.calls[i].cmd === cmd) {
        return this.calls[i].args;
      }
    }
    throw new Error(`'${cmd}' was never invoked. Invoked: ${this.commandNames().join(', ') || '(nothing)'}`);
  },
  callsTo(cmd: string) {
    return this.calls.filter((c) => c.cmd === cmd);
  },
  reset() {
    this.calls = [];
    this.handlers = {};
  },
};

/** Register handlers for a test. Overwrites anything from a previous test. */
export function mockBackend(handlers: Backend['handlers']) {
  backend.handlers = { ...handlers };
}

// ---------------------------------------------------------------------------
// Builders
//
// Every builder fills in every field its Rust counterpart serialises. A partial
// fixture would let a test pass against a shape the backend never produces, and
// the resulting `undefined` would only surface on a real screen.
// ---------------------------------------------------------------------------

export function makeManifest(overrides: Partial<WorkspaceManifest> = {}): WorkspaceManifest {
  return {
    schema_version: 1,
    workspace: {
      id: 'ws-1',
      name: 'Demo',
      captured_at: '2026-01-01T00:00:00Z',
      source_device: { id: 'dev-1', os: 'macos', os_version: '15.0' },
      portability: 'cross_platform',
    },
    projects: [
      {
        id: 'p1',
        name: 'demo',
        source_path_hint: '~/code/demo',
        destination_location_id: 'code',
        git: {
          remote_hint: 'https://github.com/acme/demo',
          branch: 'main',
          commit: 'abc1234',
          dirty_worktree: false,
          dirty_state_captured: false,
        },
      },
    ],
    applications: [
      { id: 'a1', adapter: 'vscode', project_id: 'p1', required: false, extensions: ['rust-analyzer'] },
    ],
    requirements: {
      applications: [{ id: 'a1', adapter: 'vscode', required: false }],
      runtimes: [],
      cli_tools: [],
      identities: [],
      environment: { presence_only: [], values_included: false },
      services: [],
    },
    restore: { steps: [], notes: [] },
    policy: {
      file_transfer: 'none',
      clipboard: 'excluded',
      automatic_command_execution: false,
      secret_values_included: false,
    },
    ...overrides,
  };
}

export function makePairedDevice(overrides: Partial<PairedDevice> = {}): PairedDevice {
  return {
    id: 'peer-1',
    name: "Alex's MacBook",
    public_key: 'noise-public-key-b64',
    fingerprint: 'AbCdEfGhIjKlMnOp',
    // The serde spelling, matching what `list_paired_devices` really returns.
    // This fixture said `'receive'` while the type said `TrustScope` and nothing
    // complained until the type was corrected -- which is exactly the drift that
    // disabled every transfer destination.
    trust_scopes: ['receive-workspaces'],
    os: 'macos',
    os_version: '15.0',
    app_version: '0.1.0',
    created_at: '2026-01-01T00:00:00Z',
    last_seen: null,
    revoked: false,
    revoked_at: null,
    ...overrides,
  };
}

export function makeDiscoveredDevice(
  overrides: Partial<DiscoveredDevice> = {}
): DiscoveredDevice {
  return {
    device_id: 'peer-1',
    name: "Alex's MacBook",
    os: 'macos',
    app_version: '0.1.0',
    protocol_version: 1,
    addresses: ['192.168.1.20'],
    port: 47890,
    capabilities: {},
    static_public_key: 'noise-public-key-b64',
    last_seen: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

export function makePreflightReport(overrides: Partial<PreflightReport> = {}): PreflightReport {  return {
    checks: [
      {
        requirement_id: 'rt-node',
        status: 'ready_verified',
        evidence: 'node 22.1.0',
        freshness: '2026-01-01T00:00:00Z',
        action: null,
        adapter_id: 'runtime',
        required: true,
        user_confirmed: false,
      },
      {
        requirement_id: 'id-gh',
        status: 'login_required',
        evidence: 'gh is installed but not signed in',
        freshness: '2026-01-01T00:00:00Z',
        action: {
          id: 'fix',
          action_type: 'run_command',
          description: 'Sign in to GitHub',
          command: 'gh auth login',
          url: null,
          requires_consent: true,
        },
        adapter_id: 'identity',
        required: true,
        user_confirmed: false,
      },
    ],
    overall_readiness: 50,
    required_total: 2,
    required_satisfied: 1,
    completed_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

/**
 * An arrival that was stored.
 *
 * `refusal_reason` is `null` here, which is the field that matters: a builder
 * that defaulted it to a string would let a test assert a successful arrival had
 * a reason attached, and the UI that renders it would be exercised only on the
 * branch that never happens in the app.
 */
export function makeIncomingTransfer(
  overrides: Partial<IncomingTransfer> = {}
): IncomingTransfer {
  return {
    transferId: 'tr-1',
    workspaceId: 'ws-1',
    workspaceName: 'Demo',
    senderDeviceId: 'peer-1',
    senderDeviceName: "Alex's MacBook",
    sourceDeviceId: 'peer-1',
    accepted: true,
    refusalReason: null,
    transferDigest: 'b'.repeat(64),
    bytesReceived: 2048,
    receivedAt: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

export function makeTransferHistoryEntry(
  overrides: Partial<TransferHistoryEntry> = {}
): TransferHistoryEntry {
  return {
    id: 'tr-1',
    workspaceId: 'ws-1',
    sourceDeviceId: 'peer-1',
    sourceDeviceName: "Alex's MacBook",
    destinationDeviceId: 'FINGERPRINT-1',
    destinationDeviceName: 'This device',
    status: 'completed',
    progress: 1,
    startedAt: '2026-01-01T00:00:00Z',
    completedAt: '2026-01-01T00:00:01Z',
    error: null,
    ...overrides,
  };
}
