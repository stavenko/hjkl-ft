// Первая стадия разбора ленивой записи: ОПИСАНИЕ → СПИСОК ПРОДУКТОВ.
//
// Дальше по пайплайну каждой позиции ищется аналог в базе пользователя, а
// ненайденным спрашивается КБЖУ, — но обе те стадии уже существуют
// (`ai::match_food`, `ai::lookup`). Нового здесь ровно одно: вытащить из
// свободного текста человека список позиций с количеством. Это и меряем.
//
//   node scripts/measure-description-items.mjs [--n 3] [--jobs 4]
//
// Что проверяется на каждом кейсе:
//   состав   — нашлись ли ВСЕ ожидаемые продукты (по вхождению корня слова),
//   лишнее   — не выдумала ли модель того, чего человек не называл,
//   граммы   — доехал ли явно названный вес, и НЕ выдуман ли вес там, где
//              человек его не называл (в ТЗ такой случай решает не модель:
//              разбор ставит 100 г сам).
//
// Кейсы — по четыре на один, два и три продукта. В каждой группе есть вес
// явный, счётный («два яйца») и отсутствующий. Отдельно две ловушки: блюдо с
// собственным именем разбирать на ингредиенты НЕЛЬЗЯ («борщ» — одна позиция), а
// составное описание — НУЖНО («хлеб с маслом» — две).

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

// {описание, ожидаемые продукты (корни слов — вхождением), ожидаемые граммы}
//
// `grams: null` в `want` значит «человек веса не называл» — модель ОБЯЗАНА
// вернуть null, а не выдумать число. Где вес назван счётной мерой, ждём
// диапазон: «два яйца» это 100–130 г смотря чьё яйцо, и спорить об этом незачем.
const CASES = [
  // — один продукт —
  { text: "съел банан", want: [{ key: ["банан"], grams: null }] },
  { text: "200 грамм гречки", want: [{ key: ["гречк", "греча"], grams: 200 }] },
  { text: "тарелка овсяной каши на воде", want: [{ key: ["овсян", "овсянка"], grams: null }] },
  // ловушка: у блюда есть собственное имя — разбирать на свёклу с капустой нельзя
  { text: "борщ", want: [{ key: ["борщ"], grams: null }] },

  // — два продукта —
  { text: "гречка 150 г и куриная грудка 200 г",
    want: [{ key: ["гречк"], grams: 150 }, { key: ["груд", "куриц", "куриная"], grams: 200 }] },
  { text: "творог 200 г и яблоко",
    want: [{ key: ["творог"], grams: 200 }, { key: ["яблок"], grams: null }] },
  // ловушка наоборот: составное описание разбирается на два продукта
  { text: "хлеб с маслом",
    want: [{ key: ["хлеб"], grams: null }, { key: ["масл"], grams: null }] },
  { text: "яичница из трёх яиц и помидор",
    want: [{ key: ["яич", "яйц"], grams: [120, 200] }, { key: ["помидор", "томат"], grams: null }] },

  // — три продукта —
  { text: "рис 150 г, треска 120 г и огурец",
    want: [{ key: ["рис"], grams: 150 }, { key: ["треск"], grams: 120 }, { key: ["огурц", "огурец"], grams: null }] },
  // «кусок хлеба» — такая же счётная мера, как стакан: ждём массу куска, не null
  { text: "на завтрак два яйца, кусок хлеба и кофе",
    want: [{ key: ["яйц"], grams: [90, 140] }, { key: ["хлеб"], grams: [20, 50] }, { key: ["кофе"], grams: null }] },
  { text: "салат из огурцов и помидоров с оливковым маслом",
    want: [{ key: ["огурц", "огурец"], grams: null }, { key: ["помидор", "томат"], grams: null },
           { key: ["оливков"], grams: null }] },
  { text: "стакан кефира, банан и горсть грецких орехов",
    want: [{ key: ["кефир"], grams: [200, 300] }, { key: ["банан"], grams: null },
           { key: ["орех"], grams: [20, 60] }] },
];

const PROMPT = (text) =>
  `Ты — nutrition assistant. Человек описал словами, что он съел. Выдели ОТДЕЛЬНЫМИ позициями каждый продукт, который он назвал.\n` +
  `Описание: "${text}"\n\n` +
  `Рассуждай про себя, затем выдай СТРОГИЙ JSON.\n` +
  `Правила:\n` +
  `- Название — на РУССКОМ, ОДНО каноническое название продукта (1-3 слова), БЕЗ СКОБОК и пояснений.\n` +
  `- Блюдо названо ТОЛЬКО собственным именем, состав не перечислен (борщ, плов, оливье, окрошка) — это ОДНА позиция с этим именем. НЕ разбирай его на ингредиенты.\n` +
  `- Состав ПЕРЕЧИСЛЕН («салат из огурцов и помидоров», «хлеб с маслом», «гречка с курицей») — верни ТОЛЬКО перечисленные продукты, каждый отдельной позицией.\n` +
  `- В таком описании слово-обёртка («салат», «боул», «завтрак», «блюдо», «микс», «тарелка») — НЕ еда и НЕ позиция: съеденное уже посчитано его составными частями, и лишняя позиция посчитала бы то же самое дважды.\n` +
  `- Осторожно со словом «салат»: в обороте «салат из X» это форма подачи, а НЕ листовой овощ, и позицией оно не идёт. Листовой салат добавляй позицией, только если он назван сам по себе («листья салата», «салат айсберг», «руккола»).\n` +
  `- grams — это МАССА В ГРАММАХ, НИКОГДА не число штук. Счётную меру переведи в массу: одно куриное яйцо ~60 г («три яйца» → 180), кусок хлеба ~30 г, стакан ~250 г, столовая ложка ~15 г, горсть орехов ~30 г. Значение меньше 5 г бывает только у специй.\n` +
  `- Если человек не назвал ни веса, ни счётной меры — grams: null. НЕ ВЫДУМЫВАЙ вес: «банан» без уточнения это null, а не 150.\n` +
  `- НЕ добавляй еду, которую человек не называл. Не додумывай гарниры, соусы и напитки.\n\n` +
  `Пример разбора. Описание «боул из риса, курицы и авокадо» → РОВНО ТРИ позиции: рис, курица, авокадо. ` +
  `Слова «боул» среди позиций НЕТ — это форма подачи, а не еда, и она уже посчитана своими составными частями.\n\n` +
  `Верни ТОЛЬКО JSON, без прозы: {"items":[{"name":"гречка","grams":200},{"name":"яблоко","grams":null}]}`;

const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "DescribedItems",
  type: "object",
  properties: {
    items: {
      type: "array",
      items: {
        type: "object",
        properties: {
          name: { type: "string" },
          grams: { type: ["number", "null"] },
        },
        required: ["name", "grams"],
      },
    },
  },
  required: ["items"],
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const uid = `descr-${Date.now()}`;
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function ask(token, text) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: PROMPT(text) }],
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
  return (j.items || []).map((i) => ({ name: String(i.name || ""), grams: i.grams ?? null }));
}

/// Позиция ответа, отвечающая ожидаемому продукту: сверяем ВХОЖДЕНИЕМ корня, а не
/// равенством строк — иначе мерили бы совпадение с моей орфографией, а не работу
/// модели: «яйца» и «яйцо» это одно и то же.
const findItem = (items, keys) =>
  items.find((i) => keys.some((k) => i.name.toLowerCase().includes(k)));

const gramsOk = (got, want) => {
  if (want === null) return got === null;          // веса не называли — ждём null
  if (Array.isArray(want)) return got !== null && got >= want[0] && got <= want[1];
  return got !== null && Math.abs(got - want) < 1;
};

async function main() {
  const token = await mintToken();
  console.log(`модель ${MODEL}, повторов ${N}\n`);

  const queue = CASES.map((c, idx) => ({ ...c, idx }));
  const rows = [];
  const worker = async () => {
    for (;;) {
      const c = queue.shift();
      if (!c) return;
      const runs = [];
      let failed = 0;
      for (let i = 0; i < N; i++) {
        try { runs.push(await ask(token, c.text)); } catch { failed++; }
      }
      rows.push({ ...c, runs, failed });
    }
  };
  await Promise.all(Array.from({ length: JOBS }, worker));
  rows.sort((a, b) => a.idx - b.idx);

  let missTotal = 0, extraTotal = 0, gramsTotal = 0, runsTotal = 0;
  for (const { text, want, runs, failed } of rows) {
    console.log(`«${text}»  ожидается позиций: ${want.length}${failed ? `  (сбоев ${failed})` : ""}`);
    for (const items of runs) {
      runsTotal++;
      const missing = want.filter((w) => !findItem(items, w.key)).map((w) => w.key[0]);
      const matched = new Set(want.map((w) => findItem(items, w.key)).filter(Boolean).map((i) => i.name));
      const extra = items.filter((i) => !matched.has(i.name)).map((i) => i.name);
      const badGrams = want
        .map((w) => [w, findItem(items, w.key)])
        .filter(([w, got]) => got && !gramsOk(got.grams, w.grams))
        .map(([w, got]) => `${got.name}=${got.grams}≠${Array.isArray(w.grams) ? w.grams.join("..") : w.grams}`);
      if (missing.length) missTotal++;
      if (extra.length) extraTotal++;
      if (badGrams.length) gramsTotal++;

      const shown = items.map((i) => `${i.name}${i.grams === null ? "" : ` ${i.grams}г`}`).join(", ");
      const marks = [
        missing.length ? `НЕТ: ${missing.join(", ")}` : null,
        extra.length ? `ЛИШНЕЕ: ${extra.join(", ")}` : null,
        badGrams.length ? `ГРАММЫ: ${badGrams.join(", ")}` : null,
      ].filter(Boolean);
      console.log(`   [${items.length}] ${shown}${marks.length ? `\n       ← ${marks.join(" | ")}` : ""}`);
    }
    console.log();
  }

  console.log(`итого прогонов: ${runsTotal}`);
  console.log(`  с недостачей позиций: ${missTotal}`);
  console.log(`  с лишними позициями: ${extraTotal}`);
  console.log(`  с неверными граммами: ${gramsTotal}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
