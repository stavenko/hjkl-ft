// «Мигающие» GIF-подсказки для экрана онбординга Яндекс.Браузера: тот же
// росчерковый бурст, что на dashboard-*.gif (см. shot-calcium-highlight.mjs),
// но поверх присланных скриншотов живого интерфейса.
//   1) нижняя панель Яндекс.Браузера — подсвечена кнопка «Поделиться»
//   2) системный лист «Отправка ссылки» — подсвечен Chrome
import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { mkdirSync, rmSync, readFileSync } from "node:fs";

const DIR = process.argv[2] ||
  "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/yandex";

// Круговой бурст росчерков вокруг цели: позиции стабильны, длины «кипят» по
// кадрам, показ 5 кадров из 9 — резкое мигание как в существующих гифках.
const drawSpark = ({ rects, frame }) => {
  let ov = document.getElementById("__spk");
  if (!ov) {
    ov = document.createElement("div");
    ov.id = "__spk";
    ov.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;pointer-events:none;z-index:99999;";
    document.body.appendChild(ov);
  }
  const ON = frame >= 1 && frame <= 5;
  if (!ON) { ov.innerHTML = ""; return; }
  const rS = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7) * 43758.5453; return x - Math.floor(x); };
  const rF = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7 + frame * 57.13) * 43758.5453; return x - Math.floor(x); };
  const env = 0.72 + 0.28 * Math.sin(Math.PI * (frame - 0.5) / 5);
  let paths = "";
  const gap = 4;
  for (const rect of rects) {
    const cx = rect.x + rect.w / 2, cy = rect.y + rect.h / 2;
    const N = 22;
    for (let i = 0; i < N; i++) {
      const a = (i / N) * 2 * Math.PI + (rS(i, 1) - 0.5) * 0.34;
      const r0 = Math.max(rect.w, rect.h) / 2 + gap + rS(i, 2) * 2;
      const sx = cx + Math.cos(a) * r0, sy = cy + Math.sin(a) * r0;
      const dx = Math.cos(a), dy = Math.sin(a);
      const len = (9 + rF(i, 3) * 14) * env;
      if (len < 2) continue;
      const ex = sx + dx * len, ey = sy + dy * len;
      const px = -dy, py = dx;
      const cur = (rF(i, 4) - 0.5) * 5;
      const mx = (sx + ex) / 2 + px * cur, my = (sy + ey) / 2 + py * cur;
      const w = 1.5 + rS(i, 5) * 1.0;
      paths += `<path d="M${sx.toFixed(1)} ${sy.toFixed(1)} Q${(mx + px * w).toFixed(1)} ${(my + py * w).toFixed(1)} ${ex.toFixed(1)} ${ey.toFixed(1)} Q${(mx - px * w).toFixed(1)} ${(my - py * w).toFixed(1)} ${sx.toFixed(1)} ${sy.toFixed(1)} Z" fill="#6b7482"/>`;
    }
  }
  ov.innerHTML = `<svg width="${document.documentElement.scrollWidth}" height="${document.documentElement.scrollHeight}" style="position:absolute;left:0;top:0;overflow:visible;">${paths}</svg>`;
};

const b = await chromium.launch({ headless: true });

async function makeGif({ img, w, h, rects, out }) {
  const ctx = await b.newContext({ viewport: { width: w, height: h }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  // Картинку вшиваем как data-URI: страница живёт на about:blank, откуда
  // file://-ссылки не грузятся.
  const b64 = readFileSync(`${DIR}/${img}`).toString("base64");
  await page.setContent(
    `<body style="margin:0;background:#fff;"><img src="data:image/jpeg;base64,${b64}" width="${w}" height="${h}" style="display:block"></body>`
  );
  await page.waitForTimeout(400);
  const tmp = `/tmp/yhop-${out}`;
  rmSync(tmp, { recursive: true, force: true });
  mkdirSync(tmp, { recursive: true });
  const FRAMES = 9;
  for (let f = 0; f < FRAMES; f++) {
    await page.evaluate(drawSpark, { rects, frame: f });
    await page.screenshot({ path: `${tmp}/f${String(f).padStart(2, "0")}.png` });
  }
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${tmp}/f*.png' -vf palettegen /tmp/pal-${out}.png`, { stdio: "ignore" });
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${tmp}/f*.png' -i /tmp/pal-${out}.png -lavfi paletteuse -loop 0 ${DIR}/${out}.gif`, { stdio: "ignore" });
  await ctx.close();
  console.log("готово:", `${DIR}/${out}.gif`);
}

// Координаты целей — в системе координат самих скриншотов (CSS-пиксели).
await makeGif({ img: "toolbar.jpg", w: 576, h: 99, out: "hop-share",
  rects: [{ x: 33, y: 30, w: 48, h: 44 }] });                 // кнопка «Поделиться»
await makeGif({ img: "sheet.jpg", w: 576, h: 379, out: "hop-chrome",
  rects: [{ x: 318, y: 248, w: 72, h: 72 }] });               // иконка Chrome

await b.close();
