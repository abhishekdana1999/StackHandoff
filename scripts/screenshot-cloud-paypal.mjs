/**
 * Captures UI screenshots of every PayPal checkout state:
 *
 *   01  checkout button (the payment-click point)
 *   02  approval window opened (order created, PayPal prompt)
 *   02b the PayPal sandbox page itself
 *   03  cancelled return
 *   04  capture error return (real backend + real unapproved order → error)
 *   05  payment confirmed (success notice; the capture response is simulated,
 *       since completing a real sandbox payment needs a buyer login)
 *
 * Requires both dev servers running. Outputs PNGs into docs/screenshots/paypal/.
 */
import { createRequire } from 'node:module';
import { mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
const require = createRequire(import.meta.url);
const puppeteer = require('puppeteer-core');

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const APP = process.env.STACKHANDOFF_APP_URL || 'http://localhost:1420';
const OUT = join(
  dirname(fileURLToPath(import.meta.url)),
  '..',
  'docs',
  'screenshots',
  'paypal',
);

const email = `shots+${Date.now()}@stackhandoff.dev`;
const password = 'Str0ng!Pass';

function log(step) {
  process.stdout.write(`  → ${step}\n`);
}

async function clickButton(page, text) {
  const clicked = await page.evaluate((needle) => {
    const el = [...document.querySelectorAll('button')].find(
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

async function main() {
  mkdirSync(OUT, { recursive: true });
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: 'new',
    args: ['--no-sandbox'],
  });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 900 });

  const shot = (name) =>
    page.screenshot({ path: join(OUT, name), fullPage: false });

  /**
   * The app scrolls in an inner `<main>` (`overflow-y-auto`); the visible
   * viewport fold is otherwise static. `scrollIntoView` does not drive that
   * container in headless Chrome, so compute the offset ourselves.
   */
  const reveal = (needle) =>
    page.evaluate((n) => {
      const main = document.querySelector('main');
      // Deepest match: the root div's textContent contains the whole page, so
      // guard against ancestors by matching leaf text nodes only.
      const els = [...document.querySelectorAll('p, button, span')].filter(
        (node) => node.textContent.trim().includes(n),
      );
      const el = els[els.length - 1];
      if (!el) return 'not-found';
      if (main) {
        const top =
          el.getBoundingClientRect().top -
          main.getBoundingClientRect().top +
          main.scrollTop -
          main.clientHeight / 2;
        main.scrollTop = Math.max(0, top);
      } else {
        el.scrollIntoView({ block: 'center', inline: 'nearest' });
      }
      return el.tagName;
    }, needle);

  try {
    log(`Opening ${APP}/cloud`);
    await page.goto(`${APP}/cloud`, { waitUntil: 'domcontentloaded' });
    await waitForText(page, 'Basic');

    log(`Registering ${email}`);
    await clickTab(page, 'Create account');
    await page.waitForSelector('#cloud-register-email');
    await page.type('#cloud-name', 'Screenshot Verifier');
    await page.type('#cloud-register-email', email);
    await page.type('#cloud-register-password', password);
    await clickButton(page, 'Create account');
    await waitForText(page, 'Sign out');

    await waitForText(page, 'Switch to Basic');
    log('01 — checkout button (payment-click point)');
    await reveal('Switch to Basic');
    await new Promise((r) => setTimeout(r, 400));
    await shot('01-checkout-button.png');

    log('Opening the PayPal approval window');
    const orderReq = page.waitForResponse(
      (r) => r.url().includes('/payments/paypal/orders') && r.request().method() === 'POST',
      { timeout: 20000 },
    );
    await clickButton(page, 'Switch to Basic');

    const orderRes = await orderReq;
    const orderBody = await orderRes.json();
    const orderId = orderBody?.data?.orderId ?? '';
    const approvalUrl = orderBody?.data?.approvalUrl ?? '';
    log(`Order ${orderId} created; approval: ${approvalUrl}`);

    await waitForText(page, 'Payment window opened');
    log('02 — submitted: approval window prompt');
    await reveal('Payment window opened');
    await new Promise((r) => setTimeout(r, 400));
    await shot('02-approval-window-opened.png');

    // A scripted .click() is not a user gesture, so headless Chrome blocks the
    // window.open() popup. Load the approval link in its own tab instead.
    if (approvalUrl) {
      let paypalPage = null;
      try {
        log('02b — PayPal hosted checkout page');
        paypalPage = await browser.newPage();
        await paypalPage.setViewport({ width: 1280, height: 900 });
        await paypalPage.goto(approvalUrl, {
          waitUntil: 'domcontentloaded',
          timeout: 30000,
        });
        await new Promise((r) => setTimeout(r, 4000));
        await paypalPage.screenshot({
          path: join(OUT, '02b-paypal-hosted-checkout.png'),
        });
      } catch (paypalError) {
        log(`02b — PayPal page could not be captured (${paypalError.message})`);
      } finally {
        await paypalPage?.close();
      }
    } else {
      log('02b — no approval link to capture; skipped');
    }

    if (!orderId) throw new Error('Checkout response had no orderId');

    log('03 — cancelled return (real)');
    await page.goto(`${APP}/cloud?paypal=cancelled`, { waitUntil: 'domcontentloaded' });
    await waitForText(page, 'Checkout cancelled');
    await reveal('Checkout cancelled');
    await new Promise((r) => setTimeout(r, 400));
    await shot('03-cancelled-return.png');

    log('04 — error on capture of an un-approved order (real backend)');
    await page.goto(`${APP}/cloud?paypal=success&token=${orderId}`, {
      waitUntil: 'domcontentloaded',
    });
    await page.waitForFunction(
      () => document.body.innerText.includes('PayPal API error'),
      { timeout: 20000, polling: 200 },
    );
    // Give the error notice a beat to paint, then bring it into view.
    await new Promise((r) => setTimeout(r, 800));
    await reveal('PayPal API error');
    await new Promise((r) => setTimeout(r, 400));
    await shot('04-capture-error-return.png');

    log('05 — success notice (simulated COMPLETED capture response)');
    await page.setRequestInterception(true);
    page.on('request', (req) => {
      const url = req.url();
      if (req.method() === 'OPTIONS') {
        // The app is cross-origin to the API, and the capture POST carries an
        // Authorization header, so the browser sends a CORS preflight first.
        req.respond({
          status: 204,
          headers: {
            'access-control-allow-origin': '*',
            'access-control-allow-methods': 'GET,POST,OPTIONS',
            'access-control-allow-headers': 'Authorization, Content-Type',
          },
        });
      } else if (
        req.method() === 'POST' &&
        /\/payments\/paypal\/orders\/.+\/capture$/.test(url)
      ) {
        req.respond({
          status: 200,
          contentType: 'application/json',
          headers: {
            'content-type': 'application/json',
            'access-control-allow-origin': '*',
          },
          body: JSON.stringify({
            statusCode: 200,
            message: 'OK',
            data: {
              subscription: {
                userId: 'sim',
                plan: 'basic',
                status: 'active',
                paymentProvider: 'paypal',
                startedAt: new Date().toISOString(),
                updatedAt: new Date().toISOString(),
                expiresAt: null,
                cancelledAt: null,
              },
              capture: {
                status: 'COMPLETED',
                captureId: 'cap-sim',
                amountValue: '9.99',
                currency: 'USD',
                payerEmail: 'buyer@example.com',
                customId: 'sim',
              },
            },
            timestamp: new Date().toISOString(),
          }),
        });
      } else {
        req.continue();
      }
    });
    await page.goto(`${APP}/cloud?paypal=success&token=SIM-ORDER`, {
      waitUntil: 'domcontentloaded',
    });
    await waitForText(page, 'Payment confirmed');
    await new Promise((r) => setTimeout(r, 800));
    await reveal('Payment confirmed');
    await new Promise((r) => setTimeout(r, 400));
    await shot('05-success-payment-confirmed.png');

    process.stdout.write(`\nDONE — screenshots written to ${OUT}\n`);
    process.exitCode = 0;
  } catch (error) {
    process.stdout.write(`\nFAIL — ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  } finally {
    await browser.close();
  }
}

main();