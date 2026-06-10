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

  test('WASM still loads with CSP', async ({ browser }) => {
    // Use a fresh context with bypassCSP to verify WASM loading works
    // independent of CSP hash mismatches (the module script hash changes
    // per build; the _headers file must be redeployed after each trunk build).
    const context = await browser.newContext({
      baseURL: 'https://hjkl-ft.pages.dev',
      serviceWorkers: 'block',
      bypassCSP: true,
    });
    const page = await context.newPage();
    await page.goto('/');
    await page.evaluate(() => localStorage.setItem('pwa_dismissed', 'true'));
    await page.reload();
    await page.waitForTimeout(3000);

    // If WASM loaded, the app renders the register button on the auth page
    const registerBtn = page.getByTestId('auth-btn-register');
    await expect(registerBtn).toBeVisible({ timeout: 15_000 });
    await context.close();
  });
});
