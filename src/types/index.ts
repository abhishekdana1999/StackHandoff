/**
 * TypeScript mirrors of what the Rust commands actually serialise.
 *
 * Two rules govern this file, and both exist because getting them wrong produces
 * a screen that renders blanks rather than an error:
 *
 * 1. **Field names are copied, not invented.** Every casing here matches a
 *    `#[serde(rename_all = ...)]` on the Rust side. They are not uniform: the
 *    command-result structs (`CaptureResult`, `SendOutcome`, `ProjectScan`,
 *    `SafetyNumber`, `DeviceIdentityInfo`) are camelCase, while the manifest, the
 *    database records and `DiscoveredDevice`/`PairedDevice` are snake_case.
 *    A single `camelCase` assumption across the file would silently break half
 *    of it, because `undefined` renders as an empty string rather than throwing.
 *
 * 2. **Enums are their serialised names.** A Rust `#[serde(rename_all = "snake_case")]`
 *    enum arrives as `ready_verified`, not `ReadyVerified`, and an unhandled
 *    variant must be visible. `CheckStatus` and friends are therefore unioned
 *    with `string` at the edges where a newer backend build may add a variant,
 *    and narrowed by the `is*` helpers at the bottom of this file.
 */

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/**
 * The outcome of one preflight check.
 *
 * `ready_user_confirmed` is deliberately distinct from `ready_verified`: the
 * blueprint requires user-asserted readiness to be *labelled* as such, so the
 * UI can say "you told us" rather than implying a machine checked it.
 */
export type CheckStatus =
  | 'ready_verified'
  | 'ready_user_confirmed'
  | 'ready_account_mismatch'
  | 'login_required'
  | 'configured_unverified'
  | 'unknown'
  | 'not_applicable';

export type ActionStatus = 'success' | 'skipped' | 'failed' | 'manual';

export type TransferStatus =
  | 'connecting'
  | 'handshaking'
  | 'transferring'
  | 'verifying'
  | 'completed'
  | 'failed'
  | 'cancelled';

export type RestoreActionType =
  | 'open_project'
  | 'open_application'
  | 'open_urls'
  | 'offer_command'
  | 'check_git'
  | 'map_path';

export type ActionType = 'open_url' | 'run_command' | 'install_app' | 'configure_setting';

export type TrustScope = 'receive' | 'send' | 'files' | 'clipboard';

export type Portability = 'cross_platform' | 'windows_only' | 'macos_only' | 'linux_only';

export type FileTransferPolicy = 'none' | 'explicit' | 'all';

export type ClipboardPolicy = 'excluded' | 'opt_in' | 'included';

/**
 * The lifecycle of a workspace record, as stored.
 *
 * A plain string rather than a union: the backend stores this as free text and
 * the set of values is owned by `workspace_clone_db`, not by this file. A new
 * status from a newer build must render as "unknown", not crash the list.
 */
export type WorkspaceStatus = string;

// ---------------------------------------------------------------------------
// App / device
// ---------------------------------------------------------------------------

/** `app::get_device_identity` — camelCase. */
export interface DeviceIdentity {
  signingPublicKeyB64: string;
  noisePublicKeyB64: string;
  fingerprint: string;
}

/** `device::list_paired_devices` — snake_case. */
export interface PairedDevice {
  id: string;
  name: string;
  /**
   * The connection (X25519) key, base64.
   *
   * Named `public_key` by the backend for historical reasons, but for a paired
   * peer this build stores the Noise key, because that is the key a handshake
   * actually authenticates. Do not treat it as an Ed25519 identity key.
   */
  public_key: string;
  /**
   * A fingerprint over the connection key, not the Ed25519 identity key.
   *
   * The backend will only ever verify a Noise key, so displaying this as though
   * it were an identity fingerprint would assert a binding that was never
   * established.
   */
  fingerprint: string;
  trust_scopes: TrustScope[];
  os: string;
  os_version: string;
  app_version: string;
  created_at: string;
  last_seen: string | null;
  revoked: boolean;
  revoked_at: string | null;
}

/** `core::device::DiscoveredDevice` — snake_case. */
export interface DiscoveredDevice {
  device_id: string;
  name: string;
  os: string;
  app_version: string;
  protocol_version: number;
  addresses: string[];
  port: number;
  capabilities: Record<string, unknown>;
  /**
   * The peer's Noise X25519 static public key, base64.
   *
   * Empty means the peer advertised no key and **cannot be connected to**. The
   * UI must say so rather than offering a Send button that will fail.
   */
  static_public_key: string;
  last_seen: string;
}

/** `transfer::SafetyNumber` — camelCase. */
export interface SafetyNumber {
  number: string;
  note: string;
}

/**
 * `core::device::PairingInvitation` — snake_case.
 *
 * Note what is *not* here: a safety number. It is derived from both Noise keys,
 * so the inviting device cannot compute one yet — it does not know the other
 * side's key. An invitation that appeared to carry a safety number would either
 * be meaningless or would have to be computed from one key, which is exactly the
 * thing a safety number must not be.
 */
export interface PairingInvitation {
  code: string;
  /** A `workspace-clone://pair?...` URI, carrying both public keys. */
  qr_data: string;
  expires_at: string;
  inviting_device_id: string;
  inviting_device_name: string;
  /** The inviter's Ed25519 key. Advertised, and never verified by this build. */
  inviting_device_public_key: string;
}

/** `transfer::SendOutcome` — camelCase, wrapping a snake_case `TransferSession`. */
export interface SendOutcome {
  succeeded: boolean;
  transfer: TransferSession;
  bytesSent: number;
}

// ---------------------------------------------------------------------------
// Transfer
// ---------------------------------------------------------------------------

/** `network::transfer::TransferSession` — snake_case. */
export interface TransferSession {
  id: string;
  workspace_id: string;
  source_device_id: string;
  destination_device_id: string;
  status: TransferStatus;
  /** 0.0 to 1.0. */
  progress: number;
  bytes_transferred: number;
  total_bytes: number;
  started_at: string;
  completed_at: string | null;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

/** `adapters::scan::GitSummary`. */
export interface GitSummary {
  branch: string;
  commit: string | null;
  dirty: boolean;
  remote_hint: string | null;
}

/** `commands::projects::ProjectCandidate` — camelCase. */
export interface ProjectCandidate {
  id: string;
  name: string;
  /** Absolute path, on this machine only. Never sent in a manifest. */
  path: string;
  isGitRepo: boolean;
  git: {
    branch: string;
    commit: string | null;
    dirty: boolean;
    /** Scrubbed of credentials, identical to what the manifest will carry. */
    remoteHint: string | null;
  } | null;
}

/** `commands::projects::ProjectScan` — camelCase. */
export interface ProjectScan {
  projects: ProjectCandidate[];
  roots: string[];
  /**
   * Configured roots that do not exist. Reported separately from an empty
   * `projects` list so "no repositories" is distinguishable from "we looked
   * somewhere that is not there".
   */
  missingRoots: string[];
  /** Roots that exist but could not be listed — a permissions problem. */
  unreadableRoots: string[];
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

/** `adapters::SelectedProject`. */
export interface SelectedProject {
  id: string;
  name: string;
  /** Absolute path on this machine. Redacted before it enters a manifest. */
  sourcePath: string;
  /** Logical bucket the destination maps this into, e.g. `code`. */
  destinationLocationId: string;
}

/** `adapters::ApprovedCommand`. */
export interface ApprovedCommand {
  label: string;
  command: string;
  workingDirectory: string | null;
}

export interface CapturePolicy {
  fileTransfer: FileTransferPolicy;
  clipboard: ClipboardPolicy;
  automaticCommandExecution: boolean;
  secretValuesIncluded: boolean;
}

/**
 * `adapters::CaptureSelection`.
 *
 * The lists are the single authority. There are deliberately no parallel
 * boolean flags on this type: a flag and an empty list can disagree, and the
 * adapters would have to guess which one wins.
 */
export interface CaptureSelection {
  projects: SelectedProject[];
  /** Adapter ids whose captured context to include: `vscode`, `terminal`, … */
  includeApplications: string[];
  /**
   * URLs the user typed in. Browser history is never read — there is no profile
   * scraping, by decision.
   */
  browserUrls: string[];
  terminalDirs: string[];
  terminalCommands: ApprovedCommand[];
  /** Names only. Values are never captured, whatever the caller asks for. */
  envVarNames: string[];
  policy: CapturePolicy;
}

/** `commands::capture::CaptureResult` — camelCase. */
export interface CaptureResult {
  /** The manifest in the clear, for the confirmation screen. */
  manifest: WorkspaceManifest;
  /** The sealed manifest, the form written to disk and sent. */
  sealed: string;
  /** Anything the user asked for that could not be captured. */
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Manifest (snake_case throughout)
// ---------------------------------------------------------------------------

export interface DeviceRef {
  id: string;
  os: string;
  os_version: string;
}

export interface WorkspaceMeta {
  id: string;
  name: string;
  captured_at: string;
  source_device: DeviceRef;
  portability: Portability;
}

export interface GitInfo {
  remote_hint: string | null;
  branch: string;
  commit: string | null;
  dirty_worktree: boolean;
  /**
   * Always false in practice. It exists so the manifest can state that the
   * uncommitted work itself was not captured, rather than leaving the reader to
   * assume a dirty worktree came along.
   */
  dirty_state_captured: boolean;
}

export interface Project {
  id: string;
  name: string;
  /**
   * A redacted hint such as `~/code/demo`. Never an absolute path: a manifest
   * must not be able to direct a write, and an absolute source path is the first
   * step to that.
   */
  source_path_hint: string;
  destination_location_id: string;
  git: GitInfo | null;
}

export interface Application {
  id: string;
  adapter: string;
  project_id: string | null;
  required: boolean;
  /**
   * Adapter-specific fields, flattened into this object by `#[serde(flatten)]`.
   *
   * Typed as a bag because the shape is the adapter's, not the manifest's.
   */
  [key: string]: unknown;
}

export interface AppRequirement {
  id: string;
  adapter: string;
  required: boolean;
}

export interface RuntimeRequirement {
  id: string;
  name: string;
  version: string | null;
  source: string;
  required: boolean;
  check_command: string | null;
  [key: string]: unknown;
}

export interface CliToolRequirement {
  id: string;
  name: string;
  required: boolean;
  [key: string]: unknown;
}

export interface IdentityRequirement {
  id: string;
  service: string;
  required: boolean;
  check_command: string | null;
  /** True when a check needs the user's explicit consent before it runs. */
  needs_consent: boolean;
  expected_account: string | null;
  [key: string]: unknown;
}

export interface EnvironmentRequirement {
  presence_only: string[];
  /** Always false. A manifest that claimed otherwise is rejected. */
  values_included: boolean;
}

export interface ServiceRequirement {
  id: string;
  name: string;
  required: boolean;
  [key: string]: unknown;
}

export interface Requirements {
  applications: AppRequirement[];
  runtimes: RuntimeRequirement[];
  cli_tools: CliToolRequirement[];
  identities: IdentityRequirement[];
  environment: EnvironmentRequirement;
  services: ServiceRequirement[];
}

export interface ManifestPolicy {
  file_transfer: FileTransferPolicy;
  clipboard: ClipboardPolicy;
  /**
   * Always false. The capture layer forces it, and `validate` rejects a manifest
   * that claims otherwise, so a workspace can never arrive pre-armed to run
   * commands on the destination.
   */
  automatic_command_execution: boolean;
  secret_values_included: boolean;
}

export interface RestoreAction {
  id: string;
  action_type: RestoreActionType;
  /** The adapter that routes this step. `''` for a native step. */
  adapter_id: string;
  description: string;
  required: boolean;
  approved: boolean;
  config: Record<string, unknown>;
  dependencies: string[];
}

export interface ManifestRestorePlan {
  steps: RestoreAction[];
  notes: string[];
}

export interface WorkspaceManifest {
  schema_version: number;
  workspace: WorkspaceMeta;
  projects: Project[];
  applications: Application[];
  requirements: Requirements;
  restore: ManifestRestorePlan;
  policy: ManifestPolicy;
}

// ---------------------------------------------------------------------------
// Preflight
// ---------------------------------------------------------------------------

/** `adapters::RemediationAction`. */
export interface RemediationAction {
  id: string;
  action_type: ActionType;
  description: string;
  command: string | null;
  url: string | null;
  requires_consent: boolean;
  [key: string]: unknown;
}

/** `adapters::CheckResult`, flattened into `PreflightCheck` by the backend. */
export interface CheckResult {
  requirement_id: string;
  status: CheckStatus;
  /** What the check actually found, in plain words. */
  evidence: string;
  freshness: string;
  action: RemediationAction | null;
}

/** `preflight::engine::PreflightCheck` — `check` is `#[serde(flatten)]`. */
export type PreflightCheck = CheckResult & {
  adapter_id: string;
  required: boolean;
  user_confirmed: boolean;
};

export interface PreflightReport {
  checks: PreflightCheck[];
  /** Percentage of *required* checks that are satisfied, 0-100. */
  overall_readiness: number;
  required_total: number;
  required_satisfied: number;
  completed_at: string;
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

/** `restore::planner::RestorePlan`. */
export interface RestorePlan {
  workspace_id: string;
  steps: RestoreAction[];
  /**
   * What the planner could not express, or named an adapter this build does not
   * have. Shown to the user rather than dropped.
   */
  notes: string[];
}

export interface ActionResult {
  action_id: string;
  status: ActionStatus;
  message: string;
  duration_ms: number;
  details: string | null;
}

export interface RestoreExecutionReport {
  run_id: string;
  workspace_id: string;
  results: ActionResult[];
  notes: string[];
  completed_at: string;
}

export interface PlanSummary {
  total_steps: number;
  already_approved: number;
  need_consent: number;
  opens_something: number;
  notes: string[];
}

// ---------------------------------------------------------------------------
// Database records (snake_case)
// ---------------------------------------------------------------------------

export interface WorkspaceRecord {
  id: string;
  name: string;
  schema_version: number;
  captured_at: string;
  source_device_id: string;
  /** SHA-256 of the sealed manifest, to prove two devices hold the same one. */
  manifest_digest: string;
  encrypted_manifest_path: string;
  status: WorkspaceStatus;
}

export interface SnapshotRecord {
  id: string;
  workspace_id: string;
  captured_at: string;
  source_device_id: string;
  size_bytes: number;
  transfer_status: string;
  transfer_id: string | null;
}

export interface RestoreRunRecord {
  id: string;
  workspace_id: string;
  started_at: string;
  completed_at: string | null;
  status: string;
  summary: string | null;
}

// ---------------------------------------------------------------------------
// Receive
// ---------------------------------------------------------------------------

/**
 * Something a peer tried to send, and what this device did with it.
 *
 * Both outcomes are carried, not just the successful one. A machine that silently
 * ignores an unauthorised push looks identical to a broken one, and the first
 * question when a workspace does not appear is whether it arrived -- so a refusal
 * comes back with the reason it was refused.
 */
export interface IncomingTransfer {
  /** The network layer's id for this transfer. Matches a `transfer_sessions` row. */
  transfer_id: string;
  /** Empty when the payload was refused before it could be parsed. */
  workspace_id: string;
  /**
   * For display.
   *
   * Falls back to the claimed source device when a manifest was refused before
   * its name could be trusted -- a name from an unverified payload is not shown
   * as though it were this device's own reading of it.
   */
  workspace_name: string;
  /**
   * The sending device's id, derived from the Noise key the handshake
   * authenticated.
   *
   * Never from a frame header: that is an unauthenticated string, and anything on
   * the network could put a paired device's id in one.
   */
  sender_device_id: string;
  /** This device's name for the sender, or the id if it has never been paired. */
  sender_device_name: string;
  /** Where the sender claims the workspace was captured. Usually the sender. */
  source_device_id: string;
  /** Whether the manifest is now stored on this device. */
  accepted: boolean;
  /**
   * Why it was refused, or `null` when it was not.
   *
   * Written for a person: it says what to do next, not what gate failed.
   */
  refusal_reason: string | null;
  /**
   * Digest of the payload as it arrived.
   *
   * A different value from `WorkspaceRecord.manifest_digest`, which covers the
   * locally sealed copy. Both are reported because the first proves what
   * travelled and the second what is stored.
   */
  transfer_digest: string;
  bytes_received: number;
  received_at: string;
}

/**
 * One row of `transfer_sessions`, joined with device names.
 *
 * Written by *both* ends of every transfer, so this answers "did that actually
 * get there?" for a transfer whose result the sending window no longer shows --
 * and it is the only record of a transfer that arrived, which is what makes
 * "the workspace is not in the list" answerable.
 */
export interface TransferHistoryEntry {
  id: string;
  workspace_id: string;
  source_device_id: string;
  /** Falls back to the id when this device has no row for it. */
  source_device_name: string;
  destination_device_id: string;
  destination_device_name: string;
  status: TransferStatus | string;
  /** 0.0 to 1.0. `1.0` on completion. */
  progress: number;
  started_at: string;
  completed_at: string | null;
  error: string | null;
}

// ---------------------------------------------------------------------------
// Narrowing helpers
// ---------------------------------------------------------------------------

/**
 * Whether a check means the requirement is satisfied.
 *
 * `not_applicable` and `unknown` deliberately do not count. A check that could
 * not run has established nothing, so treating it as satisfied would let a
 * workspace claim readiness on the strength of a check that never happened.
 */
export function isSatisfied(status: CheckStatus | string): boolean {
  return status === 'ready_verified' || status === 'ready_user_confirmed';
}

/**
 * Whether a result was established by a check on this machine.
 *
 * Kept apart from `isSatisfied` because the UI must not present a
 * user-asserted result as a machine one.
 */
export function isMachineVerified(status: CheckStatus | string): boolean {
  return status === 'ready_verified';
}

/**
 * Whether a check stopped short of a verdict.
 *
 * These are the ones a user needs to act on, and the ones that must never be
 * rendered as a pass.
 */
export function isUnresolved(status: CheckStatus | string): boolean {
  return (
    status === 'unknown' ||
    status === 'login_required' ||
    status === 'ready_account_mismatch' ||
    status === 'configured_unverified'
  );
}

export function isTransferFinished(status: TransferStatus | string): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled';
}

/**
 * A discovered device this build can actually connect to.
 *
 * A peer that advertised no Noise static key cannot complete a handshake, so
 * offering it in a device list produces a Send button that always fails.
 */
export function isConnectable(device: DiscoveredDevice): boolean {
  return device.static_public_key.length > 0;
}
