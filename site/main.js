/* ==========================================================================
   StackHandoff — site behaviour

   Everything here is progressive enhancement. The page is fully readable with
   this file blocked: every download link has a real href in the markup, the
   screenshot <img> tags carry width/height and alt text, and the copy buttons
   are real <button>s. Nothing is revealed by JavaScript that is not already
   visible without it.
   ========================================================================== */
(function () {
  'use strict';

  var reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ---------------------------------------------------------------------
     Downloads.

     Releases rather than files in this repo: a .dmg and an .exe committed to
     git on every build is a lot of churn for two binaries that only change
     when the app does.

     ASSET NAMES MUST MATCH THE RELEASE EXACTLY. GitHub's
     /releases/latest/download/<name> endpoint is a literal file lookup — a
     mismatch is a 404, not a redirect. Changing a name here means renaming the
     uploaded asset in the release, or the button silently stops working.
     --------------------------------------------------------------------- */
  var REPO = 'abhishekdana1999/workspace-clone';
  var RELEASE_LATEST = 'https://github.com/' + REPO + '/releases/latest';
  var RELEASE_FILE = 'https://github.com/' + REPO + '/releases/latest/download/';

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
      '<path d="M12 3v12M12 15l-4-4M12 15l4-4"/><path d="M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2"/>' +
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

  /* ------------------------------------------------------------ read bar -- */
  var progressBar = document.querySelector('[data-progress]');
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

    if (!reduceMotion) parallax();
    queueSweep();
  }

  /* ------------------------------------------------------------- parallax -- */
  var parallaxHost = document.querySelector('[data-parallax]');
  function parallax() {
    if (!parallaxHost) return;
    var rect = parallaxHost.getBoundingClientRect();
    if (rect.bottom < -200 || rect.top > window.innerHeight + 200) return;

    var centre = window.innerHeight / 2;
    var delta = (rect.top + rect.height / 2 - centre) / centre; // -1 .. 1

    parallaxHost.style.transform = 'translateY(' + (delta * -14).toFixed(2) + 'px)';

    document.querySelectorAll('[data-float]').forEach(function (el) {
      var rate = parseFloat(el.dataset.float) || 0;
      el.style.transform = 'translateY(' + (delta * rate * -40).toFixed(2) + 'px)';
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

  if (steps.length) {
    var setActive = function (el) {
      steps.forEach(function (s) {
        s.classList.toggle('is-active', s === el);
      });
    };
    steps[0].classList.add('is-active');

    if ('IntersectionObserver' in window) {
      var stepObserver = new IntersectionObserver(
        function (entries) {
          // Pick whichever step covers the middle of the viewport. A plain
          // "isIntersecting" toggle flickers between two steps while the seam
          // between them is on screen.
          var mid = window.innerHeight / 2;
          var best = null;
          var bestDist = Infinity;
          entries.forEach(function (entry) {
            if (!entry.isIntersecting) return;
            var r = entry.target.getBoundingClientRect();
            var centre = r.top + r.height / 2;
            var dist = Math.abs(centre - mid);
            if (dist < bestDist) {
              bestDist = dist;
              best = entry.target;
            }
          });
          if (best) {
            setActive(best);
            if (rail) {
              var host = best.parentElement.getBoundingClientRect();
              var reached = Math.min(
                1,
                Math.max(0, (mid - host.top) / Math.max(1, host.height))
              );
              rail.style.height = (reached * 100).toFixed(2) + '%';
            }
          }
        },
        { rootMargin: '-45% 0px -45% 0px', threshold: 0 }
      );
      steps.forEach(function (s) {
        stepObserver.observe(s);
      });
    }
  }

  /* ------------------------------------------------------------ count-up -- */
  var counters = document.querySelectorAll('[data-count]');

  function runCounter(el) {
    var target = parseInt(el.dataset.count, 10) || 0;
    if (reduceMotion || target === 0) {
      el.textContent = String(target);
      return;
    }
    var start = performance.now();
    var dur = 1100;
    var tick = function (now) {
      var t = Math.min(1, (now - start) / dur);
      var eased = 1 - Math.pow(1 - t, 3);
      el.textContent = String(Math.round(target * eased));
      if (t < 1) requestAnimationFrame(tick);
    };
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

  /* -------------------------------------------------------------- ticker -- */
  var tickerRow = document.getElementById('tickerRow');
  if (tickerRow) {
    var phrases = [
      'Noise_IK · X25519 · AEAD',
      'mDNS discovery, no server',
      '419 Rust tests',
      '102 frontend tests',
      'working-tree changes travel as a patch',
      'env var names only — never values',
      'no accounts, no sign-up',
      'macOS ↔ Windows',
      'every step reviewed before it runs',
    ];
    // Two identical runs so the -50% translate loops seamlessly.
    var one = phrases
      .map(function (p) {
        return '<span>' + p + '</span>';
      })
      .join('');
    tickerRow.innerHTML = one + one;
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
  onScroll();

  var hero = document.querySelector('.hero');
  if (hero) {
    // one frame later, so the transition has a starting state to animate from
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        hero.classList.add('is-ready');
      });
    });
  }
})();
