# Hyperframes Composition Brief: Workspace Clone

## Objective
Create a short launch-style brag video for Workspace Clone — a peer-to-peer
desktop app that captures a dev environment on one laptop and restores it on
another.

## Output
- Composition directory: `brag-output/composition/`
- Rendered video: `brag-output/brag.mp4`
- Format: landscape — 1920x1080
- Duration: 21.4 seconds

## Source Material
- Project root: `/Users/abhishekdana/Documents/openshorts`
- Primary files read: `README.md`, `docs/DEVELOPER_GUIDE.md`, `src/styles/index.css`
  (design tokens), `src/screens/*` (Capture / Transfer / RestorePreview flows)
- Product name: **Workspace Clone**
- Tagline / strongest claim: "Pick up right where you left off — on the other laptop."
- **Product visuals are real screenshots, not recreations.** Capture them with
  `scripts/capture-shots.mjs` (puppeteer-core → system Chrome) against the dev
  server on :1420, using the browser-only Tauri IPC mock in `src/dev/mockTauri.ts`
  to seed the Capture / Transfer / Restore flows. Screens land in
  `assets/shots/` and are used as backgrounds (never `Math.random`, no network at
  render time — local PNGs only, so the render stays deterministic).
- Key UI to show: the Capture screen (workspace name, project rows with git
  state, app adapters), the Transfer destinations card with the target machine
  selected, the Restore preview plan, and the Restore report with every step Done.
- **Masking:** fixtures use generic device names — source `MacBook Pro`, target
  `Office PC` — plus neutral paths/accounts (`/Users/developer/...`,
  `github.com/developer/workspace-clone`). No real machine, user, or host name
  may appear on screen while the names are masked.
- Copy that must appear verbatim:
  - "gate: git status: exactly what you left" (paraphrase as UI mono row)
  - The real screen's own copy is the source of truth; do not re-typeset app text.

## Creative Direction
- Tone preset: `app-store`
- Creative direction: quiet premium product film — your dev session, packed like carry-on
- Interpretation: clean feature-card structure, medium-weight type, slide
  transitions (0.35-0.45s), consistent light SFX layer, serious and specific.
- Angle: the video is the product's real flow — Capture → Send → Restore —
  built from two laptop glyphs, a sealed packet, and the actual app UI.
- Hook: "Close your laptop." → sealed packet flies to the other machine →
  "Open the other one."
- Outro / punchline: logo card "Workspace Clone" + "Pick up right where you
  left off — on the other laptop.", landed on a strong musical cue.
- Avoid:
  - Generic SaaS language ("streamline", "workflow")
  - Abstract filler visuals
  - Unrelated visual redesign (the window must read as the real macOS app)

## Visual Identity
- Background: `#0f0f12` canvas (`--surface-canvas` dark), soft indigo center glow
- Text: `#f4f4f5` on dark; `#18181b` on the light window; status green `#15803d`
- Accent: indigo `#4f46e5` (`--primary 243 75% 59%`)
- Display font: system stack `-apple-system, ui-sans-serif, system-ui, sans-serif`
- Mono font: `ui-monospace, SFMono-Regular, Menlo, monospace`
- Visual references: cool neutral surfaces, 4px radii, indigo accent, status
  green = "ready/applied" (never used decoratively), light mode window on dark canvas

## Storyboard
Use `<output-dir>/brag-plan.md` as the creative contract.

Scene summary:
1. The promise — 3.0s — two laptops, "Close your laptop." / packet flight / "Open the other one."
2. Capture — 5.2s — real Capture screen assembles in four beat-synced strips
3. Send — 4.4s — real destinations card; packet crosses; ghost window receives; green "Received" check
4. Restore — 6.0s — real restore plan in four strips → crossfade to the real report; mono git-status payoff
5. Outro — 3.2s — logo card lands at 20.02s (beat-locked); bed fades

## Audio
- Audio role: warm upbeat bed + consistent light SFX layer
- Audio arc: quiet pop opens → productive ticks through the capture/restore →
  one bell + fade on the logo
- Music: `happy-beats-business-moves-vol-1-by-ende-dot-app.mp3`
- Music treatment: volume 0.35, from 0.0s, `data-fade-out` 1.2s over the outro
- Music cue guidance: bundled preset
  `assets/music/cues/happy-beats-business-moves-vol-1-by-ende-dot-app.music-cues.json`
  (copied alongside the track). Lock outro to 20.02s strong cue (1.00).
  Beat grid ~ every 0.5s; sequential text rows snap to every-other-beat windows
  (≈1.0s apart) to honour the reading floor: 4.02/5.03/6.03/7.02 (capture rows),
  10.52 (received), 13.52/14.52/15.52/16.52 (restore rows).
- Audio-reactive treatment: none (extraction helper unavailable in this
  standalone install; does not block render)
- Audio-coupled moments:
  - hook line 1 — drop_001 @0.15
  - packet flight — casino/card-slide-1 @0.95 (scene 1) and @9.1 (scene 3)
  - hook line 2 — drop_002 @2.05
  - window reveal — impactSoft_medium_001 @3.05
  - capture rows — drop_001 each @4.02/5.03/6.03/7.02
  - packet landing — casino/card-place-1 @10.4
  - received check — impactPlate_light_000 @10.52
  - restore step flips — switch_001 each @13.52/14.52/15.52/16.52
  - mono payoff line — interface/select_008 @17.5
  - final logo — impactBell_heavy_000 @19.9
- SFX selection guidance: light app-store layer (0.6-0.75 volume); sounds match
  motion (pop = row/props arrival, switch = status flip, bell = logo). All
  clipped at their animation start. SFX analysis guidance:
  `assets/sfx/sfx-analysis.json` (copied beside the audio library copy in this
  brief's assets). Prefer low high-frequency-risk files for the repeated pops.
- Exact SFX choice: Hyperframes chooses filenames/timestamps/density from the
  listed moment types.
- Audio files: copy the chosen music + SFX into `assets/` of the composition.

## Hyperframes Instructions
Load the Hyperframes domain skills — `hyperframes-core` (composition contract +
`data-*` timing), `hyperframes-animation` (motion), `hyperframes-creative`
(design spec), `hyperframes-cli` (lint/check/render). /brag is its own workflow:
do not enter the `hyperframes` entry-point intent interview and do not route into
its generic promo / launch-video workflow.

Requirements:
- Show real UI from the source project — use captured screenshots of the real
  app in `assets/shots/`, not a hand-built recreation of the interface.
- Keep all text readable in the final render.
- Keep the video within 15-25 seconds (target 21.4s).
- Include the planned music/SFX layer.
- Treat music cue metadata as optional timing hints; ignore cues that hurt
  readability or pacing.
- Use static ~subtle deterministic motion only (no audio-reactive, no clocks,
  no Math.random, no network).
- Run `npx hyperframes check` before render — it is brag's single gate.