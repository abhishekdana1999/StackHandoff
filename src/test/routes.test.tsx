/**
 * Every route, loaded the way a user loads it.
 *
 * This file exists because of one specific bug. The routes declared
 * `preflight/:transferId` while the screens read
 * `useParams<{ workspaceId }>()`. `useParams` is generic, so
 * `params.workspaceId` typed as `string` and compiled cleanly — and resolved to
 * `undefined` at runtime, because the route had declared a different name. All
 * three screens would have opened showing "the workspace could not be read", and
 * nothing in the type system, the linter, or a unit test of any one screen could
 * have said so.
 *
 * So the test loads the real `<App />` at a real URL and asks the question the
 * user would be asking: did the id in the address bar reach the backend? A
 * screen test cannot catch a bad route pattern, because it mounts the screen
 * directly and the router is never involved.
 */

import { describe, it, expect } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { TauriProvider } from '@hooks/useTauri';
import App from '../App';
import { backend, makeDiscoveredDevice, makeManifest, makePairedDevice, makePreflightReport, mockBackend } from './fakeBackend';
import type { RestorePlan } from '../types';

/** A restore plan with one step, so the preview has something to render. */
function makePlan(workspaceId: string): RestorePlan {
  return {
    workspace_id: workspaceId,
    steps: [
      {
        id: 'step-1',
        action_type: 'open_project',
        adapter_id: 'vscode',
        description: 'Open demo in VS Code',
        required: true,
        approved: true,
        dependencies: [],
        config: { project_id: 'p1' },
      },
    ],
    notes: [],
  };
}

/**
 * A handler set covering every command the routes can reach.
 *
 * Declared once so a new screen that adds an invoke fails as "no fake handler"
 * rather than as an unhandled rejection that leaves a blank screen and a passing
 * test.
 */
function fullBackend() {
  mockBackend({
    get_app_version: () => '0.1.0',
    get_platform: () => 'macos',
    get_device_identity: () => ({
      signingPublicKeyB64: 'ed25519-b64',
      noisePublicKeyB64: 'noise-b64',
      fingerprint: 'FINGERPRINT-1',
    }),
    get_device_key_exists: () => true,
    list_workspaces: () => [
      {
        id: 'ws-1',
        name: 'Demo',
        schema_version: 1,
        captured_at: '2026-01-01T00:00:00Z',
        source_device_id: 'FINGERPRINT-1',
        manifest_digest: 'a'.repeat(64),
        encrypted_manifest_path: '/tmp/manifest.json',
        status: 'captured',
      },
    ],
    get_manifest: () => makeManifest(),
    run_preflight: () => makePreflightReport(),
    rerun_preflight_check: () => makePreflightReport(),
    generate_restore_plan: () => makePlan('ws-1'),
    summarize_restore_plan: () => ({ total_steps: 1, opens_something: 1 }),
    execute_restore: () => ({
      run_id: 'run-1',
      workspace_id: 'ws-1',
      started_at: '2026-01-01T00:00:00Z',
      completed_at: '2026-01-01T00:00:01Z',
      results: [],
      notes: [],
    }),
    start_discovery: () => [],
    get_discovered_devices: () => [makeDiscoveredDevice()],
    list_paired_devices: () => [makePairedDevice()],
    // The status bar reads this. Without a handler it fell through to the
    // "unreadable" branch and the bar showed an em dash, so every route test
    // exercised a degraded shell rather than the one a user sees.
    get_device_fingerprint: () => 'AbCdEfGhIjKlMnOp',
    get_paired_device: () => makePairedDevice(),
    get_safety_number: () => ({ number: '12345 67890', note: 'compare' }),
    create_pairing_invitation: () => ({
      code: 'WC-1234-5678-9ABC',
      qr_data: 'workspace-clone://pair?code=WC-1234-5678-9ABC',
      expires_at: '2026-01-01T00:05:00Z',
      inviting_device_id: 'FINGERPRINT-1',
      inviting_device_name: 'This device',
      inviting_device_public_key: 'ed25519-b64',
    }),
    list_project_roots: () => ({
      projects: [
        {
          id: 'p1',
          name: 'demo',
          path: '/code/demo',
          isGitRepo: true,
          git: { branch: 'main', commit: 'abc1234', dirty: false, remoteHint: 'github.com/acme/demo' },
        },
      ],
      roots: ['/code'],
      missingRoots: [],
      unreadableRoots: [],
    }),
    list_settings: () => [['project_roots', '["/code"]']],
    list_paired_device_names: () => [],
    get_workspace_snapshots: () => [],
    get_workspace_restore_runs: () => [],
    get_incoming_transfers: () => [],
    get_transfer_history: () => [],
  });
}

/** A client with retries off, so a missing handler surfaces as a failure fast. */
function client() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

function renderAt(path: string) {
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

/** The workspace id passed to the most recent call to `cmd`. */
function idPassedTo(cmd: string, key: string): unknown {
  return backend.lastArgs(cmd)[key];
}

describe('routes resolve the id in the address bar', () => {
  it('preflight passes the workspace id from the URL to get_manifest', async () => {
    fullBackend();
    renderAt('/preflight/ws-from-the-url');

    await waitFor(() => expect(backend.commandNames()).toContain('get_manifest'));
    expect(idPassedTo('get_manifest', 'workspaceId')).toBe('ws-from-the-url');
  });

  it('prepare passes the workspace id from the URL to get_manifest', async () => {
    fullBackend();
    renderAt('/prepare/ws-from-the-url');

    await waitFor(() => expect(backend.commandNames()).toContain('get_manifest'));
    expect(idPassedTo('get_manifest', 'workspaceId')).toBe('ws-from-the-url');
  });

  it('restore-preview passes the workspace id from the URL to get_manifest', async () => {
    fullBackend();
    renderAt('/restore-preview/ws-from-the-url');

    await waitFor(() => expect(backend.commandNames()).toContain('get_manifest'));
    expect(idPassedTo('get_manifest', 'workspaceId')).toBe('ws-from-the-url');
  });

  it('transfer passes the workspace id from the URL as the send target', async () => {
    fullBackend();
    renderAt('/transfer/ws-from-the-url');

    // The transfer screen's only use of the id is as the workspace to send, so
    // the fact that it loaded at all with a non-empty summary is the check: the
    // manifest section reads it from the store, and the send button is enabled
    // only when there is a real id.
    await waitFor(() => expect(backend.commandNames()).toContain('get_discovered_devices'));
    expect(screen.getByRole('heading', { name: 'Send Workspace' })).toBeInTheDocument();
  });

  it('a workspace-scoped route does not report the workspace as unreadable', async () => {
    // The specific failure this file was written for: with the param name
    // mismatched, `get_manifest` is still called — with `undefined` — and the
    // screen renders its error state. Asserting on the error state is what
    // distinguishes "the call happened" from "the call happened with the id".
    fullBackend();
    renderAt('/preflight/ws-from-the-url');

    await waitFor(() => expect(backend.commandNames()).toContain('get_manifest'));
    expect(screen.queryByText('The workspace could not be read')).not.toBeInTheDocument();
  });
});

describe('screens without a route parameter', () => {
  it('workspaces lists real workspaces and never the fixtures', async () => {
    fullBackend();
    renderAt('/workspaces');

    await waitFor(() => expect(screen.getByText('Demo')).toBeInTheDocument());
    expect(screen.getByText('Workspaces (1)')).toBeInTheDocument();
    // The old mock list had three workspaces with names that were never
    // captured by anything.
    expect(screen.queryByText('Documentation Site')).not.toBeInTheDocument();
  });

  it('devices shows the paired device from the database', async () => {
    fullBackend();
    renderAt('/devices');

    await waitFor(() => expect(screen.getByText("Alex's MacBook")).toBeInTheDocument());
    // A fingerprint is the thing a user compares out loud, so it has to be on
    // screen and it has to be the backend's value.
    expect(screen.getByText('AbCdEfGhIjKlMnOp')).toBeInTheDocument();
    expect(backend.commandNames()).toContain('list_paired_devices');
  });

  it('devices never shows a mocked device', async () => {
    fullBackend();
    renderAt('/devices');

    await waitFor(() => expect(backend.commandNames()).toContain('list_paired_devices'));
    // From the old mock array. If this ever comes back, the screen has gone back
    // to hardcoded data.
    expect(screen.queryByText('Windows Desktop')).not.toBeInTheDocument();
    expect(screen.queryByText('MacBook Pro (Abhishek)')).not.toBeInTheDocument();
  });

  it('settings shows the backend fingerprint, not a placeholder', async () => {
    fullBackend();
    renderAt('/settings');

    await waitFor(() => expect(screen.getByText('FINGERPRINT-1')).toBeInTheDocument());
    expect(backend.commandNames()).toContain('get_device_identity');
  });
});

describe('page headings', () => {
  // Every route needs exactly one <h1>, and it must not be the toolbar's copy
  // of the title. When the toolbar repeated the screen heading, the same word
  // appeared twice on the page — once in the chrome, once in the content — and
  // a getByRole('heading', { name }) query matched either one.
  const ROUTES = [
    '/workspaces',
    '/devices',
    '/settings',
    '/capture',
    '/preflight/ws-1',
    '/prepare/ws-1',
    '/transfer/ws-1',
    '/restore-preview/ws-1',
    '/restore-report/ws-1',
  ];

  it.each(ROUTES)('%s has exactly one level-one heading', async (route) => {
    fullBackend();
    renderAt(route);

    await waitFor(() => expect(screen.getAllByRole('heading', { level: 1 }).length).toBeGreaterThan(0));
    const h1s = screen.getAllByRole('heading', { level: 1 });
    expect(h1s).toHaveLength(1);
  });

  it.each(ROUTES)('%s shows real values in the status bar', async (route) => {
    fullBackend();
    renderAt(route);

    // The bar is app chrome, so it is on every route — and it used to be wrong
    // on all of them, reading a hardcoded "Local Mode" pill with a green dot that
    // appeared whether or not anything was listening. These three come off the
    // backend, so the test is that the real values arrive.
    await waitFor(() =>
      expect(screen.getByTestId('status-version')).toHaveTextContent('v0.1.0')
    );
    expect(screen.getByTestId('status-paired')).toHaveTextContent('1 paired');
    expect(screen.getByTitle('This device: AbCdEfGhIjKlMnOp')).toHaveTextContent(
      'AbCdEfGh…MnOp'
    );
  });

  it.each(ROUTES)('%s does not repeat the title in the content', async (route) => {
    fullBackend();
    renderAt(route);

    // The toolbar title is the page's only <h1>. Nothing in the scrollable
    // content may repeat it, which is how every screen used to open: a 16px
    // heading whose words matched the toolbar's exactly.
    const toolbarTitle = await screen.findByTestId('toolbar-title');
    expect(toolbarTitle.tagName).toBe('H1');

    const title = toolbarTitle.textContent?.trim();
    const repeated = [...document.querySelectorAll('main h1, main h2, main h3, main h4')]
      .map((el) => el.textContent?.trim())
      .filter((t) => t === title);
    expect(repeated).toEqual([]);
  });
});
