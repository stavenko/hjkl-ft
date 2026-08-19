// Headed browser that just plays the calcium highlight GIFs so the user can eyeball them.
import { chromium } from "playwright";
const b = await chromium.launch({ headless: false });
const ctx = await b.newContext({ viewport: { width: 900, height: 760 } });
const page = await ctx.newPage();
const here = new URL(".", import.meta.url).pathname;
const gif = "file://" + here + "calcium-gauge.gif";
const ind = "file://" + here + "calcium-indicator.gif";
await page.setContent(`<body style="margin:0;background:#f3f4f6;font-family:-apple-system,sans-serif;display:flex;flex-direction:column;align-items:center;gap:6px;padding:24px">
  <div style="color:#6b7482">gauge</div>
  <img src="${gif}" style="width:440px;box-shadow:0 4px 20px rgba(0,0,0,.12)">
  <div style="color:#6b7482;margin-top:18px">индикатор (утверждён)</div>
  <img src="${ind}" style="width:440px;box-shadow:0 4px 20px rgba(0,0,0,.12)">
</body>`);
console.log("READY — окно открыто, гифки играют. Закрой окно когда посмотришь.");
await new Promise(() => {});
