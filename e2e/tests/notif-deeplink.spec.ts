import { test, expect } from '@playwright/test';

/**
 * Regression guard for the notification deep-link CONSUMPTION contract.
 *
 * The Chapter-1 "setup" section (`SetupControls` in story_widgets.rs) shows a
 * notification task row that is completed ONLY by opening the page with the
 * `?notif=1` deep-link (the test-push target). That handler:
 *
 *     if search.contains("notif=1") {
 *         spawn_local(async move { story::set_flag(story::NOTIFICATION_RECEIVED, true).await; });
 *     }
 *
 * is the single seam a future routing change could silently break — and until
 * this spec there was NO test for it, which is exactly how the regression
 * (tap-does-not-complete) landed green.
 *
 * The pair below IS the documentary contract: SAME page, DIFFERENT result driven
 * only by the presence of the `?notif=1` query param.
 *   - no param  → row shows ⏳ "Уведомление ещё не приходило" (PENDING)
 *   - ?notif=1  → row shows ✅ "Уведомление получено"        (DONE)
 *
 * Why this renders for a fresh, unauthenticated visitor:
 * In frontend/src/app.rs the <Router>/<Routes> (which serve `/story/:id` →
 * StorySectionPage → SetupControls) are ALWAYS mounted; the Auth / PWA / Checking
 * / Locked screens are just fixed-position z-index:100 OVERLAYS drawn on top. The
 * setup DOM therefore exists behind the overlay, and Playwright's toBeVisible()
 * (which checks CSS visibility/size, not occlusion by another element) can read
 * it. We set `pwa_dismissed` so the PWA-install overlay doesn't take a different
 * (needs_pwa_prompt) branch. Each test gets a fresh Playwright context, and
 * serviceWorkers:'block' + a fresh per-user IndexedDB mean the NOTIFICATION_RECEIVED
 * flag starts unset — so the PENDING state is deterministic.
 */

const PENDING = 'Уведомление ещё не приходило';
const DONE = 'Уведомление получено';

test.beforeEach(async ({ page }) => {
  // Land once to get an origin, then make the PWA-install overlay branch inert
  // and clear any residual state so the notif flag starts unset.
  await page.goto('/');
  await page.evaluate(() => {
    localStorage.clear();
    localStorage.setItem('pwa_dismissed', 'true');
  });
});

test.describe('Notification deep-link consumption', () => {
  test('setup notif task completes only via the ?notif=1 deep-link', async ({ page }) => {
    // 1. CONTROL — "before the link". Fresh visit, no query param: the notif row
    //    must show the PENDING (⏳) status and NOT the DONE status.
    await page.goto('/story/setup');
    await expect(page.getByText(PENDING)).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText(DONE)).toHaveCount(0);

    // 2. LINK TAPPED — same page, opened with the `?notif=1` deep-link. The flag
    //    write is async (spawn_local → IndexedDB), so rely on toBeVisible's
    //    auto-wait for the row to flip to DONE (✅).
    await page.goto('/story/setup?notif=1');
    await expect(page.getByText(DONE)).toBeVisible({ timeout: 15_000 });
  });
});
