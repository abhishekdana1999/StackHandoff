# StackHandoff — site

The promotional site. Static HTML, CSS and JavaScript: no build step, no
framework, no dependencies. Open `index.html` through any web server and it
works, which is the point — there is nothing here that can rot.

```
site/
  index.html          all content, all copy
  styles.css          design tokens + layout + motion
  main.js             platform detection, scroll behaviour, clipboard
  assets/shots/       real screenshots of the app (generated, not committed by hand)
```

## Running it

```sh
cd site && python3 -m http.server 4321
```

`scripts/capture-shots.mjs` needs a dev server on **1420** (`npm run dev`); this
one on 4321 is only for looking at the site.

---

## Downloads: how platform detection works

`main.js` reads the platform in this order and stops at the first hit:

1. `navigator.userAgentData.platform` — the only non-deprecated source, Chromium only
2. `navigator.platform` — deprecated, still populated everywhere
3. the `userAgent` string, as a last resort

`navigator.platform` is preferred over sniffing `userAgent` deliberately: the UA
string on Windows also mentions "Mac" in some embedded webviews, so a
Mac-keyword match on `userAgent` alone mis-fires there.

Everything is at the top of `main.js`:

```js
var REPO = 'abhishekdana1999/workspace-clone';
var BUILDS = {
  mac: { href: ..., label: 'Download for macOS', filename: 'StackHandoff.dmg', ... },
  win: { href: ..., label: 'Download for Windows', filename: 'StackHandoff.exe', ... },
};
```

Detection is a convenience, never a dead end. An unrecognised platform falls back
to the releases page, and the macOS/Windows buttons in the download section
override detection for anyone who needs the other build.

---

## Publishing the downloads — read this before going live

**The site points at GitHub Releases, and this repository has no release yet, so
both download buttons currently 404.** That is the one thing standing between
this site and a working download.

The URLs are built from a literal filename:

```
https://github.com/<repo>/releases/latest/download/StackHandoff.dmg
https://github.com/<repo>/releases/latest/download/StackHandoff.exe
```

`releases/latest/download/<name>` is a **literal file lookup, not a redirect**. If
the uploaded asset is not named exactly `StackHandoff.dmg`, the link is a 404 —
and it will look like a working button right up until someone clicks it.

Tauri does not produce those names. It produces versioned ones:

| Built artefact | What the site expects |
| --- | --- |
| `StackHandoff_0.1.0_aarch64.dmg` | `StackHandoff.dmg` |
| `StackHandoff_0.1.0_x64-setup.exe` | `StackHandoff.exe` |
| `StackHandoff_0.1.0_en-US.msi` | *(not linked)* |

So the release has to be published under the canonical names. Once:

```sh
# 1. build the Mac installer
npm run tauri build
#    -> src-tauri/target/release/bundle/dmg/StackHandoff_0.1.0_aarch64.dmg

# 2. build the Windows installer, on a Windows machine (Git Bash)
scripts/build_windows.sh
#    -> src-tauri/target/<triple>/release/bundle/nsis/StackHandoff_0.1.0_x64-setup.exe

# 3. tag, so "latest" has something to point at
git tag v0.1.0 && git push origin v0.1.0

# 4. create the release with the renamed assets
gh release create v0.1.0 \
  src-tauri/target/release/bundle/dmg/StackHandoff_0.1.0_aarch64.dmg#StackHandoff.dmg \
  path/to/StackHandoff_0.1.0_x64-setup.exe#StackHandoff.exe \
  --title "StackHandoff 0.1.0"
```

The `#name` suffix is GitHub's asset-rename syntax and is the entire trick.

Two constraints worth knowing:

- `releases/latest` only resolves for a **published** release. A draft or a
  prerelease is invisible to it, and the buttons break.
- `.github/workflows/windows-installer.yml` also runs on a `v*` tag, so tagging
  `v0.1.0` builds the Windows installer on CI. That path is **unverified** — the
  workflow has never executed. The manual build above is the known-good route.

### If you would rather keep Tauri's names

Change the three `href`/`filename` values at the top of `main.js` to match
whatever you upload. Just keep the filename and the URL in agreement; nothing
checks them for you at runtime.

### The Mac installer is not notarized

`bundle.macOS.signingIdentity` is `"-"` — ad-hoc. macOS Gatekeeper will block the
app on another machine, and the user has to right-click → Open once. That is fine
for your own two machines and not fine for a public download button. Real
notarization needs an Apple Developer account and `APPLE_ID` /
`APPLE_APP_PASSWORD` / `APPLE_TEAM_ID` in the build environment.

---

## Regenerating the screenshots

Every screenshot on the page is the real application, captured from the running
app. None of them are illustrations.

```sh
npm run dev                          # must be serving on 1420
SHOTS_OUT=site/assets/shots node scripts/capture-shots.mjs
```

`SHOTS_OUT` is why `capture-shots.mjs` no longer hardcodes `brag-output/shots` —
one capture run can feed more than one consumer. Re-run this whenever the app UI
changes, or the site starts describing an older version of the product.

The captures run against `src/dev/mockTauri.ts` fixtures, so they are stable and
reproducible rather than dependent on whatever state your machine happens to be
in. Device names in those fixtures are masked.

---

## Deploying

Every path in `index.html` is relative, so the site works from a domain root
(`/`) or a subpath (`/workspace-clone/`) with no configuration. That makes it a
straight drop onto GitHub Pages, Netlify, Cloudflare Pages, or any static host.

If you use Pages, publish the **`site/`** directory as the source.

---

## Design notes

The palette is lifted verbatim from the app's own tokens
(`src/styles/index.css`). That is the point: the site and the product have to read
as one thing, and the previous complaint about the launch video was that it did
not look like the UI.

Two rules are held to without exception, because they are what make it look like
the product rather than a generic SaaS page:

- **No drop shadows.** `scripts/audit-site.mjs` fails the build if one appears.
  Depth is tonal layering plus a 1px hairline.
- **No pills.** Radii are 4px, 6px and 10px. The only circle is the step number.

Two deliberate departures, both because this is a marketing surface rather than a
tool:

- **Larger type.** The app runs at a 13px base because a list has to fit a
  window. A hero set at 13px is not a hero.
- **One extra token, `--primary-solid`.** In dark mode `--primary` becomes a
  *light* indigo, because there it is used as a foreground on dark surfaces.
  Handing that same value to the large filled download panel behind white text
  measured 2.6:1. `--primary-solid` is the value to use when indigo is a
  background, in either scheme.

One token is not the app's: `--fg-subtle` is `#676f7e` here rather than the app's
`#788191`, which is only 3.7:1 on the canvas — acceptable for a 13px label in a
dense tool, short of AA for the 11–12px captions used on a marketing page. The
relationship to `--fg-muted` is unchanged, so the hierarchy survives.

---

## Auditing

```sh
node scripts/audit-site.mjs
```

Nine configurations: four widths in both colour schemes, plus reduced motion.
It checks colour contrast with alpha composited rather than ignored, that nothing
is left invisible by a reveal animation, that there is no horizontal overflow, that
no drop shadow crept in, and that every screenshot decoded and carries a real
description.

`prefers-color-scheme` and `prefers-reduced-motion` cannot be forced from page
JavaScript, so this drives real Chrome media emulation via `puppeteer-core`.
Asserting dark mode works without ever having rendered it proves nothing.

It exits non-zero on failure, so it is usable as a pre-commit or CI gate.

### Two bugs it exists to catch

Both were real, and both are the kind that review by eye does not:

- **Reveals stranded content.** `IntersectionObserver` only reports elements
  intersecting at the instant it runs. A scrollbar drag, the End key, or a
  restored scroll position can jump clean over a block, leaving it at
  `opacity: 0` permanently. `main.js` therefore also sweeps on scroll for
  anything already past the trigger line.
- **Grid overflow.** Grid children default to `min-width: auto` and refuse to
  shrink below their content, so one long line inside a scrollable `<pre>` widened
  the document by 45px on a 390px phone. Fixed with `min-width: 0` on the track
  children.
