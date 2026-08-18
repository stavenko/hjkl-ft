// ПЕРЕКРЁСТНЫЙ замер: один набор, ВСЕ пять признаков разом.
//
// Отдельные замеры проверяют признак на «своей» еде: гем на печени, мясо на
// мясе. Так не видно обратной ошибки — когда чужая еда попадает в чужой признак.
// А именно она и была боевой: «Голец» ушёл в красное мясо и съел недельную планку
// в 700 г, а в вопросе про гем тот же голец объявлялся печенью рыбы.
//
// Поэтому набор собран из РЕДКОЙ еды, которая не должна попасть НИКУДА, кроме
// своего места: незнакомые рыбы, забытые овощи, экзотические фрукты, редкие
// крупы. Всё, что модель не узнаёт, она достраивает знакомым — и достраивает в
// тон вопросу, поэтому спрашивать надо всеми вопросами сразу.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

// [название, овощ/фрукт, гем, красное мясо, переработанное, молочная глобула]
const CASES = [
  // Редкие и забытые ОВОЩИ — только овощ/фрукт, больше нигде.
  ["Топинамбур", true, false, false, false, false],
  ["Пастернак корень", true, false, false, false, false],
  ["Кольраби", true, false, false, false, false],
  ["Брюква", true, false, false, false, false],
  ["Батат печёный", true, false, false, false, false],
  ["Мангольд", true, false, false, false, false],
  ["Черемша", true, false, false, false, false],
  ["Дайкон", true, false, false, false, false],
  // Экзотические ФРУКТЫ и ягоды.
  ["Фейхоа", true, false, false, false, false],
  ["Айва", true, false, false, false, false],
  ["Кумкват", true, false, false, false, false],
  ["Личи", true, false, false, false, false],
  ["Маракуйя", true, false, false, false, false],
  ["Помело", true, false, false, false, false],
  ["Ирга", true, false, false, false, false],
  ["Жимолость", true, false, false, false, false],
  // Редкие РЫБЫ — мимо всего. Боевой случай: голец в красном мясе.
  ["Голец", false, false, false, false, false],
  ["Пикша", false, false, false, false, false],
  ["Сайда", false, false, false, false, false],
  ["Муксун", false, false, false, false, false],
  ["Зубатка", false, false, false, false, false],
  ["Нельма", false, false, false, false, false],
  // Редкие КРУПЫ и бобовые — мимо всего, овощами тоже не считаются.
  ["Булгур", false, false, false, false, false],
  ["Киноа варёная", false, false, false, false, false],
  ["Полба", false, false, false, false, false],
  ["Амарант", false, false, false, false, false],
  ["Маш", false, false, false, false, false],
  // Контроль: то, что ОБЯЗАНО попасть в свои признаки. Без него замер похвалил бы
  // модель, которая на всё отвечает «нет».
  ["Печень говяжья", false, true, false, false, false],
  ["Говядина", false, true, true, false, false],
  ["Салями", false, true, true, true, false],
  ["Яблоко", true, false, false, false, false],
  // Глобулы у масла НЕТ: мембрана рвётся сбиванием (см. `Food::is_milk_globule`).
  // Сначала здесь стояло «да» — это была ошибка эталона, модель отвечала верно.
  ["Сливочное масло", false, false, false, false, false],
  ["Творог 5 %", false, false, false, false, true],
];

// ONLY=подстрока[,подстрока] — прогнать прицельно по больным позициям. Соседи по
// набору на ответ не влияют (каждый продукт идёт своим конвейером), поэтому такой
// прогон сравним с полным.
if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((s) => s.trim().toLowerCase());
  const kept = CASES.filter(([n]) => want.some((w) => n.toLowerCase().includes(w)));
  if (!kept.length) throw new Error(`ONLY=${process.env.ONLY} не совпал ни с одним продуктом`);
  CASES.length = 0;
  CASES.push(...kept);
}

const FLAGS = ["овощ/фрукт", "гем", "красное мясо", "переработанное", "глобула"];

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const uid = `cross-${Date.now()}`;
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
// Обоснования по каждому признаку: без них видно «мимо», но не ЧЕМ модель это
// объяснила, а объяснение здесь и есть содержание замера.
const said = new Map();
page.on("console", (m) => {
  const g = m.text().match(/^(фрукты\/овощи|гем|красное мясо|переработанное мясо|молочная глобула) «(.+?)»: (.+)$/);
  if (g) said.set(`${g[1]}|${g[2]}`, g[3]);
});
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

// Нутриенты, железо и жир уже «выяснены» — спрашиваются только пять признаков.
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
      id: `c${i}`, name, kcal: 120, protein: 8, fat: 5, carbs: 12,
      nutrients: { "Кальций": 0, "Клетчатка": 1 }, package_weight: null, is_recipe: false,
      recipe_id: null, archived: false, is_restaurant: false,
      is_veg_fruit: null, is_heme: null, is_milk_globule: null,
      is_red_meat: null, is_processed_meat: null,
      iron_mg: 1.0, iron_absorption: 0.1,
      fat_profile: { sfa_pct: 25, mufa_pct: 35, pufa_pct: 40, epa_dha_pct: 0 },
      balance_fat_profile: null,
      created_at: nowIso, updated_at: nowIso,
    }));
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { u: uid, names: CASES.map(([n]) => n) });

await page.goto(BASE, { waitUntil: "domcontentloaded" });
const DEADLINE = Date.now() + 35 * 60 * 1000;
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
    return Object.fromEntries(all.map((f) => [f.name,
      [f.is_veg_fruit, f.is_heme, f.is_red_meat, f.is_processed_meat, f.is_milk_globule]]));
  }, uid);
  const done = CASES.filter(([n]) => (got[n] ?? []).every((v) => v === true || v === false)).length;
  console.log(`отвечено ${done}/${CASES.length}`);
  if (done === CASES.length) break;
}

// Ошибки считаются по ДВУМ видам отдельно: ложное «да» ставит еду в чужой счёт
// (гем набирается овощами, недельные граммы мяса — рыбой), ложное «нет» просто
// теряет её. Первое опаснее, и мешать их в одно число нельзя.
const LOG_KEY = ["фрукты/овощи", "гем", "красное мясо", "переработанное мясо", "молочная глобула"];
let falsePos = 0;
let falseNeg = 0;
let unanswered = 0;
const s = (x) => (x === true ? "да " : x === false ? "нет" : " — ");
console.log("\nпродукт                     " + FLAGS.map((f) => f.padEnd(16)).join(""));
for (const [name, ...want] of CASES) {
  const have = got[name] ?? [];
  const bad = want.map((w, i) => have[i] !== w);
  const cells = want.map((w, i) => `${s(have[i])}${bad[i] ? `≠${s(w)}` : "    "}`.padEnd(16)).join("");
  console.log(`${bad.some(Boolean) ? "MISS" : "OK  "} ${name.padEnd(24)}${cells}`);
  bad.forEach((isBad, i) => {
    if (!isBad) return;
    if (have[i] === true) falsePos++;
    else if (have[i] === false) falseNeg++;
    else unanswered++;   // прочерк: признак так и не проставился
    console.log(`     ${FLAGS[i]}: ${said.get(`${LOG_KEY[i]}|${name}`) ?? "(в консоль ничего не пришло)"}`);
  });
}
const total = CASES.length * FLAGS.length;
console.log(`\nвсего проверок: ${total}`);
console.log(`ложных «да»:    ${falsePos}   (еда попала в ЧУЖОЙ счёт)`);
console.log(`ложных «нет»:   ${falseNeg}   (еда потеряна для своего счёта)`);
console.log(`без ответа:     ${unanswered}   (признак не проставился вовсе)`);

await ctx.close();
await b.close();
process.exit(falsePos + falseNeg + unanswered ? 1 : 0);
