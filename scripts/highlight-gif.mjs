// Общий генератор «мигающей обводки» для кадров историй — тот самый рисунок
// от руки, что в welcome-persona.gif / dashboard-*.gif / calcium-highlight.gif.
// Алгоритм перенесён из scripts/shot-calcium-highlight.mjs, чтобы новые гифки
// (железо и далее) не заводили собственную копию.
import { execSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";

/// Рисует вспышку штрихов вокруг заданных прямоугольников. Выполняется ВНУТРИ
/// страницы (page.evaluate), поэтому не зависит от CSP.
export const drawSpark = ({ rects, frame }) => {
  let ov = document.getElementById("__spk");
  if (!ov) {
    ov = document.createElement("div");
    ov.id = "__spk";
    ov.style.cssText = "position:absolute;left:0;top:0;width:0;height:0;pointer-events:none;z-index:99999;";
    document.body.appendChild(ov);
  }
  // Резкое мигание, как в оригиналах: штрихи видны на 5 кадрах из 9.
  const ON = frame >= 1 && frame <= 5;
  if (!ON) { ov.innerHTML = ""; return; }
  // Углы и позиции СТАБИЛЬНЫ по индексу штриха; длина и изгиб дрожат по кадру —
  // от этого петля «кипит», как рисунок от руки.
  const rS = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7) * 43758.5453; return x - Math.floor(x); };
  const rF = (i, k) => { const x = Math.sin(i * 127.1 + k * 311.7 + frame * 57.13) * 43758.5453; return x - Math.floor(x); };
  const env = 0.72 + 0.28 * Math.sin(Math.PI * (frame - 0.5) / 5);
  const N = 22;
  let paths = "";
  const gap = 3;
  for (const rect of rects) {
    const cx = rect.x + rect.w / 2 + scrollX, cy = rect.y + rect.h / 2 + scrollY;
    // Широкие элементы (полоса gauge) пускают штрихи по периметру наружу;
    // квадратные (иконка) — радиально из центра.
    const wide = rect.w > rect.h * 1.8;
    const nn = wide ? Math.round(N * 1.4) : N;
    for (let i = 0; i < nn; i++) {
      let sx, sy, dx, dy;
      if (wide) {
        const hw = rect.w / 2 + gap, hh = rect.h / 2 + gap;
        const W = 2 * hw, H = 2 * hh, P = 2 * (W + H);
        let d = (((i + (rS(i, 7) - 0.5) * 0.7) / nn) * P % P + P) % P;
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
        const a = (i / nn) * 2 * Math.PI + (rS(i, 1) - 0.5) * 0.34;
        const r0 = Math.max(rect.w, rect.h) / 2 + gap + rS(i, 2) * 2;
        sx = cx + Math.cos(a) * r0; sy = cy + Math.sin(a) * r0;
        dx = Math.cos(a); dy = Math.sin(a);
      }
      const len = (8 + rF(i, 3) * 13) * env;
      if (len < 2) continue;
      const ex = sx + dx * len, ey = sy + dy * len;
      const px = -dy, py = dx;
      const cur = (rF(i, 4) - 0.5) * 5;
      const mx = (sx + ex) / 2 + px * cur, my = (sy + ey) / 2 + py * cur;
      const w = (1.5 + rS(i, 5) * 1.0);
      const t1x = mx + px * w, t1y = my + py * w, t2x = mx - px * w, t2y = my - py * w;
      paths += `<path d="M${sx.toFixed(1)} ${sy.toFixed(1)} Q${t1x.toFixed(1)} ${t1y.toFixed(1)} ${ex.toFixed(1)} ${ey.toFixed(1)} Q${t2x.toFixed(1)} ${t2y.toFixed(1)} ${sx.toFixed(1)} ${sy.toFixed(1)} Z" fill="#6b7482"/>`;
    }
  }
  ov.innerHTML = `<svg width="${document.documentElement.scrollWidth}" height="${document.documentElement.scrollHeight}" style="position:absolute;left:0;top:0;overflow:visible;">${paths}</svg>`;
};

/// Снимает петлю кадров с подсветкой на `sparkSels`, обрезанную по всему виджету
/// прогресса (чтобы остальные индикаторы были видны), и собирает GIF через ffmpeg.
export async function makeWidgetGif(page, sparkSels, outPath) {
  const box = (sel) => page.$eval(sel, (el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  });
  const widget = await box('[data-testid="progress-widget"]');
  const rects = [];
  for (const s of sparkSels) rects.push(await box(s));
  const M = 30; // запас, чтобы лучи выходили за карточку
  const clip = {
    x: Math.max(0, widget.x - M), y: Math.max(0, widget.y - M),
    width: widget.w + 2 * M, height: widget.h + 2 * M,
  };
  const name = outPath.replace(/^.*\//, "").replace(/\.gif$/, "");
  const dir = `/tmp/spk-${name}`;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const FRAMES = 9; // как в оригиналах: 9 кадров @ ~11 fps (петля 0.82 с)
  for (let f = 0; f < FRAMES; f++) {
    await page.evaluate(drawSpark, { rects, frame: f });
    await page.screenshot({ path: `${dir}/f${String(f).padStart(2, "0")}.png`, clip });
  }
  await page.evaluate(() => { const o = document.getElementById("__spk"); if (o) o.remove(); });
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${dir}/f*.png' -vf palettegen /tmp/pal-${name}.png`, { stdio: "ignore" });
  execSync(`ffmpeg -y -framerate 11 -pattern_type glob -i '${dir}/f*.png' -i /tmp/pal-${name}.png -lavfi paletteuse -loop 0 ${outPath}`, { stdio: "ignore" });
  console.log(`собрано: ${outPath}`);
}
