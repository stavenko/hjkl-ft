// Приложение выглядит ОДИНАКОВО на любой ширине — отдельного десктопного вида нет.
//
// Растянутое на монитор, оно разъезжалось: сторона квадратной ячейки считалась от
// ширины ОКНА и на 1440 раздувалась вчетверо (плитка 46 → 172 px), пока шрифты,
// значки и нижнее меню, заданные в пикселях, оставались прежними. Отсюда пустые
// карточки с крошечными подписями и непопадание пальцем.
//
// Проверяется числами, а не на глаз: элементы на планшете и на десктопе обязаны
// совпасть с точностью до пикселя, а на телефоне — остаться прежними.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `desk-${Date.now()}`;
const now = Math.floor(Date.now() / 1000);
const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

// Колонка приложения. Держать в одном месте: то же число стоит в app.rs, в стороне
// ячейки дашборда и в ширине нижнего меню.
const COLUMN = 480;
const SIZES = [
  { name: "phone", width: 430, height: 932 },
  { name: "tablet", width: 834, height: 1112 },
  { name: "desktop", width: 1440, height: 900 },
];
const PAGES = ["/", "/diary", "/settings", "/recipes"];

const b = await chromium.launch({ headless: true });
const seen = {};
for (const s of SIZES) {
  const ctx = await b.newContext({ viewport: { width: s.width, height: s.height },
    serviceWorkers: "block" });
  const page = await ctx.newPage();
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem("user_id", uid);
    localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "t");
    localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
  }, { uid, token });
  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(6000);
  await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    await new Promise((res, rej) => {
      const tx = db.transaction(["app_flags"], "readwrite");
      [{ key: "db_schema_version", value: "999" }, { key: "push_onboarding_dismissed", value: "true" },
       { key: "welcome_shown", value: "true" }].forEach((f) => tx.objectStore("app_flags").put(f));
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
    await new Promise((res, rej) => {
      const tx = db.transaction(["profile"], "readwrite");
      tx.objectStore("profile").put({ key: "profile", sex: "male", height_cm: 180,
        birth_year: 1985, goal: "lose", steps_planka: 9000,
        created_at: nowIso, updated_at: nowIso });
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
    db.close();
  }, uid);

  seen[s.name] = {};
  for (const path of PAGES) {
    await page.goto(BASE + path, { waitUntil: "domcontentloaded" });
    await page.waitForTimeout(path === "/" ? 9000 : 5000);
    seen[s.name][path] = await page.evaluate(() => {
      const box = (sel) => {
        const el = document.querySelector(sel);
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return [Math.round(r.width), Math.round(r.height)];
      };
      // Ширина СОДЕРЖИМОГО: колонка, в которую вписаны все страницы.
      const shell = document.querySelector('[data-ios-scroll="1"] > div');
      return {
        shell: shell ? Math.round(shell.getBoundingClientRect().width) : null,
        overflow: document.documentElement.scrollWidth > window.innerWidth + 1,
        mail: box('[data-testid="dash-mail-widget"]'),
        weight: box('[data-testid="dash-weight-widget"]'),
        progress: box('[data-testid="progress-widget"]'),
        nav: box('[data-testid="nav-dashboard"]'),
      };
    });
    if (path === "/") {
      await page.screenshot({ path: `/tmp/col-${s.name}.png` });
    }
  }
  await ctx.close();
}

for (const path of PAGES) {
  const t = seen.tablet[path];
  const d = seen.desktop[path];
  const p = seen.phone[path];
  check(`${path}: планшет и десктоп выглядят одинаково`,
    JSON.stringify(t) === JSON.stringify(d), `${JSON.stringify(t)} vs ${JSON.stringify(d)}`);
  check(`${path}: колонка не шире ${COLUMN} px`,
    d.shell !== null && d.shell <= COLUMN, `${d.shell}`);
  check(`${path}: страница не уезжает вбок`, !d.overflow);
  // Телефон уже колонки — на нём ширина остаётся его собственной.
  check(`${path}: на телефоне ширина прежняя`, p.shell !== null && p.shell <= 430, `${p.shell}`);
}
// Дашборд — единственное место с квадратной сеткой, за ней и следим отдельно.
check("плитка на десктопе размером с телефонную, а не вчетверо больше",
  seen.desktop["/"].mail !== null && seen.desktop["/"].mail[0] <= 60,
  `${JSON.stringify(seen.desktop["/"].mail)} против ${JSON.stringify(seen.phone["/"].mail)}`);

console.log("\nснимки: /tmp/col-phone.png, /tmp/col-tablet.png, /tmp/col-desktop.png");
await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
