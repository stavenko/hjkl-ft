import { existsSync } from 'node:fs';
import { defineConfig } from '@playwright/test';

// ── Окружение агента: две поправки, обе обязательные ─────────────────────────
//
// Ничего из этого не выдумано «на всякий случай»: без первой браузер не открывает
// НИ ОДНОЙ страницы, без второй он вовсе не запускается. Обе включаются только
// там, где их условие выполнено, — на обычной машине конфигурация прежняя.
// Подробности и как это выяснялось — в `docs/testing.md`, раздел про e2e.

/// Браузер из образа. В контейнере агента лежит сборка Chromium 1194, а
/// репозиторный Playwright ждёт свою; без подмены он падает ещё до первого теста,
/// и `playwright install` в этом образе запускать нельзя (см. правила окружения).
const IMAGE_CHROMIUM = '/opt/pw-browsers/chromium';
const imageBrowser = existsSync(IMAGE_CHROMIUM) ? IMAGE_CHROMIUM : undefined;

/// Наружу — только через прокси, и только по TLS 1.2.
///
/// Приветствие TLS 1.3 у Chromium весит около 1750 байт, не влезает в один
/// TCP-сегмент, и релей агентского прокси на нём разваливается: замерено 1767 B
/// отправлено, 39 B получено, обрыв ровно через 6 секунд, и так для ЛЮБОГО узла.
/// У curl приветствие втрое короче, поэтому curl через тот же прокси ходит и по
/// TLS 1.3 — отсюда обманчивое «сеть есть, а у браузера её нет». Ограничение
/// версии проверку сертификатов не ослабляет: 1.2 проверяет цепочку так же.
const proxyServer = process.env.HTTPS_PROXY || process.env.https_proxy;

const launchOptions = {
  ...(imageBrowser ? { executablePath: imageBrowser } : {}),
  ...(proxyServer ? { args: ['--ssl-version-max=tls1.2'] } : {}),
};

const shared = {
  baseURL: 'https://renorma-fit-dev.pages.dev',
  browserName: 'chromium' as const,
  serviceWorkers: 'block' as const,
  ...(proxyServer ? { proxy: { server: proxyServer } } : {}),
  ...(Object.keys(launchOptions).length ? { launchOptions } : {}),
};

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  // Tests run against the live deployed dev env; a few live-network/timing-sensitive
  // flows (e.g. the support live thread under full parallel load) can flake. Retry
  // before failing — a genuinely broken test still fails every attempt.
  retries: 2,
  use: shared,
  projects: [
    {
      name: 'csp',
      testMatch: 'csp.spec.ts',
      use: { ...shared, bypassCSP: false },
    },
    {
      name: 'chromium',
      testIgnore: 'csp.spec.ts',
      use: { ...shared, bypassCSP: true },
    },
  ],
});
