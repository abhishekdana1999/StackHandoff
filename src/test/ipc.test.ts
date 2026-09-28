/**
 * Tests for the IPC layer.
 *
 * The value of these is not that `invoke` gets called. It is that a change to a
 * command's argument shape or name fails here, in one place, rather than as a
 * screen that quietly sends the wrong field.
 */

import { describe, expect, it } from 'vitest';
import * as ipc from '@lib/ipc';
import { backend, makeManifest, makePreflightReport, mockBackend } from '@test/fakeBackend';
import type { CaptureSelection } from '@model';
import { emptySelection } from '@store/useAppStore';

describe('the IPC layer', () => {
  it('passes the workspace id under the name the command declares', async () => {
    mockBackend({ get_workspace: () => null });
    await ipc.getWorkspace('ws-7');
    expect(backend.lastArgs('get_workspace')).toEqual({ workspaceId: 'ws-7' });
  });

  it('serialises a manifest to a JSON string rather than passing the object', async () => {
    // The Rust side takes `manifest_json: String`, so handing it an object would
    // deserialise into the wrong type and fail at runtime with a serde message
    // no user could act on.
    const manifest = makeManifest();
    mockBackend({ validate_manifest: () => true });

    await ipc.validateManifest(manifest);

    const sent = backend.lastArgs('validate_manifest').manifestJson;
    expect(typeof sent).toBe('string');
    expect(JSON.parse(sent as string)).toEqual(manifest);
  });

  it('omits destination roots entirely when there are none', async () => {
    // An empty JSON object is different from no argument: the backend would
    // parse `{}` and plan against an empty root map, reporting "no location
    // chosen" for every project rather than falling back to its own default.
    mockBackend({ generate_restore_plan: () => ({ workspace_id: 'ws-1', steps: [], notes: [] }) });

    await ipc.generateRestorePlan(makeManifest());
    expect(backend.lastArgs('generate_restore_plan').destinationRootsJson).toBeUndefined();

    await ipc.generateRestorePlan(makeManifest(), { code: '/Users/me/code' });
    expect(JSON.parse(backend.lastArgs('generate_restore_plan').destinationRootsJson as string)).toEqual({
      code: '/Users/me/code',
    });
  });

  it('always sends an approvals object, even when the user approved nothing', async () => {
    // The backend treats a missing map as empty, but an explicit one means the
    // intent is recorded: this run had no approvals, rather than the call having
    // forgotten to ask.
    mockBackend({
      execute_restore: () => ({
        run_id: 'run-1',
        workspace_id: 'ws-1',
        results: [],
        notes: [],
        completed_at: '2026-01-01T00:00:00Z',
      }),
    });

    await ipc.executeRestore({ runId: 'run-1', plan: { workspace_id: 'ws-1', steps: [], notes: [] } });
    expect(JSON.parse(backend.lastArgs('execute_restore').approvalsJson as string)).toEqual({});
  });

  it('sends a capture selection whose policy starts restrictive', async () => {
    const selection: CaptureSelection = emptySelection();
    mockBackend({
      capture_workspace: () => ({ manifest: makeManifest(), sealed: 'sealed', warnings: [] }),
    });

    await ipc.captureWorkspace('Demo', selection);

    const sent = backend.lastArgs('capture_workspace').selection as CaptureSelection;
    expect(sent.policy.automaticCommandExecution).toBe(false);
    expect(sent.policy.secretValuesIncluded).toBe(false);
    expect(sent.browserUrls).toEqual([]);
  });

  it('turns a backend rejection into a message worth showing', () => {
    // A Rust error reaches the frontend as a string, so this is the only
    // conversion available. Returning `[object Object]` would be the alternative.
    expect(ipc.errorMessage('Record not found: ws-9')).toBe('Record not found: ws-9');
    expect(ipc.errorMessage(new Error('boom'))).toBe('boom');
    expect(ipc.errorMessage({ message: 'from an object' })).toBe('from an object');
  });

  it('fails loudly when a test invokes a command nobody faked', async () => {
    // The opposite of returning undefined, which renders as a blank screen and
    // is exactly the class of bug these tests exist to catch.
    mockBackend({});
    await expect(ipc.listWorkspaces()).rejects.toThrow(/No fake handler/);
  });
});

describe('the preflight bridge', () => {
  it('serialises only the requirements, not the whole report', async () => {
    mockBackend({ run_preflight: () => makePreflightReport() });

    const requirements = makeManifest().requirements;
    await ipc.runPreflight({ requirements });

    const sent = JSON.parse(backend.lastArgs('run_preflight').requirementsJson as string);
    expect(sent).toEqual(requirements);
    // The report is what comes back; sending it in the request would be a
    // silent no-op at best.
    expect(backend.lastArgs('run_preflight')).not.toHaveProperty('report');
  });

  it('sends an empty confirmed list rather than omitting it', async () => {
    mockBackend({ run_preflight: () => makePreflightReport() });

    await ipc.runPreflight({ requirements: makeManifest().requirements });

    // Omitting it is equivalent to "nothing was confirmed", but an explicit
    // empty list is what makes that visible at the call site.
    expect(backend.lastArgs('run_preflight').confirmedRequirements).toEqual([]);
    expect(backend.lastArgs('run_preflight').envFiles).toEqual([]);
  });
});

describe('pairing', () => {
  it('forwards the confirmed safety number under the name the command expects', async () => {
    mockBackend({ verify_pairing: () => ({}) });

    await ipc.verifyPairing({
      remoteNoiseKeyB64: 'remote-key',
      deviceName: 'Alex MacBook',
      confirmedSafetyNumber: '12345 67890',
      trustScopes: ['receive'],
    });

    const args = backend.lastArgs('verify_pairing');
    expect(args.confirmedSafetyNumber).toBe('12345 67890');

    // The rename is the load-bearing part. A Tauri command rejects arguments it
    // does not declare, so a frontend still sending `expectedSafetyNumber` would
    // fail at runtime while every type check passed -- which is exactly what
    // happened when the two names drifted apart in the Rust doc comment.
    expect(args).not.toHaveProperty('expectedSafetyNumber');
  });

  it('allows pairing with no number at all, because the number is no longer retyped', async () => {
    mockBackend({ verify_pairing: () => ({}) });

    // The backend recomputes the number from the key regardless, so an empty
    // field costs no verification -- it only means the UI had nothing displayed
    // when it made this call. Blocking it here would reintroduce the coupling
    // that made a correct pairing fail on a mistyped digit.
    await expect(
      ipc.verifyPairing({
        remoteNoiseKeyB64: 'remote-key',
        deviceName: 'Alex MacBook',
        confirmedSafetyNumber: '',
        trustScopes: ['receive'],
      }),
    ).resolves.toBeDefined();
  });
});
