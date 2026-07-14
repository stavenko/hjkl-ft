import { chromium, devices } from 'playwright';
const iphone = devices['iPhone 13'];
const b = await chromium.launch({ headless: false, args: ['--window-size=430,900'] });
const ctx = await b.newContext({ ...iphone });
const p = await ctx.newPage();
await p.goto('http://localhost:8792/index.html', { waitUntil: 'networkidle' });
console.log('opened — окно Playwright открыто (iPhone 13). Закройте окно, чтобы завершить.');
// keep the window open until it's closed manually
await new Promise((resolve) => b.on('disconnected', resolve));
