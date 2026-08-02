// Гейт passkey должен опираться на ФАКТИЧЕСКУЮ возможность создать ключ.
// Репро реального Яндекс.Браузера (26.6, Android 15): интерфейс
// PublicKeyCredential есть, IUVPAA отвечает true, а navigator.credentials
// отсутствует целиком — вызов create() падает TypeError'ом.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const YA_UA = "Mozilla/5.0 (Linux; arm_64; Android 15; 25078RA3EY) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.7778.104 YaBrowser/26.6.5.104.00 Mobile Safari/537.36";
const CHROME_ANDROID = "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Mobile Safari/537.36";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

const b = await chromium.launch({ headless: true });

/// Открыть экран входа с подменённой средой и вернуть, что показало приложение.
/// Сценарий пользователя: аккаунт на устройстве известен (PWA-запуск / переход из
/// Telegram), поэтому гейт обязан увести на вход по коду, когда ключ невозможен.
const KNOWN_USER = "probe-user";
async function probe({ ua, killCredentials }) {
  const ctx = await b.newContext({ viewport: { width: 430, height: 920 }, userAgent: ua, serviceWorkers: "block" });
  await ctx.addInitScript(({ kill }) => {
    // IUVPAA всегда true — как на настоящем телефоне с блокировкой экрана.
    if (window.PublicKeyCredential) {
      window.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable = async () => true;
    }
    if (kill) {
      // Ровно поведение Яндекс.Браузера: интерфейс есть, API создания нет.
      Object.defineProperty(navigator, "credentials", { get: () => undefined, configurable: true });
    }
  }, { kill: killCredentials });
  const p = await ctx.newPage();
  p.on("console", (m) => { const t = m.text(); if (/panicked/.test(t)) console.log("[p]", t.slice(0, 160)); });
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.evaluate((uid) => {
    localStorage.clear();
    localStorage.setItem("pwa_dismissed", "true");
    localStorage.setItem("user_id", uid); // аккаунт известен, сессии нет
  }, KNOWN_USER);
  await p.goto(`${FE}/?u=${KNOWN_USER}`, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(9000);
  const text = (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ");
  const state = {
    codeScreen: text.includes("Вход по коду"),
    yandexScreen: await p.locator('[data-testid="pwa-yandex-screen"]').isVisible().catch(() => false),
    passkeyBtn: await p.locator('[data-testid="auth-btn-try-passkey"]').isVisible().catch(() => false),
    registerBtn: await p.locator('[data-testid="auth-btn-register"]').isVisible().catch(() => false),
    text: text.slice(0, 120),
  };
  await ctx.close();
  return state;
}

// 1. Яндекс.Браузер: приложение вообще не пускает внутрь — показывает экран
// «откройте в Chrome» (он появился позже гейта и перекрывает его).
const ya = await probe({ ua: YA_UA, killCredentials: true });
console.log("  yandex:", JSON.stringify(ya));
check("Яндекс.Браузер: показан экран перехода в Chrome", ya.yandexScreen);
check("Яндекс.Браузер: passkey не предлагается", !ya.passkeyBtn);

// 2. Обычный Android-браузер БЕЗ navigator.credentials — ради этого случая гейт
// и делался: ключ создать нечем, значит вход по коду.
const noCreds = await probe({ ua: CHROME_ANDROID, killCredentials: true });
console.log("  android-no-credentials:", JSON.stringify(noCreds));
check("Android без credentials: приложение увело на вход по коду", noCreds.codeScreen);
check("Android без credentials: passkey не предлагается", !noCreds.passkeyBtn);

// 3. Контроль: обычный Android с рабочим API → путь с ключом сохраняется.
const ok = await probe({ ua: CHROME_ANDROID, killCredentials: false });
console.log("  control:", JSON.stringify(ok));
check("обычный Android-браузер: путь с ключом сохранён", ok.passkeyBtn && !ok.codeScreen);

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
await b.close();
process.exit(fail === 0 ? 0 : 1);
