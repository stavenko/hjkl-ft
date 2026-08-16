// Замер ДВУХ мясных признаков через ЖИВОЙ путь: красное мясо и мясо глубокой
// переработки. Оба спрашиваются на одном наборе — так видно и то, как они
// расходятся, и то, как они пересекаются.
//
// Набор собран вокруг границ, на которых признаки и должны ломаться:
//   * ОРГАНЫ против МЯСА — печень и сердце в гем входят, а в красное мясо нет;
//   * ПТИЦА против МЛЕКОПИТАЮЩИХ, при этом страус — красное мясо, а курица нет;
//   * ПРИГОТОВЛЕНО против КОНСЕРВИРОВАНО — котлета и буженина не переработаны,
//     сосиска и бастурма переработаны, хотя всё это «магазинное мясо»;
//   * колбаса обязана быть ОБОИМИ сразу: она тратит недельные граммы и портит день.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

// [название, красное мясо, переработанное, почему]
const CASES = [
  // Мясо млекопитающих — красное, но не переработанное.
  ["Говядина", true, false, "мышцы млекопитающего"],
  ["Свинина", true, false, "мышцы млекопитающего"],
  ["Баранина", true, false, "мышцы млекопитающего"],
  ["Фарш говяжий", true, false, "фарш — то же мясо"],
  ["Котлеты домашние", true, false, "готовили, но не консервировали"],
  ["Буженина запечённая", true, false, "запекли, а не законсервировали"],
  ["Крольчатина", true, false, "мясо млекопитающего"],
  ["Оленина", true, false, "дичь"],
  ["Страусятина", true, false, "страус — по правилу красное мясо"],
  // Переработанное — и то и другое сразу.
  ["Сосиски молочные", true, true, "нитритный посол"],
  ["Колбаса докторская", true, true, "нитритный посол"],
  ["Салями", true, true, "сыровяленая"],
  ["Ветчина", true, true, "посол"],
  ["Бекон", true, true, "посол и копчение"],
  ["Бастурма", true, true, "вяление"],
  ["Хамон", true, true, "вяление"],
  // Печень — орган, поэтому по определению красного мяса (мышцы) ливерная колбаса
  // в него НЕ входит. В недельные граммы она всё равно попадает: их считает
  // `red_meat::counts`, который берёт и переработанное.
  ["Ливерная колбаса", false, true, "сделана из печени — органа, а не мышц"],
  // Органы — в красное мясо НЕ входят, хотя в гем входят.
  ["Печень говяжья", false, false, "орган, а не мышцы"],
  ["Сердечки куриные", false, false, "орган птицы"],
  ["Язык говяжий", false, false, "орган"],
  // Птица, рыба, растения — мимо обоих.
  ["Куриная грудка", false, false, "птица"],
  ["Индейка филе", false, false, "птица"],
  ["Кижуч без головы", false, false, "рыба"],
  // КОНСЕРВИРОВАННАЯ РЫБА — мимо обоих, и это боевой случай: «скумбрия холодного
  // копчения» оказалась «86 г колбасы» в дневнике. Копчение и посол здесь те же,
  // что у бастурмы, но признак говорит о МЯСЕ, а рыба к нему не относится: и
  // недельная планка 700 г, и дневной индикатор колбас — про мясо.
  ["Скумбрия холодного копчения", false, false, "рыба, хоть и копчёная"],
  ["Сельдь солёная", false, false, "рыба, хоть и солёная"],
  ["Лосось слабосолёный", false, false, "рыба, хоть и солёная"],
  ["Икра трески копчёная", false, false, "рыбий продукт, не мясо"],
  // Сыровяленая колбаса под нерусским именем — обязана быть обоими сразу.
  ["Fuet truffle", true, true, "испанская сыровяленая колбаса"],
  ["Креветки", false, false, "морепродукт"],
  ["Яйцо куриное", false, false, "не мясо"],
  ["Чечевица варёная", false, false, "растение"],
  ["Творог 5%", false, false, "молочное"],
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `meat-${Date.now()}`;
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

// Всё, кроме двух мясных признаков, уже «выяснено» — лишних запросов не надо.
await page.evaluate(async ({ u, names }) => {
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
    names.forEach((name, i) => tx.objectStore("foods").put({
      id: `m${i}`, name, kcal: 150, protein: 20, fat: 8, carbs: 0,
      nutrients: { "Кальций": 0, "Клетчатка": 0 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: false, is_heme: false, is_milk_globule: false,
      is_red_meat: null, is_processed_meat: null,
      iron_mg: 2.0, iron_absorption: 0.15,
      fat_profile: { sfa_pct: 30, mufa_pct: 40, pufa_pct: 20, epa_dha_pct: 0 },
      balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, names: CASES.map(([n]) => n) });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 20 * 60 * 1000;
let got = {};
while (Date.now() < DEADLINE) {
  await page.waitForTimeout(15000);
  got = await page.evaluate(async (u) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["foods"]).objectStore("foods").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    return Object.fromEntries(all.map((f) => [f.name, [f.is_red_meat, f.is_processed_meat]]));
  }, uid);
  const done = CASES.filter(([n]) => {
    const v = got[n];
    return v && v[0] !== null && v[0] !== undefined && v[1] !== null && v[1] !== undefined;
  }).length;
  console.log(`отвечено ${done}/${CASES.length}`);
  if (done === CASES.length) break;
}

let badRed = 0;
let badProc = 0;
const s = (x) => (x === true ? "да " : x === false ? "нет" : " — ");
console.log("\n                                 красное    переработанное");
console.log("продукт                          наш ждём   наш ждём   почему");
for (const [name, wantRed, wantProc, why] of CASES) {
  const [red, proc] = got[name] ?? [undefined, undefined];
  const okRed = red === wantRed;
  const okProc = proc === wantProc;
  if (!okRed) badRed++;
  if (!okProc) badProc++;
  const mark = okRed && okProc ? "OK  " : "MISS";
  console.log(`${mark} ${name.padEnd(30)} ${s(red)} ${s(wantRed)}   ${s(proc)} ${s(wantProc)}   ${why}`);
}
console.log(`\nкрасное мясо:        ${CASES.length - badRed}/${CASES.length}`);
console.log(`переработанное мясо: ${CASES.length - badProc}/${CASES.length}`);

await ctx.close();
await b.close();
process.exit(badRed + badProc ? 1 : 0);
