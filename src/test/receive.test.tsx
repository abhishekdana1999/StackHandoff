/**
 * The receive path, as the user sees it.
 *
 * ## Why this file exists
 *
 * The backend receive path was written without a single line of frontend
 * coverage, and the frontend had nothing to call. The consequence was that
 * sending a workspace between two machines worked end to end at the protocol
 * level and still told nobody anything: the receiving machine stored the
 * workspace, the sending machine's window said "sent", and no user on either
 * machine could see that it had happened or, if it had not, why.
 *
 * These tests pin the parts that make it visible:
 *
 * * an arrival is rendered, with the sender's *name* and not a fingerprint;
 * * a refusal is rendered *too*, with the reason, and is not hidden behind the
 *   workspace list — the sender is watching the other machine;
 * * dismissing an arrival is possible and does not touch the workspace;
 * * a transfer history exists and shows both directions with their statuses.
 *
 * ## What is real and what is not
 *
 * Real: the components, the router, and the IPC call path — every `invoke` in
 * these tests goes through the same `window.__TAURI_INTERNALS__` stub the app
 * uses, so an argument name that the backend does not accept would show up as a
 * test failure here.
 *
 * Not real: the accept loop, the Noise handshake and the database. Those are
 * covered where they live, in `commands/tests/two_device_transfer.rs`, which
 * drives two real services over a real socket. What is untested and untestable
 * here is exactly the gap between them: that the window asks the backend the
 * right question and shows what comes back.
 */

import { describe, it, expect } from 'vitest';
// `fireEvent`, not `@testing-library/user-event`: the interaction under test is
// "this element is a button and it calls the command with the right id", which
// `fireEvent` states directly. `user-event` would add a dependency to assert
// something a plain click already establishes.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TauriProvider } from '@hooks/useTauri';
import App from '../App';
import { ARRIVALS_POLL_MS } from '../screens/WorkspacesScreen';
import {
  backend,
  makeIncomingTransfer,
  makeTransferHistoryEntry,
  makePairedDevice,
  mockBackend,
} from './fakeBackend';
import type { IncomingTransfer } from '../types';

const WORKSPACES = [
  {
    id: 'ws-1',
    name: 'Welcome Rewards',
    schema_version: 1,
    captured_at: '2026-01-01T00:00:00Z',
    source_device_id: 'peer-1',
    manifest_digest: 'a'.repeat(64),
    encrypted_manifest_path: '/tmp/manifest.json',
    // `received`, not `captured`. A workspace that arrived on this machine was
    // not captured on it, and the badge is the only place a user would notice
    // the difference being lost.
    status: 'received',
  },
];

/**
 * A handler set covering every command the workspaces screen can reach.
 *
 * The arrivals query is registered as `[]` by default so a test opts in to an
 * arrival explicitly. A default of "one arrival" would make the accepted and
 * refused banners ordinary background rather than the thing under test.
 */
function screenBackend(overrides: Record<string, (args: Record<string, unknown>) => unknown> = {}) {
  mockBackend({
    get_app_version: () => '0.1.0',
    get_platform: () => 'macos',
    get_device_identity: () => ({
      signingPublicKeyB64: 'ed25519-b64',
      noisePublicKeyB64: 'noise-b64',
      fingerprint: 'THIS-DEVICE',
    }),
    get_device_key_exists: () => true,
    list_workspaces: () => WORKSPACES,
    get_workspace_snapshots: () => [],
    get_workspace_restore_runs: () => [],
    get_incoming_transfers: () => [],
    get_transfer_history: () => [],
    get_manifest: () => {
      throw new Error('not needed for this test');
    },
    list_paired_devices: () => [makePairedDevice()],
    get_discovered_devices: () => [],
    start_discovery: () => [],
    list_settings: () => [],
    ...overrides,
  });
}

function client() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

function renderAt(path = '/workspaces') {
  return render(
    <QueryClientProvider client={client()}>
      <MemoryRouter initialEntries={[path]}>
        <TauriProvider>
          <App />
        </TauriProvider>
      </MemoryRouter>
    </QueryClientProvider>
  );
}

/** The arrival banner for one transfer, or a thrown error if it is not on screen. */
function banner(transferId: string): HTMLElement {
  return screen.getByTestId(`arrival-${transferId}`);
}

describe('an arrival is shown to the user', () => {
  it('names the sender and says the workspace is stored', async () => {
    screenBackend({
      get_incoming_transfers: () => [makeIncomingTransfer({ workspaceName: 'Payments Branch' })],
    });
    renderAt();

    const card = await waitFor(() => banner('tr-1'));
    expect(within(card).getByText(/'Payments Branch' arrived/)).toBeInTheDocument();
    // The *name*, because a fingerprint cannot be matched to a machine by
    // someone trying to work out whether the right laptop sent it.
    expect(within(card).getByText(/from Alex's MacBook/)).toBeInTheDocument();
    expect(within(card).getByText(/sealed with this machine's key/)).toBeInTheDocument();
  });

  it('does not show a fingerprint where a name belongs', async () => {
    screenBackend({
      get_incoming_transfers: () => [
        makeIncomingTransfer({ senderDeviceId: 'AbCdEfGhIjKlMnOp', senderDeviceName: "Sam's Windows PC" }),
      ],
    });
    renderAt();

    const card = await waitFor(() => banner('tr-1'));
    expect(within(card).getByText(/from Sam's Windows PC/)).toBeInTheDocument();
    expect(within(card).queryByText(/AbCdEfGhIjKlMnOp/)).not.toBeInTheDocument();
  });

  it('shows every arrival, newest first, rather than only the latest', async () => {
    // Collapsing to one notification would mean a workspace that arrived while
    // another one was on screen is never mentioned at all.
    screenBackend({
      get_incoming_transfers: () => [
        makeIncomingTransfer({ transferId: 'tr-new', workspaceName: 'Newest' }),
        makeIncomingTransfer({ transferId: 'tr-old', workspaceName: 'Older' }),
      ],
    });
    renderAt();

    await waitFor(() => banner('tr-old'));
    const banners = screen.getAllByRole('status');
    expect(banners).toHaveLength(2);
    expect(banners[0]).toHaveAttribute('data-testid', 'arrival-tr-new');
  });

  it('shows nothing at all when nothing arrived', async () => {
    screenBackend();
    renderAt();

    await waitFor(() => expect(backend.commandNames()).toContain('get_incoming_transfers'));
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('keeps the list usable when the arrivals query fails', async () => {
    // A failure to read the notification list must not take the workspaces with
    // it. The workspaces are already on disk; losing the banner is cosmetic,
    // losing the list is the user losing access to their own work.
    screenBackend({
      get_incoming_transfers: () => {
        throw new Error('the accept loop is not running');
      },
    });
    renderAt();

    await waitFor(() => expect(backend.commandNames()).toContain('get_incoming_transfers'));
    expect(await screen.findByText('Welcome Rewards')).toBeInTheDocument();
    // The whole list renders, not just the row that happens to match: a
    // workspace the user cannot get to because a notification query failed is
    // still a workspace they have lost access to.
    expect(screen.getByText('Workspaces (1)')).toBeInTheDocument();
    expect(backend.commandNames()).toContain('list_workspaces');
  });
});

describe('a refusal is shown, not swallowed', () => {
  const refused = makeIncomingTransfer({
    transferId: 'tr-refused',
    workspaceName: 'Not Welcome',
    accepted: false,
    refusalReason:
      "'Not paired' has not been paired on this machine. Nothing was stored. Pair it on Devices if that is what you want.",
  });

  it('shows the reason the workspace was not stored', async () => {
    // This is the whole point. The sender's `send_workspace` reports success as
    // soon as the bytes are delivered, so the receiver's refusal is the only
    // place the user can learn that the workspace did not land.
    screenBackend({ get_incoming_transfers: () => [refused] });
    renderAt();

    const card = await waitFor(() => banner('tr-refused'));
    expect(card).toHaveAttribute('data-accepted', 'false');
    expect(within(card).getByText(/was not accepted/)).toBeInTheDocument();
    expect(within(card).getByText(/has not been paired on this machine/)).toBeInTheDocument();
  });

  it('announces a refusal assertively', async () => {
    // A refusal needs attention rather than waiting for the user to look: they
    // are being told something did not happen, usually while watching the other
    // machine claim it worked.
    screenBackend({ get_incoming_transfers: () => [refused] });
    renderAt();

    const card = await waitFor(() => banner('tr-refused'));
    expect(card).toHaveAttribute('aria-live', 'assertive');
  });

  it('announces an accepted arrival politely', async () => {
    // The mirror of the case above, and a separate test rather than a second
    // render in the same one: two mounted trees would leave `screen` searching
    // both, and a wrong id would be found in the other tree.
    screenBackend({ get_incoming_transfers: () => [makeIncomingTransfer()] });
    renderAt();

    const card = await waitFor(() => banner('tr-1'));
    expect(card).toHaveAttribute('aria-live', 'polite');
  });

  it('does not claim a workspace was stored when it was not', async () => {
    screenBackend({ get_incoming_transfers: () => [refused] });
    renderAt();

    const card = await waitFor(() => banner('tr-refused'));
    expect(within(card).queryByText(/arrived from/)).not.toBeInTheDocument();
    expect(within(card).queryByText(/Open preflight/)).not.toBeInTheDocument();
  });
});

describe('dismissing an arrival', () => {
  it('sends the transfer id and hides the banner', async () => {
    // The id, not the workspace id: two devices can send the same workspace, and
    // a dismissal keyed on the workspace would hide the other arrival too.
    screenBackend({ get_incoming_transfers: () => [makeIncomingTransfer()] });
    renderAt();
    const card = await waitFor(() => banner('tr-1'));

    fireEvent.click(within(card).getByRole('button', { name: /Dismiss this notification/ }));

    await waitFor(() =>
      expect(backend.lastArgs('dismiss_incoming_transfer').transferId).toBe('tr-1')
    );
  });

  it('does not delete the workspace it announced', async () => {
    // Dismissing clears a notification. The workspace was stored on arrival and
    // has to stay listed, or a user tidying up notifications would silently
    // delete a work session.
    screenBackend({ get_incoming_transfers: () => [makeIncomingTransfer()] });
    renderAt();
    const card = await waitFor(() => banner('tr-1'));

    fireEvent.click(within(card).getByRole('button', { name: /Dismiss this notification/ }));

    await waitFor(() => expect(backend.commandNames()).toContain('dismiss_incoming_transfer'));
    expect(backend.commandNames()).not.toContain('delete_workspace');
    expect(screen.getByText('Welcome Rewards')).toBeInTheDocument();
  });
});

describe('a received workspace is distinguishable from a captured one', () => {
  it('badges it Received, not Captured', async () => {
    // "Captured" would be a false claim: this machine did not capture it, and
    // the badge is the only thing on the row saying where it came from.
    screenBackend();
    renderAt();

    const row = await waitFor(() => screen.getByTestId('workspace-ws-1'));
    expect(within(row).getByText('Received')).toBeInTheDocument();
    expect(within(row).queryByText('Captured')).not.toBeInTheDocument();
  });
});

describe('transfer history answers "did it get there?"', () => {
  it('shows both directions of a workspace', async () => {
    screenBackend({
      get_transfer_history: () => [
        makeTransferHistoryEntry({
          id: 'tr-in',
          sourceDeviceName: "Sam's Windows PC",
          destinationDeviceName: "Alex's MacBook",
        }),
        makeTransferHistoryEntry({
          id: 'tr-out',
          sourceDeviceName: "Alex's MacBook",
          destinationDeviceName: "Sam's Windows PC",
        }),
      ],
    });
    renderAt();

    fireEvent.click(await screen.findByText('History'));

    await waitFor(() => expect(backend.commandNames()).toContain('get_transfer_history'));
    expect(await screen.findByText("Sam's Windows PC → Alex's MacBook")).toBeInTheDocument();
    expect(await screen.findByText("Alex's MacBook → Sam's Windows PC")).toBeInTheDocument();
  });

  it('scopes the query to the selected workspace', async () => {
    // A history filtered only by the visible tab, with no workspace in it, would
    // show every transfer this machine has ever taken part in and quietly look
    // like a per-workspace record.
    screenBackend();
    renderAt();

    fireEvent.click(await screen.findByText('History'));

    await waitFor(() => expect(backend.commandNames()).toContain('get_transfer_history'));
    expect(backend.lastArgs('get_transfer_history').workspaceId).toBe('ws-1');
  });

  it('shows the error a failed transfer recorded', async () => {
    screenBackend({
      get_transfer_history: () => [
        makeTransferHistoryEntry({
          status: 'failed',
          progress: 0.4,
          error: 'The other machine closed the connection after 4 of 10 frames.',
        }),
      ],
    });
    renderAt();

    fireEvent.click(await screen.findByText('History'));

    expect(await screen.findByText(/closed the connection after 4 of 10 frames/)).toBeInTheDocument();
    expect(screen.getByText('failed')).toBeInTheDocument();
  });

  it('says so when a workspace has never moved', async () => {
    // An empty table is ambiguous: "never transferred" and "the query failed"
    // look identical without this line.
    screenBackend();
    renderAt();

    fireEvent.click(await screen.findByText('History'));

    expect(await screen.findByText(/No transfers recorded for this workspace/)).toBeInTheDocument();
  });
});

describe('polling', () => {
  /**
   * The accept loop is a backend task, so the only way this window learns about
   * an arrival is by asking again.
   *
   * A missing `refetchInterval` is invisible to every other test in this file:
   * a mount-only fetch renders the arrival that was already there, and the
   * screen looks correct. The only thing that catches it is a transfer that
   * completes *after* the screen loaded, so this waits on real elapsed time
   * rather than forcing an invalidation that would make the assertion pass
   * either way.
   */
  it('asks again, and shows a transfer that arrived after it loaded', async () => {
    const before = Date.now();
    let call = 0;
    screenBackend({
      get_incoming_transfers: () => {
        call += 1;
        return call > 1 ? [makeIncomingTransfer({ workspaceName: 'Arrived Later' })] : [];
      },
    });
    renderAt();

    await waitFor(() => expect(backend.commandNames()).toContain('get_incoming_transfers'));
    expect(screen.queryByText(/'Arrived Later' arrived/)).not.toBeInTheDocument();

    expect(
      await screen.findByText(/'Arrived Later' arrived/, undefined, { timeout: ARRIVALS_POLL_MS * 3 })
    ).toBeInTheDocument();
    expect(backend.callsTo('get_incoming_transfers').length).toBeGreaterThan(1);
    // Guard against the assertion above passing because something else refetched.
    expect(Date.now() - before).toBeLessThan(ARRIVALS_POLL_MS * 3);
  });

  it('puts a workspace that arrived after load into the list without a remount', async () => {
    // The list and the arrivals are both refetched here: the list started
    // before the transfer existed, and the arrival is what tells the screen to
    // ask for it again. Before the invalidation, the new workspace stayed
    // invisible until a navigation remounted the screen — the "I see the green
    // banner but my workspace is not in the list" gap.
    const lateWorkspace = {
      id: 'ws-late',
      name: 'Arrived Later',
      schema_version: 1,
      captured_at: '2026-01-01T00:00:00Z',
      source_device_id: 'peer-1',
      manifest_digest: 'c'.repeat(64),
      encrypted_manifest_path: '/tmp/late.json',
      status: 'received',
    };
    let listCall = 0;
    let arrivalCall = 0;
    screenBackend({
      list_workspaces: () => {
        listCall += 1;
        return listCall > 1 ? [...WORKSPACES, lateWorkspace] : WORKSPACES;
      },
      get_incoming_transfers: () => {
        arrivalCall += 1;
        return arrivalCall > 1
          ? [makeIncomingTransfer({ transferId: 'tr-late', workspaceId: 'ws-late', workspaceName: 'Arrived Later' })]
          : [];
      },
    });
    renderAt();

    await waitFor(() => expect(backend.callsTo('list_workspaces').length).toBeGreaterThan(0));
    expect(screen.queryByText('Arrived Later')).not.toBeInTheDocument();

    expect(
      await screen.findByText('Arrived Later', undefined, { timeout: ARRIVALS_POLL_MS * 3 })
    ).toBeInTheDocument();
    // The arrival had to provoke a second fetch of the list; the banner alone
    // would have rendered without one.
    expect(backend.callsTo('list_workspaces').length).toBeGreaterThan(1);
  });
});

describe('the arrival list is what the backend actually returns', () => {
  it('does not invent a fingerprint for an unpaired sender', async () => {
    // An unpaired sender has no row here, so there is no name to show. The id is
    // the honest fallback; a made-up label would be worse than an id.
    const unknown: IncomingTransfer = makeIncomingTransfer({
      senderDeviceName: '5OpQ2mZ8vLdR0eXyKfJ1wA==',
      senderDeviceId: '5OpQ2mZ8vLdR0eXyKfJ1wA==',
    });
    screenBackend({ get_incoming_transfers: () => [unknown] });
    renderAt();

    const card = await waitFor(() => banner('tr-1'));
    expect(within(card).getByText(/from 5OpQ2mZ8vLdR0eXyKfJ1wA==/)).toBeInTheDocument();
  });
});

describe('the arrival banner speaks the backend wire shape', () => {
  it('renders an arrival in the exact camelCase JSON the backend emits', async () => {
    // `IncomingTransfer` is serialized by Rust with `rename_all = "camelCase"`,
    // so `transferId`, `workspaceName`, `senderDeviceName`, … are the keys that
    // actually arrive over `invoke`. A fixture built from `makeIncomingTransfer`
    // would track whatever the TS type says; this object is written straight
    // against the Rust struct's serde output, so the type and the wire cannot
    // drift silently. Every field used below (the banner name, the dismiss id,
    // the preflight id) comes from these keys.
    const wire: unknown = [
      {
        transferId: 'tr-wire',
        workspaceId: 'ws-9',
        workspaceName: 'Wire Shaped',
        senderDeviceId: 'peer-9',
        senderDeviceName: 'BISWAJITA',
        sourceDeviceId: 'peer-9',
        accepted: true,
        refusalReason: null,
        transferDigest: 'd'.repeat(64),
        bytesReceived: 4096,
        receivedAt: '2026-01-01T00:00:00Z',
      },
    ];
    screenBackend({
      get_incoming_transfers: () => wire as IncomingTransfer[],
    });
    renderAt();

    const card = await waitFor(() => banner('tr-wire'));
    expect(within(card).getByText(/'Wire Shaped' arrived from BISWAJITA/)).toBeInTheDocument();

    // The dismiss sends the camelCase id over invoke, which is what the backend
    // command parameter expects.
    fireEvent.click(within(card).getByRole('button', { name: /Dismiss this notification/ }));
    await waitFor(() =>
      expect(backend.lastArgs('dismiss_incoming_transfer').transferId).toBe('tr-wire')
    );
  });
});

describe('acting on a successful arrival', () => {
  it('opens preflight for the workspace that arrived', async () => {
    // The banner's primary action must go somewhere agreed with that workspace:
    // it navigates to `/preflight/<workspaceId>`. The manifest fetch fails here
    // on purpose -- the error screen is unique to the preflight route, so it is
    // the proof that navigation happened, and the requested id is the received
    // workspace's, not whatever was selected before.
    screenBackend({ get_incoming_transfers: () => [makeIncomingTransfer()] });
    renderAt();

    const card = await waitFor(() => banner('tr-1'));
    fireEvent.click(within(card).getByRole('button', { name: /Open preflight/ }));

    expect(
      await screen.findByText('The workspace could not be read')
    ).toBeInTheDocument();
    expect(backend.lastArgs('get_manifest').workspaceId).toBe('ws-1');
  });
});
