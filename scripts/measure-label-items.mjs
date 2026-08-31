// Универсальный разбор ФОТОГРАФИЙ ленивой записи: снимки → список еды с тем, что
// удалось прочесть с упаковки.
//
// Деления картинок на «этикетку» и «еду» больше нет (ТЗ §6.5), поэтому промпт
// один на оба случая: он сам решает, что перед ним.
//
//   node scripts/measure-label-items.mjs [--n 3] [--case сметана|оливки|сырок] [--single 1|2]
//
//   --case     прогнать только один кейс
//   --single   отправить только N-й снимок кейса — видно, что даёт каждый кадр
//              поодиночке и что добавляет их сведение
//
// Разворачивать снимки не нужно: текст на них лежит на боку, и модель читает его
// как есть — проверено.
//
// КЕЙСЫ
//
// «сметана» — один продукт, снятый ДВАЖДЫ, таблица разорвана между кадрами: на
// первом снимке название и калорийность, на втором не обрезанные углеводы.
// Правильный ответ — ОДНА позиция со сведёнными цифрами, а не две половинки и не
// два продукта. Поодиночке каждый кадр даёт правдоподобную, но неверную строку.
//
// «оливки» — два снимка: на одном таблица, на другом название и МАССЫ. Их две —
// «масса нетто 290 г» и «масса пищевой продукции, помещённой в жидкую среду,
// 180 г». По ТЗ §6.2 надпись массы означает «съедена вся упаковка», но съедены
// будут оливки, а не рассол: верный ответ — 180 г.
//
// «сырок» — один снимок, зато с тремя ловушками на жир: в названии стоит
// «массовая доля жира 5%», в таблице есть «в творожной части – 5,0 г», а жир
// продукта — 11,1 г. Плюс единственный пока случай, где сахар на этикетке
// ВЫДЕЛЕН отдельной строкой («в т.ч. сахароза – 29,0 г») и обязан доехать.
//
// Эталоны вычитаны со снимков вручную. У сырка глиф жира двоится между «17,1» и
// «11,1», и решает арифметика: 10,0×4 + 36,4×4 + 11,1×9 = 285,5 ккал ровно.

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
// Картиночная модель прямого пути — та, что стоит в `vision_model` дев-конфига.
const MODEL = process.env.MODEL || "qwen3-vl-plus";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const N = Number(arg("n", 3));
const SINGLE = arg("single", null);

// `null` в ожидании значит «поле обязано остаться пустым»: этого на этикетке нет,
// и выдумывать его нельзя.
const CASES = [
  {
    name: "сметана",
    files: ["scripts/fixtures/label-smetana-1.jpg", "scripts/fixtures/label-smetana-2.jpg"],
    nameKeys: ["сметан"],
    want: { kcal: 160, protein: 2.7, fat: 15.0, carbs: 3.6, sugar: null, package_weight_g: null },
  },
  {
    name: "оливки",
    files: ["scripts/fixtures/label-olives-1.jpg", "scripts/fixtures/label-olives-2.jpg"],
    nameKeys: ["оливк"],
    // Масс на банке ДВЕ: «масса нетто 290 г» и «масса пищевой продукции,
    // помещённой в жидкую среду, 180 г». Съедены будут оливки, а не рассол,
    // поэтому «вся банка» — это 180 г.
    want: { kcal: 149, protein: 1.6, fat: 14.6, carbs: 0.0, sugar: null, package_weight_g: 180 },
  },
  {
    name: "сырок",
    files: ["scripts/fixtures/label-syrok.jpg"],
    nameKeys: ["сыр"],
    want: { kcal: 285.5, protein: 10.0, fat: 11.1, carbs: 36.4, sugar: 29.0, package_weight_g: null },
  },
];

/// Вопрос ПЕРВЫЙ: что за еда и что написано в таблице. Про массу упаковки здесь не
/// спрашивается вовсе — замер показал, что вместе эти два вопроса тянут модель вниз:
/// стоило добавить правила про массу, как поехали сами цифры таблицы (1490 ккал,
/// жиры 146, вместо ккал взяты кДж).
const PROMPT_TABLE =
  `Ты — nutrition vision assistant. На фотографиях — еда, которую съел человек. Это могут быть ` +
  `снимки упаковки с этикеткой, снимки самого продукта, снимки тарелки с готовой едой — или всё сразу.\n\n` +
  `Рассуждай про себя, затем выдай СТРОГИЙ JSON.\n` +
  `Шаги:\n` +
  `1. Пойми, что перед тобой на КАЖДОЙ фотографии: этикетка, продукт или тарелка с едой.\n` +
  `2. Собери список еды. ВАЖНО: несколько фотографий могут показывать ОДИН И ТОТ ЖЕ продукт с ` +
  `разных сторон или с разным приближением — это ОДНА позиция, а не несколько. Считай еду дважды ` +
  `только если это действительно разная еда.\n` +
  `3. Прочитай на упаковке пищевую ценность на 100 г. Таблица может быть РАЗОРВАНА между кадрами: ` +
  `название на одном снимке, цифры на другом, часть строки обрезана краем — СВЕДИ прочитанное со ` +
  `всех фотографий в одну таблицу. Текст может лежать на боку или вверх ногами — всё равно прочитай.\n` +
  `4. Заполняй только то, что ВИДНО. Не помнишь — не выдумывай: не прочитанное поле оставь null.\n\n` +
  `Правила:\n` +
  `- name — на РУССКОМ, короткое каноническое название продукта. ЖЁСТКИЙ ПРЕДЕЛ: не больше ТРЁХ слов. ` +
  `Оставь суть и определяющую цифру (жирность), отбрось описательное: «СЫРОК ТВОРОЖНЫЙ ГЛАЗИРОВАННЫЙ С ` +
  `АРОМАТОМ ВАНИЛИ, МАССОВАЯ ДОЛЯ ЖИРА 5%» → «глазированный сырок 5%»; «сметана, массовая доля жира ` +
  `15,0%» → «сметана 15%».\n` +
  `- kcal, protein, fat, carbs — на 100 г, числами. Энергию бери в ККАЛ: это число перед словом «ккал», ` +
  `а НЕ перед «кДж». Значения переписывай ровно как напечатано, не пересчитывай и не округляй.\n` +
  `- sugar — только если на упаковке есть отдельная запись про сахар. Часто она спрятана В СКОБКАХ ` +
  `внутри строки углеводов и названа «сахароза»: «углеводы – 36,4 г (в т.ч. сахароза – 29,0 г)» → ` +
  `carbs 36.4, sugar 29.0. Такой записи нет — sugar: null, и НИКОГДА не переписывай в сахар значение углеводов.\n` +
  `- fiber, saturated_fat — так же: только из своей строки на упаковке, иначе null.\n` +
  `- grams — сколько человек съел, если это видно (порция на тарелке); для снимка упаковки null.\n` +
  `- confidence — 0..1, насколько ты уверен в позиции.\n\n` +
  `Верни ТОЛЬКО JSON, без прозы.`;

/// Вопрос ВТОРОЙ, отдельным заходом: сколько еды в упаковке. Спрашивается ЦИТАТА —
/// процитировать несуществующую надпись труднее, чем назвать правдоподобное число,
/// а по ТЗ §6.2 эта масса означает «съедена вся пачка», и выдуманная масса стала бы
/// выдуманным весом съеденного.
const PROMPT_MASS =
  `На фотографиях — упаковка продукта. Один вопрос: написана ли где-нибудь МАССА ЕДЫ в этой упаковке.\n\n` +
  `- Надписи массы на снимке НЕТ — верни null в обоих полях. Не выводи массу из типичного размера такой ` +
  `упаковки и не бери числа из таблицы пищевой ценности: «на 100 г» — это не масса упаковки.\n` +
  `- Масс НЕСКОЛЬКО — бери массу продукта БЕЗ ЖИДКОСТИ: не «масса нетто 290 г» (это вместе с рассолом), ` +
  `а «масса пищевой продукции, помещённой в жидкую среду, 180 г» (бывает «масса основного продукта», ` +
  `«сухой остаток»). Рассол не едят.\n` +
  `- text — ДОСЛОВНАЯ надпись со снимка, из которой взята масса. Не можешь процитировать её словами с ` +
  `упаковки — значит её там нет, оба поля null.\n\n` +
  `Верни ТОЛЬКО JSON: {"grams": 180, "text": "Масса пищевой продукции, помещённой в жидкую среду 180 г"}`;

const NUM_OR_NULL = { type: ["number", "null"] };
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "PhotoItems",
  type: "object",
  properties: {
    items: {
      type: "array",
      items: {
        type: "object",
        properties: {
          name: { type: "string" },
          grams: NUM_OR_NULL,
          kcal: NUM_OR_NULL,
          protein: NUM_OR_NULL,
          fat: NUM_OR_NULL,
          carbs: NUM_OR_NULL,
          sugar: NUM_OR_NULL,
          fiber: NUM_OR_NULL,
          saturated_fat: NUM_OR_NULL,
          confidence: { type: "number" },
        },
        required: ["name", "grams", "kcal", "protein", "fat", "carbs",
                   "sugar", "fiber", "saturated_fat", "confidence"],
      },
    },
  },
  required: ["items"],
};

const MASS_SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "PackageMass",
  type: "object",
  properties: { grams: NUM_OR_NULL, text: { type: ["string", "null"] } },
  required: ["grams", "text"],
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const uid = `label-${Date.now()}`;
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

/// Тот же формат запроса, что у `ai::vision_chat`: текст плюс image_url с data-URL.
async function ask(token, images, prompt, schema) {
  const parts = [{ type: "text", text: prompt }];
  for (const b64 of images) {
    parts.push({ type: "image_url", image_url: { url: `data:image/jpeg;base64,${b64}` } });
  }
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: parts }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema, strict: true } },
      stream: true, think: false, max_tokens: 2000,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 200)}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const payload = line.slice(6).trim();
    if (payload === "[DONE]") continue;
    try { content += JSON.parse(payload).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
}

const near = (got, want) =>
  want === null ? got === null : got !== null && Math.abs(got - want) <= Math.max(0.15, want * 0.02);

const FIELDS = ["kcal", "protein", "fat", "carbs", "sugar", "package_weight_g"];
const SHORT = { kcal: "ккал", protein: "Б", fat: "Ж", carbs: "У", sugar: "сахар", package_weight_g: "нетто" };

async function runCase(token, c) {
  const files = SINGLE ? [c.files[Number(SINGLE) - 1]] : c.files;
  const images = files.map((f) => readFileSync(f).toString("base64"));
  const ref = FIELDS.map((k) => `${SHORT[k]} ${c.want[k] === null ? "—" : c.want[k]}`).join(", ");
  console.log(`\n[${c.name}] снимков ${images.length}, эталон на 100 г: ${ref}`);

  let okCount = 0, okName = 0, okAll = 0;
  for (let i = 0; i < N; i++) {
    let items, mass;
    try {
      items = (await ask(token, images, PROMPT_TABLE, SCHEMA)).items || [];
      mass = await ask(token, images, PROMPT_MASS, MASS_SCHEMA);
    } catch (e) { console.log(`  прогон ${i + 1}: сбой — ${e.message}`); continue; }
    // Масса — ответ отдельного вопроса, кладём её в позицию для сверки.
    for (const it of items) { it.package_weight_g = mass.grams ?? null; it.package_weight_text = mass.text ?? null; }

    if (items.length === 1) okCount++;
    for (const it of items) {
      const shown = FIELDS
        .map((k) => `${SHORT[k]}=${it[k] === null ? "—" : it[k]}${near(it[k], c.want[k]) ? "" : " ✗"}`)
        .join(" ");
      const quote = it.package_weight_text ? `  ← «${it.package_weight_text}»` : "";
      console.log(`  прогон ${i + 1}: [${items.length}] «${it.name}»  ${shown}${quote}`);
    }
    if (items.length === 1) {
      const it = items[0];
      if (c.nameKeys.some((k) => it.name.toLowerCase().includes(k))) okName++;
      if (FIELDS.every((k) => near(it[k], c.want[k]))) okAll++;
    }
  }
  console.log(`  → одна позиция ${okCount}/${N}, название ${okName}/${N}, все поля ${okAll}/${N}`);
  return { okCount, okName, okAll };
}

async function main() {
  const token = await mintToken();
  const only = arg("case", null);
  const cases = only ? CASES.filter((c) => c.name === only) : CASES;
  if (!cases.length) { console.error(`нет кейса «${only}»`); process.exit(1); }
  console.log(`модель ${MODEL}, повторов ${N}`);
  for (const c of cases) await runCase(token, c);
}

main().catch((e) => { console.error(e); process.exit(1); });
