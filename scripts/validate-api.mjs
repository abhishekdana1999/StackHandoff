/**
 * Live API validator.
 *
 * Exercises every route the backend exposes against a deployed base URL:
 * public endpoints, the auth lifecycle, and a full authenticated CRUD flow
 * (users, devices, workspaces, transfers, payments). Prints a PASS/FAIL table
 * and exits non-zero if anything unexpected happens.
 *
 *   node scripts/validate-api.mjs https://dev.api.stackhandoff.com
 *
 * Read-only mode (public + auth-guard checks only, no data created) is used
 * for production base URLs so a validation run never seeds prod.
 */
const BASE = (process.argv[2] ?? 'http://localhost:3000').replace(/\/+$/, '');
const READ_ONLY = process.argv.includes('--read-only');

const results = [];
const record = (name, expected, actual, note = '') => {
  const want = Array.isArray(expected) ? expected : [expected];
  const ok = want.includes(actual);
  results.push({ name, ok, expected: want.join('/'), actual, note });
  const tag = ok ? 'PASS' : 'FAIL';
  console.log(`${tag}  ${String(actual).padEnd(4)} ${name}${note ? `  (${note})` : ''}`);
  return ok;
};

let token = null;
let refreshToken = null;

async function call(name, method, path, { body, auth = true, expect, bearer } = {}) {
  const headers = { Accept: 'application/json' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const useToken = bearer ?? (auth ? token : null);
  if (useToken) headers.Authorization = `Bearer ${useToken}`;
  let res;
  let text = '';
  try {
    res = await fetch(`${BASE}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    text = await res.text();
  } catch (error) {
    record(name, expect, 'ERR', error.message.slice(0, 60));
    return { status: 0, json: null };
  }
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { /* non-JSON */ }
  record(name, expect, res.status, json?.message && typeof json.message === 'string' && !json.message.startsWith('Success') ? json.message.slice(0, 50) : '');
  return { status: res.status, json };
}

const stamp = Date.now();
const email = `validate+${stamp}@stackhandoff.dev`;
const password = 'Validate123!';

console.log(`\n=== Validating ${BASE}${READ_ONLY ? '  [read-only]' : ''} ===\n`);

// ---- Public / meta ----
await call('GET /', 'GET', '/', { auth: false, expect: 200 });
await call('GET /health', 'GET', '/health', { auth: false, expect: 200 });
await call('GET /payments/plans', 'GET', '/payments/plans', { auth: false, expect: 200 });
await call('GET /payments/paypal/config', 'GET', '/payments/paypal/config', { auth: false, expect: 200 });
await call('GET /definitely-not-a-route', 'GET', '/definitely-not-a-route', { auth: false, expect: 404 });

// ---- Auth guards (unauthenticated must be 401) ----
for (const [name, method, path] of [
  ['GET /auth/me', 'GET', '/auth/me'],
  ['GET /users/me', 'GET', '/users/me'],
  ['GET /devices', 'GET', '/devices'],
  ['GET /workspaces', 'GET', '/workspaces'],
  ['GET /transfers', 'GET', '/transfers'],
  ['GET /payments/subscription', 'GET', '/payments/subscription'],
]) {
  await call(`guard: ${name}`, method, path, { auth: false, expect: 401 });
}

if (READ_ONLY) {
  finish();
} else {
  await fullFlow();
  finish();
}

async function fullFlow() {
  // ---- Auth lifecycle ----
  await call('POST /auth/register', 'POST', '/auth/register', {
    auth: false, expect: 201, body: { email, password, name: 'Validation Bot' },
  });
  const login = await call('POST /auth/login', 'POST', '/auth/login', {
    auth: false, expect: 200, body: { email, password },
  });
  const loginData = login.json?.data ?? login.json;
  token = loginData?.session?.accessToken ?? loginData?.accessToken;
  refreshToken = loginData?.session?.refreshToken ?? loginData?.refreshToken;
  record('login returned access token', 1, token ? 1 : 0);
  if (!token) { console.log('\nNo token — cannot continue authenticated flow.'); return; }

  if (refreshToken) {
    await call('POST /auth/refresh', 'POST', '/auth/refresh', {
      auth: false, expect: 200, body: { refreshToken },
    });
  }
  await call('GET /auth/me', 'GET', '/auth/me', { expect: 200 });

  // ---- Users ----
  const me = await call('GET /users/me', 'GET', '/users/me', { expect: 200 });
  const myId = me.json?.data?.id;
  await call('PATCH /users/me', 'PATCH', '/users/me', { expect: 200, body: { name: 'Validation Bot II' } });
  await call('GET /users/me/settings', 'GET', '/users/me/settings', { expect: 200 });
  await call('PATCH /users/me/settings', 'PATCH', '/users/me/settings', { expect: 200, body: { theme: 'dark', notifications: true } });

  // Free users are correctly blocked from cloud sync; upgrade so the gated
  // device/transfer routes can be exercised, exactly as a paying user would.
  await call('POST /payments/upgrade (unlock cloud)', 'POST', '/payments/upgrade', { expect: 200, body: { planId: 'basic' } });

  // A second user to add/remove as a workspace member.
  const memberEmail = `validate-m+${stamp}@stackhandoff.dev`;
  await call('POST /auth/register (member)', 'POST', '/auth/register', { auth: false, expect: 201, body: { email: memberEmail, password, name: 'Member Bot' } });
  const mlogin = await call('POST /auth/login (member)', 'POST', '/auth/login', { auth: false, expect: 200, body: { email: memberEmail, password } });
  const mdata = mlogin.json?.data ?? mlogin.json;
  const memberToken = mdata?.session?.accessToken ?? mdata?.accessToken;
  const memberMe = await call('GET /users/me (member)', 'GET', '/users/me', { expect: 200, bearer: memberToken });
  const memberId = memberMe.json?.data?.id;

  // ---- Devices ----
  const dev = await call('POST /devices', 'POST', '/devices', {
    expect: [200, 201],
    body: {
      deviceName: 'Validation Mac', deviceType: 'laptop', os: 'macOS',
      osVersion: '15.0', appVersion: '1.0.0', noiseKeyBase64: 'A'.repeat(32),
    },
  });
  const deviceId = dev.json?.data?.id;
  await call('GET /devices', 'GET', '/devices', { expect: 200 });
  if (deviceId) {
    await call('GET /devices/:id', 'GET', `/devices/${deviceId}`, { expect: 200 });
    await call('PATCH /devices/:id', 'PATCH', `/devices/${deviceId}`, { expect: 200, body: { deviceName: 'Validation Mac II' } });
    await call('POST /devices/:id/heartbeat', 'POST', `/devices/${deviceId}/heartbeat`, { expect: 200 });
  }

  // ---- Workspaces ----
  const ws = await call('POST /workspaces', 'POST', '/workspaces', {
    expect: [200, 201], body: { name: 'Validation Workspace', description: 'created by validator' },
  });
  const wsId = ws.json?.data?.id;
  await call('GET /workspaces', 'GET', '/workspaces', { expect: 200 });
  if (wsId) {
    await call('GET /workspaces/:id', 'GET', `/workspaces/${wsId}`, { expect: 200 });
    await call('PATCH /workspaces/:id', 'PATCH', `/workspaces/${wsId}`, { expect: 200, body: { name: 'Validation Workspace II' } });
    await call('GET /workspaces/:id/members', 'GET', `/workspaces/${wsId}/members`, { expect: 200 });
    if (memberId) {
      const added = await call('POST /workspaces/:id/members', 'POST', `/workspaces/${wsId}/members`, {
        expect: [200, 201], body: { userId: memberId, email: memberEmail, role: 'member' },
      });
      const memberRecordId = added.json?.data?.id;
      await call('GET /workspaces/:id/members (after add)', 'GET', `/workspaces/${wsId}/members`, { expect: 200 });
      if (memberRecordId) {
        await call('DELETE /workspaces/:id/members/:memberId', 'DELETE', `/workspaces/${wsId}/members/${memberRecordId}`, { expect: 200 });
      }
    }
  }

  // ---- Transfers (full lifecycle) ----
  let transferId = null;
  const extraTransferIds = [];
  if (wsId && deviceId) {
    // Happy path: pending -> uploading -> ready -> restoring -> completed.
    const tr = await call('POST /transfers', 'POST', '/transfers', {
      expect: [200, 201],
      body: {
        workspaceId: wsId, workspaceName: 'Validation Workspace',
        sourceDeviceId: deviceId, sizeBytes: 1048576, fileCount: 12,
        useCloudRelay: true,
      },
    });
    transferId = tr.json?.data?.id;
    await call('GET /transfers', 'GET', '/transfers', { expect: 200 });
    if (transferId) {
      await call('GET /transfers/:id', 'GET', `/transfers/${transferId}`, { expect: 200 });
      await call('POST /transfers/:id/status (uploading)', 'POST', `/transfers/${transferId}/status`, { expect: 200, body: { status: 'uploading' } });
      await call('POST /transfers/:id/status (ready)', 'POST', `/transfers/${transferId}/status`, { expect: 200, body: { status: 'ready' } });
      await call('POST /transfers/:id/status (restoring)', 'POST', `/transfers/${transferId}/status`, { expect: 200, body: { status: 'restoring' } });
      await call('POST /transfers/:id/complete', 'POST', `/transfers/${transferId}/complete`, { expect: 200 });
      await call('POST /transfers/:id/complete (again -> 409)', 'POST', `/transfers/${transferId}/complete`, { expect: 409 });
    }

    const mkTransfer = (label) => call(label, 'POST', '/transfers', {
      expect: [200, 201],
      body: { workspaceId: wsId, workspaceName: 'Validation Workspace', sourceDeviceId: deviceId, sizeBytes: 2048, fileCount: 2, useCloudRelay: true },
    });

    // Failure path: fail -> retry -> revoke.
    const tr2 = await mkTransfer('POST /transfers (2)');
    const id2 = tr2.json?.data?.id;
    if (id2) {
      extraTransferIds.push(id2);
      await call('POST /transfers/:id/fail', 'POST', `/transfers/${id2}/fail`, { expect: 200, body: { failureReason: 'validation' } });
      await call('POST /transfers/:id/retry', 'POST', `/transfers/${id2}/retry`, { expect: 200 });
      await call('POST /transfers/:id/revoke', 'POST', `/transfers/${id2}/revoke`, { expect: 200 });
    }

    // Cancellation path.
    const tr3 = await mkTransfer('POST /transfers (3)');
    const id3 = tr3.json?.data?.id;
    if (id3) {
      extraTransferIds.push(id3);
      await call('POST /transfers/:id/cancel', 'POST', `/transfers/${id3}/cancel`, { expect: 200 });
    }

    // Illegal transition must be a clean 409.
    const tr4 = await mkTransfer('POST /transfers (4)');
    const id4 = tr4.json?.data?.id;
    if (id4) {
      extraTransferIds.push(id4);
      await call('POST /transfers/:id/status (illegal)', 'POST', `/transfers/${id4}/status`, { expect: 409, body: { status: 'completed' } });
    }
  } else {
    record('transfer prerequisites (workspace+device)', 1, 0, 'skipped');
  }

  // ---- Payments ----
  await call('GET /payments/subscription', 'GET', '/payments/subscription', { expect: 200 });
  await call('GET /payments/limits', 'GET', '/payments/limits', { expect: 200 });
  await call('POST /payments/cancel', 'POST', '/payments/cancel', { expect: 200 });
  const order = await call('POST /payments/paypal/orders', 'POST', '/payments/paypal/orders', { expect: [200, 201], body: { planId: 'basic' } });
  const orderId = order.json?.data?.orderId;
  if (orderId) {
    // Un-approved order capture must be a clean 409 (the 422 fix), never 502.
    await call('POST /payments/paypal/orders/:id/capture (unapproved)', 'POST', `/payments/paypal/orders/${orderId}/capture`, { expect: 409 });
  }

  // ---- Cleanup ----
  if (deviceId) {
    await call('POST /devices/:id/revoke', 'POST', `/devices/${deviceId}/revoke`, { expect: 200 });
    await call('DELETE /devices/:id', 'DELETE', `/devices/${deviceId}`, { expect: 200 });
  }
  if (transferId) await call('DELETE /transfers/:id', 'DELETE', `/transfers/${transferId}`, { expect: 200 });
  for (const id of extraTransferIds) await call('DELETE /transfers/:id (extra)', 'DELETE', `/transfers/${id}`, { expect: 200 });
  if (wsId) await call('DELETE /workspaces/:id', 'DELETE', `/workspaces/${wsId}`, { expect: 200 });

  await call('POST /auth/logout', 'POST', '/auth/logout', { expect: 200 });
  await call('DELETE /users/me', 'DELETE', '/users/me', { expect: 200 });
  if (memberToken) {
    await call('DELETE /users/me (member)', 'DELETE', '/users/me', { expect: 200, bearer: memberToken });
  }
}

function finish() {
  const failed = results.filter((r) => !r.ok);
  console.log(`\n=== ${results.length - failed.length}/${results.length} passed  (${BASE}) ===`);
  if (failed.length) {
    console.log('\nFailures:');
    for (const f of failed) console.log(`  ${f.name}: expected ${f.expected}, got ${f.actual}${f.note ? ` — ${f.note}` : ''}`);
    process.exitCode = 1;
  }
}
