/**
 * Static audit for the marketing site in `site/`.
 *
 *   node scripts/audit-site.mjs            # audits http://localhost:4321
 *   node scripts/audit-site.mjs --url ...  # or point it anywhere
 *
 * What it checks, and why each one is here rather than left to review by eye:
 *
 *   1. Colour contrast, in BOTH schemes, on the text that actually carries
 *      meaning. `prefers-color-scheme` cannot be forced from page JavaScript, so
 *      this drives real Chrome media emulation -- asserting dark mode works
 *      without ever having rendered it proves nothing.
 *   2. `prefers-reduced-motion`, for the same reason. The site hides content
 *      behind reveal animations; if the escape hatch does not restore it, the
 *      page is broken for those users rather than merely plainer.
 *   3. Horizontal overflow, which is what a long line inside a scrollable
 *      <pre> does to a grid track that forgot `min-width: 0`.
 *   4. Layout invariants that the eye should not have to catch: no drop shadows
 *      (the product's own rule), no pill radii, two-column sections collapsing
 *      to one at the right breakpoint, every screenshot decoded and described.
 *
 * Alpha is composited rather than ignored. Reading rgba(255,255,255,.06) as if
 * it were solid white reports a background that does not exist and turns the
 * whole audit into noise.
 */
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const puppeteer = require('puppeteer-core');

const CHROME =
  process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const argUrl = process.argv.indexOf('--url');
// not named URL: that would shadow the global constructor
const SITE_URL = argUrl !== -1 ? process.argv[argUrl + 1] : 'http://localhost:4321/';

// [selector, text colour property, background property or null to composite]
const CONTRAST_PROBES = [
  ['body', 'color', 'backgroundColor'],
  ['.kicker', 'color', null],
  ['.hero__title', 'color', null],
  ['.hero__sub', 'color', null],
  ['.btn--primary', 'color', 'backgroundColor'],
  ['.hero__note', 'color', null],
  ['.card--pad p', 'color', null],
  ['.manifest pre', 'color', null],
  ['.manifest .k', 'color', null],
  ['.manifest .s', 'color', null],
  ['.never__list li', 'color', null],
  ['.fcard p', 'color', null],
  ['.stat span', 'color', null],
  ['.stack__row span', 'color', null],
  ['.stack__row b', 'color', null],
  ['.foot__fine', 'color', null],
  ['.get', 'color', 'backgroundColor'],
  ['.get h2', 'color', null],
  ['.get__facts li', 'color', null],
  ['.chip--danger', 'color', 'backgroundColor'],
  ['.ticks li', 'color', null],
  ['.switch__btn em', 'color', null],
  ['.btn--light', 'color', 'backgroundColor'],
];

/* Runs inside the page, so it must be self-contained -- closures over this
   module's scope do not survive serialization. */
const inPage = (PROBES) => {
  const rgb = (c) => {
    const m = (c || '').match(/[\d.]+/g);
    return m && m.length >= 3 ? m.slice(0, 3).map(Number) : null;
  };
  const alpha = (c) =>
    c && c.startsWith('rgba') ? parseFloat((c.match(/[\d.]+/g) || [])[3] ?? '1') : 1;
  const lin = (v) => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
  const over = (fg, a, bg) => [0, 1, 2].map((k) => fg[k] * a + bg[k] * (1 - a));
  const ratio = (a, b) => {
    const [l1, l2] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (l1 + 0.05) / (l2 + 0.05);
  };

  const bgOf = (el) => {
    const layers = [];
    let n = el;
    while (n && n !== document.documentElement) {
      const c = getComputedStyle(n).backgroundColor;
      const p = rgb(c);
      if (p) {
        const a = alpha(c);
        if (a > 0) {
          layers.push([p, a]);
          if (a === 1) break;
        }
      }
      n = n.parentElement;
    }
    let out = [255, 255, 255];
    for (let i = layers.length - 1; i >= 0; i--) out = over(layers[i][0], layers[i][1], out);
    return out;
  };

  const contrast = PROBES.map(([sel, fgProp, bgProp]) => {
    const el = document.querySelector(sel);
    if (!el) return { sel, missing: true };
    const cs = getComputedStyle(el);
    const rawFg = cs[fgProp || 'color'];
    const bg = bgProp ? rgb(cs[bgProp]) : bgOf(el);
    let fg = rgb(rawFg);
    if (!fg || !bg) return { sel, unparsed: true, rawFg, bg: cs.backgroundColor };
    const a = alpha(rawFg);
    if (a < 1) fg = over(fg, a, bg);
    const r = ratio(fg, bg);
    return {
      sel,
      rawFg,
      bg: bgProp ? cs[bgProp] : `rgb(${bg.map(Math.round).join(', ')})`,
      ratio: +r.toFixed(2),
      grade: r >= 4.5 ? 'AA' : r >= 3 ? 'AA-large-only' : 'FAIL',
    };
  });

  // reveals, overflow, layout invariants
  const motionTargets = [...document.querySelectorAll('.reveal, .reveal-stagger, [data-shot], [data-hero]')];
  const faded = motionTargets.filter((el) => parseFloat(getComputedStyle(el).opacity) < 0.9);

  const vw = document.documentElement.clientWidth;
  const clipped = [];
  document.querySelectorAll('*').forEach((el) => {
    if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX === 'visible') {
      const p = el.parentElement;
      if (!p || p.scrollWidth <= p.clientWidth + 1) {
        clipped.push((el.className || el.tagName).toString().slice(0, 40));
      }
    }
  });

  const shadows = [];
  const radii = new Set();
  document.querySelectorAll('body *').forEach((el) => {
    const s = getComputedStyle(el);
    if (s.boxShadow && s.boxShadow !== 'none') shadows.push((el.className || el.tagName).toString());
    if (s.borderRadius && s.borderRadius !== '0px') radii.add(s.borderRadius);
  });

  const twoCol = (sel) => {
    const el = document.querySelector(sel);
    if (!el) return null;
    const kids = [...el.children].filter((k) => k.getBoundingClientRect().width > 0);
    if (kids.length < 2) return null;
    return kids[1].getBoundingClientRect().x > kids[0].getBoundingClientRect().width * 0.5;
  };

  return {
    contrast,
    motion: {
      reduceMotionActive: matchMedia('(prefers-reduced-motion: reduce)').matches,
      targets: motionTargets.length,
      faded: faded.map((e) => (e.className || e.tagName).toString().slice(0, 40)),
    },
    overflowX: {
      viewport: vw,
      document: document.documentElement.scrollWidth,
      overflowing: document.documentElement.scrollWidth > vw + 1,
      offenders: clipped,
    },
    noShadows: { count: shadows.length, offenders: shadows.slice(0, 6) },
    radii: [...radii].sort(),
    layout: {
      heroTwoCol: twoCol('.hero__in'),
      stepTwoCol: twoCol('[data-step]'),
      railVisible: (() => {
        const r = document.querySelector('.steps__rail');
        return r ? r.getBoundingClientRect().width > 0 : null;
      })(),
    },
    images: [...document.images].map((i) => ({
      src: (i.getAttribute('src') || '').split('/').slice(-2).join('/'),
      decoded: i.complete && i.naturalWidth > 0,
      described: (i.alt || '').length > 20,
    })),
  };
};

const VIEWPORTS = [
  ['desktop 1440', 1440, 900],
  ['desktop 1100', 1100, 800],
  ['tablet 820', 820, 1100],
  ['mobile 390', 390, 844],
];

const browser = await puppeteer.launch({
  executablePath: CHROME,
  headless: 'new',
  args: ['--no-sandbox', '--force-color-profile=srgb'],
});

let failures = 0;
const report = {};

for (const scheme of ['light', 'dark']) {
  for (const [label, w, h] of VIEWPORTS) {
    // Contrast only needs checking once per scheme; layout is width-dependent.
    const wantContrast = w === 1440;
    const page = await browser.newPage();
    await page.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await page.emulateMediaFeatures([{ name: 'prefers-color-scheme', value: scheme }]);
    await page.goto(SITE_URL, { waitUntil: 'networkidle0' });
    await page.evaluate(
      () => new Promise((r) => setTimeout(r, 300))
    );
    // walk the page so every reveal has fired before anything is measured
    await page.evaluate(async () => {
      const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
      for (let y = 0; y < document.documentElement.scrollHeight; y += 400) {
        window.scrollTo({ top: y, behavior: 'instant' });
        await sleep(35);
      }
      window.scrollTo({ top: 0, behavior: 'instant' });
      await sleep(300);
    });
    const r = await page.evaluate(inPage, CONTRAST_PROBES);
    const key = `${scheme} / ${label}`;
    report[key] = r;

    if (wantContrast) {
      for (const c of r.contrast) {
        if (c.missing || c.unparsed) {
          failures++;
          console.log(`  FAIL  ${c.sel} ${c.missing ? 'MISSING' : 'UNPARSED ' + c.rawFg + ' / ' + c.bg}`);
        } else if (c.grade === 'FAIL') {
          failures++;
          console.log(`  FAIL  ${c.ratio}:1  ${c.sel}  ${c.rawFg} on ${c.bg}`);
        } else if (c.grade === 'AA-large-only') {
          console.log(`  warn  ${c.ratio}:1  ${c.sel}  ${c.rawFg} on ${c.bg}`);
        }
      }
    }
    if (r.overflowX.overflowing) {
      failures++;
      console.log(`  FAIL  horizontal overflow ${r.overflowX.document} > ${r.overflowX.viewport}  ${JSON.stringify(r.overflowX.offenders)}`);
    }
    if (r.motion.faded.length) {
      failures++;
      console.log(`  FAIL  content left invisible: ${JSON.stringify(r.motion.faded)}`);
    }
    if (r.noShadows.count) {
      failures++;
      console.log(`  FAIL  drop shadows present: ${JSON.stringify(r.noShadows.offenders)}`);
    }
    const badImages = r.images.filter((i) => !i.decoded || !i.described);
    if (badImages.length) {
      failures++;
      console.log(`  FAIL  images not decoded or not described: ${JSON.stringify(badImages)}`);
    }

    const stacked = w <= 1000;
    if (r.layout.heroTwoCol === stacked) {
      failures++;
      console.log(`  FAIL  hero column layout wrong at ${w}px: twoCol=${r.layout.heroTwoCol}, expected ${!stacked}`);
    }
    if (r.layout.railVisible === stacked) {
      failures++;
      console.log(`  FAIL  step rail ${r.layout.railVisible ? 'shown' : 'hidden'} at ${w}px, expected ${!stacked}`);
    }

    await page.close();
  }
}

// reduced motion needs its own load so the script re-reads the query
{
  const page = await browser.newPage();
  await page.setViewport({ width: 1440, height: 900 });
  await page.emulateMediaFeatures([{ name: 'prefers-reduced-motion', value: 'reduce' }]);
  await page.goto(SITE_URL, { waitUntil: 'networkidle0' });
  await page.evaluate(() => new Promise((r) => setTimeout(r, 400)));
  const r = await page.evaluate(inPage, CONTRAST_PROBES);
  report['reduced motion'] = r;
  if (!r.motion.reduceMotionActive) {
    failures++;
    console.log('  FAIL  prefers-reduced-motion did not take effect');
  }
  if (r.motion.faded.length) {
    failures++;
    console.log(`  FAIL  reduced motion still hides content: ${JSON.stringify(r.motion.faded)}`);
  }
  const ticker = await page.evaluate(() => getComputedStyle(document.querySelector('.ticker__row')).animationName);
  if (ticker !== 'none') {
    failures++;
    console.log(`  FAIL  ticker still animating under reduced motion: ${ticker}`);
  }
  await page.close();
}

await browser.close();

console.log(
  failures === 0
    ? `\nsite audit clean -- ${Object.keys(report).length} configurations checked`
    : `\nsite audit: ${failures} problem(s)`
);
process.exit(failures === 0 ? 0 : 1);
