// ВТОРОЙ ПРОХОД ленивого разбора: из разобранных кадров и описания — список еды.
//
// Первый проход смотрит на каждый снимок ПО ОТДЕЛЬНОСТИ и потому нарочно не знает
// целого: на одном кадре название, на другом таблица, на третьем масса нетто, и
// каждый честно молчит про чужое. Свести их в то, что человек собирается съесть,
// — работа этого прохода, и она уже текстовая: картинок здесь нет, есть разбор
// кадров из scripts/fixtures/per-image.json и описание пользователя.
//
// На выходе плоский список: название, граммы и КБЖУ на 100 г, если их удалось
// прочитать. Никаких обёрток и вложенности — ровно то, что попадёт в дневник.
//
// Граммы берутся по правилам docs/lazy-diary-entry.md §6.2, в порядке убывания
// приоритета: сказанное пользователем → масса нетто с упаковки (считаем, что
// съедена вся пачка) → оценка по фотографии еды → 100 г.
//
//   node scripts/measure-merge-items.mjs [--model МОДЕЛЬ] [--n ПОВТОРОВ] [--raw]

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "@cf/qwen/qwen3-30b-a3b-fp8");
const N = Number(arg("n", 3));
const RAW = process.argv.includes("--raw");
const JOBS = 3;

const PASS1 = JSON.parse(readFileSync("scripts/fixtures/per-image.json", "utf8"));

/// Расшифровка кадра бывает длиной в двадцать тысяч знаков: на нечитаемом мелком
/// шрифте модель первого прохода уходит в повтор и гоняет одну строку до конца
/// бюджета. Целиком такое слать некуда, а обрывать жалко — в хвосте попадается
/// нужное (у оливок там масса без жидкости). Режем по длине честного текста.
const TEXT_CAP = 1500;

/// Кадр — читаемым блоком, а не сырым JSON: разбирает это текстовая модель, и
/// подписанные по-русски строки она понимает лучше, чем ключи со змеиным именем.
function frameBlock(name, r) {
  const lines = [`КАДР ${name}: ${r.what_is_on_the_photo}`];
  if (r.photo_kind === "food") {
    const list = (r.foods_on_the_photo || [])
      .map((i) => `${i.name}${i.grams == null ? " (граммы не оценены)" : ` — ${i.grams} г`}`);
    lines.push(`  на снимке еда: ${list.length ? list.join("; ") : "ничего не распознано"}`);
    return lines.join("\n");
  }
  const put = (label, v) => { if (v !== null && v !== undefined && v !== "") lines.push(`  ${label}: ${v}`); };
  put("название на упаковке", r.product_name_printed);
  put("масса нетто, г", r.netto_weight_printed_g);
  put("строка энергии", r.energy_verbatim);
  put("строка белков-жиров-углеводов", r.nutrition_line_verbatim);
  const kbju = [
    ["ккал", r.kcal_per_100g_printed], ["белки", r.protein_per_100g_printed],
    ["жиры", r.fat_per_100g_printed], ["углеводы", r.carbs_per_100g_printed],
  ].filter(([, v]) => v !== null && v !== undefined).map(([k, v]) => `${k} ${v}`);
  if (kbju.length) lines.push(`  прочитано на 100 г: ${kbju.join(", ")}`);
  const t = (r.all_text_verbatim || "").replace(/\s+/g, " ").trim();
  if (t) lines.push(`  текст с упаковки: ${t.slice(0, TEXT_CAP)}${t.length > TEXT_CAP ? " …(обрезано)" : ""}`);
  return lines.join("\n");
}

const PROMPT = (frames, description) =>
  `Человек записывает еду в дневник питания. Он приложил снимки и, может быть, описал словами, что съел. ` +
  `Снимки уже разобраны по отдельности — ниже разбор каждого. Собери из всего этого список того, что ` +
  `человек собирается съесть.\n\n` +
  `ГЛАВНОЕ. Кадров может быть несколько НА ОДИН продукт: название снято отдельно, таблица отдельно, масса ` +
  `отдельно. Такие кадры — ОДНА позиция списка, а не три. Разбирали их порознь именно потому, что по ` +
  `одному кадру целого не видно; свести их — твоя работа. И наоборот: разные продукты не сливай в один.\n\n` +
  (frames.length
    ? `РАЗБОР КАДРОВ\n\n${frames.join("\n\n")}\n\n`
    : `Снимков нет.\n\n`) +
  (description
    ? `ОПИСАНИЕ ОТ ЧЕЛОВЕКА\n«${description}»\n\n`
    : `Описания человек не оставил.\n\n`) +
  `Всё, что человек назвал в описании, — тоже позиции списка, даже если упаковки и снимка для этого нет: ` +
  `ложка мёда рядом с творогом это вторая позиция, а не примечание к первой.\n\n` +
  `Если кадры одного продукта дают РАЗНЫЕ числа — выбери одно, то, которое прочитано увереннее (полная ` +
  `строка таблицы надёжнее обрывка). Среднее между ними брать НЕЛЬЗЯ: получится число, которого нет ни ` +
  `на упаковке, ни в жизни.\n\n` +
  `Позиция списка — то, что человек кладёт в рот, а не упаковка и не блюдо целиком. Обобщающей позиции ` +
  `вроде «обед» или «продукты» быть не должно.`;

const STR_OR_NULL = { type: ["string", "null"] };
const NUM_OR_NULL = { type: ["number", "null"] };

/// Рассуждение о группировке идёт первым полем: сначала модель вслух решает, какие
/// кадры про один продукт, и только потом собирает список. Список первым — и она
/// начинает выписывать позиции покадрово, по позиции на снимок.
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "MealItems",
  type: "object",
  properties: {
    how_frames_group: {
      description: "ПО-РУССКИ, одним-двумя предложениями: какие кадры относятся к одному и тому же " +
        "продукту и почему; если кадров нет — почему список получился таким из описания.",
      type: "string",
    },
    items: {
      description: "Что человек собирается съесть, каждая позиция отдельно.",
      type: "array",
      items: {
        type: "object",
        properties: {
          name: {
            description: "Название продукта по-русски. Если оно напечатано на упаковке — бери " +
              "напечатанное, вместе с видом продукта (не «Картошка», а «Десерт «Картошка»»). Если " +
              "название пришло только из описания — бери слова человека.",
            type: "string",
          },
          from_frames: {
            description: "Имена кадров, из которых собрана эта позиция, через запятую. Позиция " +
              "целиком из описания — null.",
            ...STR_OR_NULL,
          },
          where_grams_came_from: {
            description: "Откуда берутся граммы — решается ДО самих граммов, по строгому старшинству. " +
              "\"user_text\" — человек назвал количество в описании; это старше всего остального, даже " +
              "если на упаковке написано другое. \"package_netto\" — количество не названо, но на " +
              "упаковке есть масса нетто: считаем, что съедена вся пачка. \"photo_estimate\" — " +
              "количество видно по снимку еды. \"default_100\" — узнать неоткуда, и тогда ровно 100.",
            type: "string",
            enum: ["user_text", "package_netto", "photo_estimate", "default_100"],
          },
          grams: {
            description: "Сколько граммов этого человек съест, по источнику из предыдущего поля. " +
              "Меры вроде ложки, штуки или половины пачки переведи в граммы сам.",
            type: "number",
          },
          kcal_per_100g: {
            description: "Калорийность на 100 г, ТОЛЬКО если она прочитана с упаковки в разборе " +
              "кадров. По памяти не восстанавливай: нутриенты подберём отдельным шагом. Нет — null.",
            ...NUM_OR_NULL,
          },
          protein_per_100g: { description: "Белки на 100 г с упаковки; нет — null.", ...NUM_OR_NULL },
          fat_per_100g: { description: "Жиры на 100 г с упаковки; нет — null.", ...NUM_OR_NULL },
          carbs_per_100g: { description: "Углеводы на 100 г с упаковки; нет — null.", ...NUM_OR_NULL },
        },
        required: ["name", "from_frames", "where_grams_came_from", "grams",
                   "kcal_per_100g", "protein_per_100g", "fat_per_100g", "carbs_per_100g"],
      },
    },
  },
  required: ["how_frames_group", "items"],
};

// ── случаи ──
//
// Кадры называются ключами из per-image.json, то есть на вход идёт НАСТОЯЩИЙ выход
// первого прохода со всеми его огрехами (у десерта «Картошка» название прочиталось
// как «Картюшка») — мерить надо на том, что проход правда отдаёт.
//
// `grams` в ожидании: число — с точностью до грамма, пара — допустимый разброс.
const CASES = [
  {
    what: "три кадра одного десерта, описания нет",
    frames: ["label-kartoshka-1.jpg", "label-kartoshka-2.jpg", "label-kartoshka-3.jpg"],
    description: "",
    want: [{ key: ["картош", "картюш", "картон", "десерт"], grams: 60, source: "package_netto",
             kcal: 377.7, protein: 5.9, fat: 16.5, carbs: 51.4 }],
  },
  {
    what: "тот же десерт, но человек съел половину",
    frames: ["label-kartoshka-1.jpg", "label-kartoshka-2.jpg", "label-kartoshka-3.jpg"],
    description: "съел половину десерта",
    want: [{ key: ["картош", "картюш", "картон", "десерт"], grams: 30, source: "user_text" }],
  },
  {
    what: "одна пачка творога, описания нет",
    frames: ["label-tvorog.jpg"],
    description: "",
    want: [{ key: ["творог"], grams: 250, source: "package_netto",
             kcal: 96, protein: 18, fat: 1.2 }],
  },
  {
    what: "два кадра йогурта: лицо с названием и оборот с таблицей",
    frames: ["label-teos-1.jpg", "label-teos-2.jpg"],
    description: "",
    want: [{ key: ["йогурт", "teos", "теос"], grams: 140, source: "package_netto",
             kcal: 66.8, protein: 8, fat: 2, carbs: 4.2 }],
  },
  {
    what: "курица, человек съел половину пачки",
    frames: ["label-chicken.jpg"],
    description: "съел половину пачки",
    want: [{ key: ["куриц", "цыпл", "филе"], grams: 215, source: "user_text",
             kcal: 120.9, protein: 22.8, fat: 3.3 }],
  },
  {
    what: "сметана двумя кадрами, человек назвал ложки",
    frames: ["label-smetana-1.jpg", "label-smetana-2.jpg"],
    description: "две столовые ложки сметаны",
    want: [{ key: ["сметан"], grams: [30, 60], source: "user_text", kcal: 160 }],
  },
  {
    what: "две РАЗНЫЕ пачки сразу — сливать их нельзя",
    frames: ["label-tvorog.jpg", "label-chicken.jpg"],
    description: "",
    want: [
      { key: ["творог"], grams: 250, source: "package_netto" },
      { key: ["куриц", "цыпл", "филе"], grams: 430, source: "package_netto" },
    ],
  },
  {
    what: "оливки двумя кадрами: на упаковке и нетто 290, и масса без жидкости 180",
    frames: ["label-olives-1.jpg", "label-olives-2.jpg"],
    description: "",
    want: [{ key: ["оливк"], grams: [180, 290], source: "package_netto",
             kcal: 149, protein: 1.6, fat: 14.6 }],
  },
  {
    what: "снимков нет вовсе, только описание",
    frames: [],
    description: "тарелка гречки с котлетой и салат из огурцов",
    want: [
      { key: ["гречк"], grams: [100, 250] },
      { key: ["котлет"], grams: [80, 150] },
      { key: ["огур"], grams: [80, 200] },
    ],
  },
  {
    what: "пачка творога и к ней описание про добавку без упаковки",
    frames: ["label-tvorog.jpg"],
    description: "творог с ложкой мёда",
    want: [
      { key: ["творог"], grams: 250, source: "package_netto" },
      { key: ["мёд", "мед"], grams: [10, 40], source: "user_text" },
    ],
  },
];

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `merge-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function ask(token, prompt) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: prompt }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 3000,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 200)}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const p = line.slice(6).trim();
    if (p === "[DONE]") continue;
    try { content += JSON.parse(p).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
}

/// Позицию ответа ищем ВХОЖДЕНИЕМ корня, а не равенством строк: иначе мерили бы
/// совпадение с моей орфографией, а не работу модели.
const findItem = (items, keys) =>
  items.find((i) => keys.some((k) => String(i.name || "").toLowerCase().includes(k)));

const gramsOk = (got, want) => {
  if (want === undefined) return true;
  if (Array.isArray(want)) return got != null && got >= want[0] && got <= want[1];
  return got != null && Math.abs(got - want) < 1;
};
/// КБЖУ сверяем с допуском в процент: на упаковке печатают округлённое, и спор о
/// десятой доле нам ничего не говорит.
const numOk = (got, want) => want === undefined || (got != null && Math.abs(got - want) <= Math.max(0.1, want * 0.01));

async function main() {
  const token = await mintToken();
  console.log(`модель ${MODEL}, повторов ${N}\n`);

  const queue = CASES.map((c, idx) => ({ ...c, idx }));
  const rows = [];
  const worker = async () => {
    for (;;) {
      const c = queue.shift();
      if (!c) return;
      const frames = c.frames.map((f) => {
        if (!PASS1[f]) throw new Error(`в per-image.json нет кадра ${f}`);
        return frameBlock(f, PASS1[f]);
      });
      const prompt = PROMPT(frames, c.description);
      const runs = [];
      let failed = 0;
      for (let i = 0; i < N; i++) {
        try { runs.push(await ask(token, prompt)); } catch (e) { failed++; if (RAW) console.log(e.message); }
      }
      rows.push({ ...c, runs, failed });
    }
  };
  await Promise.all(Array.from({ length: JOBS }, worker));
  rows.sort((a, b) => a.idx - b.idx);

  let miss = 0, extra = 0, badGrams = 0, badSrc = 0, badKbju = 0, total = 0;
  for (const row of rows) {
    console.log(`— ${row.what}${row.failed ? `   (сбоев ${row.failed})` : ""}`);
    for (const ans of row.runs) {
      total++;
      const items = ans.items || [];
      const got = items.map((i) => `${i.name} ${i.grams} г [${i.where_grams_came_from}]`).join(" | ");
      const missing = row.want.filter((w) => !findItem(items, w.key)).map((w) => w.key[0]);
      const matched = new Set(row.want.map((w) => findItem(items, w.key)).filter(Boolean));
      const extras = items.filter((i) => !matched.has(i));
      const gWrong = [], sWrong = [], kWrong = [];
      for (const w of row.want) {
        const it = findItem(items, w.key);
        if (!it) continue;
        if (!gramsOk(it.grams, w.grams)) gWrong.push(`${it.name}=${it.grams}г`);
        if (w.source && it.where_grams_came_from !== w.source) sWrong.push(`${it.name}:${it.where_grams_came_from}≠${w.source}`);
        const bad = [["ккал", it.kcal_per_100g, w.kcal], ["Б", it.protein_per_100g, w.protein],
                     ["Ж", it.fat_per_100g, w.fat], ["У", it.carbs_per_100g, w.carbs]]
          .filter(([, g, wv]) => !numOk(g, wv)).map(([n, g, wv]) => `${n} ${g}≠${wv}`);
        if (bad.length) kWrong.push(`${it.name}: ${bad.join(", ")}`);
      }
      miss += missing.length; extra += extras.length;
      badGrams += gWrong.length; badSrc += sWrong.length; badKbju += kWrong.length;
      console.log(`   ${got || "(пусто)"}`);
      if (missing.length) console.log(`     не найдено: ${missing.join(", ")}`);
      if (extras.length) console.log(`     лишнее: ${extras.map((i) => i.name).join(", ")}`);
      if (gWrong.length) console.log(`     граммы: ${gWrong.join(", ")}`);
      if (sWrong.length) console.log(`     источник: ${sWrong.join(", ")}`);
      if (kWrong.length) console.log(`     КБЖУ: ${kWrong.join("; ")}`);
      if (RAW) console.log(`     группировка: ${ans.how_frames_group}`);
    }
    console.log();
  }
  console.log(`итого за ${total} прогонов: пропущено ${miss}, лишних ${extra}, ` +
              `граммы ${badGrams}, источник ${badSrc}, КБЖУ ${badKbju}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
