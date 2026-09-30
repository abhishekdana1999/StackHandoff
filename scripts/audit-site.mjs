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
  ['.motion-hero h1', 'color', null],
  ['.motion-hero h1 span', 'color', null],
  ['.intro-copy', 'color', null],
  ['.btn--primary', 'color', 'backgroundColor'],
  ['.hero-meta', 'color', null],
  ['.hero-meta b', 'color', null],
  ['.scroll-cue', 'color', null],
  ['.journey-kicker', 'color', null],
  ['.journey-heading h2', 'color', null],
  ['.journey-desc', 'color', null],
  ['.journey-label', 'color', null],
  ['.journey-hint', 'color', null],
  ['.device-label span', 'color', null],
  ['.device-label b', 'color', null],
  ['.screen-bar', 'color', null],
  ['.screen-state', 'color', null],
  ['.screen-bottom', 'color', null],
  ['.screen-bottom code', 'color', null],
  ['.waiting-state b', 'color', null],
  ['.waiting-state small', 'color', null],
  ['.transfer-badge b', 'color', null],
  ['.transfer-badge small', 'color', null],
  ['.phase-tabs button', 'color', null],
  ['.skip-story', 'color', null],
  ['.lede__p', 'color', null],
  ['.card p', 'color', null],
  ['.chip--danger', 'color', 'backgroundColor'],
  ['.ticks li', 'color', null],
  ['.manifest pre', 'color', null],
  ['.manifest .k', 'color', null],
  ['.manifest .s', 'color', null],
  ['.never__list li', 'color', null],
  ['.shot__cap', 'color', null],
  ['.fcard p', 'color', null],
  ['.stat span', 'color', null],
  ['.stack__row span', 'color', null],
  ['.stack__row b', 'color', null],
  ['.repo__url', 'color', null],
  ['.foot__tag', 'color', null],
  ['.foot__links a', 'color', null],
  ['.foot__fine', 'color', null],
  ['.get', 'color', 'backgroundColor'],
  ['.get h2', 'color', null],
  ['.get__facts li', 'color', null],
  ['.download-status', 'color', null],
  // null, not 'backgroundColor': this chip's own fill is rgba(255,255,255,.14),
  // which over .get's --solid composites to something in between. Reading the
  // raw rgba as if it were an opaque backdrop reports 1.17:1 against a
  // background that does not exist.
  ['.chip--soft', 'color', null],
  ['.chip--soft b', 'color', null],
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

  // Steps are dimmed to 0.32 until they are the active one. Every step must
  // have exactly one active at any scroll position -- an IntersectionObserver
  // fires only for what intersects at the instant it runs, so arriving by
  // anchor link can leave every step dimmed with no way back.
  const steps = [...document.querySelectorAll('[data-step]')];
  const activeSteps = steps.filter((s) => s.classList.contains('is-active')).length;

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

  /* The journey is the one part of the page that depends on script to be
     legible at all: --restore, --draw and --travel are written by a scroll
     listener, and the destination screen sits at opacity: var(--restore) with a
     waiting overlay at calc(1 - var(--restore)) behind it. If those never get
     written the destination is blank. So this asserts the arithmetic of the
     cross-fade directly: at rest the two layers must be complementary, and the
     two real screenshots inside the diagram must have decoded. */
  const journeyState = (() => {
    const journey = document.querySelector('[data-journey]');
    if (!journey) return { missing: true };
    const cs = getComputedStyle(journey);
    const num = (v) => parseFloat(v);
    const restore = num(cs.getPropertyValue('--restore'));
    const img = journey.querySelector('.destination-body > img');
    const waiting = journey.querySelector('.waiting-state');
    return {
      travel: num(cs.getPropertyValue('--travel')),
      draw: num(cs.getPropertyValue('--draw')),
      restore,
      destImgOpacity: img ? parseFloat(getComputedStyle(img).opacity) : null,
      waitingOpacity: waiting ? parseFloat(getComputedStyle(waiting).opacity) : null,
      waitingIsVisible: waiting ? parseFloat(getComputedStyle(waiting).opacity) > 0.5 : null,
      destDecoded: img ? img.complete && img.naturalWidth > 0 : null,
      srcDecoded: (() => {
        const s = journey.querySelector('.source-node img');
        return s ? s.complete && s.naturalWidth > 0 : null;
      })(),
      stageHeight: Math.round(journey.querySelector('.journey-sticky').getBoundingClientRect().height),
      /* Only HTML elements. `querySelectorAll('*')` on a subtree containing the
       connection <svg> returns SVG nodes too, and SVGElement.className is an
       SVGAnimatedString rather than a string — which is harmless here but made
       the offender list unreadable. The stroke geometry is checked separately
       via threadLength. */
      stageOverflows: (() => {
        const stage = journey.querySelector('.journey-sticky');
        const r = stage.getBoundingClientRect();
        return [...stage.querySelectorAll('*')]
          .filter((el) => el instanceof HTMLElement)
          .some((el) => {
            const b = el.getBoundingClientRect();
            return b.height > 0 && b.bottom > r.bottom + 1;
          });
      })(),
      threadLength: (() => {
        const t = journey.querySelector('[data-thread-fill]');
        return t ? +t.getTotalLength().toFixed(1) : null;
      })(),
    };
  })();

  return {
    contrast,
    motion: {
      reduceMotionActive: matchMedia('(prefers-reduced-motion: reduce)').matches,
      targets: motionTargets.length,
      faded: faded.map((e) => (e.className || e.tagName).toString().slice(0, 40)),
      stepsTotal: steps.length,
      activeSteps,
    },
    journey: journeyState,
    overflowX: {
      viewport: vw,
      document: document.documentElement.scrollWidth,
      overflowing: document.documentElement.scrollWidth > vw + 1,
      offenders: clipped,
    },
    noShadows: { count: shadows.length, offenders: shadows.slice(0, 6) },
    radii: [...radii].sort(),
    layout: {
      stepTwoCol: twoCol('[data-step]'),
      gitpayTwoCol: twoCol('.gitpay'),
      railVisible: (() => {
        const r = document.querySelector('.steps__rail');
        return r ? r.getBoundingClientRect().width > 0 : null;
      })(),
    },
    // The lightbox is a <dialog> that starts closed, and its <img> has no src
    // until something is opened in it. "Not decoded" is the correct state for
    // that image and failing on it would mean the audit can never be green.
    images: [...document.images]
      .filter((i) => i.getAttribute('src'))
      .map((i) => ({
        src: (i.getAttribute('src') || '').split('/').slice(-2).join('/'),
        decoded: i.complete && i.naturalWidth > 0,
        described: (i.alt || '').length > 20,
      })),

    /* Every screenshot opens a lightbox, so the page needs a keyboard route to
       the full-size view as well as a pointer one, and a visible focus ring on
       it. The usual way a screenshot lightbox ships broken is a <figure
       role="button"> with no tabindex: reachable with a mouse, invisible to the
       keyboard, and taking the space a real button needs. */
    shots: [...document.querySelectorAll('[data-shot]')].map((el) => {
      el.focus();
      const cs = getComputedStyle(el);
      const ring = cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0;
      el.blur();
      return {
        tag: el.tagName.toLowerCase(),
        nameable: el.tagName === 'BUTTON' || el.tabIndex >= 0,
        labelled: (el.getAttribute('aria-label') || el.textContent || '').trim().length > 3,
        focusRing: ring,
      };
    }),
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

    /* ------------------------------------------------------- the journey --
       Checked at three scrub positions, because the whole point of the section
       is that the two destination layers cross-fade on scroll. Measuring once
       at the top of the page can only prove the initial state. */
    for (const [at, frac] of [
      ['start', 0],
      ['middle', 0.5],
      ['end', 1],
    ]) {
      const scrubbed = await page.evaluate(
        async (f) => {
          const journey = document.querySelector('[data-journey]');
          const sticky = journey.querySelector('.journey-sticky');
          const nav = document.querySelector('[data-nav]');
          const navH = nav ? nav.offsetHeight : 64;
          const top = journey.getBoundingClientRect().top + window.scrollY;
          const span = journey.offsetHeight - sticky.offsetHeight;
          window.scrollTo({ top: Math.max(0, top - navH + f * span), behavior: 'instant' });
          // two frames: one to run the rAF-throttled scroll handler, one to let
          // the style recalc land
          await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
          return {
            travel: parseFloat(
              getComputedStyle(journey).getPropertyValue('--travel')
            ),
            restore: parseFloat(
              getComputedStyle(journey).getPropertyValue('--restore')
            ),
            draw: parseFloat(getComputedStyle(journey).getPropertyValue('--draw')),
            destOpacity: parseFloat(
              getComputedStyle(journey.querySelector('.destination-body > img')).opacity
            ),
            waitingOpacity: parseFloat(
              getComputedStyle(journey.querySelector('.waiting-state')).opacity
            ),
            kicker: journey.querySelector('[data-phase-kicker]').textContent.trim(),
            state: journey.querySelector('[data-dest-state]').textContent.trim(),
          };
        },
        frac
      );
      report[`journey ${scheme}/${label} ${at}`] = scrubbed;

      if (Math.abs(scrubbed.restore - scrubbed.destOpacity) > 0.02) {
        failures++;
        console.log(
          `  FAIL  journey destination screenshot not tracking --restore: ${scrubbed.destOpacity} vs ${scrubbed.restore}`
        );
      }
      if (Math.abs(1 - scrubbed.restore - scrubbed.waitingOpacity) > 0.02) {
        failures++;
        console.log(
          `  FAIL  journey waiting overlay not complementary: ${scrubbed.waitingOpacity} vs ${1 - scrubbed.restore}`
        );
      }
      if (at === 'end' && scrubbed.restore < 0.99) {
        failures++;
        console.log(`  FAIL  journey did not reach its final frame: --restore=${scrubbed.restore}`);
      }
      if (at === 'start' && scrubbed.restore > 0.01) {
        failures++;
        console.log(`  FAIL  journey started already restored: --restore=${scrubbed.restore}`);
      }
      if (at === 'middle' && scrubbed.draw < 0.2) {
        failures++;
        console.log(`  FAIL  connection not drawn at the midpoint: --draw=${scrubbed.draw}`);
      }
      if (at === 'end' && scrubbed.state !== 'RECEIVED') {
        failures++;
        console.log(`  FAIL  destination not marked RECEIVED at the end: ${scrubbed.state}`);
      }
    }

    if (r.journey.missing) {
      failures++;
      console.log('  FAIL  #journey section missing');
    } else {
      if (!r.journey.destDecoded || !r.journey.srcDecoded) {
        failures++;
        console.log('  FAIL  journey screenshots did not decode');
      }
      if (r.journey.stageOverflows) {
        failures++;
        console.log('  FAIL  journey stage clips content below its own box');
      }
      if (!r.journey.threadLength || r.journey.threadLength < 10) {
        failures++;
        console.log(`  FAIL  journey thread has no usable path: ${r.journey.threadLength}`);
      }
    }

    if (r.motion.stepsTotal && r.motion.activeSteps !== 1) {
      failures++;
      console.log(
        `  FAIL  ${r.motion.activeSteps} of ${r.motion.stepsTotal} steps active, expected exactly 1`
      );
    }

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
    const badShots = r.shots.filter((s) => !s.nameable || !s.labelled || !s.focusRing);
    if (badShots.length) {
      failures++;
      console.log(
        `  FAIL  screenshot not keyboard reachable, labelled, or focusable: ${JSON.stringify(badShots)}`
      );
    }

    const stacked = w <= 1000;
    if (r.layout.stepTwoCol === stacked) {
      failures++;
      console.log(`  FAIL  step column layout wrong at ${w}px: twoCol=${r.layout.stepTwoCol}, expected ${!stacked}`);
    }
    if (r.layout.gitpayTwoCol === stacked) {
      failures++;
      console.log(`  FAIL  gitpay column layout wrong at ${w}px: twoCol=${r.layout.gitpayTwoCol}, expected ${!stacked}`);
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

  /* Reduced motion collapses a 290svh sticky sequence to a static diagram.
     The destination must land on its FINAL frame, not stay blank: the section
     is still on the page, still readable, and a blank device screen there is a
     bug that looks like an empty box. */
  const rm = await page.evaluate(() => {
    const journey = document.querySelector('[data-journey]');
    const img = journey.querySelector('.destination-body > img');
    return {
      sectionHeight: Math.round(journey.getBoundingClientRect().height),
      viewport: window.innerHeight,
      sticky: getComputedStyle(journey.querySelector('.journey-sticky')).position,
      restore: parseFloat(getComputedStyle(journey).getPropertyValue('--restore')),
      destOpacity: parseFloat(getComputedStyle(img).opacity),
      tabsHidden: getComputedStyle(journey.querySelector('.phase-tabs')).display === 'none',
      // the pulse and the scroll-rule are the only infinite animations left
      pulse: getComputedStyle(journey.querySelector('.signal-halo')).display,
      /* getAnimations(), not animationName. The reduced-motion block scales
         durations to ~0 and forces one iteration, so a named animation can
         still be *attached* while being completely inert — asserting on the
         name reports a failure that is not one, and misses the real case of an
         infinite animation surviving. What matters is whether anything is
         actually still running. */
      liveAnimations: document
        .getAnimations()
        .filter((a) => a.playState === 'running')
        .map((a) => `${a.animationName || 'transition'}:${a.effect.getTiming().iterations ?? 1}`),
      toggleLabel: document.getElementById('motion-toggle').textContent.trim(),
      toggleDisabled: document.getElementById('motion-toggle').disabled,
    };
  });
  report['reduced motion details'] = rm;

  if (rm.sectionHeight > rm.viewport * 1.5) {
    failures++;
    console.log(
      `  FAIL  reduced motion left the journey ${rm.sectionHeight}px tall for a ${rm.viewport}px viewport`
    );
  }
  // 'relative' is fine — it is what stops the collapsed section from overlapping
    // the next one. Only 'sticky' and 'fixed' pin content to the viewport.
    if (rm.sticky === 'sticky' || rm.sticky === 'fixed') {
      failures++;
      console.log(`  FAIL  journey stage still pinned under reduced motion: ${rm.sticky}`);
    }
  if (rm.restore < 0.99 || rm.destOpacity < 0.99) {
    failures++;
    console.log(
      `  FAIL  reduced motion left the destination blank: --restore=${rm.restore}, img opacity ${rm.destOpacity}`
    );
  }
  if (!rm.tabsHidden) {
    failures++;
    console.log('  FAIL  phase tabs still shown under reduced motion');
  }
  if (rm.pulse !== 'none') {
    failures++;
    console.log(`  FAIL  signal halo still rendered under reduced motion: ${rm.pulse}`);
  }
  if (rm.liveAnimations.length) {
    failures++;
    console.log(`  FAIL  animations still running under reduced motion: ${rm.liveAnimations.join(', ')}`);
  }
  if (!rm.toggleDisabled) {
    failures++;
    console.log('  FAIL  motion toggle still offers to enable motion under reduced motion');
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
