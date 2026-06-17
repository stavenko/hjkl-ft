import { test, expect } from '@playwright/test';

/**
 * In-app auto-update: the running build id (`globalThis.__APP_VERSION__`, stamped
 * into init.js) is compared against `/version.json` on launch and on resume; a
 * mismatch reloads the app. We mock `/version.json` and count document loads
 * (sessionStorage, bumped by an init script on every navigation/reload).
 *
 * Runs against the deployed build, which carries the mechanism. The launch check
 * exercises the same code path as the resume (visibilitychange) check.
 */

const COUNT_LOADS = () => {
  const n = parseInt(sessionStorage.getItem('__loads') || '0', 10) + 1;
  sessionStorage.setItem('__loads', String(n));
};

test('reloads once when a newer build is deployed', async ({ browser, request }) => {
  const real = (await (await request.get('/version.json')).json()).v as string;
  expect(real, 'prod must serve a build id').toBeTruthy();

  const ctx = await browser.newContext();
  await ctx.addInitScript(COUNT_LOADS);

  // First version probe returns a STALE id (≠ running build) → triggers a reload;
  // every subsequent probe returns the real id → no loop.
  let calls = 0;
  await ctx.route('**/version.json**', async (route) => {
    calls += 1;
    const v = calls === 1 ? `stale-${real}` : real;
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ v }) });
  });

  const page = await ctx.newPage();
  await page.goto('/');

  // load #1 (stale) → reload → load #2 (matches) → stop. Loads settle at exactly 2.
  // The read is wrapped because page.evaluate throws while the reload navigation
  // is destroying the execution context — poll should retry, not fail.
  const loads = async () => {
    try {
      return await page.evaluate(() => sessionStorage.getItem('__loads'));
    } catch {
      return null;
    }
  };
  await expect.poll(loads, { timeout: 15_000 }).toBe('2');
  await page.waitForTimeout(2500); // and it must NOT keep reloading
  expect(await loads()).toBe('2');

  await ctx.close();
});

test('does not reload when the deployed build matches', async ({ browser, request }) => {
  const real = (await (await request.get('/version.json')).json()).v as string;

  const ctx = await browser.newContext();
  await ctx.addInitScript(COUNT_LOADS);
  await ctx.route('**/version.json**', async (route) => {
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ v: real }) });
  });

  const page = await ctx.newPage();
  await page.goto('/');
  await page.waitForTimeout(3000);

  // Matching version → no spurious reload.
  expect(await page.evaluate(() => sessionStorage.getItem('__loads'))).toBe('1');

  await ctx.close();
});
