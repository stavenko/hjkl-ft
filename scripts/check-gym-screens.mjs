// Какой экран получает человек в приложении тренировок.
//
// Прогон закрывает ровно то, из чего первая версия и состоит: вход, подписку,
// установку и тупиковые браузеры. Главное здесь — НЕ сломать Яндекс и Mi: их
// пути выстраданы замерами (intent из Яндекса не работает, поэтому там учат
// листу «Поделиться», а из Mi работает — там одна кнопка), и эти экраны должны
// показываться ДО входа, иначе человек заведёт аккаунт там, где им не
// воспользуется.
//
// По умолчанию поднимает свой сервер над `gym/dist` (то есть проверяет ровно ту
// сборку, которую сейчас катят). BASE=https://… — проверить выкаченный стенд.
//
//   cd gym && trunk build --release && cd ..
//   node scripts/check-gym-screens.mjs
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { chromium } from "playwright";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const DIST = path.join(ROOT, "gym", "dist");

const UAS = {
  mi:
    "Mozilla/5.0 (Linux; Android 15; REDMI 15C Build/AP3A.240905.015.A2) AppleWebKit/537.36 " +
    "(KHTML, like Gecko) Chrome/135.0.7049.79 Mobile Safari/537.36 XiaoMi/MiuiBrowser/14.60.0-gn",
  yandex:
    "Mozilla/5.0 (Linux; Android 15; SM-A155F) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/135.0.0.0 YaBrowser/25.4.1.100 Mobile Safari/537.36",
  chrome:
    "Mozilla/5.0 (Linux; Android 15; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/135.0.0.0 Mobile Safari/537.36",
  ios:
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 " +
    "(KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
  unknown: "Mozilla/5.0 (Linux; Android 15; Nameless) SomeUnknownEngine/1.0",
  desktop:
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) " +
    "Chrome/135.0.0.0 Safari/537.36",
};

// Сессия и подтверждённая подписка — подкладываются в localStorage: в основной
// части прогона воркеров нет и звать их незачем, проверяется РАЗВИЛКА экранов, а
// не они. `exp` в токене — 2100 год; подпись поддельная и никем здесь не
// проверяется.
//
// Отдельно, в самом конце, идёт проверка НАСТОЯЩЕЙ подписки: там сессия и
// оплата заводятся тем же механизмом, что и для приложения питания
// (frontend/scripts/seed-test-subscription.mjs → TEST_ENTITLEMENT в
// payment-worker-dev), и приложение спрашивает статус по-честному.
const TOKEN = "x.eyJzdWIiOiJ1LWFiYzEyM2RlZjQ1NiIsImV4cCI6NDEwMjQ0NDgwMH0.y";
const SESSION = `
  localStorage.setItem('gym_auth_token', '${TOKEN}');
  localStorage.setItem('gym_user_id', 'u-abc123def456');
`;
const SUBSCRIBED = `${SESSION}
  localStorage.setItem('gym_subscription', '{"plan":"pro","end":0,"active":true}');
`;

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

// ── Сервер над собранным dist (если не задан BASE) ──
const MIME = {
  ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
  ".toml": "text/plain", ".png": "image/png", ".gif": "image/gif",
  ".svg": "image/svg+xml", ".woff2": "font/woff2",
  ".webmanifest": "application/manifest+json",
};
let server = null;
let BASE = process.env.BASE;
if (!BASE) {
  if (!fs.existsSync(path.join(DIST, "index.html"))) {
    console.error(`нет сборки: ${DIST}\nсоберите её: cd gym && trunk build --release`);
    process.exit(2);
  }
  // CSP отдаётся из собранного `_headers` — ровно та, что уедет на Pages.
  // Иначе прогон проверял бы приложение БЕЗ политики, а хеши встроенных скриптов
  // считает наш же build-shell.sh: ошибка в нём всплыла бы только в проде, где
  // браузер молча не выполнит ни регистрацию воркера, ни сторож обновления.
  let CSP = (() => {
    const h = path.join(DIST, "_headers");
    if (!fs.existsSync(h)) return null;
    const m = fs.readFileSync(h, "utf8").match(/Content-Security-Policy:\s*(.+)/);
    return m ? m[1].trim() : null;
  })();
  if (CSP && process.env.PAYMENT_BASE) {
    CSP = CSP.replace(/(connect-src[^;]*)/, `$1 ${process.env.PAYMENT_BASE}`);
  }
  if (!CSP) {
    console.error("в сборке нет _headers с CSP — проверять политику нечем");
    process.exit(2);
  }
  server = http.createServer((req, res) => {
    const [rawPath, rawQuery] = req.url.split("?");
    const url = decodeURIComponent(rawPath);
    const u = new URLSearchParams(rawQuery || "").get("u") || "";

    // Персональный манифест — как его отдаёт gym/pwa-worker.js. Локальный
    // сервер обязан это уметь: иначе прогон проверял бы приложение без той
    // половины, ради которой воркер и заведён.
    if (url === "/manifest.json") {
      const body = JSON.stringify({
        name: "re:Norma — тренировки",
        start_url: u ? `/?u=${encodeURIComponent(u)}` : "/",
        id: u ? `/app-${u}` : "/",
        scope: "/",
        display: "standalone",
      });
      res.writeHead(200, {
        "Content-Type": "application/manifest+json",
        "Cache-Control": "no-store",
        "Content-Security-Policy": CSP,
      });
      res.end(body);
      return;
    }

    // PAYMENT_BASE переопределяет адрес payment-worker В САМОЙ КОНФИГУРАЦИИ,
    // которую читает приложение. Без этого проверку живой подписки нельзя ни
    // прогнать против заглушки, ни нацелить на другой стенд — а значит нельзя и
    // убедиться, что она вообще работает.
    if (url === "/config/frontend.toml" && process.env.PAYMENT_BASE) {
      const cfg = fs
        .readFileSync(path.join(DIST, "config/frontend.toml"), "utf8")
        .replace(/^payment_base_url = .*$/m, `payment_base_url = "${process.env.PAYMENT_BASE}"`);
      res.writeHead(200, { "Content-Type": "text/plain", "Content-Security-Policy": CSP });
      res.end(cfg);
      return;
    }

    let file = path.join(DIST, url);
    // Одностраничное приложение: всё неизвестное — на index.html (как _redirects).
    if (!file.startsWith(DIST) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      file = path.join(DIST, "index.html");
    }
    let body = fs.readFileSync(file);
    // Та же подстановка, что делает HTMLRewriter в воркере: ?u= попадает в
    // ссылку на манифест ДО запуска wasm — браузер снимает манифест при
    // установке, и снимок обязан быть уже персональным.
    if (u && path.extname(file) === ".html") {
      body = Buffer.from(
        body.toString().replace(
          /(<link rel="manifest" href=")[^"]*(")/,
          `$1/manifest.json?u=${encodeURIComponent(u)}$2`,
        ),
      );
    }
    res.writeHead(200, {
      "Content-Type": MIME[path.extname(file)] || "application/octet-stream",
      "Content-Security-Policy": CSP,
    });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  BASE = `http://127.0.0.1:${server.address().port}`;
}
console.log(`адрес: ${BASE}\n`);

const b = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });

async function open(ua, seed, opts = {}) {
  const desktop = ua === UAS.desktop;
  const ctx = await b.newContext({
    userAgent: ua,
    viewport: desktop ? { width: 1100, height: 800 } : { width: 390, height: 844 },
    isMobile: !desktop,
    hasTouch: !desktop,
    locale: "ru-RU",
  });
  if (seed) await ctx.addInitScript({ content: seed });
  const page = await ctx.newPage();
  // «Выложена другая сборка» иначе не воспроизвести: запущенной версией служит
  // штамп внутри init.js, и подменять надо именно ответ сервера.
  if (opts.deployedVersion) {
    await page.route("**/version.json*", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ v: opts.deployedVersion }),
      }));
  }
  const errors = [];
  const csp = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  // Нарушение CSP браузер не считает ошибкой страницы — только сообщением в
  // консоли и событием. Ловим оба: заблокированный встроенный скрипт иначе
  // просто не выполнится, и прогон этого не заметит.
  page.on("console", (m) => {
    if (/Content Security Policy/i.test(m.text())) csp.push(m.text());
  });
  await ctx.addInitScript({
    content: `document.addEventListener('securitypolicyviolation',
      function (e) { (window.__csp = window.__csp || []).push(e.violatedDirective + ' ' + e.blockedURI); });`,
  });
  await page.goto(`${BASE}/${opts.query || ""}`, { waitUntil: "domcontentloaded" });
  // Ждём именно появления разметки, а не «сколько-нибудь секунд»: wasm грузится
  // по-разному, и глухая пауза либо врёт, либо тормозит прогон.
  await page.waitForSelector("[data-testid]", { timeout: 20000 }).catch(() => {});
  const inPage = await page.evaluate(() => window.__csp || []).catch(() => []);
  return { ctx, page, errors, csp: csp.concat(inPage) };
}

// Приложение, запущенное с иконки: подписка есть, установка позади.
const AS_PWA = `${SUBSCRIBED}
  Object.defineProperty(navigator, 'standalone', { get: () => true });`;

const shown = (page, id) => page.getByTestId(id).isVisible().catch(() => false);

// ── 1. Тупиковые браузеры — ДО входа ──
{
  console.log("1. Mi Browser — одна кнопка в Chrome");
  const { ctx, page, errors } = await open(UAS.mi);
  check(await shown(page, "install-chrome-handoff"), "показан экран ухода в Chrome");
  check(!(await shown(page, "gym-login")), "вход НЕ предлагается");
  const href = await page.getByTestId("install-btn-open-chrome").getAttribute("href");
  check(/^intent:\/\/.*package=com\.android\.chrome/.test(href || ""), "кнопка ведёт intent'ом в Chrome");
  check(errors.length === 0, `без ошибок исполнения${errors.length ? `: ${errors[0]}` : ""}`);
  await ctx.close();
}
{
  console.log("\n2. Яндекс.Браузер — инструкция «Поделиться» → Chrome");
  const { ctx, page } = await open(UAS.yandex);
  check(await shown(page, "install-yandex"), "показан яндексовский экран");
  check(!(await shown(page, "install-btn-open-chrome")), "кнопки intent'а тут НЕТ (оттуда не работает)");
  const gifs = await page.evaluate(() =>
    [...document.querySelectorAll('img[src^="/onboard-img/"]')].filter((i) => i.naturalWidth > 0).length);
  check(gifs === 2, `обе гифки загрузились (${gifs}/2)`);
  await ctx.close();
}
{
  console.log("\n3. Неопознанный браузер — адрес для переноса руками");
  const { ctx, page } = await open(UAS.unknown);
  check(await shown(page, "install-unknown"), "показан экран незнакомого браузера");
  const url = await page.getByTestId("install-btn-copy-url").innerText();
  check(url.startsWith(BASE), "в кнопке — адрес приложения");
  await ctx.close();
}

// ── 2. Вход ──
{
  console.log("\n4. Без сессии — экран входа");
  const { ctx, page } = await open(UAS.chrome);
  check(await shown(page, "gym-login"), "главное действие — войти существующим ключом");
  check(await shown(page, "gym-go-register"), "создание ключа — ссылкой, а не кнопкой");
  await ctx.close();
}

// ── 3. Подписка ──
{
  console.log("\n5. Сессия без подписки — блокирующий экран");
  const { ctx, page } = await open(UAS.chrome, `${SESSION}
    localStorage.setItem('gym_subscription', '{"plan":"","end":0,"active":false}');`);
  check(await shown(page, "app-locked"), "показан экран «нужна подписка»");
  check(!(await shown(page, "app-stub")), "внутрь приложения НЕ пускает");
  await ctx.close();
}

// ── 4. Установка ──
{
  console.log("\n6. Android с подпиской — инструкция по установке");
  const { ctx, page } = await open(UAS.chrome, SUBSCRIBED);
  check(await shown(page, "install-steps"), "показаны шаги установки");
  const shots = await page.evaluate(() =>
    [...document.querySelectorAll(".step-shot")].filter((i) => i.naturalWidth > 0).length);
  check(shots > 0, `снимки шагов загрузились (${shots})`);
  await ctx.close();
}
{
  console.log("\n7. iPhone с подпиской — своя инструкция");
  const { ctx, page } = await open(UAS.ios, SUBSCRIBED);
  check(await shown(page, "install-steps"), "показаны шаги установки");
  await ctx.close();
}
{
  console.log("\n8. Десктоп — «это приложение для телефона», выход есть");
  const { ctx, page } = await open(UAS.desktop, SUBSCRIBED);
  check(await shown(page, "install-desktop"), "показан десктопный экран");
  check(await shown(page, "install-btn-dismiss"), "кнопка «продолжить в браузере» есть");
  await page.getByTestId("install-btn-dismiss").click();
  check(await shown(page, "app-stub"), "по ней попадаем в приложение");
  await ctx.close();
}
{
  console.log("\n9. Поставлено, но всё ещё во вкладке — «открывайте с иконки»");
  const { ctx, page } = await open(UAS.chrome, `${SUBSCRIBED}
    localStorage.setItem('gym_pwa_installed', '1');`);
  check(await shown(page, "install-installed"), "показан экран «приложение поставлено»");
  check(!(await shown(page, "app-stub")), "во вкладку приложение НЕ пускает");
  await page.getByTestId("install-btn-show-steps").click();
  check(await shown(page, "install-steps"), "«иконки так и нет» возвращает к инструкции");
  await ctx.close();
}

// ── 5. Приложение, меню и настройки ──
{
  console.log("\n10. Запуск с иконки — приложение и меню");
  const { ctx, page, errors, csp } = await open(UAS.ios, AS_PWA);
  check(await shown(page, "app-stub"), "показана заглушка приложения тренировок");
  check(!(await shown(page, "install-steps")), "инструкция по установке больше не мешает");
  check(await shown(page, "tabbar"), "нижнее меню на месте");
  check(await shown(page, "tab-settings"), "в меню есть иконка настроек");
  check(!(await shown(page, "tab-update-dot")), "точки обновления нет — сборка свежая");
  // Экран загрузки обязан исчезнуть: он висит поверх всего, и оставшись, запер бы
  // человека на крутящемся кольце.
  check(
    await page.evaluate(() => !document.getElementById("splash")),
    "экран загрузки снят",
  );
  check(errors.length === 0, `без ошибок исполнения${errors.length ? `: ${errors[0]}` : ""}`);
  // CSP отдаётся настоящая: если build-shell.sh посчитал хеши встроенных
  // скриптов неверно, браузер молча их не выполнит — ни регистрации
  // сервис-воркера, ни сторожа обновления не будет.
  check(csp.length === 0, `CSP ничего не заблокировала${csp.length ? `: ${csp[0]}` : ""}`);
  check(
    await page.evaluate(() => typeof window.__rnUpdateArm === "function"),
    "сторож обновления объявлен (встроенный скрипт выполнился)",
  );
  await ctx.close();
}
{
  console.log("\n11. Настройки — язык, версия, ключи, выход");
  const { ctx, page } = await open(UAS.ios, AS_PWA);
  await page.getByTestId("tab-settings").click();
  check(await shown(page, "settings"), "иконка меню открывает настройки");
  check(await shown(page, "set-version"), "есть строка версии");
  check(await shown(page, "set-btn-check"), "есть ручная проверка обновления");
  check(await shown(page, "set-btn-add-key"), "есть добавление ключа на устройство");
  check(await shown(page, "set-btn-phrase"), "есть фраза восстановления");
  check(await shown(page, "set-btn-logout"), "есть выход");
  // Версия показывается ТА, что проштампована в сборке, а не «—»: иначе строка
  // ничего не сообщает, и обновление не с чем сравнивать.
  const stamped = await page.evaluate(() => globalThis.__APP_VERSION__ || "");
  const rowText = await page.getByTestId("set-version").innerText();
  check(stamped.length > 0 && rowText.includes(stamped), `в строке версия сборки (${stamped})`);

  // Язык переключается и держится: инструкция по установке и настройки должны
  // говорить на одном языке, а выбор — переживать перезагрузку.
  await page.getByTestId("set-lang-en").click();
  check((await page.getByTestId("settings").innerText()).includes("Settings"), "язык переключился");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid]", { timeout: 20000 }).catch(() => {});
  await page.getByTestId("tab-settings").click();
  check(
    (await page.getByTestId("settings").innerText()).includes("Settings"),
    "выбор языка пережил перезагрузку",
  );
  await ctx.close();
}
{
  console.log("\n12. Выложена новая сборка — точка в меню и кнопка «Обновить»");
  const { ctx, page } = await open(UAS.ios, AS_PWA, { deployedVersion: "deadbeef0000" });
  check(await shown(page, "tab-update-dot"), "на иконке меню красная точка");
  await page.getByTestId("tab-settings").click();
  check(await shown(page, "set-btn-update"), "в настройках кнопка «Обновить»");
  check(!(await shown(page, "set-btn-check")), "ручной проверки вместо неё нет");
  check(await shown(page, "set-update-dot"), "строка версии тоже отмечена");
  await ctx.close();
}
{
  console.log("\n13. Выход из аккаунта возвращает на вход");
  const { ctx, page } = await open(UAS.ios, AS_PWA);
  page.on("dialog", (d) => d.accept());
  await page.getByTestId("tab-settings").click();
  await page.getByTestId("set-btn-logout").click();
  check(await shown(page, "gym-login"), "показан экран входа");
  check(!(await shown(page, "tabbar")), "меню спрятано — приложения больше нет");
  await ctx.close();
}

// ── 6. Сервис-воркер: приложение открывается без сети ──
//
// Это и есть весь смысл прекэша. Проверяется на СОБРАННОМ dist, а не на живом
// стенде: офлайн выключается у контекста браузера, до сервера дело не доходит.
{
  console.log("\n14. Сервис-воркер — запуск без сети");
  const { ctx, page } = await open(UAS.chrome, AS_PWA);
  // Ждём, пока воркер не только зарегистрируется, но и ВОЗЬМЁТ страницу под
  // контроль: до этого его кэш пуст, и офлайн-проверка отвечала бы не на тот
  // вопрос («успел ли прекэш», а не «работает ли он»).
  const controlled = await page.evaluate(async () => {
    if (!navigator.serviceWorker) return false;
    const reg = await navigator.serviceWorker.ready.catch(() => null);
    if (!reg) return false;
    for (let i = 0; i < 60 && !navigator.serviceWorker.controller; i++) {
      await new Promise((r) => setTimeout(r, 100));
    }
    return !!navigator.serviceWorker.controller;
  });
  check(controlled, "сервис-воркер взял страницу под контроль");
  // Дать прекэшу дойти до конца: install идёт своим чередом после активации.
  await page.waitForTimeout(1500);
  const cached = await page.evaluate(async () => {
    const c = await caches.open("gym-v1");
    const keys = await c.keys();
    return keys.map((r) => new URL(r.url).pathname);
  });
  check(cached.includes("/init.js"), "init.js в кэше");
  check(cached.some((u) => u.endsWith(".wasm")), "wasm в кэше");
  check(cached.includes("/config/frontend.toml"), "конфигурация в кэше");

  await ctx.setOffline(true);
  await page.reload({ waitUntil: "domcontentloaded" }).catch(() => {});
  await page.waitForSelector("[data-testid]", { timeout: 20000 }).catch(() => {});
  check(await shown(page, "app-stub"), "без сети приложение всё равно открылось");
  check(
    await page.evaluate(() => !document.getElementById("splash")),
    "и не осталось на экране загрузки",
  );
  await ctx.setOffline(false);
  await ctx.close();
}

// ── 7. Персональный манифест и запасные пути входа ──
//
// Ровно та половина, без которой установленное приложение на iOS становится
// тупиком: своё хранилище, сессии нет, ключ не сработал — и деться некуда.
{
  console.log("\n15. Манифест уносит аккаунт в start_url");
  // Никого не знаем — ссылка общая: установка без входа тоже обязана работать.
  const { ctx, page } = await open(UAS.ios);
  const before = await page.getAttribute('link[rel="manifest"]', "href");
  check(before === "/manifest.json", `без аккаунта манифест общий (${before})`);
  await ctx.close();
}
{
  // Сессия есть (Android делит localStorage со вкладкой) — приложение само
  // перенацеливает ссылку, не дожидаясь ?u=.
  const { ctx, page } = await open(UAS.ios, SUBSCRIBED);
  const href = await page.getAttribute('link[rel="manifest"]', "href");
  check(href === "/manifest.json?u=u-abc123def456", `по сессии ссылка персональная (${href})`);
  await ctx.close();
}
{
  const { ctx, page } = await open(UAS.ios, SUBSCRIBED, { query: "?u=u-abc123def456" });
  // Воркер вписал ?u= в разметку ещё до запуска wasm — именно этот снимок
  // браузер забирает при «Добавить на экран Домой».
  const href = await page.getAttribute('link[rel="manifest"]', "href");
  check(href === "/manifest.json?u=u-abc123def456", `ссылка персональная (${href})`);
  const m = await page.evaluate(async (h) => (await fetch(h)).json(), href);
  check(m.start_url === "/?u=u-abc123def456", `start_url несёт аккаунт (${m.start_url})`);
  check(m.id === "/app-u-abc123def456", `у установки свой id (${m.id})`);
  await ctx.close();
}
{
  console.log("\n16. Запасные пути входа");
  // Без сессии и без ?u= код предлагать некому — слать некуда.
  const a = await open(UAS.ios);
  check(await shown(a.page, "gym-login"), "ключ — главный путь");
  check(await shown(a.page, "login-alt-phrase"), "фраза восстановления предложена");
  check(!(await shown(a.page, "login-alt-code")), "кода НЕТ: аккаунт неизвестен");
  await a.ctx.close();

  // Установленное приложение принесло аккаунт — код появляется запасным путём.
  // UA здесь iOS: там паскей есть всегда, поэтому главным остаётся ключ.
  const b2 = await open(UAS.ios, null, { query: "?u=u-abc123def456" });
  check(await shown(b2.page, "gym-login"), "ключ по-прежнему главный");
  check(await shown(b2.page, "login-alt-code"), "с ?u= код предложен");
  await b2.page.getByTestId("login-alt-code").click();
  check(await shown(b2.page, "login-code"), "экран кода открывается");
  await b2.page.getByTestId("login-alt-key").click();
  check(await shown(b2.page, "gym-login"), "и с него есть дорога обратно к ключу");
  await b2.ctx.close();

  // Устройство, где паскея не бывает (Android без платформенного
  // аутентификатора), да ещё и с известным аккаунтом: код открывается СРАЗУ.
  // Показывать там кнопку «Войти ключом» значило бы вести в заведомую стену.
  const d = await open(UAS.chrome, null, { query: "?u=u-abc123def456" });
  check(await shown(d.page, "login-code"), "без паскея код открыт сразу");
  check(!(await shown(d.page, "gym-login")), "кнопки ключа там нет");
  check(await shown(d.page, "login-alt-key"), "но вернуться к ключу можно");
  await d.ctx.close();

  // Фраза доступна всегда — она сама себя опознаёт, аккаунт знать не надо.
  const c = await open(UAS.ios);
  await c.page.getByTestId("login-alt-phrase").click();
  check(await shown(c.page, "login-phrase-input"), "поле фразы открылось");
  check(await shown(c.page, "login-btn-phrase"), "и кнопка входа по ней");
  await c.ctx.close();
}

// ── 8. НАСТОЯЩАЯ подписка, а не подложенная ──
//
// Всё выше проверяет нашу развилку на подсунутом localStorage. Здесь —
// единственное место, где приложение по-настоящему ходит в payment-worker:
// заводится живая dev-подписка тем же механизмом, что и для приложения питания
// (TEST_ENTITLEMENT: оплаченный guest-claim без денег), и статус спрашивается
// по-честному. Заодно это проверяет, что gym-origin вообще пускают в CORS.
//
// Пропускается, если dev-воркер недоступен: прогон над собранным dist не обязан
// требовать сети, но и молча делать вид, что проверил, не должен.
{
  console.log("\n17. Подписка спрашивается у payment-worker по-настоящему");
  const PAYMENT = process.env.PAYMENT_BASE
    || "https://payment-worker-dev.vg-stavenko.workers.dev";

  // Доступность проверяется ИЗ БРАУЗЕРА, а не из node. Это разные сети: в
  // песочнице агента node ходит наружу через прокси, а headless-браузер — нет
  // (туннель до воркера открывается и обрывается на первом же обмене). Спроси мы
  // node, прогон бы уверенно пошёл проверять и упал бы на ровном месте.
  let browserOnline = false;
  {
    const probe = await b.newContext();
    const pp = await probe.newPage();
    // Пробуем С САМОЙ СТРАНИЦЫ приложения, а не с `data:`-заглушки: у той
    // непрозрачный origin, fetch оттуда запрещён и проба всегда врала бы «нет
    // сети». Заодно это честнее — тот же origin и та же CSP, что у приложения.
    await pp.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
    browserOnline = await pp.evaluate(async (u) => {
      try {
        await fetch(u, { mode: "no-cors" });
        return true;
      } catch {
        return false;
      }
    }, `${PAYMENT}/health`);
    await probe.close();
  }

  let seeded = null;
  if (!browserOnline) {
    console.log("  ПРОПУСК  браузер не дотягивается до dev-воркера — живую подписку не проверить");
    console.log("           (в песочнице агента наружу ходит только node; на обычной машине проверка идёт)");
  } else try {
    const out = execFileSync(
      "node",
      [
        path.join(ROOT, "frontend/scripts/seed-test-subscription.mjs"),
        `gym-check-${Date.now()}`,
        "--app", "gym",
        "--payment", PAYMENT,
        "--json",
      ],
      { encoding: "utf8", timeout: 60000, stdio: ["ignore", "pipe", "pipe"] },
    );
    seeded = JSON.parse(out);
  } catch (e) {
    console.log(`  ПРОПУСК  не удалось завести подписку: ${String(e.message).split("\n")[0]}`);
  }

  if (seeded) {
    check(seeded.status?.active === true, `подписка заведена (plan=${seeded.status?.plan})`);
    const seed = Object.entries(seeded.storage)
      .map(([k, v]) => `localStorage.setItem(${JSON.stringify(k)}, ${JSON.stringify(v)});`)
      .join("\n");
    // Кэша подписки НЕ подкладываем: приложение обязано спросить сам воркер.
    const { ctx, page, errors } = await open(UAS.ios, `${seed}
      Object.defineProperty(navigator, 'standalone', { get: () => true });`);
    // Ждём, пока живой запрос разрешится: экран «проверяем» должен смениться.
    await page.waitForSelector('[data-testid="app-stub"]', { timeout: 25000 }).catch(() => {});
    check(await shown(page, "app-stub"), "приложение пустило внутрь по ЖИВОМУ ответу воркера");
    check(!(await shown(page, "app-locked")), "и не показало «нужна подписка»");
    check(!(await shown(page, "app-offline")), "и не показало «нет связи» — CORS пропустил");
    const cached = await page.evaluate(() => localStorage.getItem("gym_subscription"));
    check(
      JSON.parse(cached || "{}").active === true,
      "ответ воркера запомнен в кэше приложения",
    );
    check(errors.length === 0, `без ошибок исполнения${errors.length ? `: ${errors[0]}` : ""}`);
    await ctx.close();
  }
}

await b.close();
if (server) server.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё в порядке");
process.exit(failed ? 1 : 0);
