# StackHandoff — site

The promotional site. Static HTML, CSS and JavaScript: no build step, no framework,
no dependencies. Open `index.html` through any web server and it works, which is
the point — there is nothing here that can rot.

```
site/
  index.html          all content, all copy
  styles.css          design tokens, layout, components
  motion.css          the motion layer: hero, the handoff, reveals, reduced motion
  main.js             platform detection, the scroll scrub, clipboard, lightbox
  assets/shots/       real screenshots of the app (generated, not committed by hand)
```

`styles.css` and `motion.css` are separate on purpose. "What it looks like" and
"what it does" are different questions, and keeping the choreography in one file
means the sequence can be read top to bottom as a sequence instead of hunted for
across selectors.

## Running it

```sh
cd site && python3 -m http.server 4321
```

`scripts/capture-shots.mjs` needs a dev server on **1420** (`npm run dev`); this
one on 4321 is only for looking at the site.

---

## The handoff

The centrepiece is `#journey`: a 290svh section wrapped around a sticky stage, and
the page scrolls you through a handoff between two machines in one continuous take
rather than presenting it as four panels you click through.

It works by writing **three numbers** onto the section as you scroll, and letting
the stylesheet turn them into motion:

| Property | Range | Drives |
| --- | --- | --- |
| `--travel` | 0 → 1 | overall progress; the source's drift |
| `--draw` | 0 → 1 | how much of the connection has been drawn |
| `--restore` | 0 → 1 | the destination's cross-fade from "paired, waiting" to "received" |

```js
journey.style.setProperty('--travel', p.toFixed(4));
journey.style.setProperty('--draw', draw.toFixed(4));
journey.style.setProperty('--restore', restore.toFixed(4));
```

```css
.destination-body > img   { opacity: var(--restore); }
.waiting-state            { opacity: calc(1 - var(--restore)); }
```

One custom-property write per frame instead of a pile of transform strings: the
browser owns the interpolation, and nothing reflows.

**The thread is measured, not hardcoded.** The curve between the two device screens
is built from their real `getBoundingClientRect()` edges, and the SVG `viewBox` is
set to the stage's pixel box so the mapping is exactly 1:1. `getPointAtLength()`
therefore returns page pixels and the signal dot can be placed with a plain `cx`.
That is why the line still lands on both bezels after a resize, a font swap, or the
mobile layout flipping the nodes from side-by-side to stacked — the geometry
function re-runs and picks a horizontal or a vertical curve accordingly.

### What depends on script, and what happens without it

The journey is the only part of the page that needs JavaScript to be legible. The
`<html>` element carries `class="no-js"`, removed by a one-line inline script. If it
survives, the CSS collapses the section to its **last frame** at natural height: the
finished handoff as a static diagram.

The same three variables are forced under `prefers-reduced-motion`, for the same
reason — a sticky 290svh stage for a sequence nobody can watch animate is 290svh of
scrolling through a still image.

Both need `!important` on the custom properties. `main.js` writes them as **inline**
styles, and an inline declaration outranks any selector in a stylesheet regardless
of specificity.

### The pause control

`#motion-toggle` is a first-class state, not an error path. Pausing forces
`--restore: 1` and sets the dash offset to zero explicitly, so the paused frame shows
a completed handoff rather than two devices and an undrawn line.

Under `prefers-reduced-motion` the control reports `Motion reduced`, sets
`aria-pressed="true"` and is **disabled**. Asking for less motion is not a starting
position to be talked out of.

---

## Downloads: how platform detection works

`main.js` reads the platform in this order and stops at the first hit:

1. `navigator.userAgentData.platform` — the only non-deprecated source, Chromium only
2. `navigator.platform` — deprecated, still populated everywhere
3. the `userAgent` string, as a last resort

`navigator.platform` is preferred over sniffing `userAgent` deliberately: the UA
string on Windows also mentions "Mac" in some embedded webviews, so a Mac-keyword
match on `userAgent` alone mis-fires there.

Everything is at the top of `main.js`:

```js
var REPO = 'abhishekdana1999/workspace-clone';
var PUBLISHED = false;
var BUILDS = {
  mac: { href: ..., label: 'Download for macOS', filename: 'StackHandoff.dmg', ... },
  win: { href: ..., label: 'Download for Windows', filename: 'StackHandoff.exe', ... },
};
```

Detection is a convenience, never a dead end. An unrecognised platform falls back to
the releases page, and the macOS/Windows buttons in the download section override
detection for anyone who needs the other build. All three CTAs — nav, hero, and the
download panel — stay in sync.

`PUBLISHED` is `false` as shipped. While it is false the page **says so** below the
button rather than offering a confident button that 404s on click. Flip it to `true`
once the release exists; that is the only edit needed.

---

## Publishing the downloads — read this before going live

**The site points at GitHub Releases, and this repository has no release yet.**

The URLs are built from a literal filename:

```
https://github.com/<repo>/releases/latest/download/StackHandoff.dmg
https://github.com/<repo>/releases/latest/download/StackHandoff.exe
```

`releases/latest/download/<name>` is a **literal file lookup, not a redirect**. If the
uploaded asset is not named exactly `StackHandoff.dmg`, the link is a 404 — and it
would look like a working button right up until someone clicked it.

Tauri does not produce those names. It produces versioned ones:

| Built artefact | What the site expects |
| --- | --- |
| `StackHandoff_0.1.0_aarch64.dmg` | `StackHandoff.dmg` |
| `StackHandoff_0.1.0_x64-setup.exe` | `StackHandoff.exe` |
| `StackHandoff_0.1.0_en-US.msi` | *(not linked)* |

The **Release installers** GitHub Actions workflow publishes both assets under
the canonical names. Run it manually from the commit you want to release; it
reads `package.json`, creates the `v<version>` tag on that commit, builds the
Apple silicon DMG and Windows NSIS installer, then creates or updates the GitHub
Release. A pushed `v*` tag is also accepted, but it must exactly match the
`package.json` version. The package and Tauri config versions must match too.

After the first release is published, set `PUBLISHED = true` in
`site/main.js`; until then the site intentionally keeps installer downloads
disabled rather than linking visitors to a missing release.

The workflow stages the versioned Tauri outputs as `StackHandoff.dmg` and
`StackHandoff.exe` before uploading them, keeping these download URLs stable
across app versions.

Two constraints worth knowing:

- `releases/latest` only resolves for a **published** release. A draft or a
  prerelease is invisible to it, and the buttons break.
- `.github/workflows/windows-installer.yml` also runs on pushed `v*` tags and
  publishes the matching GitHub Release from CI.

### If you would rather keep Tauri's names

Change the three `href`/`filename` values at the top of `main.js` to match whatever
you upload. Just keep the filename and the URL in agreement; nothing checks them for
you at runtime.

### The Mac installer is not notarized

`bundle.macOS.signingIdentity` is `"-"` — ad-hoc. macOS Gatekeeper will block the app
on another machine, and the user has to right-click → Open once. That is fine for
your own two machines and not fine for a public download button. Real notarization
needs an Apple Developer account and `APPLE_ID` / `APPLE_APP_PASSWORD` /
`APPLE_TEAM_ID` in the build environment.

---

## Regenerating the screenshots

Every screenshot on the page is the real application, captured from the running app.
None of them are illustrations. The capture screen and restore-plan screen are reused
inside the journey diagram as the two device screens, which is why the section is
worth regenerating rather than approximating.

```sh
npm run dev                          # must be serving on 1420
SHOTS_OUT=site/assets/shots node scripts/capture-shots.mjs
```

`SHOTS_OUT` is why `capture-shots.mjs` no longer hardcodes `brag-output/shots` — one
capture run can feed more than one consumer. Re-run this whenever the app UI changes,
or the site starts describing an older version of the product.

The captures run against `src/dev/mockTauri.ts` fixtures, so they are stable and
reproducible rather than dependent on whatever state your machine happens to be in.
Device names in those fixtures are masked.

---

## Deploying

Every path in `index.html` is relative, so the site works from a domain root (`/`) or
a subpath (`/workspace-clone/`) with no configuration. That makes it a straight drop
onto GitHub Pages, Netlify, Cloudflare Pages, or any static host.

If you use Pages, publish the **`site/`** directory as the source.

---

## Design notes

The palette is lifted from the app's own tokens (`src/styles/index.css`). That is the
point: the site and the product have to read as one thing, and the previous complaint
about the launch video was that it did not look like the UI.

The treatment is **dark-first with a real light scheme**. The token names are
identical across both, so the entire motion layer works against either unchanged.
Both are implemented; neither is a `prefers-color-scheme` afterthought.

Two rules are held to without exception, because they are what make it look like the
product rather than a generic SaaS page:

- **No drop shadows.** `scripts/audit-site.mjs` fails the build if one appears. Depth
  is tonal layering plus a 1px hairline.
- **No pills.** Radii are 4px, 6px and 8px. The only circle is the step number.

Two tokens the app does not need but a cinematic page does:

- **`--signal`.** A green reserved exclusively for "this is moving / this
  succeeded" — the drawn thread, the received state, the pulse on the travelling
  dot. Never decorative. It is the one place a visitor looks to answer *is it
  working?*
- **`--fill` / `--solid`.** Indigo as a **background** behind white text. Distinct
  from `--accent`, which inverts to a light lilac on dark and would measure about
  2.6:1 behind white. `--fill` is `#5a53e2` rather than `--solid`'s `#433cc4`
  because a 15px/600 label still counts as body text for WCAG, and 5.6:1 reads
  lighter on a 54px button than 7.8:1 does on a 900px panel.

Two tokens **diverged** from the app, both because they carry the smallest text on
the page:

- `--fg-subtle` and `--fg-faint` are darkened. The app's `#788191` is 3.7:1 on white,
  fine for the 13px body grey it was used for and short for the 9–11px mono labels in
  the journey — phase captions, device tags, the transfer footer. Those clear 4.5:1
  on every surface they land on, including the sunken one inside a device frame. The
  relationship between the two greys is unchanged, so the hierarchy survives.

One deliberate departure, because this is a marketing surface rather than a tool:
**larger type**. The app runs at a 13px base because a list has to fit a window. A
hero set at 13px is not a hero.

---

## Auditing

```sh
node scripts/audit-site.mjs
```

**Thirty-four configurations**: four widths × two colour schemes, with the journey
scrubbed and measured at three positions inside each — plus reduced motion.

It checks:

- **Contrast** on the text that actually carries meaning, with alpha composited
  rather than ignored. Reading `rgba(255,255,255,.06)` as solid white reports a
  background that does not exist and turns the whole audit into noise.
- **The journey's cross-fade arithmetic.** At three scrub positions per page: the
  destination screenshot's opacity must equal `--restore`, the waiting overlay must
  equal `1 - --restore`, the sequence must start un-restored and end restored, and the
  connection must actually be drawn by the midpoint. Measuring once at the top of the
  page can only prove the initial state.
- **The thread has a usable path** and its stage clips nothing below its own box.
- **Nothing left invisible** by a reveal animation.
- **Exactly one active step**, always — see the bug below.
- **No horizontal overflow**, no drop shadows, no pill radii.
- **Every screenshot decoded, described, and keyboard reachable** with a visible
  focus ring.
- **Reduced motion**: the journey collapses below 1.5 viewports of height, the stage
  is not pinned, the destination is not left blank, the phase tabs are gone, the
  toggle refuses to offer to enable motion, and `document.getAnimations()` reports
  **nothing still running**.

`prefers-color-scheme` and `prefers-reduced-motion` cannot be forced from page
JavaScript, so this drives real Chrome media emulation via `puppeteer-core`. Asserting
dark mode works without ever having rendered it proves nothing.

It exits non-zero on failure, so it is usable as a pre-commit or CI gate.

### Bugs it exists to catch

All of these were real, and all are the kind review by eye does not find:

- **Reveals stranded content.** `IntersectionObserver` only reports elements
  intersecting at the instant it runs. A scrollbar drag, the End key, or a restored
  scroll position can jump clean over a block, leaving it at `opacity: 0`
  permanently. `main.js` therefore also sweeps on scroll for anything already past
  the trigger line.
- **Grid overflow.** Grid children default to `min-width: auto` and refuse to shrink
  below their content, so one long line inside a scrollable `<pre>` widened the
  document by 45px on a 390px phone. Fixed with `min-width: 0` on the track children.
- **The thread detached from the frames.** The obvious way to animate the handoff is
  to translate the whole `.device-node` — and `main.js` measures those frames to find
  where to anchor the thread, so a moved frame and a static line came up to 14px
  apart, visibly, at exactly the moment the transfer starts. The drift now lives on
  the device *content*, which is not a connection point.
- **Steps dimmed with no way back.** Step highlighting driven by an
  `IntersectionObserver` leaves *no* step active if none happens to intersect the
  middle band — arrive by anchor link or by restored scroll position and every step
  sits at 32% opacity. Now measured from the viewport midpoint, which always assigns
  exactly one.
- **No-JS left the journey a blank box.** With scripting off nothing writes
  `--restore`, so the destination screenshot sat permanently invisible behind its
  waiting overlay — and because the device screens are flex children sized by
  whatever the stage leaves over, they also collapsed to **zero height**. Both fixed
  by the `no-js` block, with the same `!important` the reduced-motion block needs.
- **Counters claimed zero.** The stats animated 0 → N with the HTML holding a literal
  `0`, so anyone the script failed to reach saw a page stating zero of everything.
  The real figures are in the markup; the counter animates *from* zero.
