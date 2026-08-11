// Форма правки БЛЮДА показывает не флаги, а количества на 100 г из состава.
//
// У блюда «да/нет» бессмысленно: рагу из говядины с капустой — и мясо, и овощи
// разом. Сеем ровно пример пользователя — 400 г говядины + 400 г капусты,
// конечный вес 700 г — и проверяем четыре выведенные строки. Числа посчитаны
// вручную и вписаны сюда, иначе проверка мерила бы саму себя:
//   овощи        400/700·100            = 57 г
//   гем          26·4 = 104 г белка → на 100 г блюда 14.86 → /25 = 0.59 порции
//   баланс       (40+30)/20 = 3.5 > 2.0 → «улучшает»
//   омега-3      20 г жира × 10 %       = 2.00 г
// Заодно у СЫРОГО продукта проверяется строка омеги-3 — раньше профиль жира
// собирался, но человеку нигде не показывался.
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
const uid = `rflags-${Date.now()}`;
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
        res(["foods", "diary", "recipes", "recipe_ingredients"].every((s) => n.includes(s))); };
      q.onerror = () => res(false);
    });
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}

await page.evaluate(async (u) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${u}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const nowIso = new Date().toISOString();
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const food = (o) => ({
    nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
    is_restaurant: false, is_snack: null, is_liquid_cal: null, is_veg_fruit: null,
    is_egg: null, is_red_meat: null, is_heme: null, iron_mg: null, iron_absorption: null,
    fat_profile: null, created_at: nowIso, updated_at: nowIso, ...o,
  });
  const rows = {
    app_flags: [{ key: "db_schema_version", value: "999" },
      { key: "push_onboarding_dismissed", value: "true" }, { key: "welcome_shown", value: "true" }],
    foods: [
      // Профили ингредиентов обязаны быть: блюдо пересчитывается из состава на
      // запуске, и профиль, проставленный блюду в обход состава, будет затёрт.
      food({ id: "beef", name: "Говядина", kcal: 250, protein: 26, fat: 10, carbs: 0,
        is_veg_fruit: false, is_heme: true,
        fat_profile: { sfa_pct: 40, mufa_pct: 45, pufa_pct: 5, epa_dha_pct: 0 } }),
      food({ id: "cabbage", name: "Капуста", kcal: 28, protein: 1.8, fat: 0.1, carbs: 5,
        is_veg_fruit: true, is_heme: false,
        fat_profile: { sfa_pct: 20, mufa_pct: 10, pufa_pct: 60, epa_dha_pct: 0 } }),
      food({ id: "stew", name: "Рагу из говядины", kcal: 160, protein: 16, fat: 20, carbs: 3,
        is_recipe: true, recipe_id: "r1" }),
      // Сырой продукт со своим профилем — для строки омеги-3.
      food({ id: "coho", name: "Кижуч", kcal: 180, protein: 21, fat: 8, carbs: 0,
        is_veg_fruit: false, is_heme: false,
        fat_profile: { sfa_pct: 22, mufa_pct: 35, pufa_pct: 25, epa_dha_pct: 15 } }),
    ],
    // Поля ровно те, что у api_types::Recipe: без notes/finalized/ingredients
    // строка не разберётся, и блюдо окажется «без состава».
    recipes: [{ id: "r1", name: "Рагу из говядины", notes: null, total_grams: 700,
      finalized: true, food_id: "stew", superseded_by: null, ingredients: [],
      created_at: nowIso, updated_at: nowIso }],
    recipe_ingredients: [
      { id: "ri1", recipe_id: "r1", food_id: "beef", grams: 400, created_at: nowIso, updated_at: nowIso },
      { id: "ri2", recipe_id: "r1", food_id: "cabbage", grams: 400, created_at: nowIso, updated_at: nowIso },
    ],
    diary: [
      { id: "d1", food_id: "stew", date: today, time: null, grams: 300, waste_grams: 0,
        meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso },
      { id: "d2", food_id: "coho", date: today, time: null, grams: 150, waste_grams: 0,
        meal_label: "dinner", deleted: false, created_at: nowIso, updated_at: nowIso },
    ],
  };
  for (const [store, list] of Object.entries(rows)) {
    if (!Array.from(db.objectStoreNames).includes(store)) continue;
    await new Promise((res, rej) => {
      const tx = db.transaction([store], "readwrite");
      for (const r of list) tx.objectStore(store).put(r);
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
  }
  db.close();
}, uid);

// Правка открывается из меню строки дневника: кнопка «⋮» → «Изменить». Строка
// выбирается по названию, меню — по опознавателю, чтобы проверка не цеплялась за
// вёрстку.
const openEditor = async (name, index) => {
  await page.goto(`${BASE}/diary`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});
  await page.waitForTimeout(3000);
  await page.getByText(name).first().waitFor({ timeout: 15000 });
  await page.locator('[data-testid="diary-row-menu"]').nth(index).click();
  await page.locator('[data-testid="diary-menu-edit"]').first().click();
  await page.waitForSelector('[data-testid="ai-found-block"]', { timeout: 15000 });
  await page.waitForTimeout(2500);
  return (await page.locator('[data-testid="ai-found-block"]').innerText()).replace(/\s+/g, " ");
};

// ── блюдо ───────────────────────────────────────────────────────────────────
const dish = await openEditor("Рагу из говядины", 0);
console.log("блюдо:", dish);
check("у блюда нет флагов «да/нет»", !/Красное мясо|Яйца|Низкокалорийный/.test(dish), dish.slice(0, 80));
const derived = async (label) =>
  (await page.locator(`[data-derived="${label}"]`).innerText()).trim();
check("овощи и фрукты — 57 г на 100 г", await derived("Овощи и фрукты") === "57");
check("порции гема — 0.59 на 100 г", await derived("Порции гемового железа") === "0.59");
// Жир блюда: говядина 400 г × 10 % = 40 г, капуста 0.4 г. Кислоты — 16.08 НЖК,
// 18.04 МНЖК, 2.24 ПНЖК, ЭПК+ДГК ноль. Отношение (18.04+2.24)/16.08 = 1.26 — ниже
// нормы 2.0, значит рагу тянет недельный баланс вниз.
check("баланс жиров — ухудшает", await derived("Баланс жирных кислот") === "ухудшает");
check("омега-3 блюда — 0.00 г", await derived("Омега-3 (ЭПК+ДГК)") === "0.00");
await page.screenshot({ path: "/tmp/recipe-flags.png", fullPage: true });

// ── сырой продукт ───────────────────────────────────────────────────────────
await page.keyboard.press("Escape").catch(() => {});
const raw = await openEditor("Кижуч", 1);
console.log("продукт:", raw);
check("у продукта флаги на месте", /Источник гемового железа/.test(raw));
// 8 г жира × 15 % = 1.20 г.
check("омега-3 продукта — 1.20 г", await derived("Омега-3 (ЭПК+ДГК)") === "1.20");
await page.screenshot({ path: "/tmp/food-omega3.png", fullPage: true });

console.log("снято: /tmp/recipe-flags.png, /tmp/food-omega3.png");
await ctx.close();
await b.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
