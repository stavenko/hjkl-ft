// ЗАПАСНАЯ МОДЕЛЬ ОПОЗНАНИЯ — проверка на живом приложении.
//
// Основная модель опознания живёт у стороннего провайдера, и отказать она может не
// из-за продукта: кончилась квота, не прошёл платёж, лежит сам провайдер. В этом
// случае следующая попытка обязана уйти к запасной (та же 27b, но у Cloudflare), а
// продукт — всё равно разобраться.
//
// Отказ подделывается честно: конфиг приложения подменяется на лету, и в
// `identity_model` встаёт имя, которого у провайдера нет. Запрос уходит настоящий и
// настоящим же образом падает.
//
// Прогонов два: со сломанной основной (должна вступить запасная) и с исправной
// (запасную не должны трогать вовсе — иначе платили бы дважды за один продукт).
//
//   node scripts/check-identity-fallback.mjs
//   FOOD="Скумбрия х/к" node scripts/check-identity-fallback.mjs
import { chromium } from "playwright";

const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const FOOD = process.env.FOOD || "Творог 5%";
/// Имени нет ни у провайдера, ни в Workers AI — провайдер ответит ошибкой.
const BROKEN = process.env.BROKEN_MODEL || "qwen3.8-27b-such-model-does-not-exist";
const WAIT_MS = Number(process.env.WAIT_MS || 180000);

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const jwt = async (sub) => {
  const enc = new TextEncoder();
  const now = Math.floor(Date.now() / 1000);
  const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, enc.encode(data))))}`;
};

let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

const MAIN = process.env.MAIN_MODEL || "qwen3.8-27b";
const SPARE = process.env.SPARE_MODEL || "@cf/qwen/qwen3.8-27b";

const b = await chromium.launch({ headless: true });

/// Один прогон: `broken` — подменять ли основную модель на несуществующую.
async function run(broken) {
  console.log(`\n— ${broken ? "основная сломана" : "основная исправна"} —`);
  const uid = `fallback-${broken ? "broken" : "ok"}-${Date.now()}`;
  const token = await jwt(uid);
  const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
    headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
  await fetch(`${PAY}/claim`, { method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const page = await ctx.newPage();

  // ПОДМЕНА КОНФИГА: он читается приложением на старте, поэтому «сломать» основную
  // модель можно, не выкатывая испорченную сборку.
  if (broken) {
    await page.route("**/config/frontend.toml", async (route) => {
      const res = await route.fetch();
      const text = (await res.text()).replace(/^identity_model\s*=.*$/m, `identity_model = "${BROKEN}"`);
      await route.fulfill({ response: res, body: text });
    });
  }

  const logs = [];
  page.on("console", (m) => logs.push(m.text()));
  // Какие модели реально спрашивали: тело запроса к ai-worker несёт имя модели.
  const models = [];
  page.on("request", (req) => {
    if (!/ai-worker|ai\.renorma/.test(req.url())) return;
    try {
      const m = JSON.parse(req.postData() || "{}").model;
      if (m) models.push(m);
    } catch {}
  });

  await page.goto(FE, { waitUntil: "domcontentloaded" });
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem("user_id", uid);
    localStorage.setItem("auth_token", token);
    localStorage.setItem("token_id", "t");
    localStorage.setItem("auth_ctx", "browser");
    localStorage.setItem("pwa_dismissed", "true");
  }, { uid, token });
  await page.goto(FE, { waitUntil: "domcontentloaded" });

  const NEED = ["app_flags", "profile", "planka_history", "foods", "diary"];
  let ready = false;
  for (let i = 0; i < 60 && !ready; i++) {
    ready = await page.evaluate(async ({ uid, NEED }) => {
      const dbs = await indexedDB.databases();
      if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
      return await new Promise((res) => {
        const q = indexedDB.open(`hjkl-ft-${uid}`);
        q.onsuccess = () => {
          const names = Array.from(q.result.objectStoreNames);
          q.result.close();
          res(NEED.every((n) => names.includes(n)));
        };
        q.onerror = () => res(false);
      });
    }, { uid, NEED }).catch(() => false);
    if (!ready) await page.waitForTimeout(500);
  }
  if (!ready) throw new Error("per-user база так и не появилась со всеми сторами");

  await page.evaluate(async ({ uid, FOOD }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const d = new Date();
    const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    const rows = {
      app_flags: [
        { key: "push_onboarding_dismissed", value: "true" },
        { key: "welcome_shown", value: "true" },
        { key: "activity_week_unlocked", value: "true" },
        { key: "calcium_week_unlocked", value: "true" },
      ],
      profile: [{ key: "profile", sex: "male", height_cm: 180,
        birth_year: new Date().getFullYear() - 40, goal: "lose", steps_planka: 9000,
        created_at: nowIso, updated_at: nowIso }],
      goals: [{ id: "g-cal", nutrient: "Calories", key: "calories", direction: "AtMost",
        amount: 2500, unit: "Kcal", period: "Day", created_at: nowIso, updated_at: nowIso }],
      // Продукт пустой: ни одного признака — за ними и пойдёт опознание.
      foods: [{ id: "f0", name: FOOD, kcal: 150, protein: 12, fat: 8, carbs: 5, nutrients: {},
        package_weight: null, is_recipe: false, recipe_id: null, archived: false,
        is_restaurant: false, is_snack: null, is_liquid_cal: null, is_veg_fruit: null,
        is_heme: null, is_milk_globule: null, is_red_meat: null, is_processed_meat: null,
        is_egg: null, iron_mg: null, iron_absorption: null, fat_profile: null,
        created_at: nowIso, updated_at: nowIso }],
      diary: [{ id: "d0", food_id: "f0", date: today, time: null, grams: 100, waste_grams: 0,
        meal_label: "lunch", deleted: false, created_at: nowIso, updated_at: nowIso }],
    };
    for (const [store, list] of Object.entries(rows)) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], "readwrite");
        for (const row of list) tx.objectStore(store).put(row);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  }, { uid, FOOD });

  await page.goto(FE, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("#splash", { state: "detached", timeout: 20000 }).catch(() => {});

  const readFood = () => page.evaluate(async (uid) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const all = await new Promise((res) => {
      const rq = db.transaction(["foods"], "readonly").objectStore("foods").getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    const f = all[0] || {};
    return { veg: f.is_veg_fruit, heme: f.is_heme, glob: f.is_milk_globule, egg: f.is_egg };
  }, uid);

  const t0 = Date.now();
  let food = {};
  while (Date.now() - t0 < WAIT_MS) {
    food = await readFood();
    if (food.veg != null && food.heme != null) break;
    await page.waitForTimeout(3000);
  }
  const secs = Math.round((Date.now() - t0) / 1000);

  const asked = [...new Set(models)];
  const usedSpare = asked.includes(SPARE);
  if (broken) {
    check("основную модель спросили первой", models[0] === BROKEN, models[0] ?? "запросов не было");
    check("после отказа спросили запасную", usedSpare, `спрашивали: ${asked.join(", ") || "—"}`);
    check("в логе отмечен переход на запасную", logs.some((l) => l.includes("пробуем запасную")));
  } else {
    check("спросили основную", models[0] === MAIN, models[0] ?? "запросов не было");
    // Запасная — деньги другого поставщика. Пока основная отвечает, её трогать нельзя.
    check("запасную не трогали", !usedSpare, `спрашивали: ${asked.join(", ") || "—"}`);
  }
  check(`продукт «${FOOD}» разобран (${secs} с)`, food.veg != null && food.heme != null,
    JSON.stringify(food));
  await ctx.close();

}

await run(true);
await run(false);
await b.close();
process.exit(failed ? 1 : 0);
