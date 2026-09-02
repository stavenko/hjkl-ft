// РАЗМЕР ПОСУДЫ — отдельной задачей.
//
// Вес порции считается из площади дна, а площадь растёт как КВАДРАТ поперечника:
// ошибка в диаметре на четверть даёт ошибку в весе в полтора раза. В замере
// measure-portion-steps.mjs это и оказалось единственным неустойчивым числом —
// доли и толщины модель называет одинаково от прогона к прогону, а диаметр на том
// же снимке рыбы выдавала то 22 см, то 27.
//
// Дома размер можно спросить у человека и хранить в настройках, но в гостях и в
// кафе посуда чужая. Значит определять её всё-таки надо, и здесь меряется, каким
// способом спрашивать.
//
// ЧЕТЫРЕ СПОСОБА СПРОСИТЬ, по два прогона на снимок. Судим по ошибке ПЛОЩАДИ — вес
// считается из неё, и ошибка диаметра в неё возводится:
//
//   просто спросить                16%   ← лучший
//   сперва предметы рядом          16%   ровно то же, выписывание предметов не помогло
//   выбор из списка               102%   всё записывает в обеденные
//   во сколько раз шире прибора   241%   на обоих снимках заявляет «в 1.8 раза»
//
// Видна одна причина всех провалов: модель не МЕРИТ по кадру, а ВСПОМИНАЕТ типичное.
// Дать ей перечисление — выберет самое ходовое. Попросить отношение — назовёт
// привычное 1.8, хотя на рыбе тарелка длиннее вилки всего в 1.08 раза. Работает
// только прямой вопрос про сантиметры, и то с остаточной систематикой: маленькую
// тарелку завышает, большую занижает, обе тянет к 22.
//
// Зато сам по себе ОТДЕЛЬНЫЙ запрос уже помог: внутри геометрического прохода
// диаметр на снимке рыбы скакал 22 и 27, здесь оба прогона дают 20.5.
//
//   node scripts/measure-plate-size.mjs [--model МОДЕЛЬ] [--n ПОВТОРОВ] [--way СПОСОБ]

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "qwen3-vl-plus");
const N = Number(arg("n", 2));
const ONE_WAY = arg("way", null);

/// Посуда человека: маленькая 19 см, большая 26. Оба снимка опознаны не на глаз —
/// на рыбе по вилке (она занимает 1068 px при тарелке в 1150), на печени по объёму
/// (400 г соцветий на маленькой тарелке лежали бы горкой почти в 6 см).
const CASES = [
  { file: "food-fish.jpg", truth: 19 },
  { file: "food-liver-cauliflower.jpg", truth: 26 },
];

const NUM = { type: "number" };
const STR = { type: "string" };

/// Спросить число прямо — так это и работало внутри геометрического прохода.
const FREE = {
  type: "object",
  properties: {
    diameter_cm: { description: "Поперечник посуды по внешнему краю, в сантиметрах.", ...NUM },
  },
  required: ["diameter_cm"],
};

/// Сначала выписать, что в кадре есть известного размера, и только потом число.
/// Порядок полей в этом проекте не раз оказывался важнее формулировок.
const REFS = {
  type: "object",
  properties: {
    what_else_is_in_frame: {
      description: "ПО-РУССКИ: какие предметы известного размера видно рядом с посудой или на ней — " +
        "вилка, ложка, нож, телефон, планшет, подставка, банка. Для каждого скажи, насколько он " +
        "велик на самом деле. Ничего подходящего нет — так и напиши.",
      ...STR,
    },
    how_it_compares: {
      description: "ПО-РУССКИ: во сколько раз посуда шире того предмета, что назван выше. Смотри по " +
        "кадру, а не по привычке: столовый прибор нередко почти во всю тарелку.",
      ...STR,
    },
    diameter_cm: {
      description: "Поперечник посуды по внешнему краю, в сантиметрах, из сравнения выше.",
      ...NUM,
    },
  },
  required: ["what_else_is_in_frame", "how_it_compares", "diameter_cm"],
};

/// Не число, а выбор из закрытого списка. В этом проекте замена свободной оценки на
/// перечисление уже выигрывала (вид массы на этикетке), и по той же причине: у посуды
/// не непрерывный размер, а несколько ходовых.
const KINDS = {
  dessert: 20, dinner: 27, large: 31, bowl: 16, wide_bowl: 22,
};
const KIND = {
  type: "object",
  properties: {
    what_else_is_in_frame: {
      description: "ПО-РУССКИ: какие предметы известного размера видно рядом с посудой или на ней, " +
        "и насколько каждый велик на самом деле. Ничего подходящего нет — так и напиши.",
      ...STR,
    },
    kind: {
      description: "Что за посуда, из перечисленного. \"dessert\" — десертная или закусочная " +
        "тарелка, она заметно меньше обеденной и лишь немного длиннее столового прибора. " +
        "\"dinner\" — обычная обеденная тарелка, самая ходовая. \"large\" — крупное блюдо или " +
        "тарелка для пасты. \"bowl\" — небольшая глубокая миска или пиала. \"wide_bowl\" — " +
        "широкая глубокая миска для супа или салата.",
      type: "string",
      enum: Object.keys(KINDS),
    },
  },
  required: ["what_else_is_in_frame", "kind"],
};

/// Не размер, а ОТНОШЕНИЕ к предмету в кадре. Замысел: назвать сантиметры — значит
/// вспомнить, какой бывает посуда, и оба снимка тянет к ходовым 22; а во сколько раз
/// одно длиннее другого — это измерение прямо по кадру, без припоминания. Сантиметры
/// приборов подставляет код.
const REF_CM = { fork: 19.5, spoon: 19, knife: 21, phone: 15, none: null };
const RATIO = {
  type: "object",
  properties: {
    reference: {
      description: "Предмет известного размера в кадре, по которому будем мерить. \"fork\" — " +
        "столовая вилка, \"spoon\" — столовая ложка, \"knife\" — столовый нож, \"phone\" — " +
        "телефон. Ничего такого в кадре нет — \"none\".",
      type: "string",
      enum: Object.keys(REF_CM),
    },
    how_measured: {
      description: "ПО-РУССКИ, коротко: где у этого предмета концы на снимке и где края посуды. " +
        "Это измерение по кадру, а не припоминание: сравнивай отрезки, которые видишь.",
      ...STR,
    },
    plate_over_reference: {
      description: "Во сколько раз поперечник посуды больше ДЛИНЫ этого предмета. Ровно столько же " +
        "— 1.0. Заметно шире — больше единицы. Предмет длиннее посуды — меньше единицы. Числа " +
        "вроде 1.5 означают полтора прибора поперёк тарелки, и это уже очень крупная посуда.",
      ...NUM,
    },
  },
  required: ["reference", "how_measured", "plate_over_reference"],
};

const WAYS = {
  "просто спросить":       { schema: FREE, get: (a) => a.diameter_cm },
  "сперва предметы рядом": { schema: REFS, get: (a) => a.diameter_cm },
  "выбор из списка":       { schema: KIND, get: (a) => KINDS[a.kind] ?? null },
  "во сколько раз шире прибора": {
    schema: RATIO,
    get: (a) => (REF_CM[a.reference] ? REF_CM[a.reference] * a.plate_over_reference : null),
  },
};

const PROMPT =
  `На снимке еда в посуде. Нужен ТОЛЬКО размер самой посуды — про еду не думай вовсе.\n\n` +
  `Учти: тарелки бывают очень разные. Десертная и закусочная заметно меньше обеденной, а миска ` +
  `бывает и совсем небольшой. Не считай всякую тарелку обеденной по умолчанию — смотри на кадр.`;

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `plate-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function ask(token, image, schema) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: [
        { type: "text", text: PROMPT },
        { type: "image_url", image_url: { url: `data:image/jpeg;base64,${image}` } },
      ] }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema, strict: true } },
      stream: true, think: false, max_tokens: 700,
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
  console.log(`модель ${MODEL}, повторов ${N}\n`);

  for (const [way, { schema, get }] of Object.entries(WAYS)) {
    if (ONE_WAY && way !== ONE_WAY) continue;
    console.log(`— ${way}`);
    let worstArea = 0;
    for (const c of CASES) {
      const image = readFileSync(`scripts/fixtures/${c.file}`).toString("base64");
      const got = [];
      for (let i = 0; i < N; i++) {
        try { got.push(get(await ask(token, image, schema))); }
        catch (e) { got.push(null); }
      }
      const line = got.map((d) => {
        if (d == null) return "сбой";
        // Судим по ПЛОЩАДИ: вес считается из неё, и ошибка диаметра в неё возводится.
        const area = (d / c.truth) ** 2 - 1;
        worstArea = Math.max(worstArea, Math.abs(area));
        return `${d} см (площадь ${area >= 0 ? "+" : ""}${(area * 100).toFixed(0)}%)`;
      }).join(", ");
      console.log(`   ${c.file.padEnd(28)} правда ${c.truth} см:  ${line}`);
    }
    console.log(`   худшая ошибка площади: ${(worstArea * 100).toFixed(0)}%\n`);
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
