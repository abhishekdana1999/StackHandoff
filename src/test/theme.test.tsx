/**
 * The theme, the shell, and the window chrome.
 *
 * These are the behaviours that a design change can quietly break without any
 * Rust test noticing. The `.dark` class was the motivating case: the whole dark
 * half of the theme was dead CSS that nothing applied, and the stylesheet was
 * correct, and the build was clean, and the app rendered light on a dark
 * desktop for the entire life of the project. Nothing in the test suite was in a
 * position to fail.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Layout } from '../components/Layout';
import { useThemeStore, initTheme, applyTheme, readStoredMode, type ThemeMode } from '../store/useThemeStore';
import { mockBackend, makePairedDevice } from './fakeBackend';

// jsdom does not implement matchMedia, and the whole theme resolution depends on
// it. Installed per test so each case can decide what the OS "says".
function setPrefersDark(dark: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      matches: query.includes('dark') ? dark : false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });
}

/**
 * A matchMedia whose `matches` can be changed after the fact, plus the set of
 * listeners registered against it.
 *
 * `matches` is a plain mutable property on purpose. The store's change handler
 * reads `query.matches` when it runs, exactly as a browser would, so a fake
 * that hardcoded `false` would make the handler always conclude "light" and the
 * test would fail for a reason unrelated to the code under test.
 */
function listeners() {
  const map = new Map<string, Set<(e: unknown) => void>>();
  const matches = new Map<string, boolean>();

  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      get matches() {
        return matches.get(query) ?? false;
      },
      media: query,
      onchange: null,
      addEventListener: (_: string, fn: (e: unknown) => void) => {
        if (!map.has(query)) map.set(query, new Set());
        map.get(query)!.add(fn);
      },
      removeEventListener: (_: string, fn: (e: unknown) => void) => {
        map.get(query)?.delete(fn);
      },
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });

  return {
    listeners: map,
    /** Change what the OS reports and notify anyone listening, as a browser would. */
    set(query: string, dark: boolean) {
      matches.set(query, dark);
      act(() => {
        map.get(query)?.forEach((fn) => fn({ matches: dark }));
      });
    },
  };
}

function renderLayout(initialPath = '/workspaces') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[initialPath]}>
        <Layout />
      </MemoryRouter>
    </QueryClientProvider>
  );
}

describe('theme store', () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.classList.remove('dark');
    setPrefersDark(false);
  });

  it('applies the dark class to the document, not to a wrapper', () => {
    applyTheme('dark');
    // The tailwind `darkMode: 'class'` strategy only looks at <html>. Applying
    // it anywhere else would leave the page light while the console claimed
    // otherwise.
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(document.documentElement.style.colorScheme).toBe('dark');

    applyTheme('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(document.documentElement.style.colorScheme).toBe('light');
  });

  it('resolves `system` to whichever mode the OS reports', () => {
    setPrefersDark(true);
    useThemeStore.getState().setMode('system');
    expect(useThemeStore.getState().resolved).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);

    setPrefersDark(false);
    useThemeStore.getState().setMode('system');
    expect(useThemeStore.getState().resolved).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('an explicit choice overrides the OS in both directions', () => {
    setPrefersDark(true);
    useThemeStore.getState().setMode('light');
    expect(useThemeStore.getState().resolved).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);

    setPrefersDark(false);
    useThemeStore.getState().setMode('dark');
    expect(useThemeStore.getState().resolved).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
  });

  it('writes the choice where the next launch will look for it', () => {
    useThemeStore.getState().setMode('dark');

    // The store's initialiser calls readStoredMode(), so this is the whole
    // reload contract: whatever is written here is what a fresh store resolves
    // to. Asserting the key holds 'dark' alone would pass even if the store
    // were reading a different key than it writes.
    expect(window.localStorage.getItem('wc.theme')).toBe('dark');
    expect(readStoredMode()).toBe('dark');
  });

  it('ignores a stored value it does not recognise', () => {
    window.localStorage.setItem('wc.theme', 'chartreuse');
    // Not a crash and not a light-mode lockout: an unknown value falls back to
    // following the system, which is the safe default.
    expect(readStoredMode()).toBe('system');
  });

  it('cycles light -> dark -> system -> light', () => {
    // The store is a module-level singleton, so the mode carries over between
    // cases. Pin the start rather than assuming it.
    useThemeStore.setState({ mode: 'light', resolved: 'light' });

    const seen: ThemeMode[] = [];
    for (let i = 0; i < 4; i += 1) {
      seen.push(useThemeStore.getState().mode);
      useThemeStore.getState().cycle();
    }
    // The point of the three-state cycle: `system` is reachable from the
    // toggle. A boolean light/dark switch could not get the user back to it.
    expect(seen).toEqual(['light', 'dark', 'system', 'light']);
  });

  it('survives a window with localStorage disabled', () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    expect(() => useThemeStore.getState().setMode('dark')).not.toThrow();
    // The theme still applies; only the remembering is lost.
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    setItem.mockRestore();
  });
});

describe('initTheme', () => {
  afterEach(() => {
    document.documentElement.classList.remove('dark');
  });

  const DARK_QUERY = '(prefers-color-scheme: dark)';

  it('follows the OS only while the mode is `system`', () => {
    const mq = listeners();
    window.localStorage.clear();

    useThemeStore.setState({ mode: 'system', resolved: 'light' });
    const stop = initTheme();

    mq.set(DARK_QUERY, true);
    expect(useThemeStore.getState().resolved).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);

    // Now pin it, and the OS must no longer be able to move it.
    useThemeStore.getState().setMode('light');
    mq.set(DARK_QUERY, true);
    expect(useThemeStore.getState().resolved).toBe('light');

    stop();
  });

  it('returns an unsubscribe function that actually detaches the listener', () => {
    const mq = listeners();
    window.localStorage.clear();
    useThemeStore.setState({ mode: 'system', resolved: 'light' });

    const stop = initTheme();
    expect(mq.listeners.get(DARK_QUERY)?.size).toBe(1);
    stop();
    expect(mq.listeners.get(DARK_QUERY)?.size).toBe(0);
  });
});

describe('app shell', () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.classList.remove('dark');
    setPrefersDark(false);
    useThemeStore.setState({ mode: 'system', resolved: 'light', sidebarCollapsed: false });
    mockBackend({
      get_app_version: () => '0.1.0',
      get_device_fingerprint: () => '6APnKHTwEwIWXA/Yg6xBdA==',
      list_paired_devices: () => [makePairedDevice()],
    });
  });

  it('renders the sidebar at the window width the app actually ships', () => {
    // Regression guard for the bug this replaces: the sidebar was
    // `hidden lg:flex`, and `lg` is 1024px, and the window's default width was
    // 800px. At the app's own default size the entire navigation was invisible
    // and the hamburger replacing it had no click handler.
    renderLayout();

    const sidebar = screen.getByTestId('sidebar');
    expect(sidebar).toBeInTheDocument();
    // No responsive hiding. If someone reintroduces `hidden lg:flex`, or
    // `hidden md:flex`, this is what notices.
    expect(sidebar.className).not.toMatch(/(^|\s)hidden(\s|$)/);
    expect(sidebar.className).not.toMatch(/\b(sm|md|lg|xl|2xl):/);

    for (const name of ['Workspaces', 'Devices', 'Settings']) {
      expect(screen.getByRole('link', { name })).toBeInTheDocument();
    }
  });

  it('the hamburger it replaced is gone rather than left doing nothing', () => {
    renderLayout();
    // The old markup shipped a `Toggle menu` button wired to no handler at all.
    expect(screen.queryByLabelText('Toggle menu')).not.toBeInTheDocument();
  });

  it('collapses the sidebar to an icon rail on request, and remembers it', () => {
    renderLayout();

    fireEvent.click(screen.getByRole('button', { name: 'Hide sidebar' }));
    expect(screen.getByTestId('sidebar')).toHaveAttribute('data-collapsed', 'true');
    // Still present and still navigable — collapsed, not gone.
    expect(screen.getByRole('link', { name: 'Workspaces' })).toBeInTheDocument();
    expect(window.localStorage.getItem('wc.sidebar.collapsed')).toBe('true');

    fireEvent.click(screen.getByRole('button', { name: 'Show sidebar' }));
    expect(screen.getByTestId('sidebar')).toHaveAttribute('data-collapsed', 'false');
  });

  it('offers all three appearance modes and switches on click', () => {
    renderLayout();
    const group = screen.getByRole('radiogroup', { name: 'Appearance' });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole('radio')).toHaveLength(3);

    fireEvent.click(screen.getByRole('radio', { name: /Dark/ }));
    expect(useThemeStore.getState().mode).toBe('dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);

    fireEvent.click(screen.getByRole('radio', { name: /Light/ }));
    expect(useThemeStore.getState().mode).toBe('light');
    expect(document.documentElement.classList.contains('dark')).toBe(false);
  });

  it('marks the active appearance mode for assistive tech', () => {
    renderLayout();
    const dark = screen.getByRole('radio', { name: /Dark/ });
    expect(dark).toHaveAttribute('aria-checked', 'false');
    fireEvent.click(dark);
    expect(dark).toHaveAttribute('aria-checked', 'true');
  });

  it('shows real values in the status bar, not a hardcoded pill', async () => {
    renderLayout();

    // The previous status bar read "Local Mode" with a green dot, unconditionally,
    // on every screen, whether or not anything was listening. These come off the
    // backend, so they are not there on the first synchronous render.
    await waitFor(() => {
      expect(screen.getByTestId('status-version')).toHaveTextContent('v0.1.0');
    });
    expect(screen.getByTestId('status-paired')).toHaveTextContent('1 paired');

    // The fingerprint is a base64 public key, too long for a status bar, so it
    // is shortened for display and the full key goes in the tooltip. The
    // tooltip is the point: a bar that only ever shows "6APnKHTw…dA==" leaves
    // the user with no way to get the real key out of the app.
    const short = await screen.findByTitle('This device: 6APnKHTwEwIWXA/Yg6xBdA==');
    expect(short).toHaveTextContent('6APnKHTw…dA==');
    // Shortened means shortened: the ellipsis must not be the whole value.
    expect(short.textContent).toHaveLength('6APnKHTw…dA=='.length);
  });

  it('degrades to a dash when the backend cannot answer', async () => {
    mockBackend({}); // no handlers at all
    renderLayout();
    await waitFor(() => {
      expect(screen.getByTestId('status-version')).toHaveTextContent('—');
    });
    expect(screen.getByTestId('status-paired')).toHaveTextContent('— paired');
  });

  it('puts the page title in the toolbar on a section and on a step', () => {
    // The toolbar owns the only <h1> in the app, on every route. A section and a
    // step differ only in whether a back button precedes it.
    const { unmount } = renderLayout('/workspaces');
    expect(screen.getByTestId('toolbar-title').tagName).toBe('H1');
    expect(screen.getByTestId('toolbar-title')).toHaveTextContent('Workspaces');
    unmount();

    renderLayout('/preflight/ws-1');
    expect(screen.getByTestId('toolbar-title').tagName).toBe('H1');
    expect(screen.getByTestId('toolbar-title')).toHaveTextContent('Preflight Check');
  });

  it('offers a way back out of a step, which the sidebar alone could not be', () => {
    renderLayout('/preflight/ws-1');
    // Preflight is a step inside Workspaces, not a section. With the sidebar
    // hidden there was no way back at all.
    expect(screen.getByRole('button', { name: 'Back to workspaces' })).toBeInTheDocument();
  });

  it('does not offer a back button on the three top-level sections', () => {
    renderLayout('/settings');
    expect(screen.queryByRole('button', { name: 'Back to workspaces' })).not.toBeInTheDocument();
  });
});
