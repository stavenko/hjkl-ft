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
    // Широкие цели (строка меню, кнопка диалога) излучают росчерки из периметра
    // скруглённого прямоугольника, квадратные — радиально из центра.
    const wide = rect.w > rect.h * 1.8;
    const N = wide ? 34 : 22;
    for (let i = 0; i < N; i++) {
      let sx, sy, dx, dy;
      if (wide) {
        const hw = rect.w / 2 + gap, hh = rect.h / 2 + gap;
        const W = 2 * hw, H = 2 * hh, P = 2 * (W + H);
        const d = ((((i + (rS(i, 7) - 0.5) * 0.7) / N) * P) % P + P) % P;
        let lx, ly, nx, ny;
        if (d < W) { lx = -hw + d; ly = -hh; nx = 0; ny = -1; }
        else if (d < W + H) { lx = hw; ly = -hh + (d - W); nx = 1; ny = 0; }
        else if (d < 2 * W + H) { lx = hw - (d - W - H); ly = hh; nx = 0; ny = 1; }
        else { lx = -hw; ly = hh - (d - 2 * W - H); nx = -1; ny = 0; }
        sx = cx + lx; sy = cy + ly;
        const ja = (rS(i, 1) - 0.5) * 0.5;
        dx = nx * Math.cos(ja) - ny * Math.sin(ja);
        dy = nx * Math.sin(ja) + ny * Math.cos(ja);
      } else {
      const a = (i / N) * 2 * Math.PI + (rS(i, 1) - 0.5) * 0.34;
      const r0 = Math.max(rect.w, rect.h) / 2 + gap + rS(i, 2) * 2;
      sx = cx + Math.cos(a) * r0; sy = cy + Math.sin(a) * r0;
      dx = Math.cos(a); dy = Math.sin(a);
      }
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

async function makeGif({ img, w, h, rects, out, pad = 0 }) {
  // pad — поле вокруг картинки, чтобы бурст не обрезался у самого края.
  const ctx = await b.newContext({ viewport: { width: w + pad * 2, height: h + pad * 2 }, deviceScaleFactor: 2 });
  const page = await ctx.newPage();
  // Картинку вшиваем как data-URI: страница живёт на about:blank, откуда
  // file://-ссылки не грузятся.
  const b64 = readFileSync(`${DIR}/${img}`).toString("base64");
  await page.setContent(
    `<body style="margin:0;background:#fff;"><img src="data:image/jpeg;base64,${b64}" width="${w}" height="${h}" style="display:block;margin:${pad}px"></body>`
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

// ── Установка PWA в Chrome: кебаб (или значок обновления), строка меню, «Установить» ──
await makeGif({ img: "menu-icon.jpg", w: 37, h: 39, out: "pwa-menu", pad: 26,
  rects: [{ x: 26, y: 26, w: 37, h: 39 }] });                  // сам значок
await makeGif({ img: "menu-row.jpg", w: 361, h: 101, out: "pwa-addscreen", pad: 16,
  rects: [{ x: 22, y: 38, w: 330, h: 42 }] });                 // строка «Добавить на гл. экран»
await makeGif({ img: "install-dialog.jpg", w: 576, h: 361, out: "pwa-install", pad: 6,
  rects: [{ x: 60, y: 130, w: 460, h: 84 }] });                // кнопка «Установить»

await b.close();
