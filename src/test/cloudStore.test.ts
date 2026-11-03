/**
 * The persisted cloud session.
 *
 * The interesting behaviour is the round trip: what `setSession` writes is what
 * `readStoredSession` must read back, because a mismatch (write `accessToken`,
 * read `token`) would let the app launch "signed in" while every request 401s.
 * The other half is refusal: a session without a token, or a corrupt stored
 * value, must resolve to signed-out rather than to a half-session.
 */

import { beforeEach, describe, expect, it } from 'vitest';
import { readStoredSession, useCloudStore } from '@store/useCloudStore';
import type { CloudSession } from '@lib/cloudApi';

const SESSION: CloudSession = {
  user: { id: 'u1', email: 'a@b.com', name: 'Ada', plan: 'basic' },
  accessToken: 'access-1',
  refreshToken: 'refresh-1',
  expiresIn: 3600,
  tokenType: 'bearer',
};

beforeEach(() => {
  window.localStorage.clear();
  useCloudStore.setState({ session: null });
});

describe('useCloudStore', () => {
  it('starts signed out', () => {
    expect(readStoredSession()).toBeNull();
    expect(useCloudStore.getState().session).toBeNull();
  });

  it('persists a session and reads it back', () => {
    useCloudStore.getState().setSession(SESSION);

    expect(useCloudStore.getState().session).toMatchObject({
      accessToken: 'access-1',
      refreshToken: 'refresh-1',
      user: { id: 'u1', plan: 'basic' },
    });
    expect(readStoredSession()).toMatchObject({
      accessToken: 'access-1',
      refreshToken: 'refresh-1',
      user: { email: 'a@b.com' },
    });
  });

  it('refuses to persist a session with no token', () => {
    useCloudStore.getState().setSession({ ...SESSION, accessToken: null });

    expect(useCloudStore.getState().session).toBeNull();
    expect(readStoredSession()).toBeNull();
  });

  it('updates the cached user without disturbing the tokens', () => {
    useCloudStore.getState().setSession(SESSION);
    useCloudStore.getState().setUser({ ...SESSION.user, plan: 'pro' });

    expect(useCloudStore.getState().session).toMatchObject({
      accessToken: 'access-1',
      user: { plan: 'pro' },
    });
  });

  it('clears the session from memory and storage', () => {
    useCloudStore.getState().setSession(SESSION);
    useCloudStore.getState().clearSession();

    expect(useCloudStore.getState().session).toBeNull();
    expect(readStoredSession()).toBeNull();
  });

  it('treats a corrupt stored value as signed out', () => {
    window.localStorage.setItem('wc.cloud.session', '{ not json');
    expect(readStoredSession()).toBeNull();
  });

  it('treats a stored value without a token as signed out', () => {
    window.localStorage.setItem('wc.cloud.session', JSON.stringify({ user: { id: 'u1' } }));
    expect(readStoredSession()).toBeNull();
  });
});
