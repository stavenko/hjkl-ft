import { test, expect } from '@playwright/test';
import { registerAccount } from './helpers';

/**
 * REGRESSION: leaving the chat via the bottom nav.
 *
 * The bug this guards: ChatInput's `on_cleanup` read `recording` — a signal OWNED by
 * ChatPage — with `get_untracked()`. On an SPA navigation away from `/chat` the parent
 * disposed that signal before the child cleanup ran → "already disposed" panic → WASM
 * abort → the route never switched, so the chat wouldn't close (you were stuck).
 *
 * Why it slipped through before: `navigation.spec` exercised the bottom nav but never
 * OPENED the chat; `support-chat.spec` opened the chat but never LEFT it via the nav
 * (its `page.goto('/chat')` is a full reload, not an SPA transition). The trigger lives
 * exactly at the intersection — mount ChatInput, then leave via the nav — and no test
 * hit it. There was also no page-error guard, so a WASM panic passed silently.
 *
 * This test does the intersection for EVERY bottom-nav destination and fails on ANY
 * uncaught page error / reactive-disposal panic.
 */
test('bottom nav closes the chat — no teardown panic', async ({ browser }) => {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();

  // Fail on any WASM panic (uncaught error, or the panic-hook's console.error).
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e}`));
  page.on('console', (m) => {
    if (m.type() === 'error' && /panicked|already been disposed|unreachable/i.test(m.text())) {
      errors.push(`console: ${m.text()}`);
    }
  });

  await page.goto('/');
  await registerAccount(page); // lands in the app with an active sub
  await expect(page.getByTestId('nav-support')).toBeVisible({ timeout: 15_000 });

  const targets: Array<[string, RegExp]> = [
    ['nav-diary', /\/diary$/],
    ['nav-recipes', /\/recipes$/],
    ['nav-settings', /\/settings$/],
    ['nav-story', /\/$/],
  ];

  for (const [testid, url] of targets) {
    // Open the chat (mounts ChatInput), then leave via the bottom nav (SPA transition
    // that tears ChatInput down — this is what used to panic).
    await page.getByTestId('nav-support').click();
    await expect(page.getByTestId('chat-messages')).toBeVisible({ timeout: 10_000 });

    await page.getByTestId(testid).click();
    await expect(page).toHaveURL(url);
    await expect(page.getByTestId('chat-messages')).toHaveCount(0);
  }

  expect(errors, `uncaught page errors:\n${errors.join('\n')}`).toEqual([]);
});
