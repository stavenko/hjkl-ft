// Кальций: та же таблица строк, но с ГОТОВЫМ опознанием на входе.
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

// [название, ожидаемая строка таблицы кальция]
//
// Кальций уже устроен на таблице строк — модель выбирает ключ, величину ограничивает
// наш диапазон. Проверяем, что будет, если дать ей ГОТОВОЕ опознание: раньше каждая
// таблица опознавала продукт сама, и «Голец» мог оказаться чем угодно.
const CASES = [
  ["Пармезан", "cheese_hard"],
  ["Сыр российский", "cheese_hard"],
  ["Брынза", "cheese_semi"],
  ["Моцарелла", "cheese_semi"],
  ["Рикотта", "cheese_soft"],
  ["Плавленый сыр", "cheese_processed"],
  ["Молоко 3.2%", "milk_liquid"],
  ["Кефир 1%", "milk_liquid"],
  ["Йогурт греческий", "yogurt_sour_cream"],
  ["Творог 5 %", "cottage_cheese"],
  ["Молоко сгущённое", "milk_concentrated"],
  ["Кунжут", "seeds_high"],
  ["Тахини", "sesame_paste"],
  ["Шпроты в масле", "fish_with_bones"],
  ["Сардины консервированные", "fish_with_bones"],
  ["Голец", "fish_plain"],
  ["Скумбрия холодного копчения", "fish_plain"],
  ["Тофу", "tofu"],
  ["Миндаль", "nuts_high_calcium"],
  ["Грецкий орех", "nuts_other"],
  ["Фасоль красная сухая", "legumes_dry"],
  ["Капуста белокочанная", "greens_leafy"],
  ["Огурец", "vegetables_other"],
  ["Шпинат", "greens_oxalate"],
  ["Курага", "dried_fruit"],
  ["Молоко овсяное обогащённое кальцием", "fortified"],
  ["Молоко овсяное", "plant_milk_plain"],
  ["Говядина", "other_none"],
  ["Хлеб бородинский", "other_none"],
];

const CALCIUM_ROWS = [
  ["cheese_hard", "пармезан, грана падано, чеддер, гауда, эмменталь, российский, пекорино"],
  ["cheese_semi", "моцарелла, сулугуни, адыгейский, брынза, фета, эдам, косичка"],
  ["cheese_soft", "рикотта, камамбер, бри, маскарпоне, творожный сыр, филадельфия"],
  ["cheese_processed", "плавленый сыр, сырок плавленый, сыр в ванночке, хохланд, виола"],
  ["milk_liquid", "молоко, кефир, ряженка, айран, простокваша, питьевой йогурт"],
  ["yogurt_sour_cream", "йогурт, греческий йогурт, сметана, сливочный сыр без соли"],
  ["cottage_cheese", "творог, зернёный творог, творожная масса, сырники"],
  ["milk_concentrated", "сухое молоко, сгущённое молоко, молочная сыворотка сухая"],
  ["cream_whey", "сливки, сыворотка, мороженое пломбир"],
  ["seeds_high", "мак, кунжут, семена чиа"],
  ["sesame_paste", "тахини, халва тахинная, кунжутная паста, козинак кунжутный"],
  ["fish_with_bones", "рыбные консервы, которые едят ВМЕСТЕ С КОСТЯМИ: сардины, шпроты, килька, консервы «с костями» на этикетке"],
  ["fish_plain", "рыба и морепродукты без костей: лосось, кижуч, горбуша, форель, треска, минтай, скумбрия, сельдь, тунец, филе любой рыбы, креветки, кальмар"],
  ["tofu", "тофу, соевый творог"],
  ["nuts_high_calcium", "миндаль, бразильский орех"],
  ["nuts_other", "фундук, фисташки, кешью, грецкий орех, арахис, кедровый орех"],
  ["legumes_dry", "фасоль сухая, нут сухой, соевые бобы, маш"],
  ["greens_leafy", "петрушка, укроп, кале, руккола, базилик, кинза"],
  ["vegetables_other", "брокколи, цветная капуста, белокочанная капуста, пекинская капуста, стручковая фасоль, репа"],
  ["greens_oxalate", "шпинат, щавель, ревень, свекольная ботва"],
  ["dried_fruit", "инжир сушёный, курага, урюк, финики"],
  ["fortified", "обогащённое кальцием растительное молоко, сок с кальцием, хлопья с кальцием"],
  ["plant_milk_plain", "соевое молоко без добавок, овсяное молоко, миндальное молоко, кокосовое молоко"],
  ["other_none", "мясо, птица, рыба без костей, яйца, крупы, макароны, хлеб, овощи, фрукты, масло, сахар, сладости, вода, чай, кофе, алкоголь — всё, чего нет выше"],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((x) => x.trim().toLowerCase());
  const kept = CASES.filter(([n]) => want.some((w) => n.toLowerCase().includes(w)));
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

const calciumPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "How much CALCIUM does it hold per 100 grams, in milligrams? Place the food in ONE row of " +
  "this table, then give a value inside that row's range.\n\n" +
  CALCIUM_ROWS.map(([k, ex]) => `  ${k}: ${ex}`).join("\n") + "\n\n" +
  "Cheese goes by HARDNESS: hard and dry to cheese_hard, semi-hard and brined to cheese_semi, " +
  "soft and fresh to cheese_soft, melted to cheese_processed. A plant drink counts as " +
  "fortified ONLY if the name says so. Canned fish counts as fish_with_bones only when the " +
  "bones are eaten. Anything not covered by a row is other_none and its value is 0 — meat, " +
  "fish fillet, eggs, cereals, bread, vegetables, fruit, oils, sweets and drinks all go " +
  "there.\n\n" +
  "Fill \"reason\" FIRST — one short sentence — then the row key, then the milligrams.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const calciumSchema = {
  type: "object",
  properties: {
    reason: { type: "string" },
    verdict: { type: "string" },
    calcium_mg_per_100g: { type: "number" },
  },
  required: ["reason", "verdict", "calcium_mg_per_100g"],
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

let bad = 0;
console.log("продукт                      строка таблицы     мг     опознание");
for (const [name, want] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(calciumPrompt(name, top.definition), calciumSchema, "calcium");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} кальций: ${r.err} ${r.raw ?? ""}`); continue; }
  const got = String(r.obj.verdict).trim();
  const ok = got === want;
  if (!ok) bad++;
  // Наш признак: печень, сердце, мышца млекопитающего и страус тратят недельные граммы.
  const mg = Number(r.obj.calcium_mg_per_100g);
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${got.padEnd(18)} ${String(mg).padEnd(6)}           ${top.definition}`);
  if (!ok) console.log(`     ждали ${want}, модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
