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
  `Оставь суть и определяющую цифру жирности с упаковки, отбрось описательное: из «СЫРОК ТВОРОЖНЫЙ ` +
  `ГЛАЗИРОВАННЫЙ С АРОМАТОМ ВАНИЛИ, МАССОВАЯ ДОЛЯ ЖИРА N%» получается «глазированный сырок N%».\n` +
  `- nutrition_text — СНАЧАЛА выпиши ДОСЛОВНО строку пищевой ценности со снимка, целиком, со всеми ` +
  `подписями и единицами, как она напечатана. Собери её из нескольких кадров, если она разорвана. Числа ` +
  `ниже бери ТОЛЬКО из этой выписанной строки — не с других мест этикетки (жирность под датой, код ` +
  `партии, срок годности) и НИКОГДА не из примеров этой инструкции.\n` +
  `- kcal, protein, fat, carbs — на 100 г, ровно как в выписанной строке: не пересчитывай и не округляй. ` +
  `Держи подписи при числах: то, что стоит после «белки», — это protein, после «жиры» — fat, после ` +
  `«углеводы» — carbs, и перепутать их нельзя.\n` +
  `- Энергия обычно записана ДВУМЯ величинами подряд, в любом порядке: «X кДж / Y ккал» или «Y ккал/X кДж». ` +
  `Выбирай по ПОДПИСИ, а не по месту: нужно число, у которого стоит «ккал». Оно всегда примерно вчетверо ` +
  `МЕНЬШЕ числа при «кДж». Число при «кДж» не бери никогда.\n` +
  `- Строки может не быть вовсе: у мяса, рыбы и масла углеводы часто не печатают. Нет строки — null, ` +
  `выдумывать нечего.\n` +

  `- sugar — только если на упаковке есть отдельная запись про сахар. Часто она спрятана В СКОБКАХ ` +
  `внутри строки углеводов и названа «сахароза»: «углеводы – X г (в т.ч. сахароза – Y г)» → carbs = X, ` +
  `sugar = Y. Такой записи нет — sugar: null, и НИКОГДА не переписывай в сахар значение углеводов.\n` +
  `- fiber, saturated_fat — так же: только из своей строки на упаковке, иначе null.\n` +
  `- grams — сколько человек съел, если это видно (порция на тарелке); для снимка упаковки null.\n` +
  `- confidence — 0..1, насколько ты уверен в позиции.\n\n` +
  `Верни ТОЛЬКО JSON, без прозы.`;

/// Вопрос ВТОРОЙ, отдельным заходом: сколько еды в упаковке. Спрашивается ЦИТАТА —
/// процитировать несуществующую надпись труднее, чем назвать правдоподобное число,
/// а по ТЗ §6.2 эта масса означает «съедена вся пачка», и выдуманная масса стала бы
/// выдуманным весом съеденного.
const PROMPT_MASS =
  `На фотографиях — упаковка продукта. Один вопрос: сколько ЕДЫ в этой упаковке.\n\n` +
  `Найди на снимке надпись, объявляющую количество, и определи, ЧТО ИМЕННО она объявляет:\n` +
  `- "net" — масса всего содержимого упаковки («масса нетто»);\n` +
  `- "drained" — масса продукта БЕЗ жидкости («масса пищевой продукции, помещённой в жидкую среду», ` +
  `«масса основного продукта», «сухой остаток»);\n` +
  `- "portion" — величина, относящаяся к ПОРЦИИ или к части содержимого, а не ко всей упаковке ` +
  `(«на порцию N г», «в одной штуке N г»);\n` +
  `- "pack_energy" — врезка, объявляющая калорийность всей пачки: «на N г: X ккал». Здесь N — масса пачки;\n` +
  `- "none" — надписи о количестве на снимке нет.\n\n` +
  `Правила:\n` +
  `- Не выводи количество из типичного размера такой упаковки и не бери числа из таблицы пищевой ` +
  `ценности: «на 100 г» — это способ подачи цифр, а не размер пачки.\n` +
  `- Масса бывает в килограммах («0,430 кг») — переведи в граммы, grams всегда в граммах.\n` +
  `- Если подходящих надписей несколько, верни ту, что точнее отвечает на вопрос «сколько еды в пачке», ` +
  `и назови её вид честно.\n` +
  `- text — ДОСЛОВНАЯ надпись со снимка. Не можешь процитировать её словами с упаковки — kind: "none".\n\n` +
  `Верни ТОЛЬКО JSON: {"kind": "net", "grams": 250, "text": "МАССА НЕТТО: 250 г"}`;

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
          nutrition_text: { type: ["string", "null"] },
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
        required: ["name", "nutrition_text", "grams", "kcal", "protein", "fat", "carbs",
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
  properties: {
    kind: { type: "string", enum: ["net", "drained", "portion", "pack_energy", "none"] },
    grams: NUM_OR_NULL,
    text: { type: ["string", "null"] },
  },
  required: ["kind", "grams", "text"],
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

/// Сходится ли прочитанное с самим собой: 4 ккал на грамм белка и углеводов, 9 на
/// грамм жира. Расхождение НЕ говорит, какое из чисел неверно, — оно говорит, что
/// чтению нельзя верить. Замер это показал дважды: на уменьшенном снимке модель
/// теряла калорийность (подставляя килоджоули и код партии) при верных макросах, а
/// на крупном читала калорийность верно, зато путала жиры с белками. Латать тут
/// нечего — такой ответ отвергается, и модель отвечает заново, как в
/// `ai::generate_validated`.
///
/// Порог широкий: у производителей свои округления, у оливок расхождение восемь
/// процентов при верно прочитанной строке. Килоджоули промахиваются вчетверо и
/// отсекаются с большим запасом.
const KCAL_TOLERANCE = 0.2;

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

function acceptMass(mass) {
  const rank = MASS_KINDS[mass.kind] ?? 0;
  if (rank === 0 || mass.grams === null) {
    return { grams: null, text: null, rejected: mass.kind === "none" ? null : `${mass.kind}: ${mass.text}` };
  }
  return mass;
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

  let okCount = 0, okName = 0, okAll = 0, refused = 0;
  for (let i = 0; i < N; i++) {
    let items, mass;
    try {
      // До трёх попыток: ответ, не сходящийся сам с собой, не показывают человеку.
      let bad = null;
      for (let attempt = 0; attempt < 3; attempt++) {
        items = (await ask(token, images, PROMPT_TABLE, SCHEMA)).items || [];
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
  console.log(`модель ${MODEL}, повторов ${N}`);
  for (const c of cases) await runCase(token, c);
}

main().catch((e) => { console.error(e); process.exit(1); });
