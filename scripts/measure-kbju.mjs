// Насколько правдиво модель называет КБЖУ продукта, которого НЕТ этикетки.
//
// Это тот случай, ради которого затевается ленивая запись: еда попала в дневник
// либо распознанной на фотографии, либо строкой в описании, и цифр с упаковки у
// нас нет — их приходится вспоминать модели. Пока не измерено, насколько хорошо
// она это делает, менять промпт бессмысленно: не с чем будет сравнить.
//
// Промпт СЛОВО В СЛОВО тот, что строит `ai::build_nutrition_prompt` с клаузой
// `lookup_prompt_by_name` (или `lookup_prompt_from_photo` при `--mode served`), и
// схема та же, что печатает `cargo test -p frontend --lib print_schema`, — иначе
// мерили бы не то, что работает в приложении.
//
//   node scripts/measure-kbju.mjs [--n 3] [--mode raw|served] [--jobs 4]
//
// Справочные значения — USDA / стандартные таблицы, на 100 г продукта КАК ОН
// ПРОДАЁТСЯ (крупа сухая, мясо сырое). Список нарочно разный: крупы, мясо, рыба,
// чистый жир, чистый углевод и овощи с околонулевыми жирами — там, где ошибиться
// проще всего.

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const MODEL = process.env.MODEL || "@cf/qwen/qwen3-30b-a3b-fp8";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] ? process.argv[i + 1] : def;
};
const N = Number(arg("n", 3));
const JOBS = Number(arg("jobs", 4));
const MODE = arg("mode", "raw");

// продукт → [ккал, белки, жиры, углеводы] на 100 г как продаётся
const FOODS = [
  ["Гречневая крупа", [343, 13.3, 3.4, 71.5]],
  ["Рис белый шлифованный", [365, 7.1, 0.7, 80.0]],
  ["Овсяные хлопья", [389, 16.9, 6.9, 66.3]],
  ["Макароны из твёрдых сортов пшеницы", [371, 13.0, 1.5, 74.7]],
  ["Чечевица сухая", [352, 24.6, 1.1, 63.4]],
  ["Куриная грудка", [120, 22.5, 2.6, 0.0]],
  ["Яйцо куриное", [143, 12.6, 9.5, 0.7]],
  ["Треска", [82, 17.8, 0.7, 0.0]],
  ["Лосось атлантический", [208, 20.4, 13.4, 0.0]],
  ["Сливочное масло", [717, 0.9, 81.1, 0.1]],
  ["Оливковое масло", [884, 0.0, 100.0, 0.0]],
  ["Молоко 2,5%", [52, 2.8, 2.5, 4.7]],
  ["Картофель", [77, 2.0, 0.1, 17.0]],
  ["Морковь", [41, 0.9, 0.2, 9.6]],
  ["Яблоко", [52, 0.3, 0.2, 13.8]],
  ["Банан", [89, 1.1, 0.3, 22.8]],
  ["Грецкий орех", [654, 15.2, 65.2, 13.7]],
  ["Семена подсолнечника", [584, 20.8, 51.5, 20.0]],
  ["Сахар-песок", [387, 0.0, 0.0, 100.0]],
  ["Огурец", [15, 0.7, 0.1, 3.6]],
];

const RAW_CLAUSE =
  "Form of the product: for items bought and weighed raw/dry (grains, rice, pasta, " +
  "flour, meat, fish, legumes, etc.), give values for the RAW / as-sold product — " +
  "NOT cooked — unless the input says cooked, boiled, fried, steamed, ready-to-eat, " +
  "or clearly describes a prepared dish.";

const SERVED_CLAUSE =
  "Form of the product: this food was photographed ON A PLATE, ready to eat, and " +
  "its weight is the COOKED / as-served portion. Give values for the food in its " +
  "COOKED / ready-to-eat state (e.g. boiled pasta ~130 kcal/100 g, boiled rice " +
  "~120, NOT the dry product), even if the name alone sounds like a raw ingredient.";

/// Точная копия `ai::build_nutrition_prompt` (lang = Russian, custom = пусто).
const prompt = (name) =>
  `You are a nutritional database. The user's input may be a plain food NAME (e.g. ` +
  `«яйцо», «рис», «гречка») OR a free-form DESCRIPTION of a dish, possibly with added ` +
  `ingredients (e.g. «жареная курица, добавил немного лука, чайную ложку масла»). ` +
  `Input: "${name}".\n\n` +
  `First, set "product_name" to a SHORT dish/product name in Russian. HARD LIMIT: at most ` +
  `THREE words, ideally TWO; a third word ONLY when indispensable to identify the dish. ` +
  `Do NOT list the added ingredients in the name — name only the core dish. Good examples ` +
  `(2–3 words): «Жареная курица», «Куриная грудка», «Овсяная каша», «Гречка с грибами». ` +
  `For a plain name, keep it (tidied); for a description, name the resulting core dish ` +
  `concisely within this limit.\n\n` +
  `Then provide nutritional values per 100 GRAMS of the resulting food/dish (account for ` +
  `the added ingredients — e.g. the oil raises fat and kcal).\n\n` +
  `${MODE === "served" ? SERVED_CLAUSE : RAW_CLAUSE}\n\n` +
  `For each nutrient (kcal, protein, fat, carbs), provide:\n` +
  `- min_value: lowest reasonable value for this food\n` +
  `- max_value: highest reasonable value for this food\n` +
  `- recommended: the most likely value to select\n` +
  `- comment: brief explanation why this value is appropriate, written in Russian\n\n` +
  `Use these units: kcal for calories, g/mg/mkg/kg for weights.\n` +
  `All values are per 100g. Compute real values specifically for the input — do not ` +
  `copy any sample numbers.\n\n` +
  `Respond with ONLY a single minified JSON object and nothing else — no markdown, no ` +
  `prose before or after. Include "product_name" as a string. EVERY key and EVERY ` +
  `string value MUST be wrapped in double quotes. EVERY \`value\` MUST be a real number ` +
  `(e.g. 12.5), never empty or null. Custom nutrients go in the "custom_nutrients" ` +
  `object (use {} if none).`;

const VALUE_UNIT = {
  type: "object",
  properties: {
    value: { type: "number", format: "double" },
    unit: { description: "One of: kcal, kg, g, mg, mkg", type: "string" },
  },
  required: ["value", "unit"],
};
const NUTRIENT_DETAIL = {
  type: "object",
  properties: {
    min_value: VALUE_UNIT,
    max_value: VALUE_UNIT,
    recommended: VALUE_UNIT,
    comment: { description: "Why this value is appropriate for this food", type: "string" },
  },
  required: ["min_value", "max_value", "recommended", "comment"],
};
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "NutritionResponse",
  type: "object",
  properties: {
    product_name: { type: "string", default: "" },
    kcal: NUTRIENT_DETAIL,
    protein: NUTRIENT_DETAIL,
    fat: NUTRIENT_DETAIL,
    carbs: NUTRIENT_DETAIL,
    custom_nutrients: { type: "object", additionalProperties: NUTRIENT_DETAIL },
  },
  required: ["kcal", "protein", "fat", "carbs", "custom_nutrients"],
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const uid = `kbju-${Date.now()}`;
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data)));
  const token = `${data}.${b64url(sig)}`;
  // Распознавание за пейволлом — гостю выдаётся тестовая подписка, иначе 402.
  const co = await (await fetch(`${PAY}/test/guest-checkout`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ planId: "test" }),
  })).json();
  await fetch(`${PAY}/claim`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ claimId: co.claimId, secret: co.secret }),
  });
  return token;
}

/// Один заход к модели. `think: false` — как в `build_executor_think(false)`,
/// которым ходит боевой lookup.
async function ask(token, name) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: prompt(name) }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 2000,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const payload = line.slice(6).trim();
    if (payload === "[DONE]") continue;
    try { content += JSON.parse(payload).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  const j = JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
  return [j.kcal, j.protein, j.fat, j.carbs].map((d) => Number(d.recommended.value));
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2;
};

/// Относительная ошибка. Там, где справочное значение почти ноль (жир огурца,
/// углеводы масла), доля не имеет смысла — считаем абсолютную разницу и меряем её
/// порогом в один грамм.
const err = (got, ref) => (ref >= 1 ? Math.abs(got - ref) / ref : null);

const MACROS = ["ккал", "белки", "жиры", "углев"];

async function main() {
  const token = await mintToken();
  console.log(`модель ${MODEL}, режим ${MODE}, повторов ${N}\n`);

  const queue = [...FOODS];
  const rows = [];
  const worker = async () => {
    for (;;) {
      const item = queue.shift();
      if (!item) return;
      const [name, ref] = item;
      const runs = [];
      let failed = 0;
      for (let i = 0; i < N; i++) {
        try { runs.push(await ask(token, name)); } catch { failed++; }
      }
      if (!runs.length) { rows.push({ name, ref, med: null, failed }); continue; }
      rows.push({ name, ref, med: [0, 1, 2, 3].map((k) => median(runs.map((r) => r[k]))), failed });
    }
  };
  await Promise.all(Array.from({ length: JOBS }, worker));
  rows.sort((a, b) => FOODS.findIndex((f) => f[0] === a.name) - FOODS.findIndex((f) => f[0] === b.name));

  const head = "продукт".padEnd(36) + MACROS.map((m) => m.padStart(15)).join("");
  console.log(head);
  console.log("-".repeat(head.length));
  const errors = [[], [], [], []];
  const absMisses = [0, 0, 0, 0];
  for (const { name, ref, med, failed } of rows) {
    if (!med) { console.log(name.padEnd(36) + "  — не ответила —" + (failed ? ` (${failed})` : "")); continue; }
    let line = name.padEnd(36);
    med.forEach((got, k) => {
      const e = err(got, ref[k]);
      if (e === null) {
        if (Math.abs(got - ref[k]) > 1) absMisses[k]++;
        line += `${ref[k]}→${got}`.padStart(15);
      } else {
        errors[k].push(e);
        line += `${ref[k]}→${got} ${(e * 100).toFixed(0)}%`.padStart(15);
      }
    });
    console.log(line + (failed ? `  (сбоев ${failed})` : ""));
  }

  console.log("\nмедианная относительная ошибка (там, где справочное ≥ 1):");
  MACROS.forEach((m, k) => {
    const es = errors[k];
    const gross = es.filter((e) => e > 0.3).length;
    console.log(
      `  ${m.padEnd(7)} ${(median(es) * 100).toFixed(0).padStart(3)}%   ` +
      `грубых (>30%): ${gross}/${es.length}` +
      (absMisses[k] ? `   мимо на >1 г там, где справочное ~0: ${absMisses[k]}` : "")
    );
  });
}

main().catch((e) => { console.error(e); process.exit(1); });
