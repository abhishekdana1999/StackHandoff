/**
 * The signed-in cloud session, kept in memory and mirrored to `localStorage`.
 *
 * Unlike the capture draft in `useAppStore`, this *is* persisted: a cloud
 * account is a property of the machine the user set up, and signing in again on
 * every launch would make the paid features feel broken. It is still not a
 * credential store in the security sense — the token is a short-lived bearer
 * token the backend can revoke, and the refresh token is what actually persists
 * the login.
 *
 * Kept out of TanStack Query because auth state is not a server resource with a
 * cache key; it is a single value every screen reads and two screens write
 * (`CloudScreen` on sign-in, the header on sign-out).
 */

import { create } from 'zustand';
import type { CloudSession, CloudUser } from '@lib/cloudApi';

const STORAGE_KEY = 'wc.cloud.session';

/** What survives a restart: the tokens and the cached user, and nothing else. */
export interface PersistedCloudSession {
  accessToken: string;
  refreshToken: string | null;
  user: CloudUser;
}

/**
 * Reads the persisted session, tolerating anything unreadable.
 *
 * A half-written or hand-edited value must not crash the app on launch, so a
 * parse failure is treated as "signed out" rather than surfaced. The `shape`
 * check is the part that matters: `JSON.parse` happily returns `null` or `{}`,
 * and a session object missing `accessToken` would let `isAuthenticated` report
 * true while every request 401s.
 */
export function readStoredSession(): PersistedCloudSession | null {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<PersistedCloudSession>;
    if (
      parsed &&
      typeof parsed.accessToken === 'string' &&
      parsed.accessToken.length > 0 &&
      parsed.user &&
      typeof parsed.user.id === 'string'
    ) {
      return {
        accessToken: parsed.accessToken,
        refreshToken: parsed.refreshToken ?? null,
        user: parsed.user as CloudUser,
      };
    }
  } catch {
    // Storage disabled, or a corrupt value. Either way: signed out.
  }
  return null;
}

function writeStoredSession(session: PersistedCloudSession | null): void {
  try {
    if (session) {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(session));
    } else {
      window.localStorage.removeItem(STORAGE_KEY);
    }
  } catch {
    // Not remembering the session just means signing in again next launch.
  }
}

interface CloudState {
  session: PersistedCloudSession | null;
  /** Store a login/refresh result. A session without a token is not storable. */
  setSession: (session: CloudSession) => void;
  /** Replace the cached user after a profile or plan change. */
  setUser: (user: CloudUser) => void;
  clearSession: () => void;
}

export const useCloudStore = create<CloudState>((set, get) => ({
  session: readStoredSession(),

  setSession: (session) => {
    if (!session.accessToken) {
      // Registration in some Supabase configurations returns no session until
      // the email is confirmed. Refusing to persist it keeps the store honest:
      // there is nothing to authenticate with yet.
      return;
    }
    const persisted: PersistedCloudSession = {
      accessToken: session.accessToken,
      refreshToken: session.refreshToken,
      user: session.user,
    };
    writeStoredSession(persisted);
    set({ session: persisted });
  },

  setUser: (user) => {
    const current = get().session;
    if (!current) return;
    const persisted: PersistedCloudSession = { ...current, user };
    writeStoredSession(persisted);
    set({ session: persisted });
  },

  clearSession: () => {
    writeStoredSession(null);
    set({ session: null });
  },
}));
