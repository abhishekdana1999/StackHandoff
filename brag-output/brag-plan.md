# Brag Plan: Workspace Clone

## What is this app?
Workspace Clone is a peer-to-peer desktop app that captures a *description* of
your dev environment on one laptop — open project folders, git branches, tools,
environment variables, signed-in accounts — and rebuilds that setup on another
machine, over a direct encrypted connection with no server and no cloud.

## The angle
A quiet, confident product film. Your whole dev session travels between two
laptops like carry-on luggage — because that is literally what it does. The
video is built entirely from the product's real flow: two laptop glyphs, a
sealed packet crossing between them, and the actual app window doing
Capture → Send → Restore.

## Hook (first 2-3 seconds)
Two laptop outlines on a cool dark canvas. Display text slams in, one line at a
time: **"Close your laptop."** — a sealed packet flies from the left machine to
the right — **"Open the other one."** The setup followed you.

## Key moments (the middle)
- **The manifest fills in.** The captured app window is the real Capture screen —
  the workspace name field, the two project rows with their git state, the app
  adapters — arriving in four horizontal strips, one per beat, until the whole
  window is standing there.
- **The transfer lands.** The packet crosses to the second machine and a green
  "Received" check pops; the send window beside it is the real destinations
  card with the paired machine selected. Copy: direct between machines, no cloud.
- **The delta survives.** The restore-preview assembles the same way, then
  crossfades to the real restore report — every step Done — landing on a mono
  `git status` bar: the uncommitted edits travelled as a patch, so `git status`
  shows exactly what you left.

## Screenshot provenance
Every product surface in the film is a **real screenshot of the app**, captured
by `scripts/capture-shots.mjs` (puppeteer-core driving the real React app in
Chrome against a browser-only Tauri IPC mock, `src/dev/mockTauri.ts`). Nothing
is a hand-drawn recreation of the UI. The fixtures use **masked device names** —
source `MacBook Pro`, target `Office PC` — and neutral paths/accounts
(`/Users/developer/...`, `github.com/developer/workspace-clone`) so no real
machine, user, or host name appears on screen while the names are masked.
Re-run `npm run dev` + `node scripts/capture-shots.mjs` to refresh the shots,
then re-render.

## Outro / punchline
Full-bleed logo card: **Workspace Clone** / "Pick up right where you left off —
on the other laptop." Landed on the music's strongest cue, then a clean fade.

## User flow worth showing
Capture (entry) → Transfer (key action) → Restore (result). The three
centerpiece scenes show the actual app UI in use, not marketing lists.

## Tone
- Preset: `app-store`
- Creative direction: quiet premium product film — your dev session, packed like carry-on
- Interpretation: clean feature-card structure, medium-weight type, slide/wipe
  transitions (0.35-0.45s), consistent light SFX layer. The product is real, so
  the film treats it seriously; no jokes, no generic SaaS language.

## Format: landscape — 1920x1080
## Duration: 22.3s (5 scenes: 3.0 + 5.2 + 4.4 + 6.0 + 3.7)

## Visual identity (from the project)
- Background: `#0f0f12` canvas (dark mode `--surface-canvas`), soft indigo center glow
- Accent: indigo `#4f46e5` / `#6366f1` (the app's `--primary: 243 75% 59%`)
- Text: `#f4f4f5` on dark; `#18181b` on the light app window; mono `#18181b` for git rows
- Display font: system stack (`-apple-system, ui-sans-serif, system-ui`)
- Mono font: `ui-monospace, SFMono-Regular, Menlo`
- Strongest visual element: the real app screenshots (Capture, Transfer destinations, Restore preview/report) and the two-laptop transfer graphic

## Share copy (draft)
```
Workspace Clone copies your dev session — projects, branches, tools, env vars —
straight between two laptops. No cloud. No USB. Close one laptop, open the other.
```

## Audio direction
- Role: warm, upbeat-but-clean bed + consistent light SFX layer
- Music: `happy-beats-business-moves-vol-1-by-ende-dot-app.mp3` (120 BPM)
- Music treatment: bed at 0.35 volume from 0.0s, `data-fade-out` 1.2s over the outro
- Music cue guidance: bundled preset
  (`happy-beats-business-moves-vol-1-by-ende-dot-app.music-cues.json`); strong
  cues in window 16-23s; **lock outro logo to 20.02s** (1.00 strong_beat).
  Beat grid ~0.5s apart — manifest rows (4.02/5.03/6.03/7.02), received check
  (10.52) and restore rows (13.52/14.52/15.52/16.52) snap to every-other-beat
  windows so sequential text holds the reading floor.
- Audio-reactive treatment: unavailable — the extraction helper ships with the
  `hyperframes-creative` skill, not this standalone install; documented and skipped.
- SFX posture: light, app-store (0.6-0.75 volume). `interface/drop_001` per
  manifest row, `interface/drop_002` on the second hook line,
  `casino/card-slide-1` for the packet crossing, `casino/card-place-1` on the
  packet landing, `impact/impactPlate_light_000` on "Received",
  `interface/switch_001` per restore step, `impact/impactBell_heavy_000` on the
  final logo. Nothing aggressive; the bell only once, at the outro.
- Restraint rule: no chaotic/glitch/metal cues; the sfx stays in the product's
  own visual language (pops, placements, switches).

## Storyboard

### Scene 1 — "The promise" — 0.0–3.0s (3.0s)
Dark canvas, two laptop glyphs (left powered on, right off). Text line 1
"Close your laptop." slams in at 0.2s. A sealed indigo packet flies from the
left laptop to the right at 0.95→1.9s; the right laptop powers on. Text line 2
"Open the other one." enters at 2.05s and holds to 3.0.
Sequential/interaction: yes — line 1, packet flight, line 2.
Audio intent: opening clarity; each beat lands with a soft pop.
Audio-coupled idea: line 1 = drop_001 @0.15; packet whoosh = card-slide-1
@0.95; line 2 + landing = drop_002 @2.05 (all aligned to the animation start).
Music: bed from 0.0.
Transition mood: clean wipe → Scene 2.

### Scene 2 — "Capture" — 3.0–8.2s (5.2s)
Caption above: "It captures your setup — as a manifest." The real Capture screen
(light mode) scales in at 3.1s, then assembles in four horizontal strips at
4.02/5.03/6.03/7.02 (every-other-beat windows) until the whole window is visible:
workspace name typed in, both projects and both app adapters ticked.
Sequential/interaction: yes — window reveal, then 4 strips.
Audio intent: tidy, productive; each arrival is a dry pop.
Audio-coupled idea: window = impactSoft_medium_001 @3.05; each strip =
drop_001 @ its beat window.
Transition mood: slide → Scene 3.

### Scene 3 — "Send" — 8.2–12.6s (4.4s)
Captions: "Sent directly between your machines." then "No server. No cloud."
Left window is a close-up of the real Transfer screen — the destinations card
with the masked machine ("Office PC · On network · Paired") selected and the
"Send to Office PC" panel open. A ghost window appears on the right "awaiting".
The packet crosses (9.1→10.4), lands, the right window powers on with a green
"Received" row (10.52, beat grid).
Sequential/interaction: yes — ghost window, packet flight, received check.
Audio intent: the crossing is the whoosh; the landing is a soft placement + a
crisp notification chime on the check.
Audio-coupled idea: packet = card-slide-1 @9.1; landing = card-place-1 @10.4;
received check = impactPlate_light_000 @10.52.
Transition mood: slide → Scene 4.

### Scene 4 — "Restore" — 12.6–18.6s (6.0s)
Caption above: "Restore it — with your edits intact." The real Restore preview
window returns and assembles in four strips at
13.52/14.52/15.52/16.52 (every-other-beat windows). At 17.12 it crossfades to
the real Restore report — every step Done — and the mono payoff bar lands with
it: "git status: exactly what you left."
Sequential/interaction: yes — 4 strips, then the report crossfade + mono payoff.
Audio intent: rhythmic, satisfying; each arrival is a soft switch, the payoff line
is a single selection click.
Audio-coupled idea: switch_001 per strip; select_008 on the mono line.
Transition mood: soft crossfade → Scene 5.

### Scene 5 — "Outro" — 18.6–22.3s (3.7s)
Logo card: indigo mark, "Workspace Clone", tagline "Pick up right where you
left off — on the other laptop." The card lands at 20.02 (beat-locked to the
1.00 strong cue); music fades over the last 1.2s; hold to 21.4.
Sequential/interaction: none.
Audio intent: the one big moment — a deep bell on the logo, bed fades under it.
Audio-coupled idea: impactBell_heavy_000 @19.9 (fires as the card lands).
Music: bed fades out 1.2s (data-fade-out).
Transition mood: end — hold.

**Music mood for this video:** upbeat, clean, business-moves energy.
**Audio summary:** a warm 120 BPM bed carries the whole film; dry pops mark each
manifest row and hook line, a whoosh + soft landing signal the transfer, switch
ticks flip the restore steps, and one deep bell lands the logo as the bed fades.