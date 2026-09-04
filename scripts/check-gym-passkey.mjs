// Настоящий паскей: полный жизненный цикл аккаунта против ЖИВОГО auth-worker-dev.
//
// Это единственное место, где церемонии WebAuthn выполняются по-настоящему.
// Остальные прогоны проверяют развилку экранов; здесь заводится ключ, создаётся
// аккаунт, оформляется подписка и происходит вход — всё через настоящие ручки.
//
// Две уловки, обе обязательные:
//
// 1. ВИРТУАЛЬНЫЙ АУТЕНТИФИКАТОР. У headless-браузера паскея нет, поэтому он
//    заводится через CDP (WebAuthn.addVirtualAuthenticator): резидентный ключ с
//    подтверждением личности, как платформенный на телефоне. Для сервера он
//    неотличим от настоящего — подписи честные.
//
// 2. СТРАНИЦА ОТДАЁТСЯ ПОД ИМЕНЕМ СТЕНДА. rpId приходит от auth-worker и зависит
//    от origin церемонии; браузер отвергнет ceremony, если rpId не суффикс
//    origin страницы. С `127.0.0.1` это невозможно, поэтому собранный dist
//    подсовывается запросам к https://renorma-gym-dev.pages.dev через перехват.
//    Запросы к auth-worker и payment-worker НЕ перехватываются — они идут в сеть.
//
// ВНИМАНИЕ: прогон создаёт НАСТОЯЩИЕ аккаунты в dev-хранилище auth-worker (по
// одному на запуск) и оформляет им тестовую подписку. Это dev, денег не берётся
// (ручка /test/* в проде отдаёт 404), но аккаунты остаются.
//
//   cd gym && trunk build --release && cd ..
//   node scripts/check-gym-passkey.mjs
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const DIST = path.join(ROOT, "gym", "dist");
// Имя стенда важно не для сети, а для origin: от него auth-worker производит
// rpId (GYM_RP_ORIGIN в dev), и по нему же браузер проверяет церемонию.
const ORIGIN = process.env.GYM_ORIGIN || "https://renorma-gym-dev.pages.dev";
const PAYMENT = process.env.PAYMENT_BASE
  || "https://payment-worker-dev.vg-stavenko.workers.dev";

const IOS = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 "
  + "(KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";

const MIME = {
  ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
  ".toml": "text/plain", ".png": "image/png", ".gif": "image/gif",
  ".svg": "image/svg+xml", ".woff2": "font/woff2", ".json": "application/json",
};

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

if (!fs.existsSync(path.join(DIST, "index.html"))) {
  console.error(`нет сборки: ${DIST}\nсоберите её: cd gym && trunk build --release`);
  process.exit(2);
}
const CSP = (fs.readFileSync(path.join(DIST, "_headers"), "utf8")
  .match(/Content-Security-Policy:\s*(.+)/) || [])[1]?.trim();

// Те же две поправки окружения, что в scripts/check-gym-screens.mjs: браузер из
// образа и выход наружу только по TLS 1.2 (приветствие 1.3 рвёт релей прокси).
const IMAGE_CHROMIUM = "/opt/pw-browsers/chromium";
const proxyServer = process.env.HTTPS_PROXY || process.env.https_proxy;
const browser = await chromium.launch({
  ...(fs.existsSync(IMAGE_CHROMIUM) ? { executablePath: IMAGE_CHROMIUM } : {}),
  ...(proxyServer
    ? {
        proxy: { server: proxyServer, bypass: "127.0.0.1,localhost" },
        args: [
          "--ssl-version-max=tls1.2",
          "--disable-background-networking",
          "--disable-component-update",
          "--disable-extensions",
          "--disable-sync",
        ],
      }
    : {}),
});

// Сервис-воркер выключен: он перехватывает навигации и конфликтует с подменой
// ответов, а проверяется здесь не он.
const ctx = await browser.newContext({
  userAgent: IOS,
  viewport: { width: 390, height: 844 },
  isMobile: true,
  hasTouch: true,
  locale: "ru-RU",
  serviceWorkers: "block",
});

// Собранный dist под именем стенда.
await ctx.route(`${ORIGIN}/**`, async (route) => {
  const url = new URL(route.request().url());
  let file = path.join(DIST, decodeURIComponent(url.pathname));
  if (!file.startsWith(DIST) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    file = path.join(DIST, "index.html");
  }
  let body = fs.readFileSync(file);
  // Персональный манифест — как его отдаёт pwa-worker.js.
  const u = url.searchParams.get("u") || "";
  if (url.pathname === "/manifest.json") {
    body = Buffer.from(JSON.stringify({ start_url: u ? `/?u=${u}` : "/", scope: "/" }));
  } else if (u && path.extname(file) === ".html") {
    body = Buffer.from(body.toString().replace(
      /(<link rel="manifest" href=")[^"]*(")/, `$1/manifest.json?u=${u}$2`));
  }
  await route.fulfill({
    status: 200,
    headers: {
      "Content-Type": MIME[path.extname(file)] || "application/octet-stream",
      ...(CSP ? { "Content-Security-Policy": CSP } : {}),
    },
    body,
  });
});

const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
// DEBUG_PASSKEY=1 показывает сеть и консоль страницы. Оставлено намеренно:
// когда церемония не идёт, причина почти всегда в CORS или в невыкаченном
// воркере, и без этих строк её ищешь вслепую.
if (process.env.DEBUG_PASSKEY) {
  page.on("console", (m) => console.log("   [console]", m.text().slice(0, 200)));
  page.on("requestfailed", (r) =>
    console.log("   [req-failed]", r.url().slice(0, 90), r.failure()?.errorText));
  page.on("response", (r) => {
    if (/auth-worker|payment-worker/.test(r.url())) {
      console.log("   [net]", r.status(), r.url().replace(/https:\/\/[^/]+/, ""));
    }
  });
}

// Виртуальный платформенный аутентификатор: резидентный ключ, личность
// подтверждена, присутствие подтверждается само — так ведёт себя Face ID.
const cdp = await ctx.newCDPSession(page);
await cdp.send("WebAuthn.enable");
const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
  options: {
    protocol: "ctap2",
    transport: "internal",
    hasResidentKey: true,
    hasUserVerification: true,
    isUserVerified: true,
    automaticPresenceSimulation: true,
  },
});
console.log(`адрес:  ${ORIGIN}  (dist подменой)`);
console.log(`ключ:   виртуальный аутентификатор ${authenticatorId}\n`);

const goto = async () => {
  await page.goto(`${ORIGIN}/`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid]", { timeout: 30000 }).catch(() => {});
};
const shown = (id) => page.getByTestId(id).isVisible().catch(() => false);

// ── 1. Завести ключ: настоящая церемония создания ──
console.log("1. Создание ключа — /register/begin + WebAuthn.create + /register/finish");
await goto();
check(await shown("gym-login"), "экран входа показан");
await page.getByTestId("gym-go-register").click();
await page.getByTestId("gym-name").fill("Проверка зала");
await page.getByTestId("gym-register").click();

// Аккаунт заведён — приложение уходит спрашивать подписку. Её нет, значит
// блокирующий экран: это и есть доказательство, что вход СОСТОЯЛСЯ.
const locked = await page.getByTestId("app-locked").waitFor({ timeout: 30000 })
  .then(() => true).catch(() => false);
if (!locked && process.env.DEBUG_PASSKEY) {
  console.log("   [экран]", (await page.locator("body").innerText()).replace(/\n+/g, " | ").slice(0, 300));
}
check(locked, "ключ создан, аккаунт заведён (дальше упёрлись в подписку — как и должно)");

const userId = await page.evaluate(() => localStorage.getItem("gym_user_id"));
const token = await page.evaluate(() => localStorage.getItem("gym_auth_token"));
check(!!userId, `сервер выдал аккаунт (…${String(userId).slice(-8)})`);
check(!!token && token.split(".").length === 3, "и подписанный токен сессии");
const cred = await cdp.send("WebAuthn.getCredentials", { authenticatorId });
check(cred.credentials.length === 1, `ключ лёг в аутентификатор (${cred.credentials.length})`);
check(
  cred.credentials[0]?.rpId === new URL(ORIGIN).host,
  `rpId ключа = ${cred.credentials[0]?.rpId}`,
);
const firstKey = cred.credentials[0]?.credentialId;

// ── 2. Подписка тем же механизмом, что у приложения питания ──
console.log("\n2. Подписка на этот аккаунт (TEST_ENTITLEMENT)");
let seeded = null;
try {
  seeded = JSON.parse(execFileSync("node", [
    path.join(ROOT, "frontend/scripts/seed-test-subscription.mjs"),
    userId, "--app", "gym", "--payment", PAYMENT, "--json",
  ], { encoding: "utf8", timeout: 60000 }));
} catch (e) {
  check(false, `не удалось оформить подписку: ${String(e.message).split("\n")[0]}`);
}
if (seeded) {
  check(seeded.status?.active === true, `подписка активна (plan=${seeded.status?.plan})`);
  await page.evaluate(() => localStorage.removeItem("gym_subscription"));
  await goto();
  const inside = await page.getByTestId("install-steps").waitFor({ timeout: 30000 })
    .then(() => true).catch(() => false);
  check(inside, "с подпиской приложение пустило дальше — к установке");
}

// ── 3. Выйти и войти ТЕМ ЖЕ ключом: настоящая церемония получения ──
console.log("\n3. Вход существующим ключом — /authenticate/begin + WebAuthn.get + finish");
await page.evaluate(() => {
  localStorage.removeItem("gym_user_id");
  localStorage.removeItem("gym_auth_token");
  localStorage.removeItem("gym_subscription");
});
await goto();
check(await shown("gym-login"), "после выхода снова экран входа");
await page.getByTestId("gym-login").click();
const back = await page.getByTestId("install-steps").waitFor({ timeout: 30000 })
  .then(() => true).catch(() => false);
check(back, "вошли существующим ключом, БЕЗ имени и логина (discoverable)");
const sameUser = await page.evaluate(() => localStorage.getItem("gym_user_id"));
check(sameUser === userId, "и это тот же самый аккаунт, а не новый");

// ── 4. Второй ключ на это же устройство ──
console.log("\n4. «Добавить ключ на это устройство» — /add-device/*");
await page.evaluate(() => localStorage.setItem("gym_pwa_dismissed", "1"));
await goto();
const ready = await page.getByTestId("tab-settings").waitFor({ timeout: 30000 })
  .then(() => true).catch(() => false);
check(ready, "дошли до приложения");
if (ready) {
  await page.getByTestId("tab-settings").click();
  await page.getByTestId("set-btn-add-key").click();
  const ok = await page.getByTestId("set-key-ok").waitFor({ timeout: 30000 })
    .then(() => true).catch(() => false);
  check(ok, "второй ключ добавлен к тому же аккаунту");

  // Ключей в аутентификаторе по-прежнему ОДИН, и это не ошибка. Резидентный
  // ключ адресуется парой (rpId, user handle), и второй с той же парой не
  // добавляется рядом, а ЗАМЕЩАЕТ прежний — так велит спецификация и так ведут
  // себя настоящие платформенные аутентификаторы. «Добавить ключ на это
  // устройство» и придумано для ДРУГОГО устройства (у поставленного на iOS
  // приложения своё хранилище); на том же самом оно перевыпускает ключ.
  //
  // Поэтому проверяется не счёт, а что ключ действительно НОВЫЙ и что им
  // по-прежнему пускает: ручка, которая ответила 200 и записала мусор, прошла
  // бы любую проверку счётом.
  const after = await cdp.send("WebAuthn.getCredentials", { authenticatorId });
  check(after.credentials.length === 1,
    `в аутентификаторе один резидентный ключ на аккаунт (${after.credentials.length})`);
  check(after.credentials[0]?.credentialId !== firstKey, "и это НОВЫЙ ключ, а не прежний");

  await page.evaluate(() => {
    localStorage.removeItem("gym_user_id");
    localStorage.removeItem("gym_auth_token");
    localStorage.removeItem("gym_subscription");
  });
  await goto();
  await page.getByTestId("gym-login").click();
  const again = await page.getByTestId("tab-settings").waitFor({ timeout: 30000 })
    .then(() => true).catch(() => false);
  const sameAgain = await page.evaluate(() => localStorage.getItem("gym_user_id"));
  check(again && sameAgain === userId, "новым ключом входит, и в тот же аккаунт");
}

check(errors.length === 0, `без ошибок исполнения${errors.length ? `: ${errors[0]}` : ""}`);

await browser.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё в порядке");
process.exit(failed ? 1 : 0);
