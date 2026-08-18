// ВЕРИФИКАЦИЯ КАЛЬЦИЯ: строка таблицы и величина против справочных значений.
//
// Наш признак «овощ или фрукт» собирается ИЗ ЭТИХ флагов: растение, но не корнеплод
// и не бобовое. Прежний единый `is_veggy` был нашим термином, который модель должна
// была угадывать, — и угадывала по-разному: картофель получал то овощ, то корнеплод,
// чипсы гуляли между прогонами. Части растения — факты, их угадывать не надо.
//
// Клетчатка спрашивается здесь же: продукт уже опознан, и отдельный запрос за ней —
// лишние деньги и лишний повод для разногласий.
//
// ВНИМАНИЕ: промпты — копия того, что пойдёт в `flags_pipeline.rs`; итоговая проверка
// живым путём.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const THINK = process.env.THINK !== "0";

// ВЕРИФИКАЦИЯ ЖЕЛЕЗА: строка таблицы и величина против справочных значений.
//
// У железа та же болезнь, что была у кальция: строку модель выбирает, а число
// называет по памяти. Диапазоны строк местами вчетверо шире разброса внутри группы
// (liver_offal 5–18, nuts_seeds 3.5–15), так что проверка коридором пропускает
// почти любую ошибку.
//
// [группа, название, ожидаемая строка, справочные мг/100 г]
const CASES = [
  ["субпродукты", "Печень куриная", "liver_offal", 9.0],
  ["субпродукты", "Печень говяжья", "liver_offal", 6.9],
  ["субпродукты", "Печень свиная", "liver_offal", 18.0],
  ["субпродукты", "Сердечки куриные", "liver_offal", 5.9],
  ["мясо", "Говядина", "meat_red", 2.6],
  ["мясо", "Свинина", "meat_red", 0.9],
  ["мясо", "Баранина", "meat_red", 1.9],
  ["мясо", "Куриная грудка", "meat_poultry", 1.0],
  ["мясо", "Индейка филе", "meat_poultry", 1.4],
  ["рыба и морепродукты", "Голец", "fish", 0.5],
  ["рыба и морепродукты", "Скумбрия", "fish", 1.6],
  ["рыба и морепродукты", "Мидии варёно-мороженые", "shellfish", 6.7],
  ["рыба и морепродукты", "Кальмар", "shellfish", 0.7],
  ["рыба и морепродукты", "Креветки", "shellfish", 1.8],
  ["бобовые и орехи", "Чечевица варёная", "legumes", 3.3],
  ["бобовые и орехи", "Фасоль красная сухая", "legumes", 6.7],
  ["бобовые и орехи", "Нут сухой", "legumes", 6.2],
  ["бобовые и орехи", "Кунжут", "nuts_seeds", 14.6],
  ["бобовые и орехи", "Миндаль", "nuts_seeds", 3.7],
  ["бобовые и орехи", "Семена тыквы", "nuts_seeds", 8.8],
  ["крупы и хлеб", "Гречка варёная", "grains_whole", 1.5],
  ["крупы и хлеб", "Овсянка на воде", "grains_whole", 1.7],
  ["крупы и хлеб", "Хлеб бородинский", "grains_refined", 3.9],
  ["зелень и овощи", "Шпинат", "spinach_oxalate", 2.7],
  ["зелень и овощи", "Петрушка", "greens_herbs", 6.2],
  ["зелень и овощи", "Брокколи", "vegetables", 0.7],
  ["прочее", "Яйцо куриное", "eggs", 1.7],
  ["прочее", "Творог 5 %", "dairy", 0.4],
  ["прочее", "Курага", "fruit_dried", 3.2],
  ["прочее", "Яблоко", "fruit_fresh", 0.1],
];

const IRON_REFERENCE = [
  ["печень свиная", 18.0, 0.25],
  ["печень куриная", 9.0, 0.25],
  ["печень говяжья", 6.9, 0.25],
  ["печень индейки", 7.5, 0.25],
  ["печень трески", 1.9, 0.25],
  ["сердечки куриные", 5.9, 0.25],
  ["сердце говяжье", 4.3, 0.25],
  ["почки говяжьи", 5.9, 0.25],
  ["желудки куриные", 3.2, 0.25],
  ["язык говяжий", 4.1, 0.25],
  ["говядина", 2.6, 0.2],
  ["баранина", 1.9, 0.2],
  ["крольчатина", 1.3, 0.2],
  ["свинина", 0.9, 0.2],
  ["телятина", 1.1, 0.2],
  ["оленина", 3.4, 0.2],
  ["куриная грудка", 1.0, 0.15],
  ["куриное бедро", 1.3, 0.15],
  ["индейка филе", 1.4, 0.15],
  ["колбаса варёная", 1.7, 0.15],
  ["сосиски", 1.8, 0.15],
  ["салями", 1.5, 0.15],
  ["ветчина", 1.3, 0.15],
  ["устрицы", 9.2, 0.25],
  ["мидии", 6.7, 0.25],
  ["креветки", 1.8, 0.25],
  ["гребешки", 0.6, 0.25],
  ["кальмар", 0.7, 0.25],
  ["скумбрия", 1.6, 0.15],
  ["сельдь", 1.1, 0.15],
  ["лосось", 0.8, 0.15],
  ["тунец", 1.0, 0.15],
  ["треска", 0.4, 0.15],
  ["голец", 0.5, 0.15],
  ["икра красная", 1.8, 0.2],
  ["кунжут", 14.6, 0.04],
  ["семена тыквы", 8.8, 0.04],
  ["чечевица сухая", 7.5, 0.05],
  ["фасоль сухая", 6.7, 0.05],
  ["нут сухой", 6.2, 0.05],
  ["соя", 15.7, 0.03],
  ["семена подсолнечника", 5.3, 0.04],
  ["кешью", 6.7, 0.04],
  ["фисташки", 4.2, 0.04],
  ["миндаль", 3.7, 0.04],
  ["грецкий орех", 2.9, 0.04],
  ["арахис", 4.6, 0.04],
  ["чечевица варёная", 3.3, 0.05],
  ["фасоль варёная", 2.9, 0.05],
  ["тофу", 5.4, 0.03],
  ["гречка сухая", 6.7, 0.04],
  ["гречка варёная", 1.5, 0.04],
  ["овсянка сухая", 4.3, 0.04],
  ["овсянка на воде", 1.7, 0.04],
  ["рис белый варёный", 0.5, 0.08],
  ["хлеб ржаной", 3.9, 0.04],
  ["хлеб пшеничный", 1.5, 0.08],
  ["макароны варёные", 1.3, 0.08],
  ["петрушка", 6.2, 0.08],
  ["укроп", 6.6, 0.08],
  ["шпинат", 2.7, 0.02],
  ["руккола", 1.5, 0.08],
  ["брокколи", 0.7, 0.08],
  ["капуста белокочанная", 0.6, 0.08],
  ["картофель", 0.9, 0.08],
  ["морковь", 0.7, 0.08],
  ["яблоко", 0.1, 0.1],
  ["банан", 0.3, 0.1],
  ["курага", 3.2, 0.1],
  ["изюм", 1.9, 0.1],
  ["чернослив", 0.9, 0.1],
  ["яйцо куриное", 1.7, 0.04],
  ["творог", 0.4, 0.02],
  ["молоко", 0.1, 0.02],
  ["сыр твёрдый", 0.7, 0.02],
  ["шоколад тёмный", 11.9, 0.02],
  ["какао порошок", 13.9, 0.02],
];

const IRON_ROWS = [
  ["liver_offal", 5.0, 18.0, "печень куриная, печень говяжья, печень свиная, сердце, почки"],
  ["shellfish", 1.5, 8.0, "мидии, устрицы, гребешки, кальмар, креветки"],
  ["roe", 1.5, 3.0, "икра красная, икра чёрная"],
  ["meat_red", 1.2, 3.0, "говядина, телятина, баранина, свинина, кролик"],
  ["meat_poultry", 0.7, 2.5, "курица, индейка, утка, куриная грудка"],
  ["fish", 0.4, 1.8, "лосось, треска, тунец, скумбрия, сельдь, голец, форель, горбуша, кета, минтай, судак, щука, палтус, камбала — ЛЮБАЯ рыба"],
  ["meat_processed", 0.9, 2.5, "колбаса, сосиски, ветчина, бекон, паштет"],
  ["dish_with_meat", 0.8, 2.5, "плов с мясом, борщ с говядиной, паста болоньезе, пельмени"],
  ["fruit_fresh", 0.1, 0.5, "яблоко, апельсин, клубника, киви, черешня, арбуз, банан, виноград"],
  ["fruit_dried", 0.9, 2.8, "курага, изюм, чернослив, инжир сушёный, финики"],
  ["vegetables", 0.2, 0.9, "болгарский перец, помидор, огурец, брокколи, капуста, морковь, кабачок, лук, картофель"],
  ["greens_herbs", 1.0, 6.5, "петрушка, укроп, руккола, базилик, кинза, зелёный лук"],
  ["grains_refined", 0.8, 2.0, "белый хлеб, белый рис, макароны, манка"],
  ["fortified", 3.0, 20.0, "хлопья с добавленным железом, каши быстрого приготовления, детские смеси"],
  ["dish_meatless", 0.3, 1.5, "овощное рагу, вегетарианский суп, каша на воде"],
  ["legumes", 5.0, 8.0, "фасоль, нут, чечевица, горох, маш (в сухом виде)"],
  ["grains_whole", 1.5, 5.0, "гречка, овсянка, бурый рис, цельнозерновой хлеб, булгур"],
  ["nuts_seeds", 3.5, 15.0, "кунжут, кешью, миндаль, тыквенные семечки, фундук"],
  ["eggs", 1.2, 2.0, "яйцо куриное, омлет, яичница"],
  ["soy", 0.4, 5.0, "тофу, соевое молоко, соевое мясо, эдамаме"],
  ["spinach_oxalate", 0.8, 3.0, "шпинат, щавель, свёкла, ревень"],
  ["dairy", 0.05, 0.7, "молоко, творог, сыр, йогурт, кефир, ряженка, ацидофилин, сливки"],
  ["drinks", 0.0, 0.1, "вода, сок, морс, лимонад, пиво, безалкогольное пиво, вино, компот, квас"],
  ["tea_coffee_cocoa", 0.05, 3.0, "чай, кофе, какао, шоколад"],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((x) => x.trim().toLowerCase());
  // В наборе железа первый элемент — ГРУППА, название второе.
  const kept = CASES.filter((c) => want.some((w) => String(c[1]).toLowerCase().includes(w)));
  if (!kept.length) throw new Error(`ONLY=${process.env.ONLY} не совпал ни с одним продуктом`);
  CASES.length = 0;
  CASES.push(...kept);
}

const RARE = [
  ["голец", "a cold-water fish of the salmon family", ["Arctic char", "char"]],
  ["пикша", "a fish of the cod family", ["haddock"]],
  ["сайда", "a fish of the cod family", ["saithe", "pollock", "coalfish"]],
  ["муксун", "a northern whitefish", ["muksun"]],
  ["страусятина", "the flesh of an ostrich, that is of a BIRD", ["ostrich meat"]],
  ["бастурма", "air-dried cured BEEF, a whole muscle", ["basturma", "pastirma"]],
  ["буженина", "a whole piece of PORK, baked and not cured", ["buzhenina", "baked pork"]],
  ["fuet", "a Catalan dry cured PORK sausage", ["fuet", "fuet truffle"]],
  ["маш", "mung bean, a small green LEGUME", ["mung bean", "moong"]],
  ["полба", "spelt, an ancient wheat GRAIN", ["spelt", "farro"]],
  ["ирга", "saskatoon, a dark sweet BERRY on a shrub", ["saskatoon berry", "juneberry"]],
  ["жимолость", "honeyberry, an edible blue BERRY", ["honeyberry", "haskap"]],
  ["топинамбур", "jerusalem artichoke, an edible TUBER", ["jerusalem artichoke", "sunchoke"]],
  ["печень трески", "cod liver, the LIVER of a fish — not roe", ["cod liver"]],
];
const SHORT = [
  ["с/м", "fresh-frozen, nothing added"],
  ["х/к", "cold-smoked"],
  ["с/с", "lightly salted"],
  ["в/с", "top grade — a grade and nothing more"],
  ["ц/з", "wholegrain"],
];
const dictionary = () =>
  "DICTIONARY of rare or confusable names:\n" +
  RARE.map(([w, what, tr]) => `  ${w}: ${what}, could be translated as [${tr.join(", ")}]`).join("\n") +
  "\n\nABBREVIATIONS from Russian labels (storage, cut or grade only):\n" +
  SHORT.map(([w, what]) => `  ${w}: ${what}`).join("\n") + "\n\n";

const identityPrompt = (name) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  "Answer three things about it.\n" +
  "1. \"from_own_knowledge\": the closest definition you can recall YOURSELF, without the " +
  "dictionary. If you do not know this word, say so plainly — that is a valid answer.\n" +
  "2. \"from_dictionary\": the closest definition from the dictionary below, or NONE if nothing " +
  "in it fits this name.\n" +
  "3. \"options\": the three most likely definitions of the food, the surest first, each a " +
  "sentence of five or six words, each with your confidence from 0.0 to 1.0.\n\n" +
  dictionary() +
  "Respond with ONLY a minified JSON object and nothing else.";

const identitySchema = {
  type: "object",
  properties: {
    from_own_knowledge: { type: "string" },
    from_dictionary: { type: "string" },
    options: { type: "array", items: { type: "object",
      properties: { definition: { type: "string" }, confidence: { type: "number" } },
      required: ["definition", "confidence"], additionalProperties: false } },
  },
  required: ["from_own_knowledge", "from_dictionary", "options"],
  additionalProperties: false,
};

const ironPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "How much IRON does it hold per 100 grams, in milligrams?\n\n" +
  "FIRST look for this food in the REFERENCE below. If the food is there — or is plainly the " +
  "same food under another name, in another grammatical case or with a cut or grade attached " +
  "— put that entry's name into \"reference_key\", copied exactly, and we take the numbers " +
  "ourselves. Answer NONE when the reference has nothing for it.\n\n" +
  IRON_REFERENCE.map(([n, mg]) => `  ${n}: ${mg}`).join("\n") + "\n\n" +
  "Whether or not you found it in the reference, you MUST also place the food in ONE row of " +
  "the table below and answer with that row's key — the row decides how well the iron is " +
  "absorbed, and we need it either way. If the food is not in the reference, the value must " +
  "also fall inside that row's range; a reference value stands even when it does not.\n\n" +
  IRON_ROWS.map(([k, , , ex]) => `  ${k}: ${ex}`).join("\n") + "\n\n" +
  "For raw or dry as-sold products use the RAW value unless the name says cooked or boiled.\n\n" +
  "Fill \"reason\" FIRST — one short sentence — then the row key, then the milligrams.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const ironSchema = {
  type: "object",
  properties: {
    reason: { type: "string" },
    verdict: { type: "string" },
    iron_mg_per_100g: { type: "number" },
    reference_key: { type: "string" },
  },
  required: ["reason", "verdict", "iron_mg_per_100g", "reference_key"],
  additionalProperties: false,
};

const b64 = (b) => Buffer.from(b).toString("base64url");
const uid = `parts-${Date.now()}`;
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

const ask = async (text, schema, name) => {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: text }],
      response_format: { type: "json_schema", json_schema: { name, schema } },
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

let badRow = 0;
let badMg = 0;
let group = "";
console.log("продукт                      строка             мг      справка  откл.");
for (const [g, name, wantRow, refMg] of CASES) {
  if (g !== group) { group = g; console.log(`\n── ${g} ──`); }
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { badRow++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  // Ни одной повторной попытки: значение из справочника берётся как есть, прочее
  // поджимается границами строки — ровно как в приложении.
  const r = await ask(ironPrompt(name, top.definition),
    ironSchema, "iron");
  if (r.err) { badRow++; console.log(`FAIL ${name.padEnd(26)} железо: ${r.err} ${r.raw ?? ""}`); continue; }
  const row = String(r.obj.verdict).trim();
  const rawMg = Number(r.obj.iron_mg_per_100g);
  const rowDef = IRON_ROWS.find(([k]) => k === row);
  // Ключ справочника даёт ОБА числа: миллиграммы и долю усвоения продукта.
  const hit = IRON_REFERENCE.find(([n]) => n.toLowerCase() === String(r.obj.reference_key).trim().toLowerCase());
  const mg = hit ? hit[1] : (!rowDef ? rawMg : Math.min(Math.max(rawMg, rowDef[1]), rowDef[2]));
  const absorption = hit ? hit[2] : null;
  const okRow = row === wantRow;
  // Величина: считаем годной, если она в коридоре ±50 % от справочной. Нам нужен
  // порядок, а не точность до миллиграмма.
  const okMg = mg >= refMg * 0.5 && mg <= refMg * 1.5;
  if (!okRow) badRow++;
  if (!okMg) badMg++;
  const dev = refMg ? `${(mg / refMg).toFixed(2)}×` : "—";
  const src = hit ? `справочник «${hit[0]}», усвоение ${absorption}` : "строка";
  console.log(
    `${okRow && okMg ? "OK  " : "MISS"} ${name.padEnd(26)} ${row.padEnd(18)} ` +
    `${String(mg).padEnd(7)} ${String(refMg).padEnd(8)} ${okMg ? "  " : "≠ "}${dev}  ${src}`
  );
  if (!okRow) console.log(`     ждали строку ${wantRow}, модель: ${r.obj.reason}`);
}
console.log(`\nстрока таблицы: ${CASES.length - badRow}/${CASES.length}`);
console.log(`величина:       ${CASES.length - badMg}/${CASES.length}`);
process.exit(badRow + badMg ? 1 : 0);
