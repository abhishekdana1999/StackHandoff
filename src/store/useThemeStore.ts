/**
 * Light / dark / system.
 *
 * This store exists because `darkMode: 'class'` in tailwind.config.js makes the
 * `.dark` class on `<html>` the only thing that switches the palette. Before
 * this, that class was defined in the stylesheet and never applied by anything,
 * so the entire dark half of the theme was dead CSS: the app always rendered
 * light regardless of the OS setting.
 *
 * Three modes rather than two, because "follow the system" is the correct
 * default for a desktop app but a user who has overridden it at 11pm does not
 * want it flipped back by a system change at 7am.
 *
 * The choice is persisted. Appearance is a property of the machine the user set
 * up, not of the session, so unlike the capture draft in `useAppStore` this
 * *is* worth keeping across restarts.
 */

import { create } from 'zustand';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

const STORAGE_KEY = 'wc.theme';
const SIDEBAR_KEY = 'wc.sidebar.collapsed';

function readStoredFlag(key: string, fallback: boolean): boolean {
  try {
    const raw = window.localStorage.getItem(key);
    if (raw === 'true') return true;
    if (raw === 'false') return false;
  } catch {
    // See the note in readStoredMode.
  }
  return fallback;
}

/**
 * The mode this machine last chose, or `system` if there isn't one.
 *
 * Exported because it is the other half of a contract worth stating out loud:
 * the store's *initialiser* calls this, so whatever `setMode` writes is exactly
 * what the next launch will read. Exposing it lets a test assert that round
 * trip, which is the part that can silently break (write the key, read a
 * different one) and which "the button changed the class" would never notice.
 */
export function readStoredMode(): ThemeMode {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (raw === 'light' || raw === 'dark' || raw === 'system') return raw;
  } catch {
    // A window with storage disabled (or a private window under some settings)
    // should still get a working theme, just not a remembered one.
  }
  return 'system';
}

function prefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  );
}

/**
 * Push the resolved theme onto the document.
 *
 * Also sets `color-scheme`, which is what makes the window's own scrollbars and
 * the text caret around form fields match the mode. Without it macOS keeps
 * drawing light scrollbars inside a dark window.
 */
export function applyTheme(resolved: ResolvedTheme): void {
  const root = document.documentElement;
  root.classList.toggle('dark', resolved === 'dark');
  root.style.colorScheme = resolved;
}

function resolve(mode: ThemeMode): ResolvedTheme {
  if (mode === 'system') return prefersDark() ? 'dark' : 'light';
  return mode;
}

interface ThemeState {
  mode: ThemeMode;
  /** What `mode` currently means, with `system` already looked up. */
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
  /**
   * Light -> dark -> system -> light.
   *
   * The quick toggle in the sidebar. Deliberately a three-state cycle rather
   * than a boolean: a two-state toggle cannot express "just follow the system",
   * so a user who wanted that could not get back to it without Settings.
   */
  cycle: () => void;

  /**
   * Whether the sidebar is collapsed to its icon rail.
   *
   * The sidebar used to be hidden below Tailwind's `lg` breakpoint, which any
   * window narrower than 1024px hit — including the app's own default width. The
   * fix is to keep it present and let the user reclaim the space, so the choice
   * is a preference and is persisted like the theme.
   */
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  mode: readStoredMode(),
  resolved: 'light',
  sidebarCollapsed: readStoredFlag(SIDEBAR_KEY, false),

  setMode: (mode) => {
    const resolved = resolve(mode);
    try {
      window.localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      // Not being able to remember the choice is not worth failing over.
    }
    applyTheme(resolved);
    set({ mode, resolved });
  },

  cycle: () => {
    const order: ThemeMode[] = ['light', 'dark', 'system'];
    const next = order[(order.indexOf(get().mode) + 1) % order.length];
    get().setMode(next);
  },

  toggleSidebar: () => {
    const next = !get().sidebarCollapsed;
    try {
      window.localStorage.setItem(SIDEBAR_KEY, String(next));
    } catch {
      // Not being able to remember it just means it resets next launch.
    }
    set({ sidebarCollapsed: next });
  },
}));

/**
 * Apply the stored theme, and keep following the OS while in `system` mode.
 *
 * Called once from `main.tsx` before the first render. Returns an unsubscribe
 * function so a test can drive it without leaking a listener between cases.
 */
export function initTheme(): () => void {
  const { mode, setMode } = useThemeStore.getState();
  setMode(mode);

  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return () => {};
  }

  const query = window.matchMedia('(prefers-color-scheme: dark)');
  const onChange = () => {
    // Only follow the OS while the user has actually asked us to.
    if (useThemeStore.getState().mode === 'system') {
      const resolved: ResolvedTheme = query.matches ? 'dark' : 'light';
      applyTheme(resolved);
      useThemeStore.setState({ resolved });
    }
  };

  // Safari below 14 only has the deprecated API.
  if (typeof query.addEventListener === 'function') {
    query.addEventListener('change', onChange);
    return () => query.removeEventListener('change', onChange);
  }
  query.addListener(onChange);
  return () => query.removeListener(onChange);
}
