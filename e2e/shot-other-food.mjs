// Снимок экрана «Другая еда» — нового пути записи еды, который живёт за
// кураторским флагом `lazy_food`. Не испытание: просто показать, как это выглядит.
//
// Сеанс кладётся засевом, без онбординга (`docs/testing.md`, раздел про e2e).
// Токен ненастоящий — экран сетевого счёта не касается.
import { existsSync } from 'node:fs';
import { chromium, devices } from '@playwright/test';

// Те же две поправки окружения, что в `playwright.config.ts`: этот скрипт —
// обычный node, конфигурацию Playwright он не читает, а без них в контейнере
// агента браузер не запустится и наружу не выйдет. Почему именно так — там же.
const IMAGE_CHROMIUM = '/opt/pw-browsers/chromium';
const proxyServer = process.env.HTTPS_PROXY || process.env.https_proxy;
const browser = await chromium.launch({
  ...(existsSync(IMAGE_CHROMIUM) ? { executablePath: IMAGE_CHROMIUM } : {}),
  ...(proxyServer ? { proxy: { server: proxyServer }, args: ['--ssl-version-max=tls1.2'] } : {}),
});
const ctx = await browser.newContext({
  ...devices['iPhone 13'],
  serviceWorkers: 'block',
  bypassCSP: true,
});
const page = await ctx.newPage();
page.on('console', m => { if (/засев|ошиб/i.test(m.text())) console.log(m.text().slice(0, 160)); });

const BASE = process.env.BASE || 'https://renorma-fit-dev.pages.dev';
const OUT = process.env.OUT || './';
await page.goto(BASE + '/', { waitUntil: 'load' });
const now = new Date().toISOString();
await page.evaluate((now) => {
  localStorage.setItem('user_id', 'e2e-shot-' + Date.now());
  localStorage.setItem('auth_token', 'e2e-not-a-real-token');
  localStorage.setItem('auth_ctx', 'browser');
  localStorage.setItem('pwa_dismissed', 'true');
  localStorage.setItem('ft_test_seed', JSON.stringify({
    profile: [{ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1990, goal: 'lose', updated_at: now }],
    app_flags: [
      { key: 'ft_subscription', value: JSON.stringify({ plan: 'e2e', end: Date.now() + 3e10, active: true }), updated_at: now },
      { key: 'ft_subscription_checked_at', value: String(Date.now()), updated_at: now },
      { key: 'push_onboarding_dismissed', value: 'true', updated_at: now },
      { key: 'feature.lazy_food', value: 'true', updated_at: now },
    ],
  }));
}, now);
await page.reload({ waitUntil: 'load' });

await page.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 25000 });
await page.getByTestId('nav-diary').click();
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 15000 });
await page.getByTestId('meal-add').first().click();

const btn = page.getByTestId('diary-add-btn-other-food');
await btn.waitFor({ state: 'visible', timeout: 15000 });
console.log('кнопка:', (await btn.textContent())?.trim());
await page.screenshot({ path: OUT + 'other-food-1-picker.png' });

await btn.click();
await page.getByTestId('other-food-panel').waitFor({ state: 'visible', timeout: 15000 });
await page.waitForTimeout(800);
await page.screenshot({ path: OUT + 'other-food-2-panel.png' });
console.log('готово');
await browser.close();
