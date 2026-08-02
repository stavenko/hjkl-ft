// Экран «re:Norma лучше всего работает в браузере Chrome» обязан показываться
// ВСЕГДА, когда определён Яндекс.Браузер, — включая страницу онбординга из
// Telegram и устройство, где PWA-подсказку когда-то уже закрывали.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const YA = "Mozilla/5.0 (Linux; arm_64; Android 15; 25078RA3EY) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.7778.104 YaBrowser/26.6.5.104.00 Mobile Safari/537.36";
const YA_APP = "Mozilla/5.0 (Linux; Android 14; SM-A536B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Mobile Safari/537.36 YaSearchBrowser/24.61 BroPP/1.0 YaSearchApp/24.61";
const CHROME = "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Mobile Safari/537.36";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b = await chromium.launch({ headless: true });

async function open({ ua, path = "/", dismissed = false, shot = null }) {
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, userAgent: ua, serviceWorkers: "block" });
  const p = await ctx.newPage();
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.evaluate((d) => { localStorage.clear(); if (d) localStorage.setItem("pwa_dismissed", "true"); }, dismissed);
  await p.goto(FE + path, { waitUntil: "domcontentloaded" });
  // Ждём именно экран (а не фиксированную паузу) — иначе медленная загрузка
  // читается как «экран не показан».
  await p.locator('[data-testid="pwa-yandex-screen"]')
    .waitFor({ state: "visible", timeout: 15000 }).catch(() => {});
  await p.waitForTimeout(2500);
  // Гифки грузятся отдельными запросами — дождаться, иначе проверка ловит момент
  // до их загрузки.
  await p.waitForFunction(() => {
    const imgs = [...document.querySelectorAll('img[src^="/onboard-img/"]')];
    return imgs.length === 0 || imgs.every((i) => i.complete && i.naturalWidth > 0);
  }, null, { timeout: 15000 }).catch(() => {});
  const screen = p.locator('[data-testid="pwa-yandex-screen"]');
  const visible = await screen.isVisible().catch(() => false);
  const fullText = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  const state = {
    visible,
    fullText,
    text: fullText.slice(0, 140),
    gifs: await p.locator('[data-testid="pwa-yandex-screen"] img[src^="/onboard-img/"]').count().catch(() => 0),
    gifsLoaded: await p.evaluate(() => [...document.querySelectorAll('img[src^="/onboard-img/"]')]
      .every((i) => i.complete && i.naturalWidth > 0)).catch(() => false),
    // Ни кнопки «остаться», ни любого другого выхода на экране быть не должно.
    exits: await p.locator('[data-testid="pwa-yandex-screen"] button, [data-testid="pwa-yandex-screen"] a').count().catch(() => 0),
  };
  if (shot) await p.screenshot({ path: shot, fullPage: true });
  await ctx.close();
  return state;
}

const shotPath = process.env.SHOT_PATH || "/tmp/yandex-screen.png";
const ya = await open({ ua: YA, shot: shotPath });
console.log("  yandex:", JSON.stringify(ya));
check("Яндекс.Браузер: экран показан", ya.visible);
check("Яндекс.Браузер: обе гифки на месте и загрузились", ya.gifs === 2 && ya.gifsLoaded, `img=${ya.gifs}`);
check("Яндекс.Браузер: выхода с экрана нет", ya.exits === 0, `кнопок/ссылок=${ya.exits}`);
check("Яндекс.Браузер: текст на месте",
  ya.fullText.includes("лучше всего работает в браузере Chrome")
    && ya.fullText.includes("Для того чтобы открыть его в Chrome")
    && ya.fullText.includes("Затем вы выберете браузер"),
  ya.fullText.slice(0, 200));

const yaApp = await open({ ua: YA_APP });
check("Встроенный браузер приложения «Яндекс»: экран показан", yaApp.visible);

const dismissed = await open({ ua: YA, dismissed: true });
check("Яндекс: показан даже после прошлого закрытия подсказки", dismissed.visible);

const onboard = await open({ ua: YA, path: "/onboard?u=probe-user" });
check("Яндекс: показан и на странице онбординга из Telegram", onboard.visible);

const chrome = await open({ ua: CHROME });
check("Chrome на Android: экран НЕ показан", !chrome.visible, chrome.text.slice(0, 60));

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
