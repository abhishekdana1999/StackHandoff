/**
 * Test setup.
 *
 * The important part of this file is the Tauri IPC stub. Tests run in jsdom,
 * where `@tauri-apps/api/core`'s `invoke` would try to reach a window that does
 * not exist and fail with a message about `__TAURI_INTERNALS__`. A test that
 * stubs the module itself would pass while the real call path stayed broken, so
 * instead the internals object is installed: every `invoke` in the app goes
 * through the same function, and the fake backend is reachable from any test.
 */

import '@testing-library/jest-dom';
import { afterEach, beforeEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';
import { backend, type Call } from './fakeBackend';

/**
 * jsdom does not implement `window.localStorage` or `window.matchMedia`, and the
 * theme store reads both: the mode is remembered in storage and `system` is
 * resolved through a media query. Without these, every theme test failed on
 * `Cannot read properties of undefined (reading 'clear')` — which says nothing
 * about the theme, and a suite that dies that way teaches you to ignore it.
 *
 * The fakes are installed per test file rather than globally so that a test
 * which needs to change what the OS "reports" can replace them; see
 * `theme.test.tsx`.
 */
const store = new Map<string, string>();
beforeEach(() => {
  Object.defineProperty(window, 'localStorage', {
    writable: true,
    configurable: true,
    value: {
      getItem: (k: string) => (store.has(k) ? store.get(k)! : null),
      setItem: (k: string, v: string) => void store.set(k, String(v)),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
    },
  });

  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    configurable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });
});

declare global {
  interface Window {
    __TAURI_INTERNALS__?: {
      invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>;
      transformCallback?: (cb: (value: unknown) => void, once?: boolean) => number;
      metadata?: Record<string, unknown>;
    };
  }
}

window.__TAURI_INTERNALS__ = {
  // The signature is Tauri's own, so `args` arrives as `unknown` and is narrowed
  // here. Widening the declared parameter instead would make this object
  // assignable to the real one while quietly disagreeing about what a command
  // receives.
  invoke: async (cmd: string, args?: unknown): Promise<unknown> => {
    const received = (args ?? {}) as Record<string, unknown>;
    const call: Call = { cmd, args: received };
    backend.calls.push(call);
    const handler = backend.handlers[cmd];
    if (!handler) {
      // Mirrors the real behaviour: an unregistered command is an error the
      // caller has to handle, not a silent `undefined`. A test that invokes a
      // command nobody implemented should fail loudly here rather than render a
      // screen full of blanks.
      throw new Error(
        `No fake handler for '${cmd}'. Registered: ${Object.keys(backend.handlers).join(', ') || '(none)'}`
      );
    }
    return handler(received);
  },
  transformCallback: (cb: (value: unknown) => void) => {
    callbacks.set(nextCallbackId, cb);
    nextCallbackId += 1;
    return nextCallbackId - 1;
  },
  metadata: {},
};

const callbacks = new Map<number, (value: unknown) => void>();
let nextCallbackId = 1;

afterEach(() => {
  cleanup();
  backend.reset();
  callbacks.clear();
  nextCallbackId = 1;
  vi.restoreAllMocks();
});
