import { test, expect } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * Update detection is now MANUAL: the app polls /version.json (launch + resume)
 * and, when a newer build is deployed, raises a flag — it does NOT auto-reload.
 * The flag shows a red dot on the Settings nav icon and a "Версия" row in
 * Settings with an "Обновить" button (which reloads). Run against the deployed
 * build; __APP_VERSION__ matches the live /version.json, so a mismatch is forced
 * by mocking /version.json.
 */

test('new build deployed: highlight in nav + Settings, no auto-reload', async ({ browser }) => {
  const ctx = await browser.newContext();
  // Always serve a stale id ≠ the running __APP_VERSION__.
  await ctx.route('**/version.json**', (route) =>
    route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ v: 'stale-test-id' }) }),
  );
  // If the app (wrongly) auto-reloaded on a mismatch, it would loop forever on
  // the always-stale mock; count loads to prove it does NOT.
  await ctx.addInitScript(() => {
    const n = parseInt(sessionStorage.getItem('__loads') || '0', 10) + 1;
    sessionStorage.setItem('__loads', String(n));
  });

  const page = await ctx.newPage();
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);

  // Red dot on the Settings nav icon.
  await expect(page.getByTestId('nav-settings-update-dot')).toBeVisible({ timeout: 10_000 });

  // No reload loop: load count stays bounded (registration does a few reloads,
  // but a mismatch must not trigger any more).
  const loads = parseInt((await page.evaluate(() => sessionStorage.getItem('__loads'))) || '0', 10);
  await page.waitForTimeout(2000);
  const loads2 = parseInt((await page.evaluate(() => sessionStorage.getItem('__loads'))) || '0', 10);
  expect(loads2, 'must not auto-reload on a mismatch').toBe(loads);

  // Settings → Version row shows the update + an Обновить button.
  await page.getByTestId('nav-settings').click();
  await expect(page).toHaveURL(/\/settings/);
  await expect(page.getByTestId('settings-btn-update')).toBeVisible({ timeout: 10_000 });
  await expect(page.getByText('Доступна новая версия')).toBeVisible();

  await ctx.close();
});

test('up to date: no update highlight', async ({ browser }) => {
  // No version.json mock → the deployed /version.json matches __APP_VERSION__.
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  await page.goto('/');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(3000);
  await registerAccount(page);
  await page.waitForTimeout(1500); // let the launch check settle

  await expect(page.getByTestId('nav-settings-update-dot')).toHaveCount(0);

  await ctx.close();
});
