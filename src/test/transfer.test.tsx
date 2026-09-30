/**
 * Why this file exists.
 *
 * Every destination in the send screen was permanently disabled. The reason
 * shown was "Not allowed to receive workspaces", on a database row that plainly
 * contained `receive-workspaces`. The screen tested
 * `trust_scopes.includes('receive')`; the backend serialises the `TrustScope`
 * enum with `#[serde(rename_all = "kebab-case")]`, so it can only ever report
 * `receive-workspaces`. The comparison could not be true, for any device, ever.
 *
 * Nothing caught it. `tsc` was clean because the TypeScript agreed with itself:
 * `TrustScope` was declared as `'receive' | 'send' | 'files' | 'clipboard'`, the
 * short form that the *pairing request* direction uses, and the paired-device
 * response was typed as that same union. The types described a wire format that
 * does not exist. The fixture in `fakeBackend` had the same wrong spelling, so
 * any test written against it would have agreed with the bug.
 *
 * The lesson is the one worth encoding: an enum that crosses a language boundary
 * in two directions with two spellings needs a test that pins the actual wire
 * value, not one that re-uses the same fictional value throughout.
 */
// `fireEvent`, not `@testing-library/user-event`, to match the existing suite:
// the project deliberately avoids that dependency, and a click on a plain button
// is what is under test here.
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { TransferScreen } from '@screens/TransferScreen';
import { makeDiscoveredDevice, makePairedDevice } from './fakeBackend';
import type { PairedDevice } from '@model';
import { useAppStore } from '@store/useAppStore';

const SEND_WORKSPACE = 'send_workspace';
const DISCOVER = 'get_discovered_devices';
const PAIRED = 'list_paired_devices';
const START_DISCOVERY = 'start_discovery';

const LAPTOP_ID = 'hnODGDVs/oYDiOhuQO941A==';

function mockBackend(overrides: { paired?: PairedDevice[]; discovered?: unknown[] }) {
  const invoke = vi.fn(async (cmd: string, _args?: unknown) => {
    switch (cmd) {
      case DISCOVER:
        return overrides.discovered ?? [];
      case PAIRED:
        return overrides.paired ?? [];
      case START_DISCOVERY:
        return null;
      case SEND_WORKSPACE:
        return {
          succeeded: true,
          bytesSent: 1024,
          transfer: {
            id: 't1',
            workspace_id: 'w1',
            source_device_id: 'me',
            destination_device_id: LAPTOP_ID,
            status: 'completed',
            progress: 1,
            started_at: '2026-01-01T00:00:00Z',
            completed_at: '2026-01-01T00:00:01Z',
            error: null,
          },
        };
      default:
        return null;
    }
  });
  // Tauri hands the command name to the first argument of the real `invoke`.
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: (_cmd: string, args: unknown) => invoke(_cmd, args),
  };
  return invoke;
}

function renderTransfer() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/transfer/w1']}>
        <Routes>
          <Route path="/transfer/:workspaceId" element={<TransferScreen />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

/** A device as the backend actually reports one, on the network and paired. */
function onlineLaptop(scopes: PairedDevice['trust_scopes']): PairedDevice {
  // Only `paired` is returned to the screen; `discovered` is passed separately so
  // the pairing and network halves stay distinguishable in the test body. An
  // earlier version spread both into one object, which silently let a
  // `DiscoveredDevice` field overwrite a `PairedDevice` one.
  return makePairedDevice({ id: LAPTOP_ID, name: 'Office PC', trust_scopes: scopes });
}

describe('the send screen can actually send', () => {
  beforeEach(() => {
    useAppStore.setState({ manifest: null, captureWarnings: [], transfer: null });
  });

  it('offers a paired device holding the receive scope, spelled as the backend spells it', async () => {
    // The exact row from the live database. If this test fails with "Not allowed to
    // receive workspaces" on a row containing `receive-workspaces`, the screen is
    // asking for a spelling the backend cannot produce.
    mockBackend({
      paired: [onlineLaptop(['receive-workspaces', 'send-workspaces'])],
      discovered: [makeDiscoveredDevice({ device_id: LAPTOP_ID, name: 'Office PC' })],
    });
    renderTransfer();

    const destination = await screen.findByRole('button', { name: /Office PC/ });
    await waitFor(() => {
      expect(destination).toBeEnabled();
    });
    expect(
      screen.queryByText('Not allowed to receive workspaces')
    ).not.toBeInTheDocument();
  });

  it('shows the scopes as words rather than raw wire strings', async () => {
    // The display lookup matched the short form against a stored kebab-case
    // string, so a correctly-paired device listed "receive-workspaces" where a
    // person should read "Receive". A raw identifier reaching the UI means the
    // two vocabularies have drifted apart again.
    mockBackend({
      paired: [onlineLaptop(['receive-workspaces', 'file-transfer'])],
      discovered: [makeDiscoveredDevice({ device_id: LAPTOP_ID, name: 'Office PC' })],
    });
    renderTransfer();

    const destination = await screen.findByRole('button', { name: /Office PC/ });
    await waitFor(() => {
      expect(destination).toBeEnabled();
    });

    // Matched as a substring: the label and the scope list share one text node
    // ("Allowed: Receive, Files"), which is what the screen renders.
    const allowed = destination.textContent ?? '';
    expect(allowed).toContain('Receive');
    expect(allowed).toContain('Files');
    // The raw wire strings must not reach a person. Seeing one means the two
    // vocabularies have drifted apart again, since these are exactly the values
    // the backend sends.
    expect(allowed).not.toContain('receive-workspaces');
    expect(allowed).not.toContain('file-transfer');
  });

  it.each([['send-workspaces'], ['clipboard-transfer'], ['file-transfer']] as const)(
    'refuses a device holding only %s, because receive is what sending requires',
    async (scope) => {
      mockBackend({
        paired: [onlineLaptop([scope])],
        discovered: [makeDiscoveredDevice({ device_id: LAPTOP_ID, name: 'Office PC' })],
      });
      renderTransfer();

      const destination = await screen.findByRole('button', { name: /Office PC/ });
      await waitFor(() => {
        expect(destination).toBeDisabled();
      });
      expect(await screen.findByText('Not allowed to receive workspaces')).toBeInTheDocument();
    }
  );

  it('still refuses a device that is on the network but was never paired', async () => {
    mockBackend({
      paired: [],
      discovered: [makeDiscoveredDevice({ device_id: LAPTOP_ID, name: 'Office PC' })],
    });
    renderTransfer();

    const destination = await screen.findByRole('button', { name: /Office PC/ });
    await waitFor(() => {
      expect(destination).toBeDisabled();
    });
    expect(await screen.findByText('Not paired')).toBeInTheDocument();
  });

  it('still refuses a paired device that is not currently on the network', async () => {
    mockBackend({
      paired: [makePairedDevice({ id: LAPTOP_ID, name: 'Office PC', trust_scopes: ['receive-workspaces'] })],
      discovered: [],
    });
    renderTransfer();

    const destination = await screen.findByRole('button', { name: /Office PC/ });
    await waitFor(() => {
      expect(destination).toBeDisabled();
    });
    expect(await screen.findByText('Not on the network right now')).toBeInTheDocument();
  });

  it('sends to the device the user picked', async () => {
    const invoke = mockBackend({
      paired: [onlineLaptop(['receive-workspaces'])],
      discovered: [makeDiscoveredDevice({ device_id: LAPTOP_ID, name: 'Office PC' })],
    });
    renderTransfer();

    fireEvent.click(await screen.findByRole('button', { name: /Office PC/ }));
    fireEvent.click(await screen.findByRole('button', { name: /Send workspace/ }));

    await waitFor(() => {
      const call = invoke.mock.calls.find((c) => c[0] === SEND_WORKSPACE);
      expect(call).toBeDefined();
      const args = call?.[1] as { destinationDeviceId: string } | undefined;
      expect(args?.destinationDeviceId).toBe(LAPTOP_ID);
    });
  });
});
