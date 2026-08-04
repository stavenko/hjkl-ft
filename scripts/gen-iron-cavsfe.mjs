// Генерация картинки «Ca vs Fe» для последнего кадра недели железа.
//
// Фон — скачанный градиент с шумом (кадрируем на цветную часть). Символы
// набраны Unbounded, тем же display-шрифтом, что и остальная типографика
// приложения; Ca заливается бело-жёлтым (кости, как маркер кальция), Fe —
// бело-красным (кровь, как маркер железа).
import { chromium } from "playwright";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(import.meta.dirname, "..");
const SCRATCH = process.env.SCRATCH ||
  "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/iron";
const OUT = process.env.OUT || path.join(ROOT, "frontend/story-img/iron-ca-vs-fe.jpg");

const b64 = (p) => fs.readFileSync(p).toString("base64");
const grad = b64(path.join(SCRATCH, "grad.jpg"));
const font = b64(path.join(ROOT, "frontend/fonts/unbounded-latin.woff2"));

const html = `<!doctype html><html><head><meta charset="utf-8"><style>
@font-face {
  font-family: "Unbounded";
  font-style: normal;
  font-weight: 700;
  src: url(data:font/woff2;base64,${font}) format("woff2");
}
* { margin: 0; padding: 0; box-sizing: border-box; }
body { width: 1200px; height: 800px; overflow: hidden; }
.frame {
  width: 1200px; height: 800px; position: relative;
  background-image: url(data:image/jpeg;base64,${grad});
  /* Кадрируем на цветную часть градиента: сверху остаётся немного чёрного. */
  /* Media::Cover в кадре истории срезает примерно по 14 % ширины с каждой
     стороны, поэтому набор держим в центральных ~60 % — иначе Ca и Fe уезжают
     за края экрана. */
  background-size: cover; background-position: center 78%;
  display: flex; align-items: center; justify-content: center; gap: 44px;
}
.sym {
  font-family: "Unbounded", sans-serif; font-weight: 700; font-size: 148px;
  line-height: 1; letter-spacing: -0.02em;
  -webkit-background-clip: text; background-clip: text;
  -webkit-text-fill-color: transparent; color: transparent;
  filter: drop-shadow(0 6px 28px rgba(0,0,0,0.55));
}
/* Ca — кость: белый в жёлтый. Fe — кровь: белый в красный. */
.ca { background-image: linear-gradient(150deg, #ffffff 0%, #fff4c2 42%, #fde047 100%); }
.fe { background-image: linear-gradient(150deg, #ffffff 0%, #fca5a5 38%, #e0304f 100%); }
.vs {
  font-family: "Unbounded", sans-serif; font-weight: 700; font-size: 46px;
  line-height: 1; color: rgba(255,255,255,0.62); letter-spacing: 0.02em;
  text-shadow: 0 4px 20px rgba(0,0,0,0.6);
}
</style></head><body>
<div class="frame">
  <span class="sym ca">Ca</span>
  <span class="vs">vs</span>
  <span class="sym fe">Fe</span>
</div>
</body></html>`;

const tmp = path.join(SCRATCH, "cavsfe.html");
fs.writeFileSync(tmp, html);

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 1200, height: 800 }, deviceScaleFactor: 1 });
await page.goto(`file://${tmp}`, { waitUntil: "load" });
await page.evaluate(() => document.fonts.ready);
await page.waitForTimeout(500);
const png = path.join(SCRATCH, "cavsfe.png");
await page.screenshot({ path: png });
await browser.close();

// В JPEG — как и остальные фото истории.
const { execFileSync } = await import("node:child_process");
execFileSync("sips", ["-s", "format", "jpeg", "-s", "formatOptions", "72", png, "--out", OUT], { stdio: "ignore" });
console.log("сгенерировано:", OUT, fs.statSync(OUT).size, "байт");
