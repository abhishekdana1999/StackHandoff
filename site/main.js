/* ==========================================================================
   StackHandoff — site behaviour

   Everything here is progressive enhancement. The page is fully readable with
   this file blocked: every download link has a real href in the markup, the
   screenshot <img> tags carry width/height and alt text, and the copy buttons
   are real <button>s. Nothing is revealed by JavaScript that is not already
   visible without it.

   The one thing that IS script-dependent is #journey, the scroll-scrubbed
   handoff. Without this file the CSS collapses it to its final frame at its
   natural height (see the reduced-motion block in motion.css, and the
   no-JS guard in .journey-sticky), so a reader without scripts gets the end
   state of the sequence as a static diagram rather than an empty box.
   ========================================================================== */
(function () {
  'use strict';

  var reduceQuery = window.matchMedia('(prefers-reduced-motion: reduce)');
  var narrowQuery = window.matchMedia('(max-width: 760px)');
  var reduceMotion = reduceQuery.matches;

  /* ---------------------------------------------------------------------
     Downloads.

     Releases rather than files in this repo: a .dmg and an .exe committed to
     git on every build is a lot of churn for two binaries that only change
     when the app does.

     ASSET NAMES MUST MATCH THE RELEASE EXACTLY. GitHub's
     /releases/latest/download/<name> endpoint is a literal file lookup — a
     mismatch is a 404, not a redirect. Changing a name here means renaming the
     uploaded asset in the release, or the button silently stops working.

     PUBLISHED says whether that release actually exists yet. It is false as
     shipped, and the page says so out loud below the button rather than
     letting a visitor click a confident blue button into a 404. Flip it to
     true once `gh release create` has run — that is the only edit needed to
     make these buttons live.
     --------------------------------------------------------------------- */
  var REPO = 'abhishekdana1999/workspace-clone';
  var RELEASE_LATEST = 'https://github.com/' + REPO + '/releases/latest';
  var RELEASE_FILE = 'https://github.com/' + REPO + '/releases/latest/download/';
  var PUBLISHED = false;

  var BUILDS = {
    mac: {
      href: RELEASE_FILE + 'StackHandoff.dmg',
      label: 'Download for macOS',
      filename: 'StackHandoff.dmg',
      size: '8.7 MB · Apple silicon',
      name: 'macOS',
      glyph:
        '<svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true">' +
        '<path d="M16.4 12.7c0-2.4 2-3.6 2.1-3.6-1.1-1.7-2.9-1.9-3.5-1.9-1.5-.2-2.9.9-3.7.9-.8 0-1.9-.9-3.1-.8-1.6 0-3.1.9-3.9 2.4-1.7 2.9-.4 7.2 1.2 9.5.8 1.1 1.7 2.4 3 2.4 1.2 0 1.6-.8 3.1-.8 1.4 0 1.8.8 3.1.7 1.3 0 2.1-1.2 2.9-2.3.9-1.3 1.3-2.6 1.3-2.7 0 0-2.5-1-2.5-3.8zM14 5.6c.7-.8 1.1-2 1-3.1-1 0-2.2.7-2.9 1.5-.6.7-1.2 1.9-1 3 1.1.1 2.2-.6 2.9-1.4z"/>' +
        '</svg>',
    },
    win: {
      href: RELEASE_FILE + 'StackHandoff.exe',
      label: 'Download for Windows',
      filename: 'StackHandoff.exe',
      size: 'NSIS installer · no admin rights',
      name: 'Windows',
      glyph:
        '<svg viewBox="0 0 24 24" width="16" height="16" fill="currentColor" aria-hidden="true">' +
        '<path d="M3 5.5l7.5-1v6.6H3zM11.5 4.3L21 3v8.1h-9.5zM3 12.1h7.5v6.6L3 17.7zM11.5 12.1H21V21l-9.5-1.3z"/>' +
        '</svg>',
    },
  };

  var UNKNOWN = {
    href: RELEASE_LATEST,
    label: 'Get StackHandoff',
    filename: 'StackHandoff',
    size: 'macOS · Windows',
    name: 'this device',
    glyph:
      '<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
      '<path d="M12 3v12M12 15l-4-4M12 15l4-4"/><path d="M4 17v2a2 0 0 0 2 2h12a2 0 0 0 0 2-2v-2"/>' +
      '</svg>',
  };

  /**
   * Which platform is this?
   *
   * `userAgentData` is the only non-deprecated source and it is Chromium-only,
   * so there are two fallbacks behind it. `navigator.platform` is deprecated but
   * still populated everywhere and is more reliable than sniffing `userAgent`,
   * because the UA string on Windows also mentions Mac in some embedded
   * webviews — a Mac-keyword match on `userAgent` alone will mis-fire there.
   */
  function detectPlatform() {
    var hint = '';

    if (navigator.userAgentData && typeof navigator.userAgentData.platform === 'string') {
      hint = navigator.userAgentData.platform;
    } else if (typeof navigator.platform === 'string') {
      hint = navigator.platform;
    }

    if (!hint) hint = navigator.userAgent || '';

    if (/mac|iphone|ipad|ipod/i.test(hint)) return 'mac';
    if (/win/i.test(hint)) return 'win';

    // Last resort: the UA string, which is all that is left on a locked-down
    // browser that exposes neither of the above.
    var ua = navigator.userAgent || '';
    if (/mac/i.test(ua)) return 'mac';
    if (/windows|win32|win64/i.test(ua)) return 'win';

    return 'other';
  }

  var detected = detectPlatform();
  var current = detected === 'other' ? 'unknown' : detected;

  function applyPlatform(key) {
    var build = key === 'unknown' ? UNKNOWN : BUILDS[key];
    if (!build) return;
    current = key;

    document.querySelectorAll('[data-download][data-auto]').forEach(function (a) {
      a.href = build.href;
      a.setAttribute('aria-label', build.label);
    });

    document.querySelectorAll('[data-download-label]').forEach(function (el) {
      el.textContent = build.label;
    });

    document.querySelectorAll('[data-platform-glyph]').forEach(function (el) {
      el.innerHTML = build.glyph;
    });

    document.querySelectorAll('[data-download-filename]').forEach(function (el) {
      el.textContent = build.filename;
    });

    document.querySelectorAll('[data-download-size]').forEach(function (el) {
      el.textContent = build.size;
    });

    document.querySelectorAll('[data-platform-name]').forEach(function (el) {
      el.textContent = build.name;
    });

    document.querySelectorAll('[data-pick]').forEach(function (btn) {
      btn.setAttribute('aria-pressed', String(btn.dataset.pick === key));
    });
  }

  // Manual override: detection is a convenience, never a dead end. Someone on a
  // Mac helping a Windows user still needs the .exe.
  document.querySelectorAll('[data-pick]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      applyPlatform(btn.dataset.pick);
    });
  });

  var platformNote = document.querySelector('[data-platform-detected]');
  if (platformNote) platformNote.hidden = false;

  applyPlatform(current);

  var status = document.querySelector('[data-download-status]');
  if (status && !PUBLISHED) {
    status.textContent =
      'Installer downloads are not published yet. This page is live; the release is the part that is still to come.';
  }

  /* ---------------------------------------------------------------------
     #journey — the scroll-scrubbed handoff.

     The section is 290svh tall around a sticky stage. Progress through that
     scroll range becomes three custom properties on the section:

       --travel    0..1  overall position in the sequence
       --draw      0..1  how much of the connection has been drawn
       --restore   0..1  how far the destination has accepted the workspace

     The stylesheet does the rest. Writing three numbers per frame rather than
     a pile of transform strings keeps this to one style recalculation and no
     layout, and it means the choreography stays readable in one place.

     The path geometry is MEASURED rather than hardcoded: the curve is built
     from the real edges of the two device screens, and the SVG viewBox is set
     to the stage's pixel box so the mapping is exactly 1:1. That is why the
     thread still lands on both screens after a resize, a font swap, or the
     mobile layout flipping the nodes from side-by-side to stacked.
     --------------------------------------------------------------------- */
  var journey = document.querySelector('[data-journey]');

  if (journey) {
    var sticky = journey.querySelector('.journey-sticky');
    var map = journey.querySelector('.handoff-map');
    var svg = journey.querySelector('.connection');
    var threadFill = journey.querySelector('[data-thread-fill]');
    var threadBase = journey.querySelector('.thread-base');
    var signal = journey.querySelector('[data-signal]');
    var halo = journey.querySelector('[data-signal-halo]');
    var srcScreen = journey.querySelector('.source-node .device-screen');
    var dstScreen = journey.querySelector('.destination-node .device-screen');
    var destState = journey.querySelector('[data-dest-state]');
    var restoreState = journey.querySelector('[data-restore-state]');
    var destReview = journey.querySelector('[data-dest-review]');
    var destHint = journey.querySelector('[data-dest-hint]');
    var badge = journey.querySelector('.transfer-badge b');
    var kicker = journey.querySelector('[data-phase-kicker]');
    var title = journey.querySelector('[data-phase-title]');
    var desc = journey.querySelector('[data-phase-desc]');
    var heading = journey.querySelector('.journey-heading');
    var count = journey.querySelector('[data-phase-count]');
    var tabs = Array.prototype.slice.call(journey.querySelectorAll('[data-phase]'));

    var PHASES = [
      {
        kicker: '01 / CAPTURE',
        title: 'Everything you need.<br>Right where you left it.',
        desc: 'Capture the projects, tools, and git changes you choose.',
        badge: 'Paired. Private. Direct.',
        state: 'PAIRED',
        restore: 'awaiting handoff',
        review: 'Review before restoring',
        hint: 'Your paired device is ready when you are.',
      },
      {
        kicker: '02 / TRANSFER',
        title: 'A direct connection.<br>A little green light.',
        desc: 'Your encrypted workspace manifest moves between paired devices.',
        badge: 'Workspace on its way.',
        state: 'PAIRED',
        restore: 'receiving…',
        review: 'Encrypted, end to end',
        hint: 'Receiving the encrypted manifest.',
      },
      {
        kicker: '03 / CONTINUE',
        title: 'New screen.<br>Same train of thought.',
        desc: 'Review the restore plan. Sign in where needed. Keep going.',
        badge: 'Handoff complete.',
        state: 'RECEIVED',
        restore: 'main · patch ready',
        review: 'Ready to review and restore',
        hint: 'Workspace received.',
      },
    ];

    var length = 1;
    var phase = -1;
    var navH = 0;
    // Starts paused if, and only if, the reader has asked for reduced motion.
    var paused = reduceMotion;

    // Where each phase starts along the scrub, as a fraction of the sequence.
    // Phase 0 owns the first third so the capture screen is legible before
    // anything moves; the draw begins inside phase 0 rather than on its edge so
    // there is a beat of stillness before the transfer starts.
    var DRAW_START = 0.14;
    var DRAW_END = 0.62;
    var RESTORE_START = 0.6;
    var RESTORE_END = 0.88;

    function clamp(n) {
      return n < 0 ? 0 : n > 1 ? 1 : n;
    }

    function navHeight() {
      var nav = document.querySelector('[data-nav]');
      return nav ? nav.offsetHeight : 64;
    }

    /**
     * Rebuild the thread so it runs from the right edge of the source screen to
     * the left edge of the destination screen, in the coordinate space the SVG
     * is using.
     */
    function geometry() {
      if (!map || !threadFill) return;

      var mb = map.getBoundingClientRect();
      if (!mb.width || !mb.height) return;

      var a = srcScreen.getBoundingClientRect();
      var b = dstScreen.getBoundingClientRect();

      var x0 = a.right - mb.left;
      var y0 = a.top + a.height * 0.5 - mb.top;
      var x1 = b.left - mb.left;
      var y1 = b.top + b.height * 0.5 - mb.top;

      // Anchor exactly at the edge so the line starts on the bezel rather than
      // in the middle of the screenshot.
      var arc = Math.min(74, Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0)) * 0.42);
      var d;

      if (Math.abs(y1 - y0) > Math.abs(x1 - x0)) {
        // Stacked layout: an S running down the page.
        var h = y1 - y0;
        d =
          'M ' + x0 + ' ' + y0 +
          ' C ' + (x0 - arc) + ' ' + (y0 + h * 0.44) +
          ', ' + (x1 + arc) + ' ' + (y0 + h * 0.56) +
          ', ' + x1 + ' ' + y1;
      } else {
        // Side by side: a wave across the gap.
        var w = x1 - x0;
        d =
          'M ' + x0 + ' ' + y0 +
          ' C ' + (x0 + w * 0.46) + ' ' + (y0 - arc) +
          ', ' + (x0 + w * 0.54) + ' ' + (y1 + arc) +
          ', ' + x1 + ' ' + y1;
      }

      // viewBox equal to the pixel box means preserveAspectRatio never scales
      // anything, so getPointAtLength() returns page pixels and the signal dot
      // can be placed directly. The non-scaling-stroke is then belt-and-braces.
      svg.setAttribute('viewBox', '0 0 ' + mb.width + ' ' + mb.height);
      threadFill.setAttribute('d', d);
      threadBase.setAttribute('d', d);

      length = threadFill.getTotalLength() || 1;
      threadFill.style.strokeDasharray = length;
      threadBase.style.strokeDasharray = length;
      update(true);
    }

    function setPhase(index, animate) {
      if (index === phase) return;
      phase = index;
      var p = PHASES[index];

      var write = function () {
        kicker.textContent = p.kicker;
        title.innerHTML = p.title;
        desc.textContent = p.desc;
        badge.textContent = p.badge;
        destState.textContent = p.state;
        restoreState.textContent = p.restore;
        destReview.textContent = p.review;
        destHint.textContent = p.hint;
        count.textContent = index < 2 ? '0' + (index + 1) : '03';
        tabs.forEach(function (tab, i) {
          var on = i === index;
          tab.classList.toggle('is-active', on);
          tab.setAttribute('aria-pressed', String(on));
        });
      };

      if (animate && !paused && !reduceMotion) {
        // Only the click path fades. Text that tracks a scroll position should
        // follow it directly — cross-fading here just smears during a fast
        // scrub and flickers when a phase boundary is crossed twice in a row.
        heading.classList.add('is-swapping');
        badge.parentElement.classList.add('is-swapping');
        window.setTimeout(function () {
          write();
          heading.classList.remove('is-swapping');
          badge.parentElement.classList.remove('is-swapping');
        }, 160);
      } else {
        write();
      }
    }

    function update(force) {
      if (!journey || paused || reduceMotion) {
        if (force) setPhase(2, false);
        return;
      }

      var jb = journey.getBoundingClientRect();
      // How far the sticky stage has travelled inside its own scroll range.
      var travel = navH - jb.top;
      var span = journey.offsetHeight - sticky.offsetHeight;
      var p = span > 0 ? clamp(travel / span) : 0;

      var draw = clamp((p - DRAW_START) / (DRAW_END - DRAW_START));
      var restore = clamp((p - RESTORE_START) / (RESTORE_END - RESTORE_START));

      journey.style.setProperty('--travel', p.toFixed(4));
      journey.style.setProperty('--draw', draw.toFixed(4));
      journey.style.setProperty('--restore', restore.toFixed(4));

      threadFill.style.strokeDashoffset = (length * (1 - draw)).toFixed(2);

      if (draw > 0 && draw < 1) {
        var pt = threadFill.getPointAtLength(length * draw);
        signal.setAttribute('cx', pt.x.toFixed(2));
        signal.setAttribute('cy', pt.y.toFixed(2));
        halo.setAttribute('cx', pt.x.toFixed(2));
        halo.setAttribute('cy', pt.y.toFixed(2));
      }

      // destState reads RECEIVED only at the end; mid-scrub it should not claim
      // to have arrived before the restore plan is actually showing.
      if (destState) {
        destState.classList.toggle('is-waiting', restore < 0.5);
      }

      setPhase(Math.min(2, Math.floor(p * 3)), false);
    }

    /* -------------------------------------------------- phase tab clicks -- */
    tabs.forEach(function (tab) {
      tab.addEventListener('click', function () {
        var index = parseInt(tab.dataset.phase, 10) || 0;
        var jb = journey.getBoundingClientRect();
        var span = journey.offsetHeight - sticky.offsetHeight;
        /* Land in the middle of the phase, not at its start. At the start of
           phase 3 the destination is still 38% restored and mostly blank, so
           clicking "Continue" showed a half-crossfaded screen and read as the
           button being broken. Mid-phase is also where the copy actually
           describes what is on screen: phase 1 is a partly-drawn thread, phase
           3 is a mostly-arrived workspace. */
        var target = (index + 0.5) / 3;
        var y = window.scrollY + jb.top - navH + target * span;
        window.scrollTo({
          top: Math.max(0, y),
          behavior: reduceMotion ? 'auto' : 'smooth',
        });
        setPhase(index, true);
      });
    });

    /* ------------------------------------------------------- pause switch -- */
    var toggle = document.getElementById('motion-toggle');

    function syncToggle() {
      if (!toggle) return;
      if (reduceMotion) {
        // Asking for less motion is not a starting position to be talked out of.
        // The control reports the state and refuses to offer the alternative.
        toggle.textContent = 'Motion reduced';
        toggle.setAttribute('aria-pressed', 'true');
        toggle.disabled = true;
        toggle.title = 'Your system asks for reduced motion, so animation stays off.';
      } else {
        toggle.textContent = paused ? 'Resume motion' : 'Pause motion';
        toggle.setAttribute('aria-pressed', String(paused));
        toggle.disabled = false;
      }
    }

    function setPaused(next) {
      paused = next;
      document.body.classList.toggle('motion-paused', paused);
      if (paused) {
        // Force the finished frame explicitly. The CSS block forces --travel
        // and --restore, but the dash offset and the dot position are written
        // as inline styles from script, and inline styles are what they are.
        threadFill.style.strokeDashoffset = '0';
        var end = threadFill.getPointAtLength(length);
        signal.setAttribute('cx', end.x.toFixed(2));
        signal.setAttribute('cy', end.y.toFixed(2));
        halo.setAttribute('cx', end.x.toFixed(2));
        halo.setAttribute('cy', end.y.toFixed(2));
        setPhase(2, false);
      } else {
        // Re-measure: pausing may have changed the stage's box.
        geometry();
      }
      syncToggle();
    }

    if (toggle) {
      toggle.addEventListener('click', function () {
        setPaused(!paused);
      });
    }

    if (reduceMotion) document.body.classList.add('motion-paused');

    // Honour a change to the preference while the page is open — macOS flips
    // it when the ambient light changes enough to trip auto appearance.
    var onReduceChange = function (e) {
      reduceMotion = e.matches;
      if (reduceMotion) {
        setPaused(true);
      } else {
        setPaused(false);
      }
    };
    if (typeof reduceQuery.addEventListener === 'function') {
      reduceQuery.addEventListener('change', onReduceChange);
    }

    navH = navHeight();
    syncToggle();

    // Fonts and images both change the measured layout. Resolving once after
    // load and again on the first late paint covers the two cases where the
    // first measurement is taken against fallback metrics.
    geometry();
    window.addEventListener('load', geometry);
    if (document.fonts && document.fonts.ready) document.fonts.ready.then(geometry);

    var remeasure;
    window.addEventListener(
      'resize',
      function () {
        navH = navHeight();
        clearTimeout(remeasure);
        remeasure = setTimeout(geometry, 120);
      },
      { passive: true }
    );

    journey.addEventListener('scrub', function () {
      update(false);
    });

    setPhase(0, false);
  }

  /* ------------------------------------------------------------ read bar -- */
  var progressBar = document.querySelector('[data-progress]');
  var frameQueued = false;

  function onScroll() {
    var doc = document.documentElement;
    var max = doc.scrollHeight - doc.clientHeight;
    var pct = max > 0 ? doc.scrollTop / max : 0;
    if (progressBar) progressBar.style.width = (pct * 100).toFixed(2) + '%';

    var nav = document.querySelector('[data-nav]');
    if (nav) {
      if (doc.scrollTop > 8) nav.setAttribute('data-stuck', '');
      else nav.removeAttribute('data-stuck');
    }

    if (frameQueued) return;
    frameQueued = true;
    requestAnimationFrame(function () {
      frameQueued = false;
      updateSteps();
      queueSweep();
      if (journey) journey.dispatchEvent(new CustomEvent('scrub'));
    });
  }

  /* -------------------------------------------------------------- reveal -- */

  // The card grids read better as one group than as four separate reveals
  // arriving at four different moments. This has to happen BEFORE the reveal
  // targets are collected below -- a group tagged after the observer has
  // already been handed its list is simply never watched, and stays at
  // opacity 0 for good.
  document.querySelectorAll('.split, .cards, .stats').forEach(function (group) {
    group.classList.add('reveal-stagger');
    group.classList.add('reveal');
  });

  var revealTargets = Array.prototype.slice.call(
    document.querySelectorAll('.reveal, .reveal-stagger, [data-shot]')
  );

  // Blocks still waiting to be shown. Shared with the scroll sweep below.
  var pendingReveals = [];

  function markRevealed(el) {
    el.classList.add('is-in');
    var i = pendingReveals.indexOf(el);
    if (i !== -1) pendingReveals.splice(i, 1);
  }

  if (reduceMotion || !('IntersectionObserver' in window)) {
    // No observer, or motion is unwelcome: everything is simply shown.
    revealTargets.forEach(markRevealed);
  } else {
    pendingReveals = revealTargets.slice();

    var revealObserver = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          markRevealed(entry.target);
          revealObserver.unobserve(entry.target);
        });
      },
      { rootMargin: '0px 0px -12% 0px', threshold: 0.12 }
    );
    revealTargets.forEach(function (el) {
      revealObserver.observe(el);
    });
  }

  /**
   * Safety net for skipped content.
   *
   * IntersectionObserver only reports what is intersecting at the instant it
   * runs. Dragging the scrollbar thumb, pressing End, or having the browser
   * restore a scroll position on reload can jump clean over a block, and that
   * block would then sit at opacity 0 forever -- real content, permanently
   * invisible, with nothing on the page to explain why. So every scroll also
   * sweeps for anything that has already reached the trigger line and shows it.
   */
  var sweepQueued = false;

  function sweepReveals() {
    sweepQueued = false;
    if (!pendingReveals.length) return;

    var line = window.innerHeight * 0.92;
    for (var i = pendingReveals.length - 1; i >= 0; i--) {
      if (pendingReveals[i].getBoundingClientRect().top <= line) {
        markRevealed(pendingReveals[i]);
      }
    }
  }

  function queueSweep() {
    if (sweepQueued || !pendingReveals.length) return;
    sweepQueued = true;
    requestAnimationFrame(sweepReveals);
  }

  /* ---------------------------------------------------------------- steps -- */
  var steps = Array.prototype.slice.call(document.querySelectorAll('[data-step]'));
  var rail = document.querySelector('[data-rail]');
  var activeStep = null;

  function setActive(el) {
    if (el === activeStep) return;
    activeStep = el;
    steps.forEach(function (s) {
      s.classList.toggle('is-active', s === el);
    });
  }

  /**
   * Which step is the reader actually on?
   *
   * Measured from the viewport midpoint rather than driven by an
   * IntersectionObserver, because an observer only reports what is intersecting
   * at the instant it runs — arrive by anchor link, by restored scroll
   * position, or by scrollbar drag and there may be no intersecting step at
   * all, leaving every step stuck at 32% opacity with no way back. Picking the
   * nearest always assigns one, so the section is never left half-dimmed.
   */
  function updateSteps() {
    if (!steps.length) return;

    var mid = window.innerHeight / 2;
    var best = steps[0];
    var bestDist = Infinity;

    steps.forEach(function (s) {
      var r = s.getBoundingClientRect();
      var d = r.top <= mid && r.bottom >= mid ? 0 : Math.min(Math.abs(r.top - mid), Math.abs(r.bottom - mid));
      if (d < bestDist) {
        bestDist = d;
        best = s;
      }
    });

    setActive(best);

    if (rail) {
      var host = steps[0].parentElement.getBoundingClientRect();
      var reached = Math.min(1, Math.max(0, (mid - host.top) / Math.max(1, host.height)));
      rail.style.height = (reached * 100).toFixed(2) + '%';
    }
  }

  /* ------------------------------------------------------------ count-up -- */
  var counters = document.querySelectorAll('[data-count]');

  function runCounter(el) {
    // The real figure is already in the markup, so this animates 0 -> N and a
    // reader without scripts still sees the number. Animating the other way —
    // writing 0 into the HTML and counting up — means the page claims zero of
    // everything for anyone it fails to reach.
    var target = parseInt(el.dataset.count, 10) || 0;
    if (reduceMotion || el.hasAttribute('data-count-literal') || target === 0) return;

    var start = performance.now();
    var dur = 1100;
    var tick = function (now) {
      var t = Math.min(1, (now - start) / dur);
      var eased = 1 - Math.pow(1 - t, 3);
      el.textContent = String(Math.round(target * eased));
      if (t < 1) requestAnimationFrame(tick);
    };
    el.textContent = '0';
    requestAnimationFrame(tick);
  }

  if ('IntersectionObserver' in window && counters.length) {
    var countObserver = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          runCounter(entry.target);
          countObserver.unobserve(entry.target);
        });
      },
      { threshold: 0.6 }
    );
    counters.forEach(function (el) {
      countObserver.observe(el);
    });
  }

  /* ------------------------------------------------------------ lightbox -- */
  /* Native <dialog>, so focus trapping, the backdrop, Escape and inertness of
     the page behind it are the platform's job rather than a reimplementation. */
  var shots = document.querySelectorAll('[data-shot]');

  if (shots.length && typeof HTMLDialogElement === 'function') {
    var dlg = document.createElement('dialog');
    dlg.className = 'shot-view';
    dlg.innerHTML =
      '<button type="button" class="shot-view__close" aria-label="Close">Close</button>' +
      '<img alt="" />' +
      '<p class="shot-view__cap"></p>';
    document.body.appendChild(dlg);

    var dlgImg = dlg.querySelector('img');
    var dlgCap = dlg.querySelector('.shot-view__cap');

    dlg.querySelector('.shot-view__close').addEventListener('click', function () {
      dlg.close();
    });

    // Clicking the backdrop closes. The dialog's own box is the only child
    // target, so a click landing on ::backdrop reports the dialog itself.
    dlg.addEventListener('click', function (e) {
      if (e.target === dlg) dlg.close();
    });

    shots.forEach(function (btn) {
      btn.addEventListener('click', function () {
        var img = btn.querySelector('img');
        var cap = btn.querySelector('.shot__cap');
        if (!img) return;
        dlgImg.src = img.currentSrc || img.src;
        dlgImg.alt = img.alt || '';
        dlgCap.textContent = cap ? cap.textContent : '';
        dlg.showModal();
      });
    });
  }

  /* ---------------------------------------------------------------- copy -- */
  document.querySelectorAll('[data-copy-btn]').forEach(function (btn) {
    btn.addEventListener('click', function () {
      var host = btn.closest('[data-copy]');
      var code = host ? host.querySelector('code') : null;
      if (!code) return;

      var text = code.innerText;
      var done = function () {
        var original = btn.textContent;
        btn.textContent = 'Copied';
        setTimeout(function () {
          btn.textContent = original;
        }, 1600);
      };

      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () {
          fallbackCopy(text, done);
        });
      } else {
        fallbackCopy(text, done);
      }
    });
  });

  function fallbackCopy(text, done) {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand('copy');
      done();
    } catch (err) {
      /* clipboard unavailable — the text is on screen to copy by hand */
    }
    document.body.removeChild(ta);
  }

  /* ---------------------------------------------------------------- boot -- */
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('resize', onScroll, { passive: true });

  var hero = document.querySelector('.motion-hero');
  if (hero) {
    // two frames later, so the transition has a starting state to animate from
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        hero.classList.add('is-ready');
      });
    });
  }

  onScroll();
})();
