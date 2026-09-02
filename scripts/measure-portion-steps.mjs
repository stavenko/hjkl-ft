// ВЕС ПОРЦИИ ЧЕРЕЗ ГЕОМЕТРИЮ: спрашиваем не граммы, а размеры.
//
// Прямой вопрос «сколько тут граммов» все модели отвечают вдвое ниже правды, и это
// не чинится ни промптом, ни выбором модели (таблица в measure-per-image.mjs).
//
// Но в их же ответах видно противоречие: qwen3-vl-plus описала мясо как «половина
// тарелки, слой 2–3 см» — это около 700 см³ — и тут же назвала 150 г. Геометрию она
// описывает щедро, а число называет скупо; выходит, число берётся не из описания.
//
// Отсюда замысел: спросить ОТДЕЛЬНЫМ запросом только размеры — поперечник посуды,
// какую долю дна занимает еда, какой толщины слой, — а объём и вес посчитать кодом.
// Разнести по запросам важно: в этом проекте уже дважды подтверждалось, что два
// вопроса в одном запросе тянут друг друга вниз, и поле с оценкой доли тарелки,
// стоявшее в ОДНОМ запросе с граммами, не сдвинуло ничего.
//
//   node scripts/measure-portion-steps.mjs [--model МОДЕЛЬ] [--only ПОДСТРОКА]

import { readFileSync, readdirSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "qwen3-vl-plus");
const ONLY = arg("only", null);

/// Эталоны — то, что человек взвесил. Без них замер бессмыслен: геометрию можно
/// получить какую угодно, вопрос только в том, сходится ли она с правдой.
const TRUTH = {
  // Ключ — корни, по которым узнаём позицию: модель называет печень то «мясом», то
  // «говядиной», и сверять надо с тем, что она сказала, а не с тем, что мы ждали.
  "food-liver-cauliflower.jpg": [
    { keys: ["печен", "мясо", "говяд", "субпрод"], grams: 300 },
    { keys: ["капуст"], grams: 400 },
  ],
};

const SCHEMA = {
  type: "object",
  properties: {
    tableware: {
      description: "Что за посуда в кадре и как ты поняла её размер: обеденная тарелка, десертная, " +
        "миска, доска. ПО-РУССКИ, одним предложением.",
      type: "string",
    },
    tableware_diameter_cm: {
      description: "Поперечник посуды по внешнему краю, в сантиметрах. Обычная обеденная тарелка " +
        "крупнее десертной, миска глубокая. Если в кадре есть вилка, нож или ложка — они помогают: " +
        "столовая вилка примерно с ладонь.",
      type: "number",
    },
    regions: {
      description: "Каждая отдельная еда на посуде. Граммы НЕ называй — их посчитают без тебя.",
      type: "array",
      items: {
        type: "object",
        properties: {
          name: { description: "Что это, по-русски, 1-3 слова.", type: "string" },
          share_of_plate: {
            description: "Какую долю ДНА посуды закрывает эта еда, если смотреть сверху: 0.5 — " +
              "половину, 0.25 — четверть. Считай площадь пятна, а не объём.",
            type: "number",
          },
          thickness_cm: {
            description: "Насколько высоко еда поднимается над дном в самом типичном месте, в " +
              "сантиметрах. Плоский ломтик тонкий, горка высокая.",
            type: "number",
          },
          how_dense: {
            description: "Насколько плотно уложена еда. \"solid\" — сплошной кусок без пустот " +
              "(филе, котлета, ломоть). \"packed\" — вязкая масса, пустот почти нет (каша, пюре, " +
              "тушёное с подливой). \"loose\" — куски с большими промежутками воздуха между ними " +
              "(соцветия, салат, ягоды горкой).",
            type: "string",
            enum: ["solid", "packed", "loose"],
          },
        },
        required: ["name", "share_of_plate", "thickness_cm", "how_dense"],
      },
    },
  },
  required: ["tableware", "tableware_diameter_cm", "regions"],
};

/// Сколько объёма реально занято едой и сколько весит кубический сантиметр. Числа
/// грубые и взяты не из замера, а из здравого смысла, — поэтому и разнесены сюда, в
/// код: их видно, их можно поправить, и они не спрятаны внутри модели.
const DENSITY = {
  solid:  { fill: 0.90, g_per_cm3: 1.00 },
  packed: { fill: 0.80, g_per_cm3: 0.95 },
  loose:  { fill: 0.45, g_per_cm3: 0.90 },
};

function grams(diameterCm, r) {
  const area = Math.PI * (diameterCm / 2) ** 2;
  const { fill, g_per_cm3 } = DENSITY[r.how_dense] || DENSITY.packed;
  return area * r.share_of_plate * r.thickness_cm * fill * g_per_cm3;
}

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `geom-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data)));
  const token = `${data}.${b64url(sig)}`;
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

async function ask(token, image) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: [
        { type: "text", text:
          `Перед тобой снимок еды. Твоя задача — ТОЛЬКО РАЗМЕРЫ, ничего больше.\n\n` +
          `Не думай о калорийности и не называй вес: граммы посчитают без тебя, по твоим размерам. ` +
          `От тебя нужны поперечник посуды, доля дна под каждой едой и высота слоя. Отвечай так, ` +
          `как если бы измерял линейкой, а не как если бы прикидывал порцию на глаз.` },
        { type: "image_url", image_url: { url: `data:image/jpeg;base64,${image}` } },
      ] }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 1200,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const p = line.slice(6).trim();
    if (p === "[DONE]") continue;
    try { content += JSON.parse(p).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
}

async function main() {
  const token = await mintToken();
  console.log(`модель ${MODEL}\n`);
  const files = readdirSync("scripts/fixtures")
    .filter((f) => f.startsWith("food-") && f.endsWith(".jpg") && (!ONLY || f.includes(ONLY)));

  for (const f of files) {
    const image = readFileSync(`scripts/fixtures/${f}`).toString("base64");
    let a;
    try { a = await ask(token, image); }
    catch (e) { console.log(`${f}\n  сбой — ${e.message}\n`); continue; }

    console.log(`${f}`);
    console.log(`  посуда: ${a.tableware}  →  ${a.tableware_diameter_cm} см`);
    for (const r of a.regions || []) {
      const g = grams(a.tableware_diameter_cm, r);
      const truth = (TRUTH[f] || [])
        .find((t) => t.keys.some((k) => r.name.toLowerCase().includes(k)))?.grams;
      console.log(`  ${r.name}: доля ${r.share_of_plate}, слой ${r.thickness_cm} см, ${r.how_dense}` +
        `  →  ${g.toFixed(0)} г${truth ? `   (эталон ${truth}, ошибка ${((g / truth - 1) * 100).toFixed(0)}%)` : ""}`);
    }
    console.log();
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
