// Проверка сцены виджета: герой ОРАНЖЕВЫЙ и первый в ряду, остальные зелёные.
//
//   node scripts/check-widget-scene.mjs [неделя] [герой]
//   node scripts/check-widget-scene.mjs fats epa_dha
//   DAYS=5 node scripts/check-widget-scene.mjs base   # короткая история новичка
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { sceneSeed } from "./widget-scene.mjs";

const BASE = process.env.FE || DEFAULT_URL;
const week = process.argv[2] || "egg";
const target = process.argv[3] || "egg";
const SHOT = process.env.SHOT || `scene-${week}-${target}.png`;

const browser = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(browser, {
  baseUrl: BASE,
  context: { serviceWorkers: "block", deviceScaleFactor: 2 },
  uid: `scene-${Math.floor(Math.random() * 1e6)}`,
  seed: sceneSeed({ week, target, ...(process.env.DAYS ? { days: Number(process.env.DAYS) } : {}) }),
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForTimeout(6000);
await page.setViewportSize({ width: 440, height: 1200 });
await page.waitForTimeout(3000);

const row = await page.evaluate(() => {
  const tint = (el) => getComputedStyle(el.querySelector("div")).backgroundColor;
  const name = (t) =>
    /224, *48/.test(t) ? "красный"
      : /232, *133/.test(t) ? "ОРАНЖЕВЫЙ"
      : /31, *164/.test(t) ? "зелёный"
      : /154, *160/.test(t) ? "серый"
      : t;
  return [...document.querySelectorAll("[data-ind]")].map((el) =>
    `${el.getAttribute("data-ind")}:${name(tint(el))}`);
});
const gauges = await page.evaluate(() =>
  [...document.querySelectorAll("*")]
    .filter((n) => n.children.length === 0 && /\/ ?\d/.test(n.textContent || ""))
    .map((n) => n.textContent.trim())
    .slice(0, 14));

console.log(`сцена: неделя ${week}, герой ${target}`);
console.log("ряд значков:", row.join("  "));
console.log("шкалы:", gauges.join(" · "));
await page.screenshot({ path: SHOT, fullPage: true });
console.log(`снимок: ${SHOT}`);
await context.close();
await browser.close();
