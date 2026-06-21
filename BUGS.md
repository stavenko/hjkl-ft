# Bugs

_No known open bugs._

## Fixed

### Bottom nav visible on the PWA install onboarding screen — FIXED 2026-06-21
- **Where:** `AppState::PwaPrompt` overlay (`frontend/src/app.rs`).
- **Symptom:** the fixed bottom nav (z-index:40) showed through at the bottom of the "Food Tracker — install the app" onboarding screen, because `PwaPrompt` (a normal-flow `min-height:100vh` div) created no stacking context above it — unlike the `Auth`/`PushOnboarding` overlays.
- **Fix:** wrapped the `PwaPrompt` overlay in `position:fixed; inset:0; z-index:100; background:var(--bulma-scheme-main); overflow-y:auto`, matching the other overlays.
- **Verified (Playwright):** with the prompt showing, `document.elementFromPoint` at the nav's center returns the overlay `DIV`, not the `A[data-testid=nav-story]` link, and the nav is not visible in a screenshot.
