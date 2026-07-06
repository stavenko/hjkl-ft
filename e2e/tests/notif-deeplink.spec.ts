import { test, expect } from '@playwright/test';

/**
 * Regression guard for the notification RECEIPT contract (docs/notification-receipt.md).
 *
 * The setup «notification» task completes when a push carrying
 * `ntf=<kind>.<section>.<task>.<rand>` (e.g. `tc.setup.notif.a4f2`) is RECEIVED —
 * no tap, no navigation. The service worker writes the code to IndexedDB
 * (`rn-notif`/kv, the iOS-safe live channel) and a Cache marker; the page
 * (index.html) bridges either into `localStorage['rn_notif_received']`; a WASM
 * poll (lib.rs, install_notif_receipt_poll) consumes it, resolves the task via
 * story::flag_for_task and sets NOTIFICATION_RECEIVED — the row flips reactively.
 *
 * A real push can't be injected here (serviceWorkers are blocked in this config),
 * so each test seeds the channel one hop downstream of the SW and asserts the
 * SAME task row flips PENDING → DONE:
 *   1. localStorage seed  → covers the WASM poll + flag resolution + reactive UI;
 *   2. IndexedDB seed     → additionally covers the page-side bridge (idbTakeNotif).
 *
 * Why this renders for a fresh, unauthenticated visitor: in frontend/src/app.rs
 * the <Router>/<Routes> (`/story/:id` → StorySectionPage → SetupControls) are
 * ALWAYS mounted; Auth / PWA overlays are drawn on top, and toBeVisible() checks
 * CSS visibility, not occlusion. `pwa_dismissed` keeps the PWA overlay branch
 * inert; a fresh context per test means NOTIFICATION_RECEIVED starts unset.
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

test.describe('Notification receipt completes the setup task', () => {
  test('ntf code in localStorage (WASM poll channel)', async ({ page }) => {
    // CONTROL — fresh visit: the notif row must be PENDING (⏳), not DONE.
    await page.goto('/story/setup');
    await expect(page.getByText(PENDING)).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText(DONE)).toHaveCount(0);

    // RECEIPT — what the page bridge writes when the SW delivers a push. The
    // 1s WASM poll consumes it, sets the flag, and the row flips reactively —
    // same page, no navigation.
    await page.evaluate(() => {
      localStorage.setItem('rn_notif_received', 'tc.setup.notif.e2e1');
    });
    await expect(page.getByText(DONE)).toBeVisible({ timeout: 15_000 });
    // Consumed exactly once: the poll removes the code after processing.
    expect(await page.evaluate(() => localStorage.getItem('rn_notif_received'))).toBeNull();
  });

  test('ntf code in IndexedDB (SW live channel + page bridge)', async ({ page }) => {
    await page.goto('/story/setup');
    await expect(page.getByText(PENDING)).toBeVisible({ timeout: 15_000 });

    // RECEIPT — what the SW's push handler writes (idbPutNotif in sw.js). The
    // page's idbTakeNotif (index.html, 1s tick) must bridge it to localStorage,
    // then the WASM poll completes the task.
    await page.evaluate(
      () =>
        new Promise<void>((resolve, reject) => {
          const req = indexedDB.open('rn-notif', 1);
          req.onupgradeneeded = () => req.result.createObjectStore('kv');
          req.onsuccess = () => {
            const db = req.result;
            const tx = db.transaction('kv', 'readwrite');
            tx.objectStore('kv').put('tc.setup.notif.e2e2', 'notif_received');
            tx.oncomplete = () => {
              db.close();
              resolve();
            };
            tx.onabort = () => reject(tx.error);
          };
          req.onerror = () => reject(req.error);
        }),
    );
    await expect(page.getByText(DONE)).toBeVisible({ timeout: 15_000 });
  });
});
