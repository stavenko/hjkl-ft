# Story «мигающая обводка» highlight GIFs — spec & how to remake

The story frames highlight a UI element with an animated **hand-drawn sparkle
burst** (радиальные штрихи, мигают). This doc pins down the exact look + the
generation pipeline so it can be reproduced (incl. after context compaction).

**Generator script (this IS the recipe):** `scripts/shot-calcium-highlight.mjs`.
**Reference originals** (all in `frontend/story-img/`, made earlier — study these):
`welcome-persona.gif` (frame 2 of the «?» welcome story — highlights the profile
icon), `dashboard-indicators.gif`, `dashboard-planka-cal.gif`,
`dashboard-planka-macros.gif`, `dashboard-steps.gif`, `dashboard-weight.gif`.

---

## 1. Exact visual spec (reverse-engineered from `welcome-persona.gif`)

- **Framing:** the WHOLE widget/card is shown (all siblings visible), NOT a tight
  crop of one element. Crop = the widget bounding box + ~30px margin (rays overflow
  into the light background).
- **Timing:** **9 frames, ~11 fps, 0.82 s loop.**
- **Blink = a burst that SHOOTS OUT, peaks, fades, then pauses** (NOT a symmetric
  breathe, NOT a steady glow):
  - frame 1: nothing;
  - frames 2–3: short strokes, growing outward;
  - frame ~5: PEAK — full length, darkest;
  - frame 6: shorter + fainter (fading);
  - frames 7–9: gone (pause), then loop.
  - So strokes are visible in ~5 of 9 frames, off ~3–4. In the generator: `active =
    min(phase/0.72, 1)`, off when `phase ≥ 0.72`; `len ∝ min(1, active*2)` (full by
    mid), `opacity = sin(π*active)^0.7`.
- **The strokes are HAND-DRAWN, not straight bars:**
  - **~22 strokes** per highlighted element, radiating ~360°.
  - **Uneven**, not mechanically even: per-stroke jitter on angle (±~0.17 rad),
    start radius, length (≈9–21px at full), and curve — so they cluster irregularly.
  - **Tapered**: pointed at BOTH tips, fattest in the middle (~1.5–2.5px) — like a
    pencil/ink stroke. Rendered as an SVG lens path, not a `<div>` rectangle.
  - **Slightly curved** (a small perpendicular offset at the midpoint).
  - Colour **grey-blue `#6b7482`**; a small GAP between the element and the stroke
    starts (start radius = `max(w,h)/2 + 6`).
  - The SAME strokes grow/fade each loop (deterministic jitter seeded by index) —
    they are NOT re-randomised per frame.

Common past mistakes (don't repeat): straight uniform `<div>` bars; too few (18) and
too even; symmetric fade with no off-pause; wrong fps/frame-count; a glow RING
instead of strokes (the user explicitly rejected changing the style).

---

## 2. Pipeline (`shot-calcium-highlight.mjs`)

1. **Seed the dashboard** to the wanted state via Playwright: mint a dev JWT
   (`JWT_SECRET=dev-secret-change-in-production`), activate a fake sub
   (`payment-worker-dev /test/guest-checkout` → `/claim`) so the app boots past the
   lock, inject `localStorage` identity, write IndexedDB `hjkl-ft-<uid>`
   (profile / app_flags{sub + unlock flags} / goals / weight_entries / step_entries
   / foods / diary). ≥7 diary days → the indicators row shows. Drive each metric with
   a separate food (protein food; `is_veg_fruit` food; food with
   `nutrients:{"Кальций":mg}`). `serviceWorkers:'block'`, `deviceScaleFactor:2`.
2. **Locate targets by data attribute** (added for this):
   `[data-testid="progress-widget"]` (the widget CARD), `[data-gauge="<Label>"]`
   (a Gauge), `[data-ind="<Label>"]` (an indicator icon column;
   `[data-ind="…"] > div` = the 38px icon circle). `getBoundingClientRect`.
3. **Inject the sparkle overlay INSIDE `page.evaluate`** — NOT `addScriptTag` (the
   page CSP `script-src 'self'` blocks inline `<script>`; `page.evaluate` runs via
   CDP and bypasses it). Build an absolutely-positioned `<svg>` of tapered lens
   `<path>` strokes (see §1) at the widget's page coords.
4. **Capture 9 frames** across `phase 0..1` (`page.screenshot({clip})` cropped to the
   widget + 30px).
5. **Encode with ffmpeg** two-pass palette for a crisp GIF:
   `ffmpeg -framerate 11 -pattern_type glob -i 'frames/f*.png' -vf palettegen pal.png`
   then `… -i frames -i pal.png -lavfi paletteuse -loop 0 out.gif`.

Output → `frontend/story-img/<name>.gif`, referenced from `stories.rs` as
`Media::Shot("<name>.gif")`.

## 3. To highlight a DIFFERENT element

Change the seed to the state you want, point the `makeWidgetGif([...selectors])`
call at the target(s) (`[data-ind="…"]`, `[data-gauge="…"]`, or add a `data-*`
attribute to a new element), keep §1's stroke params. Verify by extracting a peak
frame and zooming to check the strokes are tapered + uneven before shipping.

See also memory `reference_story_highlight_gifs` and `project_stories_engine`.
