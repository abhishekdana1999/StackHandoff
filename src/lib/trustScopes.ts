/**
 * The two vocabularies for a trust scope, in one place.
 *
 * ## Why this file exists
 *
 * A trust scope is written one way and read another, and confusing them breaks
 * the app in a way that looks like something else entirely.
 *
 * - **Requested** (`'receive' | 'send' | 'files' | 'clipboard'`) is what the
 *   pairing dialog sends. `parse_trust_scopes` in `commands/src/transfer.rs`
 *   accepts exactly these four strings.
 * - **Stored** (`'receive-workspaces' | 'send-workspaces' | 'file-transfer' |
 *   'clipboard-transfer'`) is what a `PairedDevice` reports. The `TrustScope`
 *   enum carries `#[serde(rename_all = "kebab-case")]`, so the wire value is the
 *   kebab-case variant name and nothing else.
 *
 * The two are genuinely different strings. They are not cosmetic variants of one
 * another, and the backend does not translate between them — `parse_trust_scopes`
 * maps the short form to the enum, and the enum then serialises to the long one.
 *
 * ## What went wrong when this was in two files
 *
 * `TransferScreen` tested `trust_scopes.includes('receive')` against a stored
 * scope. That comparison is false for every device, always, so **every
 * destination in the send screen was permanently disabled**, showing "Not
 * allowed to receive workspaces" on a database row that contained
 * `receive-workspaces`. `DevicesScreen` had the same list typed as the short
 * form, so its display lookup could not match either and printed the raw
 * `receive-workspaces` where a person should read "Receive".
 *
 * `tsc` was clean throughout, because the TypeScript agreed with itself. The
 * types described a wire format that did not exist, and the test fixture
 * repeated the same wrong spelling, so any test written against it would have
 * agreed with the bug. That is the failure this module is here to prevent: keep
 * both spellings adjacent, and derive every use from this table.
 */

/** A scope as the pairing request spells it. Never compare to a stored scope. */
export type RequestedTrustScope = 'receive' | 'send' | 'files' | 'clipboard';

/** A scope as a paired device reports it. This is what arrives from the backend. */
export type TrustScope =
  | 'receive-workspaces'
  | 'send-workspaces'
  | 'file-transfer'
  | 'clipboard-transfer';

/**
 * One row per scope, carrying both spellings and the words a person reads.
 *
 * Every scope list, checkbox and label in the app is derived from this, so a
 * scope cannot be added to one direction and forgotten in the other — which is
 * how the two drifted apart in the first place.
 */
export const TRUST_SCOPES: ReadonlyArray<{
  /** Sent to the backend when pairing. */
  requested: RequestedTrustScope;
  /** Reported by the backend on a paired device. */
  stored: TrustScope;
  label: string;
  detail: string;
}> = [
  {
    requested: 'receive',
    stored: 'receive-workspaces',
    label: 'Receive',
    detail: 'May be sent workspaces',
  },
  {
    requested: 'send',
    stored: 'send-workspaces',
    label: 'Send',
    detail: 'May send workspaces to this device',
  },
  {
    requested: 'files',
    stored: 'file-transfer',
    label: 'Files',
    detail: 'May transfer files',
  },
  {
    requested: 'clipboard',
    stored: 'clipboard-transfer',
    label: 'Clipboard',
    detail: 'May transfer clipboard contents',
  },
];

/** Whether a paired device may be sent workspaces. Checks the *stored* spelling. */
export function canReceive(stored: readonly TrustScope[]): boolean {
  return stored.includes('receive-workspaces');
}

/**
 * A stored scope as a person should read it.
 *
 * Falls back to the raw string rather than to nothing: an unrecognised scope is
 * still true information about what the device holds, and hiding it would make
 * the list look shorter than the device's actual trust.
 */
export function scopeLabel(stored: TrustScope): string {
  return TRUST_SCOPES.find((s) => s.stored === stored)?.label ?? stored;
}
