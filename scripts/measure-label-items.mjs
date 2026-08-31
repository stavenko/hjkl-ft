// Универсальный разбор ФОТОГРАФИЙ ленивой записи: снимки → список еды с тем, что
// удалось прочесть с упаковки.
//
// Деления картинок на «этикетку» и «еду» больше нет (ТЗ §6.5), поэтому промпт
// один на оба случая: он сам решает, что перед ним.
//
//   node scripts/measure-label-items.mjs [--n 3] [--case сметана|оливки|творог|курица|йогурт|сырок]
//                                         [--single 1|2]
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
// «творог» — один снимок, масса на нём есть и равна 250 г. Рядом с цифрами стоят
// числа-обманки: крупная «1.2 %» жирности под датой, код «497» и килоджоули 406,5
// прямо перед калорийностью.
//
// «курица» — филе грудки: строки углеводов на этикетке нет вовсе, масса написана в
// килограммах, а калорийность стоит ПЕРЕД килоджоулями — наоборот к творогу.
//
// «йогурт» — два снимка, и три ловушки разом: врезка «на 140 г: 93,5 ккал» рядом с
// таблицей на 100 г (калорийность порции, не ста граммов), калории в скобках после
// килоджоулей, и маркетинговое «11 г белка» крупно на крышке.
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
const OCR = process.env.OCR || "https://ocr-queue-dev.vg-stavenko.workers.dev";
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
// По умолчанию СВОЙ сервер: он бесплатный. Прямой путь через ai-worker — за деньги
// и только по явному флагу.
const ROUTE = arg("route", "queue");
const RAW = process.argv.includes("--raw");
// Показать сырой ответ модели целиком — чем она обосновала числа.

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
    name: "творог",
    files: ["scripts/fixtures/label-tvorog.jpg"],
    nameKeys: ["творог"],
    // Числа-соседи: крупная «1.2 %» жирности под датой, код «497» и килоджоули
    // 406,5 прямо перед калориями. Масса напечатана дважды и оба раза 250 г.
    want: { kcal: 96, protein: 18, fat: 1.2, carbs: 3.3, sugar: null, package_weight_g: 250 },
  },
  {
    name: "курица",
    files: ["scripts/fixtures/label-chicken.jpg"],
    nameKeys: ["груд", "филе", "куриц", "цыпл"],
    // Строки углеводов на этикетке НЕТ — у мяса её обычно и не печатают. Масса
    // написана в килограммах. Калорийность стоит ПЕРЕД килоджоулями, наоборот
    // к творогу, — порядок величин не признак, признак это подпись.
    want: { kcal: 120.9, protein: 22.8, fat: 3.3, carbs: null, sugar: null, package_weight_g: 430 },
  },
  {
    name: "йогурт",
    files: ["scripts/fixtures/label-teos-1.jpg", "scripts/fixtures/label-teos-2.jpg"],
    nameKeys: ["йогурт", "teos", "теос"],
    // Рядом с таблицей на 100 г стоит врезка «на 140 г: 93,5 ккал» — калорийность
    // ПОРЦИИ, а не ста граммов. На крышке маркетинговое «11 г белка». Калории в
    // таблице спрятаны в скобках после килоджоулей.
    want: { kcal: 66.8, protein: 8.0, fat: 2.0, carbs: 4.2, sugar: null, package_weight_g: 140 },
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
  `Ты — nutrition vision assistant. На фотографиях — еда, которую съел человек: снимки упаковки с ` +
  `этикеткой, снимки самого продукта, снимки тарелки с готовой едой — или всё сразу.\n\n` +
  `Собери список еды. Несколько фотографий могут показывать ОДИН И ТОТ ЖЕ продукт с разных сторон или ` +
  `с разным приближением — это ОДНА позиция. Считай еду дважды только если это действительно разная еда.\n\n` +
  `Заполняй поля ПО ПОРЯДКУ: сначала то, что видишь, затем то, что из увиденного следует. Заполняй ` +
  `только прочитанное — не помнишь, не разобрал, обрезано краем кадра: оставь null. Не бери числа с ` +
  `лицевой стороны упаковки («много белка, 11 г») — это реклама, а не таблица. И никогда не бери числа ` +
  `из примеров в описаниях полей.`;

/// Вопрос ВТОРОЙ, отдельным заходом: сколько еды в упаковке. Спрашивается ЦИТАТА —
/// процитировать несуществующую надпись труднее, чем назвать правдоподобное число,
/// а по ТЗ §6.2 эта масса означает «съедена вся пачка», и выдуманная масса стала бы
/// выдуманным весом съеденного.
const PROMPT_MASS =
  `На фотографиях — упаковка продукта. Один вопрос: сколько ЕДЫ в этой упаковке.\n\n` +
  `Заполняй поля ПО ПОРЯДКУ: сначала выпиши надпись, затем определи, чем она является, и лишь потом ` +
  `назови число. Не выводи количество из типичного размера такой упаковки: по этой массе мы считаем, ` +
  `что человек съел всю пачку, и выдуманная масса станет выдуманным весом съеденного.`;

const NUM_OR_NULL = { type: ["number", "null"] };
const STR_OR_NULL = { type: ["string", "null"] };

/// Порядок полей — рабочий инструмент, а не оформление.
///
/// Схема уезжает модели ТЕКСТОМ, и она заполняет поля сверху вниз, опираясь на уже
/// написанное. Поэтому сначала идёт то, что модель ВИДИТ (что на кадрах, дословная
/// строка), и только потом то, что из увиденного СЛЕДУЕТ (название, числа). Стой
/// «kcal» выше выписки — модель называла бы число раньше, чем прочла строку, и
/// выписка превращалась бы в оправдание задним числом.
///
/// Описания полей — тоже инструкция модели, а не комментарий для людей: в проекте
/// это записано прямо (`ai.rs:1405`, «`///` на полях уезжает В ПРОМПТ»). Порядок
/// здесь стоил кому-то точности: правка ради гема молча испортила глобулу,
/// 20/20 → 17/20 (`ai.rs:1811`).
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "PhotoItems",
  type: "object",
  properties: {
    items: {
      description: "Еда, которую человек съел. Один продукт, снятый с разных сторон, — ОДНА позиция. " +
        "Состав на упаковке («Состав: сливки, закваска») — это НЕ отдельные позиции: там перечислено, " +
        "из чего сделан продукт, а съеден он один.",
      type: "array",
      items: {
        type: "object",
        properties: {
          what_the_photos_show: {
            description: "Что именно на каждом кадре: этикетка упаковки, продукт без этикетки или " +
              "тарелка с готовой едой. Одной фразой.",
            type: "string",
          },
          nutrition_line_verbatim: {
            description: "ДОСЛОВНАЯ строка пищевой ценности со снимка, целиком, со всеми подписями и " +
              "единицами, как напечатана. Строка бывает разорвана между кадрами — собери её из всех. " +
              "Текст может лежать на боку. Строки не видно — null.",
            ...STR_OR_NULL,
          },
          energy_verbatim: {
            description: "Как записана энергия, ЦЕЛИКОМ и обе величины, как напечатано: " +
              "«281,4 кДж (66,8 ккал)» или «120,9ккал/509,7кДж». Не видно — null.",
            ...STR_OR_NULL,
          },
          food_name: {
            description: "Короткое каноническое название продукта по-русски. ЖЁСТКИЙ ПРЕДЕЛ: не больше " +
              "трёх слов. Оставь суть и определяющую цифру жирности, отбрось описательное.",
            type: "string",
          },
          kcal_per_100g: {
            description: "Из energy_verbatim: число, при котором стоит «ккал». Не то, при котором «кДж» " +
              "— оно примерно вчетверо больше. Если рядом есть врезка на порцию («на N г: X ккал»), она " +
              "сюда НЕ идёт: здесь только сто граммов.",
            ...NUM_OR_NULL,
          },
          protein_per_100g: {
            description: "Из nutrition_line_verbatim: число при подписи «белки» или «белок».",
            ...NUM_OR_NULL,
          },
          fat_per_100g: {
            description: "Из nutrition_line_verbatim: число при подписи «жиры» или «жир». У РАЗНЫХ " +
              "подписей числа РАЗНЫЕ — не повторяй сюда значение белков. Если в скобках указан жир " +
              "части продукта («в творожной части»), он сюда не идёт: нужен жир всего продукта.",
            ...NUM_OR_NULL,
          },
          carbs_per_100g: {
            description: "Из nutrition_line_verbatim: число при подписи «углеводы». У мяса и рыбы такой " +
              "строки часто нет вовсе — тогда null.",
            ...NUM_OR_NULL,
          },
          sugar_per_100g: {
            description: "Отдельная запись про сахар, если она есть. Часто спрятана В СКОБКАХ внутри " +
              "строки углеводов и названа «сахароза». Такой записи нет — null; значение углеводов сюда " +
              "не переписывай.",
            ...NUM_OR_NULL,
          },
          fiber_per_100g: {
            description: "Клетчатка, если у неё есть своя строка на упаковке; иначе null.",
            ...NUM_OR_NULL,
          },
          saturated_fat_per_100g: {
            description: "Насыщенные жиры, если у них есть своя строка на упаковке; иначе null.",
            ...NUM_OR_NULL,
          },
        },
        required: ["what_the_photos_show", "nutrition_line_verbatim", "energy_verbatim", "food_name",
                   "kcal_per_100g", "protein_per_100g", "fat_per_100g", "carbs_per_100g",
                   "sugar_per_100g", "fiber_per_100g", "saturated_fat_per_100g"],
      },
    },
  },
  required: ["items"],
};

/// Тот же приём: сперва выписка надписи, затем чем эта надпись является, и лишь
/// потом число. Раньше здесь первым стоял ответ, а цитата последней — то есть
/// модель называла массу, а потом сочиняла, откуда её взяла.
const MASS_SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "PackageMass",
  type: "object",
  properties: {
    mass_label_verbatim: {
      description: "ДОСЛОВНАЯ надпись со снимка, объявляющая количество продукта: «МАССА НЕТТО: 250 г», " +
        "«Масса нетто 0,430 кг», «на 140 г: 93,5 ккал». Процитировать нечего — null.",
      ...STR_OR_NULL,
    },
    what_this_label_declares: {
      description: "Чем является выписанная надпись. net — масса всего содержимого упаковки; drained — " +
        "масса продукта БЕЗ жидкости («помещённой в жидкую среду», «сухой остаток»); portion — величина " +
        "для порции или части содержимого, а не для всей пачки; pack_energy — врезка с калорийностью " +
        "всей пачки «на N г: X ккал», где N и есть масса пачки; none — надписи нет.",
      type: "string",
      enum: ["net", "drained", "portion", "pack_energy", "none"],
    },
    grams_of_food_in_package: {
      description: "Сколько ГРАММОВ еды в упаковке — следует из двух предыдущих полей. Если на упаковке " +
        "указаны ДВЕ массы, бери меньшую — ту, что БЕЗ жидкости: рассол и сироп не едят, поэтому " +
        "«масса пищевой продукции, помещённой в жидкую среду, 180 г» важнее, чем «масса нетто 290 г». " +
        "Килограммы переведи в граммы. Числа из таблицы пищевой ценности сюда не идут: «на 100 г» — " +
        "способ подачи цифр, а не размер пачки. Надписи нет — null.",
      ...NUM_OR_NULL,
    },
  },
  required: ["mass_label_verbatim", "what_this_label_declares", "grams_of_food_in_package"],
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

/// Схема ТЕКСТОМ в промпт.
///
/// На on-prem её больше передать нечем: поллер шлёт в llama-swap только `model`,
/// `temperature`, `stream` и `messages` — поля `response_format` там нет. Так что
/// описания и порядок полей доезжают до модели единственным способом — текстом.
///
/// Это не костыль ради очереди: ai-worker поступает так же и на своём пути
/// (`cloudflare/ai-worker/src/lib.rs:534`) — шлёт схему ДВАЖДЫ, полем и текстом,
/// потому что одного поля стороннему провайдеру не хватает. Формулировка взята
/// оттуда же, из `thirdparty_json_instruction`.
const schemaInstruction = (schema) => {
  const { $schema, title, ...rest } = schema;
  return `\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). ` +
    `Respond with ONE object, never an array of objects. ` +
    `The JSON MUST conform to this exact schema:\n${JSON.stringify(rest)}`;
};

/// Вытащить JSON из сырого текста модели: снять заборы кода и взять внешний
/// контейнер. Ровно то же делает `ai::extract_json_value` — на очереди разбор
/// клиентский, потому что поллер отдаёт сырой ответ как есть.
function parseRaw(raw) {
  const cleaned = raw.trim().replace(/^```(json)?/, "").replace(/```$/, "").trim();
  const o = cleaned.indexOf("{"), a = cleaned.indexOf("[");
  const start = a > -1 && (o === -1 || a < o) ? a : o;
  if (start === -1) throw new Error(`в ответе нет JSON: ${cleaned.slice(0, 120)}`);
  const close = cleaned[start] === "[" ? "]" : "}";
  const end = cleaned.lastIndexOf(close);
  return JSON.parse(cleaned.slice(start, end + 1));
}

/// ON-PREM: задание в очередь, дальше опрос до готовности. Поллер запускает промпт
/// на своём Qwen2.5-VL и возвращает СЫРОЙ текст — ни схемы, ни разбора, ни проверок
/// на его стороне нет (`services/ocr-poller/poller.py`).
async function askQueue(token, images, prompt, schema) {
  const submit = await fetch(`${OCR}/submit`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ images, prompt: prompt + schemaInstruction(schema) }),
  });
  if (!submit.ok) throw new Error(`submit HTTP ${submit.status}: ${(await submit.text()).slice(0, 160)}`);
  const { job_id: jobId } = await submit.json();

  // Своя очередь и своя видеокарта: 32b по нескольким кадрам думает долго.
  for (let i = 0; i < 200; i++) {
    await new Promise((r) => setTimeout(r, 1500));
    const res = await fetch(`${OCR}/job/${jobId}`, { headers: { Authorization: `Bearer ${token}` } });
    if (!res.ok) continue;
    const job = await res.json();
    if (job.status === "done") return parseRaw(job.result || "");
    if (job.status === "error") throw new Error(`очередь: ${job.error || "recognition failed"}`);
  }
  throw new Error("очередь не ответила за пять минут");
}

/// ПРЯМОЙ путь через ai-worker — за деньги, поэтому только по флагу `--route direct`.
/// Здесь схема уходит и полем, и текстом: воркер сам добавит её в промпт.
async function askDirect(token, images, prompt, schema) {
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
  return parseRaw(content);
}

const ask = (token, images, prompt, schema) =>
  ROUTE === "direct" ? askDirect(token, images, prompt, schema) : askQueue(token, images, prompt, schema);

/// Сходится ли прочитанное с самим собой: 4 ккал на грамм белка и углеводов, 9 на
/// грамм жира. Расхождение НЕ говорит, какое из чисел неверно, — оно говорит, что
/// чтению нельзя верить. Замер это показал дважды: на уменьшенном снимке модель
/// теряла калорийность (подставляя килоджоули и код партии) при верных макросах, а
/// на крупном читала калорийность верно, зато путала жиры с белками. Латать тут
/// нечего — такой ответ отвергается, и модель отвечает заново, как в
/// `ai::generate_validated`.
///
/// Порог выбран по замеру, а не на глаз. Законное расхождение — округления
/// производителя: у оливок восемь процентов при верно прочитанной строке. Ложное
/// чтение сырка на своей модели дало девятнадцать и при прежнем пороге в двадцать
/// процентов проскочило как верное. Двенадцать разделяют эти два случая.
const KCAL_TOLERANCE = 0.12;

function kcalDisagreement(item) {
  const { kcal } = item;
  if (kcal === null) return null;
  // Ненапечатанная строка — это ноль: у мяса и рыбы углеводы не печатают вовсе, и
  // без такого допущения проверка на них молчала бы.
  const [p, f, c] = [item.protein, item.fat, item.carbs].map((v) => v ?? 0);
  const computed = p * 4 + f * 9 + c * 4;
  if (computed <= 0) return null;
  const off = Math.abs(kcal - computed) / computed;
  return off > KCAL_TOLERANCE ? { computed: Math.round(computed * 10) / 10, off } : null;
}

/// Какие виды надписи годятся в «сколько съедено, если съедена вся пачка».
///
/// Вид называет сама модель — она читает упаковку и знает, что там написано.
/// Кодом решается только политика: чему верить. Раньше здесь стояла регулярка по
/// словам «масса|нетто|вес», и она ровно этим и была плоха — угадывала смысл
/// надписи по подстроке. Стоило расширить шаблон ради врезки «на 140 г», как под
/// него подошло «на порцию 20 г продукта» с другой этикетки, и масса пачки стала
/// двадцатью граммами.
///
/// `drained` важнее `net`: рассол не едят. `portion` отбрасывается — это часть
/// содержимого, а не пачка.
const MASS_KINDS = { drained: 3, net: 2, pack_energy: 1, portion: 0, none: 0 };

function acceptMass(answer) {
  const kind = answer.what_this_label_declares;
  const grams = answer.grams_of_food_in_package ?? null;
  const text = answer.mass_label_verbatim ?? null;
  if ((MASS_KINDS[kind] ?? 0) === 0 || grams === null) {
    return { grams: null, text: null, rejected: kind === "none" ? null : `${kind}: ${text}` };
  }
  return { grams, text };
}

const near = (got, want) =>
  want === null ? got === null : got !== null && Math.abs(got - want) <= Math.max(0.15, want * 0.02);

// Имена полей ответа → короткие подписи отчёта.
const FIELDS = ["kcal", "protein", "fat", "carbs", "sugar", "package_weight_g"];
const FROM_ANSWER = {
  kcal: "kcal_per_100g", protein: "protein_per_100g", fat: "fat_per_100g",
  carbs: "carbs_per_100g", sugar: "sugar_per_100g",
};

/// Ответ модели → плоская запись, которой оперируют проверки и отчёт.
function flatten(it) {
  const out = { name: it.food_name ?? "", nutrition_text: it.nutrition_line_verbatim ?? null };
  for (const [short, full] of Object.entries(FROM_ANSWER)) out[short] = it[full] ?? null;
  return out;
}
const SHORT = { kcal: "ккал", protein: "Б", fat: "Ж", carbs: "У", sugar: "сахар", package_weight_g: "нетто" };

async function runCase(token, c) {
  const files = SINGLE ? [c.files[Number(SINGLE) - 1]] : c.files;
  const images = files.map((f) => readFileSync(f).toString("base64"));
  const ref = FIELDS.map((k) => `${SHORT[k]} ${c.want[k] === null ? "—" : c.want[k]}`).join(", ");
  console.log(`\n[${c.name}] снимков ${images.length}, эталон на 100 г: ${ref}`);

  let okCount = 0, okName = 0, okAll = 0, refused = 0;
  for (let i = 0; i < N; i++) {
    let items, mass;
    try {
      // До трёх попыток: ответ, не сходящийся сам с собой, не показывают человеку.
      let bad = null;
      for (let attempt = 0; attempt < 3; attempt++) {
        const answer = await ask(token, images, PROMPT_TABLE, SCHEMA);
        if (RAW) console.log(JSON.stringify(answer, null, 2));
        items = (answer.items || []).map(flatten);
        bad = items.map(kcalDisagreement).find(Boolean);
        if (!bad) break;
        const it = items[0];
        console.log(`  прогон ${i + 1}: попытка ${attempt + 1} отвергнута — ккал ${it.kcal} против ` +
          `${bad.computed} по макросам (${(bad.off * 100).toFixed(0)}%); прочитано Б=${it.protein} ` +
          `Ж=${it.fat} У=${it.carbs}`);
      }
      // Попытки кончились, а ответ так и не сошёлся сам с собой: этикетка признаётся
      // НЕПРОЧИТАННОЙ. Позиция идёт дальше без цифр с упаковки — КБЖУ ей подберут по
      // названию, как любой еде без этикетки. Числа, за которые никто не отвечает, в
      // дневник не попадают.
      if (bad) {
        for (const it of items) {
          it.kcal = it.protein = it.fat = it.carbs = it.sugar = null;
          it.label_unreadable = true;
        }
      }
      mass = acceptMass(await ask(token, images, PROMPT_MASS, MASS_SCHEMA));
      if (mass.rejected) console.log(`  прогон ${i + 1}: масса отклонена — ${mass.rejected.slice(0, 90)}`);
    } catch (e) { console.log(`  прогон ${i + 1}: сбой — ${e.message}`); continue; }
    // Масса — ответ отдельного вопроса, кладём её в позицию для сверки.
    for (const it of items) { it.package_weight_g = mass.grams ?? null; it.package_weight_text = mass.text ?? null; }

    if (items.length === 1) okCount++;
    for (const it of items) {
      const shown = it.label_unreadable
        ? `этикетка не прочитана, КБЖУ подберут по названию (нетто ${it.package_weight_g ?? "—"})`
        : FIELDS
            .map((k) => `${SHORT[k]}=${it[k] === null ? "—" : it[k]}${near(it[k], c.want[k]) ? "" : " ✗"}`)
            .join(" ");
      const quote = it.package_weight_text ? `  ← «${it.package_weight_text}»` : "";
      console.log(`  прогон ${i + 1}: [${items.length}] «${it.name}»  ${shown}${quote}`);
    }
    if (items.length === 1) {
      const it = items[0];
      if (c.nameKeys.some((k) => it.name.toLowerCase().includes(k))) okName++;
      if (it.label_unreadable) refused++;
      else if (FIELDS.every((k) => near(it[k], c.want[k]))) okAll++;
    }
  }
  const wrong = N - okAll - refused;
  console.log(`  → одна позиция ${okCount}/${N}, название ${okName}/${N}, ` +
    `все поля ${okAll}/${N}` +
    (refused ? `, честный отказ ${refused}/${N}` : "") +
    (wrong ? `, НЕВЕРНО ${wrong}/${N}` : ""));
  return { okCount, okName, okAll, refused };
}

async function main() {
  const token = await mintToken();
  const only = arg("case", null);
  const cases = only ? CASES.filter((c) => c.name === only) : CASES;
  if (!cases.length) { console.error(`нет кейса «${only}»`); process.exit(1); }
  console.log(ROUTE === "direct"
    ? `прямой путь, модель ${MODEL}, повторов ${N}`
    : `свой сервер (ocr-queue → Qwen2.5-VL), повторов ${N}`);
  for (const c of cases) await runCase(token, c);
}

main().catch((e) => { console.error(e); process.exit(1); });
