// Samsung Internet уводим в Chrome — так же, как Mi.
//
// Проверяется то, что решает исход: страница-перехватчик отдаётся ДО приложения
// (сам воркер, ещё до манифеста, сервис-воркера и WASM), в ней одна кнопка с
// intent-ссылкой, и эта ссылка несёт ПУТЬ И `?u=` — иначе человек попадёт в Chrome
// на корень без аккаунта. Для контроля рядом гоняется настоящий Chrome: его
// трогать нельзя, у него UA тоже содержит «Chrome».
//
// UA Samsung взят с устройства пользователя (SamsungBrowser/30.0, Android 10).
const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const UID = "demo-user-123";

const UAS = {
  samsung: "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "SamsungBrowser/30.0 Chrome/143.0.0.0 Mobile Safari/537.36",
  mi: "Mozilla/5.0 (Linux; U; Android 15; ru-ru; Redmi Build/AQ3A) AppleWebKit/537.36 " +
    "(KHTML, like Gecko) Version/4.0 Chrome/136.0.0.0 Mobile Safari/537.36 XiaoMi/MiuiBrowser/14.60.0",
  chrome: "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/137.0.0.0 Mobile Safari/537.36",
};

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const fetchPage = (ua, path) => fetch(`${BASE}${path}`, {
  headers: { "User-Agent": ua, Accept: "text/html,application/xhtml+xml" },
}).then((r) => r.text());

for (const [name, ua] of Object.entries(UAS)) {
  const html = await fetchPage(ua, `/?u=${UID}`);
  const handoff = html.includes('data-testid="pwa-mi-page"');
  if (name === "chrome") {
    check("настоящий Chrome не трогаем", !handoff);
    check("Chrome получает приложение с персональным манифестом",
      html.includes(`/manifest.json?u=${UID}`));
    continue;
  }
  check(`${name}: отдаётся страница перехода`, handoff);
  const intent = (/href="(intent:\/\/[^"]+)"/.exec(html) || [])[1] || "";
  check(`${name}: кнопка ведёт в Chrome по intent`,
    intent.includes("package=com.android.chrome"), intent.slice(0, 60));
  check(`${name}: intent несёт аккаунт`, intent.includes(`u=${UID}`), intent.slice(0, 90));
  // Ни манифеста, ни РЕГИСТРАЦИИ сервис-воркера: страница нарочно голая. Снятие
  // старой регистрации (`unregister`) на ней, наоборот, обязано быть — иначе
  // сервис-воркер с прошлых визитов отдаст кэш и перекроет этот экран.
  check(`${name}: манифест не подключается`, !html.includes('rel="manifest"'));
  check(`${name}: сервис-воркер не регистрируется, а снимается`,
    !html.includes("serviceWorker.register") && html.includes("unregister()"));
}

// Ссылка из Telegram ведёт на /onboard — путь обязан сохраниться, иначе человек
// уедет на корень мимо своего входа.
const onboard = await fetchPage(UAS.samsung, `/onboard?u=${UID}`);
const intent2 = (/href="(intent:\/\/[^"]+)"/.exec(onboard) || [])[1] || "";
check("samsung: путь /onboard сохраняется", intent2.includes("/onboard"), intent2.slice(0, 90));

// ── второй рубеж: экран внутри приложения ───────────────────────────────────
// Сервис-воркер с прошлых визитов может отдать оболочку из кэша, и до воркера
// управление не дойдёт — тогда тот же экран обязано показать само приложение.
// Чтобы это проверить, страницу забираем как Chrome (иначе воркер перехватит), а
// приложению подсовываем Samsung в navigator.userAgent.
import { chromium } from "playwright";

const b = await chromium.launch({ headless: true });
const context = await b.newContext({
  userAgent: UAS.chrome, viewport: { width: 412, height: 915 }, serviceWorkers: "block",
});
await context.addInitScript((ua) => {
  Object.defineProperty(navigator, "userAgent", { get: () => ua });
}, UAS.samsung);
const page = await context.newPage();
await page.goto(`${BASE}/?u=${UID}`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(10000);
check("в приложении Samsung тоже видит экран перехода",
  await page.locator('[data-testid="pwa-mi-screen"]').count() > 0,
  (await page.locator("body").innerText()).replace(/\s+/g, " ").slice(0, 80));
const href = await page.getAttribute('[data-testid="pwa-btn-open-chrome"]', "href");
check("кнопка в приложении ведёт в Chrome и несёт аккаунт",
  String(href).includes("package=com.android.chrome") && String(href).includes(`u=${UID}`),
  String(href).slice(0, 90));
await page.screenshot({ path: "/tmp/samsung-hop.png", fullPage: true });
console.log("снято: /tmp/samsung-hop.png");
await context.close();
await b.close();

console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
