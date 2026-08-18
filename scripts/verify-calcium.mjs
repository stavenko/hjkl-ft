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

// ВЕРИФИКАЦИЯ КАЛЬЦИЯ по группам источников: строка таблицы И величина.
//
// Повод: в приложении кальций выглядит заниженным. Первый же прогон дал моцарелле
// 36 мг при справочных ~500 и кефиру 35 мг при ~120.
//
// [группа, название, ожидаемая строка, справочные мг/100 г]
const CASES = [
  ["жидкие молочные", "Молоко 3.2%", "milk_liquid", 120],
  ["жидкие молочные", "Кефир 1%", "milk_liquid", 120],
  ["жидкие молочные", "Ряженка", "milk_liquid", 124],
  ["жидкие молочные", "Айран", "milk_liquid", 118],
  ["йогурты и скиры", "Йогурт натуральный", "yogurt_sour_cream", 120],
  ["йогурты и скиры", "Йогурт греческий", "yogurt_sour_cream", 110],
  ["йогурты и скиры", "Скир", "yogurt_sour_cream", 150],
  ["йогурты и скиры", "Сметана 15%", "yogurt_sour_cream", 100],
  ["творог", "Творог 5 %", "cottage_cheese", 150],
  ["творог", "Творог обезжиренный", "cottage_cheese", 120],
  ["творог", "Творожная масса", "cottage_cheese", 135],
  ["мягкие сыры", "Рикотта", "cheese_soft", 210],
  ["мягкие сыры", "Камамбер", "cheese_soft", 390],
  ["мягкие сыры", "Бри", "cheese_soft", 185],
  ["мягкие сыры", "Творожный сыр", "cheese_soft", 100],
  ["средние сыры", "Моцарелла", "cheese_semi", 500],
  ["средние сыры", "Брынза", "cheese_semi", 530],
  ["средние сыры", "Сулугуни", "cheese_semi", 650],
  ["средние сыры", "Адыгейский сыр", "cheese_semi", 520],
  ["твёрдые сыры", "Пармезан", "cheese_hard", 1180],
  ["твёрдые сыры", "Сыр российский", "cheese_hard", 880],
  ["твёрдые сыры", "Чеддер", "cheese_hard", 720],
  ["твёрдые сыры", "Гауда", "cheese_hard", 700],
  ["крестоцветные", "Брокколи", "vegetables_other", 47],
  ["крестоцветные", "Капуста белокочанная", "vegetables_other", 40],
  ["крестоцветные", "Цветная капуста", "vegetables_other", 22],
  ["крестоцветные", "Кале", "greens_leafy", 150],
  ["рыба с костями", "Сардины консервированные", "fish_with_bones", 380],
  ["рыба с костями", "Шпроты в масле", "fish_with_bones", 300],
  ["рыба с костями", "Лосось консервированный", "fish_with_bones", 180],
];

const CALCIUM_REFERENCE = [
  ["молоко", 120.0],
  ["кефир", 120.0],
  ["ряженка", 124.0],
  ["простокваша", 118.0],
  ["айран", 118.0],
  ["питьевой йогурт", 110.0],
  ["йогурт натуральный", 120.0],
  ["йогурт греческий", 110.0],
  ["скир", 150.0],
  ["сметана", 90.0],
  ["творог", 150.0],
  ["творог обезжиренный", 120.0],
  ["творожная масса", 135.0],
  ["пармезан", 1180.0],
  ["грана падано", 1160.0],
  ["эмменталь", 1000.0],
  ["маасдам", 800.0],
  ["сыр российский", 880.0],
  ["чеддер", 720.0],
  ["гауда", 700.0],
  ["сулугуни", 650.0],
  ["брынза", 530.0],
  ["моцарелла", 500.0],
  ["адыгейский сыр", 520.0],
  ["фета", 360.0],
  ["камамбер", 390.0],
  ["рикотта", 210.0],
  ["бри", 185.0],
  ["творожный сыр", 100.0],
  ["плавленый сыр", 700.0],
  ["молоко сухое", 1000.0],
  ["молоко сгущённое", 120.0],
  ["сливки", 86.0],
  ["сыворотка молочная", 60.0],
  ["мороженое пломбир", 130.0],
  ["мороженое сливочное", 120.0],
  ["мак", 1460.0],
  ["кунжут", 975.0],
  ["семена чиа", 630.0],
  ["тахини", 420.0],
  ["тофу", 350.0],
  ["миндаль", 260.0],
  ["сардины консервированные", 380.0],
  ["шпроты", 300.0],
  ["лосось консервированный", 180.0],
  ["петрушка", 245.0],
  ["укроп", 223.0],
  ["руккола", 160.0],
  ["кале", 150.0],
  ["шпинат", 99.0],
  ["брокколи", 47.0],
  ["капуста белокочанная", 40.0],
  ["цветная капуста", 22.0],
  ["фасоль сухая", 143.0],
  ["нут сухой", 105.0],
  ["курага", 160.0],
  ["инжир сушёный", 162.0],
  ["апельсин", 40.0],
  ["яйцо куриное", 55.0],
  ["рыба, филе", 25.0],
  ["хлеб", 25.0],
  ["говядина", 12.0],
];

const CALCIUM_ROWS = [
  ["cheese_hard", 600.0, 1300.0, "пармезан, грана падано, чеддер, гауда, эмменталь, российский, пекорино"],
  ["cheese_semi", 350.0, 700.0, "моцарелла, сулугуни, адыгейский, брынза, фета, эдам, косичка"],
  ["cheese_soft", 80.0, 400.0, "рикотта, камамбер, бри, маскарпоне, творожный сыр, филадельфия"],
  ["cheese_processed", 200.0, 900.0, "плавленый сыр, сырок плавленый, сыр в ванночке, хохланд, виола"],
  ["milk_liquid", 90.0, 150.0, "молоко, кефир, ряженка, айран, простокваша, питьевой йогурт"],
  ["yogurt_sour_cream", 80.0, 180.0, "йогурт, греческий йогурт, скир, сметана, простокваша густая, сливочный сыр без соли"],
  ["cottage_cheese", 80.0, 200.0, "творог, зернёный творог, творожная масса, сырники"],
  ["milk_concentrated", 250.0, 1300.0, "сухое молоко, сгущённое молоко, молочная сыворотка сухая"],
  ["cream_whey", 50.0, 150.0, "сливки, сыворотка, мороженое пломбир, мороженое сливочное, эскимо, молочный коктейль"],
  ["seeds_high", 500.0, 1500.0, "мак, кунжут, семена чиа"],
  ["sesame_paste", 250.0, 550.0, "тахини, халва тахинная, кунжутная паста, козинак кунжутный"],
  ["fish_with_bones", 120.0, 450.0, "рыбные консервы, которые едят ВМЕСТЕ С КОСТЯМИ: сардины, шпроты, килька, консервы «с костями» на этикетке"],
  ["fish_plain", 10.0, 60.0, "рыба и морепродукты без костей: лосось, кижуч, горбуша, форель, треска, минтай, скумбрия, сельдь, тунец, филе любой рыбы, креветки, кальмар"],
  ["tofu", 100.0, 700.0, "тофу, соевый творог"],
  ["nuts_high_calcium", 200.0, 300.0, "миндаль, бразильский орех"],
  ["nuts_other", 40.0, 150.0, "фундук, фисташки, кешью, грецкий орех, арахис, кедровый орех"],
  ["legumes_dry", 60.0, 300.0, "фасоль сухая, нут сухой, соевые бобы, маш"],
  ["greens_leafy", 130.0, 260.0, "петрушка, укроп, кале, руккола, базилик, кинза"],
  ["vegetables_other", 15.0, 90.0, "брокколи, цветная капуста, белокочанная капуста, пекинская капуста, стручковая фасоль, репа"],
  ["greens_oxalate", 40.0, 120.0, "шпинат, щавель, ревень, свекольная ботва"],
  ["dried_fruit", 30.0, 180.0, "инжир сушёный, курага, урюк, финики"],
  ["fortified", 100.0, 180.0, "обогащённое кальцием растительное молоко, сок с кальцием, хлопья с кальцием"],
  ["plant_milk_plain", 5.0, 40.0, "соевое молоко без добавок, овсяное молоко, миндальное молоко, кокосовое молоко"],
  ["other_none", 0.0, 0.0, "мясо, птица, яйца, крупы, макароны, хлеб, растительное масло, сахар, конфеты, вода, чай, кофе, алкоголь"],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((x) => x.trim().toLowerCase());
  // В наборе кальция первый элемент — ГРУППА, название второе.
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

// Копия запроса кальция из `ai::lookup_calcium`.
const calciumPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "How much CALCIUM does this food hold per 100 grams, in milligrams?\n\n" +
  "FIRST look for it in the REFERENCE below. If it is there — or is plainly the same food " +
  "under another name, in another grammatical case or with a fat percentage attached — put " +
  "that entry's name into \"reference_key\", copied exactly.\n\n" +
  CALCIUM_REFERENCE.map(([n, mg]) => `  ${n}: ${mg}`).join("\n") + "\n\n" +
  "Whether or not you found it, ALSO place the food in one row of the table below and answer " +
  "with that row's key. If the reference had nothing, answer \"reference_key\" with NONE and " +
  "give a value that fits the row.\n\n" +
  CALCIUM_ROWS.map(([k, , , ex]) => `  ${k.padEnd(20)} — ${ex}`).join("\n") + "\n\n" +
  "For raw or dry as-sold products (grains, seeds, legumes, flour) use the RAW value unless " +
  "the name says cooked, boiled or soaked.\n\n" +
  "Rules for choosing the row:\n" +
  "- Cheese goes by HARDNESS: hard and dry → cheese_hard, semi-hard and brined → cheese_semi, " +
  "soft and fresh → cheese_soft, melted/processed → cheese_processed.\n" +
  "- A plant drink counts as fortified ONLY if the name says so («обогащённое», «с кальцием», " +
  "«fortified»). Otherwise it is plant_milk_plain, which has very little.\n" +
  "- Canned fish counts as fish_with_bones only when the bones are eaten (sardines, sprats, " +
  "canned salmon). Fillet without bones is other_none.\n" +
  "- The examples in a row are EXAMPLES, not the whole row. A food that is not named there " +
  "still belongs to the row it is closest to: скир goes with yogurts, мороженое with cream, " +
  "айран with milk, любой сыр — to the cheese row that matches its hardness. Answer " +
  "other_none ONLY when the food genuinely carries no calcium worth counting — meat, eggs, " +
  "cereals, bread, oil, sugar, water, tea, coffee. Never answer other_none merely because the " +
  "name is missing from the examples.\n" +
  "- Dairy is NEVER other_none: every milk product belongs to one of the dairy rows. " +
  "Vegetables are never other_none either: they go to vegetables_other or greens_leafy.\n\n" +
  "Fill \"reason\" FIRST, then the two keys, then the milligrams.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const calciumSchema = {
  type: "object",
  properties: {
    reason: { type: "string" },
    reference_key: { type: "string" },
    category: { type: "string" },
    calcium_mg: { type: "number" },
  },
  required: ["reason", "reference_key", "category", "calcium_mg"],
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
  const r = await ask(calciumPrompt(name, top.definition),
    calciumSchema, "calcium");
  if (r.err) { badRow++; console.log(`FAIL ${name.padEnd(26)} кальций: ${r.err} ${r.raw ?? ""}`); continue; }
  const row = String(r.obj.category).trim();
  const rawMg = Number(r.obj.calcium_mg);
  const rowDef = CALCIUM_ROWS.find(([k]) => k === row);
  // Ключ справочника решает, откуда взять число: нашлась запись — её миллиграммы,
  // иначе зажим границами строки. Ровно как в `ai::lookup_calcium`.
  const hit = CALCIUM_REFERENCE.find(([n]) =>
    n.toLowerCase() === String(r.obj.reference_key).trim().toLowerCase());
  const mg = hit ? hit[1] : (!rowDef ? rawMg : Math.min(Math.max(rawMg, rowDef[1]), rowDef[2]));
  const okRow = row === wantRow;
  // Величина: считаем годной, если она в коридоре ±50 % от справочной. Нам нужен
  // порядок, а не точность до миллиграмма.
  const okMg = mg >= refMg * 0.5 && mg <= refMg * 1.5;
  if (!okRow) badRow++;
  if (!okMg) badMg++;
  const dev = refMg ? `${(mg / refMg).toFixed(2)}×` : "—";
  console.log(
    `${okRow && okMg ? "OK  " : "MISS"} ${name.padEnd(26)} ${row.padEnd(18)} ` +
    `${String(mg).padEnd(7)} ${String(refMg).padEnd(8)} ${okMg ? "  " : "≠ "}${dev}`
  );
  if (!okRow) console.log(`     ждали строку ${wantRow}, модель: ${r.obj.reason}`);
}
console.log(`\nстрока таблицы: ${CASES.length - badRow}/${CASES.length}`);
console.log(`величина:       ${CASES.length - badMg}/${CASES.length}`);
process.exit(badRow + badMg ? 1 : 0);
