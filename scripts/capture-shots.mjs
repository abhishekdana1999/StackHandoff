/**
 * Product screenshot capture for the brag videos.
 *
 * Drives the *real* React app (the same components/CSS the desktop build
 * renders) in plain Chrome against the dev server, with the browser-only Tauri
 * IPC mock from `src/dev/mockTauri.ts` supplying fixture data. Device names in
 * the fixtures are masked, so nothing here exposes real machine names.
 *
 * Requires:
 *   - the Vite dev server running on :1420  (`npm run dev`)
 *   - Google Chrome installed at the path below
 *
 * Usage: node scripts/capture-shots.mjs
 */
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const puppeteer = require('puppeteer-core');

const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const BASE = 'http://localhost:1420';
const OUT = 'brag-output/shots';

const LANDSCAPE = { width: 1440, height: 900, dir: 'land' };
const PORTRAIT = { width: 1080, height: 1162, dir: 'port' };

async function waitForText(page, text, timeout = 15000) {
  await page.waitForFunction(
    (t) => document.body && document.body.innerText.includes(t),
    { timeout, polling: 200 },
    text,
  );
}

async function scrollMainTo(page, targetText) {
  await page.evaluate((text) => {
    const main = document.querySelector('main');
    if (!main) return;
    main.scrollTop = main.scrollHeight;
    // Step back up in chunks until the target text is visible (or we exhaust).
    const step = 220;
    const visible = () => {
      const el = [...document.querySelectorAll('main *')].find(
        (node) => node instanceof HTMLElement && node.innerText && node.innerText.trim() === text,
      );
      if (!el) return true; // not found — give up stepping
      const r = el.getBoundingClientRect();
      return r.top >= 0 && r.top < window.innerHeight - 40;
    };
    if (visible()) return;
    let guard = 30;
    while (!visible() && guard-- > 0) {
      main.scrollTop = Math.max(0, main.scrollTop - step);
    }
  }, targetText);
}

async function clickDeviceRow(page) {
  const clicked = await page.evaluate(() => {
    const target = 'Office PC';
    const btn = [...document.querySelectorAll('button')].find(
      (b) => b.innerText && b.innerText.includes(target) && b.offsetParent !== null,
    );
    if (!btn) return false;
    btn.click();
    return true;
  });
  if (!clicked) throw new Error('device row "Office PC" not found to click');
}

async function capture(page, viewport, shot) {
  const file = `${OUT}/${viewport.dir}/${shot.name}.png`;
  const vp =
    (shot.viewportByDir && shot.viewportByDir[viewport.dir]) ||
    shot.viewport ||
    viewport;
  await page.setViewport({ width: vp.width, height: vp.height, deviceScaleFactor: 1 });
  await page.goto(`${BASE}${shot.url}`, { waitUntil: 'domcontentloaded' });
  await shot.setup(page, vp);
  await new Promise((r) => setTimeout(r, 900)); // settle fonts/transitions
  await page.screenshot({ path: file });
  console.log(`  shot ${viewport.dir}/${shot.name}.png`);
}

// Each setup receives the freshly loaded page (still resolving its queries).
const shots = [
  {
    name: 'capture',
    url: '/capture?story=capture&theme=light',
    settle: 'Choose what to include',
    setup: async (page) => {
      await waitForText(page, 'Choose what to include');
      await page.type('#capture-name', 'Workspace Clone - Win to Mac Transfer');
      await page.evaluate(() => {
        document.getElementById('project-p1')?.click();
        document.getElementById('project-p2')?.click();
        document.getElementById('adapter-vscode')?.click();
        document.getElementById('adapter-terminal')?.click();
      });
      await waitForText(page, '4 items selected');
    },
  },
  {
    name: 'dest-card',
    url: '/transfer/ws-1?story=transfer&theme=light',
    settle: 'Send to Office PC',
    // Close-up on the destinations card + send panel, so the masked device name
    // reads clearly when the shot is scaled into scene 3.
    viewportByDir: {
      land: { width: 1120, height: 800 },
      port: { width: 900, height: 560 },
    },
    setup: async (page) => {
      await waitForText(page, 'Captured workspace');
      await waitForText(page, 'Office PC');
      await scrollMainTo(page, 'Destinations');
      await clickDeviceRow(page);
      await waitForText(page, 'Send to Office PC');
      await scrollMainTo(page, 'Destinations');
      await page.evaluate(() => {
        const main = document.querySelector('main');
        if (!main) return;
        const target = [...document.querySelectorAll('main *')].find(
          (node) => node instanceof HTMLElement && node.innerText && node.innerText.trim() === 'Destinations',
        );
        if (target) main.scrollTop -= target.getBoundingClientRect().top - 100;
      });
    },
  },
  {
    name: 'transfer-top',
    url: '/transfer/ws-1?story=transfer&theme=light',
    settle: 'Captured workspace',
    setup: async (page) => {
      await waitForText(page, 'Captured workspace');
      await waitForText(page, 'Office PC');
      await page.evaluate(() => document.querySelector('main')?.scrollTo(0, 0));
    },
  },
  {
    name: 'transfer-dest',
    url: '/transfer/ws-1?story=transfer&theme=light',
    settle: 'Office PC',
    setup: async (page) => {
      await waitForText(page, 'Captured workspace');
      await waitForText(page, 'Office PC');
      await scrollMainTo(page, 'Destinations');
      await clickDeviceRow(page);
      await waitForText(page, 'Send to Office PC');
    },
  },
  {
    name: 'restore-preview',
    url: '/restore-preview/ws-1?story=restore&theme=light',
    settle: 'Run the restore',
    setup: async (page) => {
      await waitForText(page, 'Run the restore');
    },
  },
  {
    name: 'restore-report',
    url: '/restore-report/restore-ws-1-001?story=report&theme=light',
    settle: 'Restore Report',
    setup: async (page) => {
      await waitForText(page, 'Restore Report');
    },
  },
];

async function main() {
  const browser = await puppeteer.launch({
    executablePath: CHROME,
    headless: true,
    args: ['--hide-scrollbars', '--force-device-scale-factor=1', '--disable-gpu'],
  });
  const page = await browser.newPage();
  try {
    for (const viewport of [LANDSCAPE, PORTRAIT]) {
      for (const shot of shots) {
        if (shot.landOnly && viewport.dir !== 'land') continue;
        await capture(page, viewport, shot);
      }
    }
  } finally {
    await browser.close();
  }
  console.log('done');
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});