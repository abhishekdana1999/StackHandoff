/**
 * The only place in the frontend that names a Tauri command.
 *
 * Every call goes through a named function with a declared return type, so a
 * signature change in Rust surfaces as a TypeScript error at this line rather
 * than as `undefined` rendered into a screen three components away.
 *
 * Two conventions are load-bearing:
 *
 * * **Arguments are camelCase from JavaScript and converted here.** Tauri
 *   converts an argument object to snake_case automatically, but only for the
 *   parameter names, not for the JSON body strings we pass. Anything sent as a
 *   JSON string is serialised by hand below, and `captureSelection` exists
 *   because the Rust `CaptureSelection` field names are camelCase while the
 *   manifest's are snake_case.
 * * **`*_json` parameters are strings by design.** The Rust commands take
 *   `manifest_json: String` rather than a typed value, so the caller controls
 *   exactly what is serialised. `JSON.stringify` is therefore explicit and
 *   visible at each call site rather than hidden.
 */

import { invoke } from '@tauri-apps/api/core';
import type {
  CaptureResult,
  CaptureSelection,
  DiscoveredDevice,
  DeviceIdentity,
  IncomingTransfer,
  PairedDevice,
  PairingInvitation,
  PlanSummary,
  PreflightReport,
  CheckResult,
  ProjectScan,
  RestoreExecutionReport,
  RestorePlan,
  RestoreRunRecord,
  SafetyNumber,
  SendOutcome,
  SnapshotRecord,
  TransferHistoryEntry,
  WorkspaceManifest,
  WorkspaceRecord,
} from '../types';

/**
 * Turn whatever the backend rejected with into something worth showing.
 *
 * A Rust `WorkspaceError` reaches the frontend as a string, because that is what
 * `thiserror` produces and Tauri serialises. So the "structured" error the
 * frontend would like does not exist, and pretending otherwise would mean
 * matching on prose. The honest fallback is the message itself, which the
 * backend is written to make user-facing.
 */
export function errorMessage(error: unknown): string {
  if (typeof error === 'string') {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

export const getAppVersion = (): Promise<string> => invoke('get_app_version');

export const getPlatform = (): Promise<string> => invoke('get_platform');

export const getDeviceKeyExists = (): Promise<boolean> => invoke('get_device_key_exists');

export const generateDeviceKey = (): Promise<string> => invoke('generate_device_key');

export const getDeviceFingerprint = (): Promise<string | null> =>
  invoke('get_device_fingerprint');

export const getDeviceIdentity = (): Promise<DeviceIdentity> => invoke('get_device_identity');

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

export const listProjectRoots = (): Promise<ProjectScan> => invoke('list_project_roots');

export const setProjectRoots = (roots: string[]): Promise<string[]> =>
  invoke('set_project_roots', { roots });

// ---------------------------------------------------------------------------
// Workspaces
// ---------------------------------------------------------------------------

export const listWorkspaces = (limit?: number, offset?: number): Promise<WorkspaceRecord[]> =>
  invoke('list_workspaces', { limit, offset });

export const getWorkspace = (workspaceId: string): Promise<WorkspaceRecord | null> =>
  invoke('get_workspace', { workspaceId });

export const getWorkspaceSnapshots = (workspaceId: string): Promise<SnapshotRecord[]> =>
  invoke('get_workspace_snapshots', { workspaceId });

export const getWorkspaceRestoreRuns = (workspaceId: string): Promise<RestoreRunRecord[]> =>
  invoke('get_workspace_restore_runs', { workspaceId });

export const deleteWorkspace = (workspaceId: string): Promise<void> =>
  invoke('delete_workspace', { workspaceId });

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

export const captureWorkspace = (
  name: string,
  selection: CaptureSelection
): Promise<CaptureResult> => invoke('capture_workspace', { name, selection });

export const getManifest = (workspaceId: string): Promise<WorkspaceManifest> =>
  invoke('get_manifest', { workspaceId });

export const validateManifest = (manifest: WorkspaceManifest): Promise<boolean> =>
  invoke('validate_manifest', { manifestJson: JSON.stringify(manifest) });

export const scrubManifestSecrets = (manifest: WorkspaceManifest): Promise<string> =>
  invoke('scrub_manifest_secrets', { manifestJson: JSON.stringify(manifest) });

// ---------------------------------------------------------------------------
// Transfer and pairing
// ---------------------------------------------------------------------------

export const startDiscovery = (): Promise<DiscoveredDevice[]> => invoke('start_discovery');

export const getDiscoveredDevices = (): Promise<DiscoveredDevice[]> =>
  invoke('get_discovered_devices');

export const probeDevice = (address: string, port: number): Promise<DiscoveredDevice> =>
  invoke('probe_device', { address, port });

export const createPairingInvitation = (deviceName: string): Promise<PairingInvitation> =>
  invoke('create_pairing_invitation', { deviceName });

export const getSafetyNumber = (remoteNoiseKeyB64: string): Promise<SafetyNumber> =>
  invoke('get_safety_number', { remoteNoiseKeyB64 });

/**
 * Complete pairing. The confirmed safety number is required, not optional: a
 * pairing stored without one is a device this app will send a workspace to on
 * the strength of an advertisement alone, which is the attack the safety number
 * exists to prevent.
 */
export const verifyPairing = (args: {
  remoteNoiseKeyB64: string;
  deviceName: string;
  expectedSafetyNumber: string;
  trustScopes: string[];
}): Promise<PairedDevice> => invoke('verify_pairing', args);

export const sendWorkspace = (workspaceId: string, destinationDeviceId: string): Promise<SendOutcome> =>
  invoke('send_workspace', { workspaceId, destinationDeviceId });

// ---------------------------------------------------------------------------
// Receive
//
// The accept loop these report on is started by the backend at startup, not by
// anything the window does. That is deliberate: receiving has to work while the
// window is closed, otherwise a workspace sent by a peer while this machine was
// shut down would sit unauthenticated in a socket until someone opened the app,
// which is the same failure as having no receive path at all.
// ---------------------------------------------------------------------------

/**
 * Arrivals this device has not dismissed yet, newest first.
 *
 * A dismissal only clears the notification. The workspace is stored either way,
 * so a user who dismisses a banner cannot lose a workspace that already landed.
 */
export const getIncomingTransfers = (): Promise<IncomingTransfer[]> =>
  invoke('get_incoming_transfers');

export const dismissIncomingTransfer = (transferId: string): Promise<void> =>
  invoke('dismiss_incoming_transfer', { transferId });

/**
 * Transfers this device took part in, newest first.
 *
 * Omit `workspaceId` for every transfer. The table is written by both ends, so
 * this is the one place that can answer whether a workspace reached a machine
 * after the sending window stopped showing the result.
 */
export const getTransferHistory = (workspaceId?: string): Promise<TransferHistoryEntry[]> =>
  invoke('get_transfer_history', { workspaceId });

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

export const runPreflight = (args: {
  requirements: unknown;
  envFiles?: string[];
  confirmedRequirements?: string[];
}): Promise<PreflightReport> =>
  invoke('run_preflight', {
    requirementsJson: JSON.stringify(args.requirements),
    envFiles: args.envFiles ?? [],
    confirmedRequirements: args.confirmedRequirements ?? [],
  });

export const rerunPreflightCheck = (
  requirement: unknown,
  confirmed?: boolean
): Promise<CheckResult> =>
  invoke('rerun_preflight_check', {
    requirementJson: JSON.stringify(requirement),
    confirmed,
  });

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

export const generateRestorePlan = (
  manifest: WorkspaceManifest,
  destinationRoots?: Record<string, string>
): Promise<RestorePlan> =>
  invoke('generate_restore_plan', {
    manifestJson: JSON.stringify(manifest),
    destinationRootsJson:
      destinationRoots && Object.keys(destinationRoots).length > 0
        ? JSON.stringify(destinationRoots)
        : undefined,
  });

export const executeRestore = (args: {
  runId: string;
  plan: RestorePlan;
  approvals?: Record<string, boolean>;
}): Promise<RestoreExecutionReport> =>
  invoke('execute_restore', {
    runId: args.runId,
    planJson: JSON.stringify(args.plan),
    approvalsJson: JSON.stringify(args.approvals ?? {}),
  });

export const summarizeRestorePlan = (plan: RestorePlan): Promise<PlanSummary> =>
  invoke('summarize_restore_plan', { planJson: JSON.stringify(plan) });

// ---------------------------------------------------------------------------
// Devices
// ---------------------------------------------------------------------------

export const listPairedDevices = (): Promise<PairedDevice[]> => invoke('list_paired_devices');

export const getPairedDevice = (deviceId: string): Promise<PairedDevice | null> =>
  invoke('get_paired_device', { deviceId });

export const addPairedDevice = (args: {
  name: string;
  publicKey: string;
  noisePublicKey: string;
  os: string;
  osVersion: string;
  appVersion: string;
  trustScopes: string[];
}): Promise<PairedDevice> => invoke('add_paired_device', args);

export const updatePairedDevice = (deviceId: string, name?: string): Promise<void> =>
  invoke('update_paired_device', { deviceId, name });

export const revokePairedDevice = (deviceId: string): Promise<void> =>
  invoke('revoke_paired_device', { deviceId });

export const deletePairedDevice = (deviceId: string): Promise<void> =>
  invoke('delete_paired_device', { deviceId });

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

export const getSetting = (key: string): Promise<string | null> => invoke('get_setting', { key });

export const setSetting = (key: string, value: string): Promise<void> =>
  invoke('set_setting', { key, value });

export const deleteSetting = (key: string): Promise<void> => invoke('delete_setting', { key });

export const listSettings = (): Promise<[string, string][]> => invoke('list_settings');

export const exportSettings = (): Promise<Record<string, string>> => invoke('export_settings');

export const importSettings = (settings: Record<string, string>): Promise<void> =>
  invoke('import_settings', { settingsJson: JSON.stringify(Object.entries(settings)) });
