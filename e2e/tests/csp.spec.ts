import { test, expect } from '@playwright/test';

test.describe('Content Security Policy', () => {
  test('CSP header present', async ({ request }) => {
    const response = await request.get('/');
    const csp = response.headers()['content-security-policy'];
    expect(csp).toBeTruthy();
    // Verify key directives are present
    expect(csp).toContain("script-src");
    expect(csp).toContain("wasm-unsafe-eval");
    expect(csp).toContain("default-src 'none'");
  });

  test('CSP blocks inline script injection', async ({ page }) => {
    const cspViolations: string[] = [];

    page.on('console', msg => {
      const text = msg.text();
      if (
        text.includes('Content-Security-Policy') ||
        text.includes('Content Security Policy') ||
        text.includes('Refused to execute inline script') ||
        text.includes("'unsafe-inline'")
      ) {
        cspViolations.push(text);
      }
    });

    await page.goto('/');
    await page.waitForTimeout(2000);

    // Attempt to inject an inline script — CSP should block it
    await page.evaluate(() => {
      const s = document.createElement('script');
      s.textContent = 'window.__csp_test_ran = true';
      document.head.appendChild(s);
    });

    await page.waitForTimeout(500);

    const scriptRan = await page.evaluate(() => (window as any).__csp_test_ran === true);

    // Either the script did not run OR a CSP violation was logged
    const cspBlocked = !scriptRan || cspViolations.length > 0;
    expect(cspBlocked).toBe(true);
  });

  test('App renders with CSP enabled (no bypass)', async ({ browser }) => {
    // This test does NOT bypass CSP — it verifies the real user experience
    const context = await browser.newContext({
      baseURL: 'https://hjkl-ft.pages.dev',
      serviceWorkers: 'block',
      bypassCSP: false,
    });
    const page = await context.newPage();

    const cspErrors: string[] = [];
    page.on('console', msg => {
      const text = msg.text();
      if (text.includes('Content-Security-Policy') || text.includes('Refused')) {
        cspErrors.push(text);
      }
    });
    page.on('pageerror', err => cspErrors.push(`PAGE ERROR: ${err.message}`));

    await page.goto('/');

    // Wait for actual WASM-rendered content — not just HTML skeleton
    // Either PWA prompt dismiss button OR auth register button must appear
    const rendered = await Promise.race([
      page.getByTestId('pwa-btn-dismiss').waitFor({ timeout: 15_000 }).then(() => 'pwa'),
      page.getByTestId('auth-btn-register').waitFor({ timeout: 15_000 }).then(() => 'auth'),
    ]).catch(() => 'nothing');

    // Log ALL console messages for debugging
    const allConsole: string[] = [];
    page.on('console', msg => allConsole.push(`[${msg.type()}] ${msg.text()}`));

    if (rendered === 'nothing') {
      // Capture more debug info
      await page.waitForTimeout(2000);
      const bodyText = await page.textContent('body');
      console.log('CSP errors:', cspErrors);
      console.log('All console:', allConsole);
      console.log('Body text length:', bodyText?.length);
      console.log('Body text:', bodyText?.substring(0, 300));

      const pageErrors: string[] = [];
      page.on('pageerror', err => pageErrors.push(err.message));
      await page.waitForTimeout(1000);
      console.log('Page errors:', pageErrors);
    }

    expect(rendered).not.toBe('nothing');

    await context.close();
  });
});
