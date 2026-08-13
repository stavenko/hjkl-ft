// Определения доезжают до аналитики: не только сбои, но и обычные вердикты.
//
// Проверяется ЖИВОЙ ПУТЬ: заводим продукты без признаков и профиля жира, ждём
// фоновый проход, а потом спрашиваем НЕ приложение, а сам Analytics Engine — то
// есть ровно то место, откуда потом будем брать материал для замеров.
//
// Аналитика записывает с задержкой (точки видны не мгновенно), поэтому чтение
// повторяется до дедлайна, а не однократно.
import { chromium } from "playwright";
import { execFileSync } from "node:child_process";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const ACC = process.env.CF_ACCOUNT || "a2809d140765b9e7915c5644c249863b";
const DATASET = process.env.DATASET || "renorma_client_detections_dev";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

// Продукты нарочно РАЗНЫЕ по ветке: базовый идёт в таблицу строк, блюдо — мимо неё.
const CASES = ["Скумбрия", "Голубцы"];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `detect-${Date.now()}`;
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

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();
let sent = 0;
page.on("request", (r) => { if (r.url().endsWith("/detection")) sent += 1; });

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
for (let i = 0; i < 60; i++) {
  const ok = await page.evaluate(async (u) => {
    const dbs = await indexedDB.databases();
    if (!dbs.some((d) => d.name === `hjkl-ft-${u}`)) return false;
    return await new Promise((res) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => { const n = Array.from(q.result.objectStoreNames); q.result.close();
        res(["foods", "app_flags"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

// Признаки и профиль НЕ заполнены — их и должен выяснить фоновый проход.
await page.evaluate(async ({ u, items }) => {
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
    const tx = db.transaction(["foods"], "readwrite");
    items.forEach((name, i) => tx.objectStore("foods").put({
      id: `t${i}`, name, kcal: 200, protein: 15, fat: 12, carbs: 5,
      nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: null, is_heme: null, is_milk_globule: null,
      iron_mg: 1.0, iron_absorption: 0.15,
      fat_profile: null, balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, items: CASES });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 12 * 60 * 1000;
while (Date.now() < DEADLINE) {
  await page.waitForTimeout(15000);
  const done = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["foods"]).objectStore("foods").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return all.filter((f) => f.fat_profile).length;
  }, uid);
  console.log(`разобрано ${done}/${CASES.length}, отправлено определений ${sent}`);
  if (done === CASES.length) break;
}
check("клиент отправлял определения", sent > 0, `${sent} запросов`);
await ctx.close();
await b.close();

// ── Спрашиваем сам Analytics Engine ─────────────────────────────────────────
const sql = (q) => {
  const out = execFileSync("curl", ["-s", "-X", "POST",
    `https://api.cloudflare.com/client/v4/accounts/${ACC}/analytics_engine/sql`,
    "-H", `Authorization: Bearer ${process.env.CLOUDFLARE_API_TOKEN}`,
    "-d", q], { encoding: "utf8" });
  try { return JSON.parse(out).data || []; } catch { console.log(out.slice(0, 200)); return []; }
};

// Датасет Analytics Engine создаётся ПЕРВОЙ ЗАПИСЬЮ, и пока он не появился, SQL
// отвечает «unable to find type of column: timestamp» — то есть пустой ответ тут
// значит и «данных ещё нет», и «таблицы ещё нет». Ждём дольше, чем кажется
// нужным: на самом первом прогоне ждать приходится создания датасета.
// Ждать надо не «появились строки», а появления ВСЕХ видов: точки доезжают до
// аналитики вразнобой и с разной задержкой, поэтому первые записи видны, когда
// последние ещё в пути. На этом проверка уже один раз ошибочно провалилась.
const EXPECTED = ["dish.composite", "fat.row", "fat.dish",
  "flag.heme", "flag.veg_fruit", "flag.milk_globule"];
let rows = [];
const READ_DEADLINE = Date.now() + 10 * 60 * 1000;
while (Date.now() < READ_DEADLINE) {
  // `timestamp` обязан быть В СПИСКЕ SELECT: сортировать по столбцу, которого нет
  // в выборке, Analytics Engine не умеет — и говорит об этом ровно так же, как об
  // отсутствующем датасете («unable to find type of column»).
  rows = sql(`SELECT timestamp, index1 AS kind, blob1 AS subject, blob2 AS verdict, blob3 AS reason,
      double1 AS n1, double2 AS n2, double3 AS n3, double4 AS n4
    FROM ${DATASET} WHERE timestamp > NOW() - INTERVAL '1' HOUR AND blob6 = '${uid}'
    ORDER BY timestamp FORMAT JSON`);
  const seen = new Set(rows.map((r) => r.kind));
  if (EXPECTED.every((k) => seen.has(k))) break;
  console.log(`в аналитике ${rows.length} записей, видов ${seen.size}/${EXPECTED.length}, ждём…`);
  await new Promise((r) => setTimeout(r, 20000));
}

console.log("");
for (const r of rows) {
  console.log(`${String(r.kind).padEnd(18)} ${String(r.subject).padEnd(12)} ${String(r.verdict).padEnd(18)} ` +
    `${[r.n1, r.n2, r.n3, r.n4].map((x) => Math.round(x)).join("/").padEnd(14)} ${String(r.reason).slice(0, 70)}`);
}
console.log("");

const kinds = new Set(rows.map((r) => r.kind));
check("определения доехали до аналитики", rows.length > 0, `${rows.length} записей`);
check("видно, базовый продукт или блюдо", kinds.has("dish.composite"));
check("видно строку жира базового продукта", kinds.has("fat.row"));
check("видно профиль блюда", kinds.has("fat.dish"));
check("видно признаки", kinds.has("flag.heme") && kinds.has("flag.veg_fruit"));
check("у гема записаны голоса",
  rows.some((r) => r.kind === "flag.heme" && Number(r.n1) + Number(r.n2) === 3));
check("у строки жира записаны доли",
  rows.some((r) => r.kind === "fat.row" && Number(r.n1) > 0));
check("названия продуктов на месте",
  CASES.every((n) => rows.some((r) => r.subject === n)));
check("обоснование модели сохранено",
  rows.every((r) => String(r.reason).length > 0));

console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
