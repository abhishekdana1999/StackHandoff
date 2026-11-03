/**
 * End-to-end verification that the app talks to a locally running cloud API.
 *
 * The rest of the suite proves the pieces (the client's envelope handling, the
 * screen's states) against mocks. This script proves the seam those tests
 * cannot: a real browser, loading the real dev-server bundle, with the real
 * `cloudApi` module, calling the real NestJS service over CORS. It is the only
 * check that would catch a wrong base URL, a missing auth header, or a CORS
 * rejection, because every other test stubs the transport.
 *
 * Run the two servers first, then this:
 *
 *   (backend)  npm run dev
 *   (app)      npm run dev
 *   (app)      node scripts/verify-cloud.mjs
 *
 * Journey: register (+auto login) → open a PayPal checkout for Basic (a real
 * sandbox order) → exercise the `?paypal=cancelled` return → grant Basic via
 * the dev-only record upgrade (headless Chrome cannot complete a payment) so
 * the device step can run → register this device → sign out. Prints PASS or
 * exits non-zero with the failed step.
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const puppeteer = require('puppeteer-core');

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const APP = process.env.STACKHANDOFF_APP_URL || 'http://localhost:1420';
const API = process.env.STACKHANDOFF_API_URL || 'http://localhost:3000';

const email = `e2e+${Date.now()}@stackhandoff.dev`;
const password = 'Str0ng!Pass';

function log(step) {
  process.stdout.write(`  → ${step}\n`);
}

async function clickButton(page, text) {
  const clicked = await page.evaluate((needle) => {
    const buttons = [...document.querySelectorAll('button')];
    const el = buttons.find(
      (b) => b.getAttribute('role') !== 'tab' && b.textContent.trim().includes(needle),
    );
    if (!el) return false;
    el.click();
    return true;
  }, text);
  if (!clicked) throw new Error(`No button containing "${text}"`);
}

async function clickTab(page, text) {
  const clicked = await page.evaluate((needle) => {
    const el = [...document.querySelectorAll('button[role="tab"]')].find((b) =>
      b.textContent.trim().includes(needle),
    );
    if (!el) return false;
    el.click();
    return true;
  }, text);
  if (!clicked) throw new Error(`No tab labelled "${text}"`);
}

const waitForText = (page, text, timeout = 15000) =>
  page.waitForFunction(
    (t) => document.body && document.body.innerText.includes(t),
    { timeout, polling: 150 },
    text,
  );

const hasText = (page, text) =>
  page.evaluate((t) => document.body.innerText.includes(t), text);

async function main() {
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: 'new',
    args: ['--no-sandbox'],
  });

  const consoleErrors = [];
  const failedRequests = [];
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });

  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });
  page.on('pageerror', (err) => consoleErrors.push(String(err)));
  page.on('requestfailed', (req) => {
    // Aborted requests are routine: React StrictMode mounts effects twice and
    // React Query cancels the first plan fetch, which surfaces as ERR_ABORTED.
    const errorText = req.failure()?.errorText ?? '';
    if (errorText === 'net::ERR_ABORTED') return;
    failedRequests.push(`${req.method()} ${req.url()} — ${errorText}`);
  });

  try {
    log(`Opening ${APP}/cloud`);
    await page.goto(`${APP}/cloud`, { waitUntil: 'domcontentloaded' });

    log('Waiting for plans from the live API');
    await waitForText(page, 'Basic');
    if (!(await hasText(page, '$9.99/mo'))) {
      throw new Error('Plan prices are not the backend values (expected $9.99/mo for Basic)');
    }
    if (!(await hasText(page, 'Create account'))) {
      throw new Error('Signed-out pricing/auth page did not render');
    }

    log(`Registering ${email}`);
    await clickTab(page, 'Create account');
    await page.waitForSelector('#cloud-register-email');
    await page.type('#cloud-name', 'E2E Verifier');
    await page.type('#cloud-register-email', email);
    await page.type('#cloud-register-password', password);
    await clickButton(page, 'Create account');

    log('Waiting for the account to appear (register + auto login)');
    await waitForText(page, 'Sign out');
    if (!(await hasText(page, email))) throw new Error('Signed-in account email not shown');

    log('Opening a PayPal checkout for Basic');
    await waitForText(page, 'Switch to Basic');
    const orderReq = page.waitForResponse(
      (r) =>
        r.url().includes('/payments/paypal/orders') &&
        r.request().method() === 'POST',
      { timeout: 20000 },
    );
    await clickButton(page, 'Switch to Basic');
    const orderRes = await orderReq;
    if (orderRes.status() !== 201) {
      throw new Error(`Checkout order returned HTTP ${orderRes.status()}`);
    }
    const orderBody = await orderRes.json();
    const approvalUrl = orderBody?.data?.approvalUrl ?? '';
    if (!approvalUrl.includes('sandbox.paypal.com')) {
      throw new Error(`Approval URL is not a sandbox link: ${approvalUrl}`);
    }
    if (orderBody?.data?.priceCents !== 999) {
      throw new Error(`Checkout did not use the catalog price (got ${orderBody?.data?.priceCents})`);
    }
    log(`Approval link received (${approvalUrl.split('?')[0]})`);
    await waitForText(page, 'Payment window opened');

    log('Exercising the cancelled return (no charge)');
    await page.goto(`${APP}/cloud?paypal=cancelled`, { waitUntil: 'domcontentloaded' });
    await waitForText(page, 'Checkout cancelled');
    if (page.url().includes('paypal=')) {
      throw new Error('Cancelled-return URL was not cleaned up');
    }

    log('Granting Basic via the dev-only record upgrade (headless cannot pay)');
    const token = await page.evaluate(() => {
      try {
        return JSON.parse(window.localStorage.getItem('wc.cloud.session') ?? '{}').accessToken ?? '';
      } catch {
        return '';
      }
    });
    if (!token) throw new Error('Could not read the persisted session token');
    const upReq = await fetch(`${API}/payments/upgrade`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify({ planId: 'basic' }),
    });
    if (!upReq.ok) throw new Error(`Dev upgrade returned HTTP ${upReq.status}`);
    await page.goto(`${APP}/cloud`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(
      () =>
        document.body.innerText.includes('Plan limits') &&
        document.body.innerText.includes('Cloud relay') &&
        document.body.innerText.includes('Enabled'),
      { timeout: 15000, polling: 150 },
    );
    log('Basic limits are live in the UI');

    log('Registering this device against the cloud');
    await page.waitForSelector('#cloud-device-name');
    await page.type('#cloud-device-name', 'E2E Mac');
    await clickButton(page, 'Register');
    await waitForText(page, 'E2E Mac');
    await page.waitForFunction(
      () => document.body.innerText.includes('registered'),
      { timeout: 15000, polling: 150 },
    );
    log('Cloud device appeared in the list');

    log('Signing out');
    await clickButton(page, 'Sign out');
    await page.waitForFunction(
      () => [...document.querySelectorAll('button[role="tab"]')].some((b) =>
        b.textContent.includes('Sign in'),
      ),
      { timeout: 15000, polling: 150 },
    );

    if (failedRequests.length > 0) {
      throw new Error(`Failed network requests:\n${failedRequests.join('\n')}`);
    }
    if (consoleErrors.length > 0) {
      throw new Error(`Console errors:\n${consoleErrors.join('\n')}`);
    }

    process.stdout.write(`\nPASS — full cloud journey verified against ${API}\n`);
    process.stdout.write(
      `     (account ${email}, checkout opened, cancelled-return handled, device registered, signed out)\n`,
    );
    process.exitCode = 0;
  } catch (error) {
    process.stdout.write(`\nFAIL — ${error instanceof Error ? error.message : String(error)}\n`);
    if (failedRequests.length) {
      process.stdout.write(`Failed requests:\n${failedRequests.join('\n')}\n`);
    }
    if (consoleErrors.length) {
      process.stdout.write(`Console errors:\n${consoleErrors.join('\n')}\n`);
    }
    process.exitCode = 1;
  } finally {
    await browser.close();
  }
}

main();