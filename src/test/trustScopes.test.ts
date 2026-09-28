/**
 * The two trust-scope vocabularies, pinned against the Rust that defines them.
 *
 * A scope is *requested* as `'receive'` and *reported* as `'receive-workspaces'`.
 * The two are different strings because `parse_trust_scopes` maps the short form
 * onto the `TrustScope` enum, and the enum serialises with
 * `#[serde(rename_all = "kebab-case")]`. Nothing translates between them at the
 * boundary.
 *
 * Conflating them disabled every transfer destination in the app: the send
 * screen asked for `'receive'` and so never found it. These tests are the
 * cheapest possible guard against that recurring — if the spellings ever change
 * on the Rust side, the assertion that spells them out fails here rather than in
 * a live send.
 *
 * The spellings below are transcribed from `src-tauri/core/src/device.rs` and
 * `src-tauri/commands/src/transfer.rs`. A change there should change here in the
 * same commit, and this file says so at each constant.
 */
import { describe, expect, it } from 'vitest';
import {
  TRUST_SCOPES,
  canReceive,
  scopeLabel,
  type RequestedTrustScope,
  type TrustScope,
} from '@lib/trustScopes';

/**
 * `#[serde(rename_all = "kebab-case")]` applied to the four `TrustScope`
 * variants, transcribed from `core/src/device.rs`. These are the strings the
 * backend puts on the wire and in the database.
 */
const SERDE_SPELLINGS: TrustScope[] = [
  'receive-workspaces',
  'send-workspaces',
  'file-transfer',
  'clipboard-transfer',
];

/**
 * The strings `parse_trust_scopes` matches on, transcribed from
 * `commands/src/transfer.rs`. These are the only four it accepts; anything else
 * is refused with "'{other}' is not something this device can be trusted with".
 */
const REQUEST_SPELLINGS: RequestedTrustScope[] = ['receive', 'send', 'files', 'clipboard'];

describe('the trust scope vocabularies', () => {
  it('names exactly the four scopes the enum defines', () => {
    expect(TRUST_SCOPES.map((s) => s.stored)).toEqual(SERDE_SPELLINGS);
  });

  it('offers exactly the four scopes parse_trust_scopes accepts', () => {
    expect(TRUST_SCOPES.map((s) => s.requested)).toEqual(REQUEST_SPELLINGS);
  });

  it('keeps the two spellings distinct for every scope', () => {
    // If these ever became equal the two directions would share a vocabulary,
    // which is fine — but it must then be a deliberate change, not a coincidence.
    // Asserting they differ is what documents the current design.
    for (const scope of TRUST_SCOPES) {
      expect(scope.requested).not.toBe(scope.stored);
    }
  });

  it('uses a stored spelling no caller should ever send, and vice versa', () => {
    // The exact conflation that broke sending: asking `canReceive` about a
    // requested spelling, or about a stored one that is not the receive scope.
    expect(canReceive(['receive' as TrustScope])).toBe(false);
    expect(canReceive(['send' as TrustScope])).toBe(false);
    expect(canReceive(['file-transfer', 'clipboard-transfer'])).toBe(false);
    expect(canReceive([])).toBe(false);
  });

  it('grants sending on the stored receive scope alone', () => {
    expect(canReceive(['receive-workspaces'])).toBe(true);
    expect(canReceive(['receive-workspaces', 'send-workspaces'])).toBe(true);
    // `send-workspaces` is the mirror scope: it lets the *other* machine push
    // here. It must not be mistaken for permission to send there.
    expect(canReceive(['send-workspaces'])).toBe(false);
  });

  it('labels a scope for a person, and never leaks the wire string', () => {
    for (const scope of SERDE_SPELLINGS) {
      const label = scopeLabel(scope);
      expect(label).not.toBe(scope);
      expect(label).not.toContain('-');
      expect(label.length).toBeGreaterThan(0);
    }
  });

  it('shows an unrecognised scope rather than hiding it', () => {
    // Falling back to `''` would make a device look less trusted than it is,
    // which is the wrong direction for a privacy decision to fail in.
    const unknown = 'future-scope' as TrustScope;
    expect(scopeLabel(unknown)).toBe('future-scope');
  });
});
