import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
  // Tests run against the live deployed dev env; a few live-network/timing-sensitive
  // flows (e.g. the support live thread under full parallel load) can flake. Retry
  // before failing — a genuinely broken test still fails every attempt.
  retries: 2,
  use: {
    baseURL: 'https://hjkl-ft.pages.dev',
    browserName: 'chromium',
    serviceWorkers: 'block',
  },
  projects: [
    {
      name: 'csp',
      testMatch: 'csp.spec.ts',
      use: { browserName: 'chromium', bypassCSP: false },
    },
    {
      name: 'chromium',
      testIgnore: 'csp.spec.ts',
      use: { browserName: 'chromium', bypassCSP: true },
    },
  ],
});
