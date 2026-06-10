import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 60_000,
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
