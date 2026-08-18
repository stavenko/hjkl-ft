// ВЕРИФИКАЦИЯ КБЖУ СО СПРАВОЧНИКОМ.
//
// Справочник на полтораста позиций против всего, что человек может написать, —
// покрытие ничтожное, и польза от него не в покрытии. Проверяются РОВНО ДВЕ вещи:
//
//   ПОПАДАНИЕ — продукт, который в справочнике ЕСТЬ, должен быть найден, и найден в
//               НУЖНОЙ ФОРМЕ: «гречка» и «гречка варёная» — разные записи, 343 ккал
//               против 92;
//   ЛОЖНОЕ    — продукт, которого в справочнике НЕТ, не должен притягивать чужую
//               запись. «Творог 2 %» под видом «творог 5%» хуже честного незнания:
//               ошибка молча уезжает в дневник и выглядит справочной.
//
// Первое без второго ничего не значит: справочник, который срабатывает всегда,
// врёт всегда.
//
// ВНИМАНИЕ: промпт — копия `build_nutrition_prompt` + `lookup_prompt_by_name`;
// итоговая проверка живым путём.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const THINK = process.env.THINK !== "0";

// [группа, ввод, ожидаемый ключ справочника или null, если ждём NONE]
const CASES = [
  ["есть в справочнике", "Куриная грудка", "куриная грудка"],
  ["есть в справочнике", "Гречка", "гречка сухая"],
  ["есть в справочнике", "Гречка варёная", "гречка варёная"],
  ["есть в справочнике", "Рис отварной", "рис белый варёный"],
  ["есть в справочнике", "Яйцо куриное", "яйцо куриное"],
  ["есть в справочнике", "Творог 5%", "творог 5%"],
  ["есть в справочнике", "Оливковое масло", "оливковое масло"],
  ["есть в справочнике", "Банан", "банан"],
  ["есть в справочнике", "Говядина", "говядина"],
  ["есть в справочнике", "Скумбрия х/к", "скумбрия холодного копчения"],
  ["есть в справочнике", "Голец", "голец"],
  ["есть в справочнике", "Маш", "маш"],
  ["есть в справочнике", "Буженина", "буженина"],
  ["есть в справочнике", "Fuet truffle", "фуэт"],
  ["есть в справочнике", "Миндаль", "миндаль"],
  ["есть в справочнике", "Хлеб бородинский", "хлеб бородинский"],

  // Похоже на запись, но НЕ ОНА: иная жирность, иная готовность, иной продукт.
  ["ловушка: близко, но не то", "Творог 2%", null],
  ["ловушка: близко, но не то", "Молоко 1%", null],
  ["ловушка: близко, но не то", "Свинина жареная", null],
  ["ловушка: близко, но не то", "Куриная грудка в панировке", null],
  ["ловушка: близко, но не то", "Гречка с грибами", null],
  ["ловушка: близко, но не то", "Сметана 10%", null],
  ["ловушка: близко, но не то", "Банановый смузи", null],
  ["ловушка: близко, но не то", "Икра минтая", null],

  // Описание блюда: в справочнике таких нет и быть не может.
  ["блюда и описания", "Жареная курица, добавил лука и ложку масла", null],
  ["блюда и описания", "Салат оливье", null],
  ["блюда и описания", "Сырники", null],
  ["блюда и описания", "Шаурма с курицей", null],
];

// Копия MACRO_REFERENCE.
const MACRO_REFERENCE = [
  ["говядина", 220, 26, 13, 0], ["телятина", 130, 21, 5, 0], ["свинина", 270, 20, 21, 0],
  ["баранина", 280, 17, 23, 0], ["крольчатина", 175, 21, 10, 0],
  ["куриная грудка", 165, 31, 3.6, 0], ["куриное бедро", 210, 18, 15, 0],
  ["индейка филе", 135, 29, 2, 0], ["утка", 340, 16, 30, 0], ["фарш говяжий", 250, 17, 20, 0],
  ["печень говяжья", 135, 20, 3.6, 4], ["печень куриная", 140, 20, 6, 1],
  ["сердечки куриные", 160, 16, 10, 1], ["язык говяжий", 225, 16, 17, 2],
  ["сосиски", 270, 11, 24, 2], ["колбаса варёная", 300, 12, 27, 2], ["салями", 420, 22, 37, 1],
  ["фуэт", 450, 25, 38, 1], ["ветчина", 165, 18, 10, 1], ["бекон", 500, 12, 50, 1],
  ["буженина", 233, 15, 19, 0], ["бастурма", 240, 35, 11, 0], ["страусятина", 145, 27, 3, 0],
  ["лосось", 208, 20, 13, 0], ["форель", 140, 20, 6, 0], ["скумбрия", 205, 19, 14, 0],
  ["скумбрия холодного копчения", 220, 18, 16, 0], ["сельдь", 160, 18, 9, 0],
  ["сельдь солёная", 250, 17, 20, 0], ["треска", 82, 18, 0.7, 0], ["минтай", 72, 16, 0.9, 0],
  ["тунец", 130, 24, 3, 0], ["голец", 154, 20, 8, 0], ["горбуша", 140, 21, 6, 0],
  ["кета", 130, 22, 5, 0], ["кижуч", 140, 21, 6, 0], ["муксун", 88, 17.5, 2, 0],
  ["палтус", 105, 19, 3, 0], ["креветки", 95, 20, 1, 0], ["кальмар", 92, 18, 1.4, 0],
  ["мидии", 77, 12, 2, 3.7], ["икра красная", 250, 31, 13, 0],
  ["печень трески", 613, 4.2, 65, 1.2], ["крабовые палочки", 95, 6, 1, 15],
  ["молоко 3.2%", 60, 3, 3.2, 4.7], ["молоко 2.5%", 52, 2.9, 2.5, 4.7],
  ["кефир 1%", 40, 3, 1, 4], ["йогурт натуральный", 60, 5, 3.2, 4],
  ["творог 5%", 121, 16, 5, 3], ["творог 9%", 160, 16, 9, 3],
  ["творог обезжиренный", 71, 18, 0.6, 1.8], ["сметана 20%", 200, 2.5, 20, 3],
  ["сливки 10%", 120, 3, 10, 4], ["сыр твёрдый", 360, 25, 28, 0], ["моцарелла", 280, 22, 22, 2],
  ["брынза", 260, 17, 20, 0], ["творожный сыр", 250, 6, 24, 4],
  ["сливочное масло", 750, 0.5, 82, 1], ["яйцо куриное", 157, 12.7, 11.5, 0.7],
  ["мороженое пломбир", 230, 4, 15, 21],
  ["гречка сухая", 343, 12.6, 3.3, 62], ["рис белый сухой", 360, 7, 0.7, 79],
  ["рис бурый сухой", 350, 7.5, 2.8, 72], ["овсянка сухая", 370, 13, 7, 62],
  ["перловка сухая", 320, 9, 1, 73], ["пшено сухое", 350, 11, 3.3, 69],
  ["булгур сухой", 340, 12, 1.3, 76], ["киноа сухая", 370, 14, 6, 57],
  ["полба сухая", 338, 14.6, 2.4, 70], ["макароны сухие", 350, 12, 1.5, 71],
  ["манка сухая", 330, 10, 1, 73], ["кукурузная крупа сухая", 330, 8, 1.2, 71],
  ["гречка варёная", 92, 3.4, 0.6, 20], ["рис белый варёный", 130, 2.7, 0.3, 28],
  ["макароны варёные", 130, 4.5, 0.6, 25], ["овсянка на воде", 88, 3, 1.7, 15],
  ["чечевица варёная", 115, 9, 0.4, 20], ["фасоль варёная", 125, 8, 0.5, 22],
  ["нут варёный", 165, 9, 2.6, 27], ["картофель варёный", 80, 2, 0.1, 17],
  ["картофель жареный", 190, 2.8, 9, 23],
  ["хлеб пшеничный", 260, 8, 3, 50], ["хлеб ржаной", 250, 8.5, 3.3, 48],
  ["хлеб бородинский", 210, 6.8, 1.3, 40],
  ["чечевица сухая", 350, 24, 1, 60], ["фасоль сухая", 320, 21, 2, 54],
  ["нут сухой", 360, 19, 6, 61], ["горох сухой", 350, 23, 1.6, 60], ["маш", 323, 23.9, 1.2, 62],
  ["соя", 380, 36, 20, 30], ["тофу", 76, 8, 4.8, 1.9], ["арахис", 570, 26, 49, 16],
  ["миндаль", 579, 21, 50, 22], ["грецкий орех", 655, 15, 65, 7], ["кешью", 550, 18, 44, 30],
  ["фундук", 630, 15, 61, 17], ["фисташки", 560, 20, 45, 28], ["кедровый орех", 670, 14, 68, 13],
  ["семена подсолнечника", 580, 21, 53, 20], ["семена тыквы", 560, 25, 49, 11],
  ["кунжут", 570, 18, 50, 23], ["семена льна", 530, 18, 42, 29], ["семена чиа", 490, 17, 31, 42],
  ["картофель", 77, 2, 0.1, 17], ["морковь", 35, 1.3, 0.1, 7],
  ["капуста белокочанная", 28, 1.8, 0.1, 5], ["брокколи", 34, 2.8, 0.4, 7],
  ["помидор", 20, 1, 0.2, 4], ["огурец", 15, 0.8, 0.1, 3], ["лук репчатый", 40, 1.4, 0.1, 9],
  ["болгарский перец", 27, 1, 0.3, 6], ["кабачок", 24, 1.2, 0.3, 4.6], ["свёкла", 43, 1.6, 0.2, 10],
  ["тыква", 26, 1, 0.1, 6], ["топинамбур", 73, 2, 0.1, 17],
  ["авокадо", 160, 2, 15, 9], ["банан", 89, 1.1, 0.3, 23], ["яблоко", 52, 0.4, 0.2, 14],
  ["апельсин", 47, 0.9, 0.1, 12], ["груша", 57, 0.4, 0.1, 15], ["виноград", 69, 0.7, 0.2, 18],
  ["клубника", 33, 0.7, 0.3, 8], ["черника", 57, 0.7, 0.3, 14], ["вишня", 52, 1, 0.3, 11],
  ["жимолость", 41, 0.8, 0.1, 8], ["ирга", 45, 0.8, 0.2, 12], ["курага", 240, 3.4, 0.5, 63],
  ["изюм", 300, 3, 0.5, 79], ["чернослив", 240, 2.2, 0.4, 64], ["финики", 280, 2.5, 0.4, 75],
  ["оливковое масло", 884, 0, 100, 0], ["подсолнечное масло", 884, 0, 100, 0],
  ["мёд", 300, 0.3, 0, 82], ["сахар", 400, 0, 0, 100], ["шоколад тёмный", 550, 6, 35, 50],
  ["шоколад молочный", 535, 7, 30, 58],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((x) => x.trim().toLowerCase());
  const kept = CASES.filter((c) => want.some((w) => String(c[1]).toLowerCase().includes(w)));
  if (!kept.length) throw new Error(`ONLY=${process.env.ONLY} не совпал ни с одним продуктом`);
  CASES.length = 0;
  CASES.push(...kept);
}

const FORM_BY_NAME =
  "Form of the product: for items bought and weighed raw/dry (grains, rice, pasta, flour, " +
  "meat, fish, legumes, etc.), give values for the RAW / as-sold product — NOT cooked — " +
  "unless the input says cooked, boiled, fried, steamed, ready-to-eat, or clearly describes " +
  "a prepared dish.";

// Копия `build_nutrition_prompt` + `lookup_prompt_by_name`.
const prompt = (name) =>
  "You are a nutritional database. The user's input may be a plain food NAME (e.g. «яйцо», " +
  "«рис», «гречка») OR a free-form DESCRIPTION of a dish, possibly with added ingredients " +
  "(e.g. «жареная курица, добавил немного лука, чайную ложку масла»). " +
  `Input: "${name}".\n\n` +
  "First, set \"product_name\" to a SHORT dish/product name in Russian. HARD LIMIT: at most " +
  "THREE words, ideally TWO; a third word ONLY when indispensable to identify the dish. Do " +
  "NOT list the added ingredients in the name — name only the core dish. Good examples (2–3 " +
  "words): «Жареная курица», «Куриная грудка», «Овсяная каша», «Гречка с грибами». For a " +
  "plain name, keep it (tidied); for a description, name the resulting core dish concisely " +
  "within this limit.\n\n" +
  "Then look for the food in the REFERENCE below. Put that entry's name into " +
  "\"reference_key\", copied exactly, and we take the numbers ourselves. An entry fits ONLY " +
  "when it is THE SAME FOOD — the same name in another grammatical case, or a cut or grade of " +
  "it. Everything else is NONE, and NONE is a good answer: numbers you work out yourself are " +
  "used just as readily, while a wrong entry is worse than no entry at all, because it looks " +
  "exact.\n\n" +
  "Before naming the key, fill \"reference_check\": name the closest entry you found and " +
  "answer THREE questions about it in one sentence — is it the SAME FOOD (not a part of it, " +
  "not something made from it, not something it is made from); is it in the SAME FORM (raw or " +
  "boiled, fresh or smoked, plain or coated); does it carry the SAME FAT PERCENTAGE if the " +
  "input names one. A single NO means the key is NONE.\n\n" +
  "A DESCRIBED DISH IS NEVER IN THE REFERENCE: if the input adds ingredients or names a dish " +
  "cooked of several foods, the key is NONE and you work the numbers out yourself.\n\n" +
  MACRO_REFERENCE.map(([n, k, p, f, c]) => `  ${n}: ${k} ккал, Б ${p}, Ж ${f}, У ${c}`).join("\n") +
  "\n\n" +
  "Whether or not you found it, provide nutritional values per 100 GRAMS of the resulting " +
  "food/dish (account for the added ingredients — e.g. the oil raises fat and kcal).\n\n" +
  FORM_BY_NAME + "\n\n" +
  "For each nutrient (kcal, protein, fat, carbs), provide:\n" +
  "- min_value: lowest reasonable value for this food\n" +
  "- max_value: highest reasonable value for this food\n" +
  "- recommended: the most likely value to select\n" +
  "- comment: brief explanation why this value is appropriate, written in Russian\n\n" +
  "Use these units: kcal for calories, g/mg/mkg/kg for weights.\n" +
  "All values are per 100g. Compute real values specifically for the input — do not copy any " +
  "sample numbers.\n\n" +
  "Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose " +
  "before or after. Include \"product_name\", \"reference_check\" and \"reference_key\" as strings. EVERY key and " +
  "EVERY string value MUST be wrapped in double quotes. EVERY `value` MUST be a real number " +
  "(e.g. 12.5), never empty or null.";

const detail = {
  type: "object",
  properties: {
    min_value: { type: "object", properties: { value: { type: "number" }, unit: { type: "string" } },
      required: ["value", "unit"], additionalProperties: false },
    max_value: { type: "object", properties: { value: { type: "number" }, unit: { type: "string" } },
      required: ["value", "unit"], additionalProperties: false },
    recommended: { type: "object", properties: { value: { type: "number" }, unit: { type: "string" } },
      required: ["value", "unit"], additionalProperties: false },
    comment: { type: "string" },
  },
  required: ["min_value", "max_value", "recommended", "comment"],
  additionalProperties: false,
};

const schema = {
  type: "object",
  properties: {
    product_name: { type: "string" },
    reference_check: { type: "string" },
    reference_key: { type: "string" },
    kcal: detail, protein: detail, fat: detail, carbs: detail,
  },
  required: ["product_name", "reference_check", "reference_key", "kcal", "protein", "fat", "carbs"],
  additionalProperties: false,
};

const b64 = (b) => Buffer.from(b).toString("base64url");
const uid = `macros-${Date.now()}`;
const now = Math.floor(Date.now() / 1000);
const data = b64(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const ask = async (text) => {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: text }],
      response_format: { type: "json_schema", json_schema: { name: "nutrition", schema } },
      stream: true, think: THINK, max_tokens: Number(process.env.MAXTOK || (THINK ? 8000 : 6000)),
    }),
  });
  const t = await r.text();
  if (!r.ok) return { err: `HTTP ${r.status}` };
  let out = "";
  for (const line of t.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const p = line.slice(5).trim();
    if (!p || p === "[DONE]") continue;
    try { out += JSON.parse(p)?.choices?.[0]?.delta?.content ?? ""; } catch {}
  }
  const clean = out.trim().replace(/^```json\s*|^```\s*|```$/g, "").trim();
  try { return { obj: JSON.parse(clean) }; }
  catch (e) { return { err: `parse: ${e.message}`, raw: clean.slice(0, 100) }; }
};

let missHit = 0;   // не нашли того, что есть
let falseHit = 0;  // притянули чужую запись
let group = "";
console.log("ввод                              ключ справочника             ккал/б/ж/у");
for (const [g, name, want] of CASES) {
  if (g !== group) { group = g; console.log(`\n── ${g} ──`); }
  const r = await ask(prompt(name));
  if (r.err) { console.log(`FAIL ${name.padEnd(33)} ${r.err} ${r.raw ?? ""}`); continue; }
  const rawKey = String(r.obj.reference_key ?? "").trim();
  const hit = MACRO_REFERENCE.find(([n]) => n.toLowerCase() === rawKey.toLowerCase());
  const v = hit
    ? [hit[1], hit[2], hit[3], hit[4]]
    : ["kcal", "protein", "fat", "carbs"].map((k) => Number(r.obj[k].recommended.value));
  const shown = hit ? `справочник «${hit[0]}»` : (rawKey && rawKey.toUpperCase() !== "NONE"
    ? `НЕТ ТАКОЙ ЗАПИСИ «${rawKey}»` : "сама");
  let ok;
  if (want === null) {
    ok = !hit;
    if (!ok) falseHit++;
  } else {
    ok = hit && hit[0] === want;
    if (!ok) missHit++;
  }
  console.log(
    `${ok ? "OK  " : "MISS"} ${name.padEnd(33)} ${shown.padEnd(28)} ` +
    v.map((x) => Number(x).toFixed(Number(x) < 10 ? 1 : 0)).join("/")
  );
  if (!ok && want) console.log(`     ждали «${want}»`);
  if (!ok && want === null) console.log(`     ждали, что запись НЕ подойдёт`);
}
const inRef = CASES.filter(([, , w]) => w !== null).length;
const outRef = CASES.length - inRef;
console.log(`\nнайдено в справочнике:  ${inRef - missHit}/${inRef}`);
console.log(`ложных попаданий:       ${falseHit}/${outRef}`);
process.exit(missHit + falseHit ? 1 : 0);
