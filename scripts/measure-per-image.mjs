// ПЕРВЫЙ ПРОХОД ленивого разбора: КАЖДЫЙ СНИМОК ПО ОТДЕЛЬНОСТИ.
//
// До сих пор все кадры уезжали в один запрос, и модель путалась: два снимка одной
// банки становились двумя продуктами, а строка таблицы склеивалась из обеих
// сторон упаковки. Здесь наоборот — по запросу на снимок, и первым делом вопрос
// «что это вообще»:
//
//   food  — на снимке ЕДА, которую человек ест. Ответ: список того, что видно.
//   label — на снимке ЭТИКЕТКА с текстом. Ответ: весь текст плюс то, что удалось
//           из него вычитать — название, масса нетто, КБЖУ на 100 г.
//
// Главное правило прохода: человек снимает ОДИН продукт двумя-тремя кадрами, и на
// каждом отдельном кадре сведений НЕ ХВАТАЕТ. Название может быть на одном кадре,
// таблица на другом, масса на третьем. Поэтому пустое поле здесь — нормальный,
// ожидаемый ответ, а не провал: сводить кадры будет следующий проход.
//
//   node scripts/measure-per-image.mjs [--route queue|direct] [--only ПОДСТРОКА] [--raw]

import { readFileSync, readdirSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const OCR = process.env.OCR || "https://ocr-queue-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const MODEL = process.env.MODEL || "qwen3-vl-plus";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const ROUTE = arg("route", "queue");
const ONLY = arg("only", null);
const RAW = process.argv.includes("--raw");

const PROMPT =
  `На фотографии — то, что человек собирается записать в дневник питания. Разбери ЭТОТ ОДИН снимок.\n\n` +
  `Сначала определи, что перед тобой: ЕДА, которую человек ест, или ЭТИКЕТКА упаковки с текстом. ` +
  `Дальше отвечай только на вопросы своего случая, поля чужого оставь пустыми.\n\n` +
  `ВАЖНО. Человек снимает ОДИН продукт двумя или тремя кадрами, и на этом отдельном кадре сведений ` +
  `почти наверняка НЕ ХВАТАЕТ: название может быть снято отдельно, таблица отдельно, масса отдельно. ` +
  `Часть надписей обрезана краем кадра или не попала в него вовсе. Это НОРМАЛЬНО. Не видно — оставь ` +
  `поле пустым, ничего не додумывай и не восстанавливай по памяти. Пустое поле здесь — правильный ` +
  `ответ, кадры сведём потом. Это касается ВСЕХ полей, включая название продукта.`;

const STR_OR_NULL = { type: ["string", "null"] };
const NUM_OR_NULL = { type: ["number", "null"] };

/// Поля идут сверху вниз так, как модель их и заполняет: сперва увиденное, затем
/// вид снимка, затем — то, что из увиденного следует.
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "OnePhoto",
  type: "object",
  properties: {
    what_is_on_the_photo: {
      description: "ПО-РУССКИ, одним предложением: что видно на снимке.",
      type: "string",
    },
    photo_kind: {
      description: "Что это, из предыдущего поля. \"food\" — еда, которую человек ест: тарелка, " +
        "порция, продукт без упаковки. \"label\" — упаковка с текстом: этикетка, состав, таблица " +
        "пищевой ценности.",
      type: "string",
      enum: ["food", "label"],
    },

    // ── если food ──
    foods_on_the_photo: {
      description: "ТОЛЬКО для \"food\": еда, которую видно на снимке, каждая позиция отдельно. " +
        "Для \"label\" — пустой список.",
      type: "array",
      items: {
        type: "object",
        properties: {
          name: {
            description: "Короткое название по-русски, 1-3 слова.",
            type: "string",
          },
          grams: {
            description: "Сколько этого на снимке, в граммах, если можно оценить по кадру; иначе null.",
            ...NUM_OR_NULL,
          },
        },
        required: ["name", "grams"],
      },
    },

    // ── если label ──
    product_name_printed: {
      description: "Название продукта, НАПЕЧАТАННОЕ на этой этикетке. Названия в кадре нет — null; " +
        "не выводи его из состава, вида упаковки или своих представлений.",
      ...STR_OR_NULL,
    },
    netto_weight_printed_g: {
      description: "Масса нетто, НАПЕЧАТАННАЯ на этой этикетке, в граммах (килограммы переведи). " +
        "Такой надписи в кадре нет — null.",
      ...NUM_OR_NULL,
    },
    nutrition_line_verbatim: {
      description: "Строка пищевой ценности с ЭТОЙ этикетки, дословно и целиком, как напечатана — " +
        "вместе с подписями и единицами. Выписывается ПЕРЕД числами, чтобы числа ниже брались из " +
        "неё, а не из памяти. Таблицы в кадре нет — null.",
      ...STR_OR_NULL,
    },
    energy_verbatim: {
      description: "Строка энергетической ценности, дословно и ЦЕЛИКОМ, с обоими числами и обеими " +
        "единицами, как напечатано. Энергия почти всегда стоит ОТДЕЛЬНОЙ строкой от белков-жиров-" +
        "углеводов, и выписывается она здесь отдельно: рядом с «ккал» всегда стоит кДж, число при " +
        "кДж примерно вчетверо больше, и без этой строки перед глазами берут не то. Нет — null.",
      ...STR_OR_NULL,
    },
    kcal_per_100g_printed: {
      description: "Калорийность на 100 г из таблицы этой этикетки. Бери число при «ккал», а не при " +
        "«кДж». Таблицы в кадре нет или строка обрезана — null.",
      ...NUM_OR_NULL,
    },
    protein_per_100g_printed: {
      description: "Белки на 100 г из таблицы: число при подписи «белки» или «белок». Нет — null.",
      ...NUM_OR_NULL,
    },
    carbs_per_100g_printed: {
      description: "Углеводы на 100 г из таблицы: число при подписи «углеводы». Нет — null.",
      ...NUM_OR_NULL,
    },
    fat_per_100g_printed: {
      description: "Жиры на 100 г из таблицы: число при подписи «жиры» или «жир». Нет — null.",
      ...NUM_OR_NULL,
    },

    // Полная расшифровка идёт ПОСЛЕДНЕЙ намеренно. Мелкий нечитаемый шрифт —
    // адрес завода, юридические оговорки — загоняет модель в повтор: она гоняет
    // одну и ту же строку, пока не кончится бюджет, и ответ обрывается на
    // полуслове. Стояло это поле выше — вместе с ним пропадали и название, и
    // КБЖУ; стоит последним — теряется только хвост расшифровки.
    all_text_verbatim: {
      description: "ТОЛЬКО для \"label\": ВЕСЬ текст с этикетки, дословно, строка за строкой, как " +
        "напечатано. Не пересказывай, не переставляй, не дополняй. Обрезанное краем кадра выписывай " +
        "настолько, насколько видно. Строку, которую не разобрать, пропусти и иди дальше: НИКОГДА не " +
        "повторяй уже выписанное. Для \"food\" — null.",
      ...STR_OR_NULL,
    },
  },
  required: ["what_is_on_the_photo", "photo_kind", "foods_on_the_photo", "product_name_printed",
             "netto_weight_printed_g", "nutrition_line_verbatim", "energy_verbatim",
             "kcal_per_100g_printed",
             "protein_per_100g_printed", "carbs_per_100g_printed", "fat_per_100g_printed",
             "all_text_verbatim"],
};

/// Схема уходит модели текстом: на своём сервере поле `response_format` до неё не
/// доходит — поллер шлёт в llama-swap только model, temperature, stream и messages.
const schemaInstruction = (schema) => {
  const { $schema, title, ...rest } = schema;
  return `\n\nYou MUST respond with ONLY valid JSON (no markdown, no explanation, no code fences). ` +
    `Respond with ONE object, never an array of objects. ` +
    `The JSON MUST conform to this exact schema:\n${JSON.stringify(rest)}`;
};

function parseRaw(raw) {
  const cleaned = raw.trim().replace(/^```(json)?/, "").replace(/```$/, "").trim();
  const start = cleaned.indexOf("{");
  if (start === -1) throw new Error(`в ответе нет JSON: ${cleaned.slice(0, 160)}`);
  let depth = 0, inStr = false, esc = false;
  for (let i = start; i < cleaned.length; i++) {
    const c = cleaned[i];
    if (esc) { esc = false; continue; }
    if (c === "\\") { esc = true; continue; }
    if (c === '"') { inStr = !inStr; continue; }
    if (inStr) continue;
    if (c === "{") depth++;
    else if (c === "}" && --depth === 0) return JSON.parse(cleaned.slice(start, i + 1));
  }
  // Обрыв на полуслове. Ответы стоят выше расшифровки, поэтому в оборванном
  // объекте они уже есть — закрываем незакрытую строку и скобки и читаем, что
  // успело прийти, вместо того чтобы выбросить весь ответ из-за хвоста.
  return JSON.parse(closeTruncated(cleaned.slice(start)));
}

/// Дописать обрыву закрывающие кавычку и скобки — ровно столько, сколько открыто.
function closeTruncated(text) {
  let out = text, depth = 0, inStr = false, esc = false;
  const stack = [];
  for (const c of out) {
    if (esc) { esc = false; continue; }
    if (c === "\\") { esc = true; continue; }
    if (c === '"') { inStr = !inStr; continue; }
    if (inStr) continue;
    if (c === "{" || c === "[") stack.push(c);
    else if (c === "}" || c === "]") stack.pop();
  }
  if (esc) out = out.slice(0, -1);
  if (inStr) out += '"';
  while (stack.length) out += stack.pop() === "{" ? "}" : "]";
  return out;
}

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `per-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function askQueue(token, image) {
  const submit = await fetch(`${OCR}/submit`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ images: [image], prompt: PROMPT + schemaInstruction(SCHEMA) }),
  });
  if (!submit.ok) throw new Error(`submit HTTP ${submit.status}`);
  const { job_id: jobId } = await submit.json();
  // Дословная расшифровка плотной этикетки идёт долго — на пяти минутах срезало
  // оба кадра сметаны, хотя очередь считала. Ждём столько, сколько она считает.
  for (let i = 0; i < 600; i++) {
    await new Promise((r) => setTimeout(r, 1500));
    const res = await fetch(`${OCR}/job/${jobId}`, { headers: { Authorization: `Bearer ${token}` } });
    if (!res.ok) continue;
    const job = await res.json();
    if (job.status === "done") {
      const raw = job.result || "";
      if (process.env.DUMP_RAW) console.log(`\n--- сырой ответ (${raw.length} симв.) ---\n${raw}\n--- конец ---`);
      return parseRaw(raw);
    }
    if (job.status === "error") throw new Error(`очередь: ${job.error || "recognition failed"}`);
  }
  throw new Error("очередь не ответила за пятнадцать минут");
}

async function askDirect(token, image) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: [
        { type: "text", text: PROMPT },
        { type: "image_url", image_url: { url: `data:image/jpeg;base64,${image}` } },
      ] }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 2000,
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
  return parseRaw(content);
}

const num = (v) => (v === null || v === undefined ? "—" : v);

async function main() {
  const token = await mintToken();
  console.log(ROUTE === "direct" ? `прямой путь, модель ${MODEL}` : "свой сервер (ocr-queue → Qwen2.5-VL)");

  const files = readdirSync("scripts/fixtures")
    .filter((f) => f.endsWith(".jpg") && (!ONLY || f.includes(ONLY)))
    .sort();
  for (const f of files) {
    const image = readFileSync(`scripts/fixtures/${f}`).toString("base64");
    let a;
    try {
      a = ROUTE === "direct" ? await askDirect(token, image) : await askQueue(token, image);
    } catch (e) {
      console.log(`\n${f}\n  сбой — ${e.message}`);
      continue;
    }
    if (RAW) { console.log(`\n${f}`); console.log(JSON.stringify(a, null, 2)); continue; }

    console.log(`\n${f}  [${a.photo_kind}]  ${a.what_is_on_the_photo}`);
    if (a.photo_kind === "food") {
      const list = (a.foods_on_the_photo || [])
        .map((i) => `${i.name}${i.grams === null ? "" : ` ${i.grams} г`}`).join(", ");
      console.log(`  еда: ${list || "—"}`);
    } else {
      console.log(`  название: ${a.product_name_printed ?? "—"}   нетто: ${num(a.netto_weight_printed_g)}`);
      console.log(`  строка КБЖУ: ${(a.nutrition_line_verbatim || "—").replace(/\s+/g, " ").trim()}`);
      console.log(`  строка энергии: ${(a.energy_verbatim || "—").replace(/\s+/g, " ").trim()}`);
      console.log(`  на 100 г: ккал ${num(a.kcal_per_100g_printed)}, Б ${num(a.protein_per_100g_printed)}, ` +
                  `Ж ${num(a.fat_per_100g_printed)}, У ${num(a.carbs_per_100g_printed)}`);
      const t = (a.all_text_verbatim || "").replace(/\s+/g, " ").trim();
      console.log(`  текст (${t.length} симв.): ${t.slice(0, 220)}${t.length > 220 ? "…" : ""}`);
    }
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
