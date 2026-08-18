// ВЕРИФИКАЦИЯ ЖИРОВ: развилка «продукт или блюдо», запись справочника, строка таблицы.
//
// Меряется три разных дела, и путать их нельзя:
//   РАЗВИЛКА — базовый продукт не должен уходить в ветку блюда и наоборот;
//   КЛЮЧ    — продукт, который ЕСТЬ в справочнике, должен быть в нём найден: иначе
//             профиль возьмётся из строки, а строка описывает группу (пальмовое масло
//             в строке кокоса получило бы 82–92 % насыщенных вместо своих ~49);
//   СТРОКА  — продукт, которого в справочнике НЕТ, должен попасть в верную строку:
//             её границы — единственное, что тогда держит ответ.
//
// ВНИМАНИЕ: промпты — копия того, что лежит в `ai.rs`; итоговая проверка живым путём.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const THINK = process.env.THINK !== "0";

// [группа, название, ожидание]
//   dish             — должно уйти в ветку составного блюда;
//   {ref: "ключ"}    — должно найтись в справочнике под этим ключом;
//   {row: "строка"}  — в справочнике этого нет, ждём эту строку таблицы.
const CASES = [
  ["справочник: масла", "Оливковое масло", { ref: "оливковое масло" }],
  ["справочник: масла", "Пальмовое масло", { ref: "пальмовое масло" }],
  ["справочник: масла", "Масло сливочное 82%", { ref: "сливочное масло" }],
  ["справочник: масла", "Сало солёное", { ref: "сало свиное" }],
  ["справочник: орехи", "Кедровые орехи", { ref: "кедровый орех" }],
  ["справочник: орехи", "Макадамия", { ref: "макадамия" }],
  ["справочник: орехи", "Грецкий орех", { ref: "грецкий орех" }],
  ["справочник: орехи", "Семена чиа", { ref: "семена чиа" }],
  ["справочник: орехи", "Авокадо", { ref: "авокадо" }],
  ["справочник: рыба", "Скумбрия х/к", { ref: "скумбрия" }],
  ["справочник: рыба", "Треска отварная", { ref: "треска" }],
  ["справочник: рыба", "Тилапия филе", { ref: "тилапия" }],
  ["справочник: рыба", "Креветки варёные", { ref: "креветки" }],
  ["справочник: прочее", "Яйцо куриное", { ref: "яйцо куриное" }],
  ["справочник: прочее", "Говядина", { ref: "говядина" }],

  ["вне справочника", "Сыр бри", { row: "dairy_fat" }],
  ["вне справочника", "Кабачок тушёный", { row: "no_fat" }],
  ["вне справочника", "Гречка варёная", { row: "grain_legume" }],
  ["вне справочника", "Вафли шоколадные", { row: "cocoa_confectionery" }],
  ["вне справочника", "Сибас", { row: "fish_warm_low_n3" }],
  ["вне справочника", "Гребешки", { row: "fish_lean_seafood" }],
  ["вне справочника", "Горбуша", { row: "fish_fatty_cold" }],
  ["вне справочника", "Баранина", { ref: "баранина" }],
  ["вне справочника", "Фундук", { ref: "фундук" }],

  ["блюда", "Голубцы", "dish"],
  ["блюда", "Плов с бараниной", "dish"],
  ["блюда", "Оливье", "dish"],
  ["блюда", "Борщ с говядиной", "dish"],
];

// (имя, SFA, MUFA, PUFA, EPA+DHA) — копия FAT_REFERENCE.
const FAT_REFERENCE = [
  ["оливковое масло", 14, 73, 11, 0],
  ["подсолнечное масло", 11, 20, 66, 0],
  ["высокоолеиновое подсолнечное масло", 9, 83, 4, 0],
  ["кукурузное масло", 13, 28, 55, 0],
  ["соевое масло", 15, 23, 58, 0],
  ["рапсовое масло", 7, 63, 28, 0],
  ["льняное масло", 9, 18, 68, 0],
  ["кунжутное масло", 14, 40, 42, 0],
  ["арахисовое масло", 17, 46, 32, 0],
  ["кокосовое масло", 87, 6, 2, 0],
  ["пальмовое масло", 49, 37, 9, 0],
  ["сливочное масло", 63, 26, 4, 0],
  ["топлёное масло", 65, 25, 4, 0],
  ["сало свиное", 39, 45, 11, 0],
  ["говяжий жир", 42, 44, 4, 0],
  ["куриный жир", 30, 45, 21, 0],
  ["грецкий орех", 9, 14, 72, 0],
  ["миндаль", 8, 65, 25, 0],
  ["фундук", 8, 78, 10, 0],
  ["кешью", 20, 59, 17, 0],
  ["фисташки", 13, 54, 31, 0],
  ["кедровый орех", 7, 28, 62, 0],
  ["бразильский орех", 24, 37, 36, 0],
  ["макадамия", 16, 79, 2, 0],
  ["арахис", 17, 49, 31, 0],
  ["семена подсолнечника", 11, 19, 66, 0],
  ["семена тыквы", 19, 34, 45, 0],
  ["кунжут", 15, 40, 42, 0],
  ["семена чиа", 11, 8, 78, 0],
  ["семена льна", 9, 19, 68, 0],
  ["авокадо", 15, 68, 13, 0],
  ["оливки", 13, 74, 9, 0],
  ["говядина", 40, 45, 4, 0],
  ["баранина", 45, 42, 6, 0],
  ["свинина", 37, 45, 12, 0],
  ["курица", 29, 44, 21, 0],
  ["индейка", 30, 40, 25, 0],
  ["утка", 33, 49, 13, 0],
  ["яйцо куриное", 31, 44, 15, 1],
  ["кижуч", 21, 36, 34, 18],
  ["сельдь", 23, 41, 24, 17],
  ["лосось", 23, 28, 29, 15],
  ["скумбрия", 24, 40, 24, 18],
  ["печень трески", 18, 50, 25, 20],
  ["треска", 21, 15, 47, 40],
  ["тунец", 26, 26, 30, 25],
  ["креветки", 25, 17, 40, 35],
  ["мидии", 22, 22, 35, 28],
  ["тилапия", 35, 33, 24, 5],
  ["пангасиус", 38, 40, 17, 2],
  ["молоко", 63, 26, 4, 0],
  ["творог", 63, 26, 4, 0],
  ["сыр твёрдый", 63, 26, 4, 0],
  ["сметана", 63, 26, 4, 0],
  ["шоколад тёмный", 60, 33, 3, 0],
  ["какао порошок", 60, 33, 3, 0],
];

// [ключ, sfa, mufa, pufa, epa_dha, примеры] — копия FAT_ROWS.
const FAT_ROWS = [
  ["coconut_palm", [82, 92], [5, 12], [1, 3], [0, 0],
    "кокосовое масло, пальмовое масло, кокосовая стружка, кокосовое молоко, пальмоядровый жир"],
  ["dairy_fat", [60, 70], [22, 30], [2, 6], [0, 0],
    "сливочное масло, топлёное масло, сливки, сметана, сыр, молоко, творог, мороженое, кефир"],
  ["beef_lamb_fat", [38, 48], [40, 50], [2, 6], [0, 0],
    "говядина, телятина, баранина, курдючное сало, говяжий жир, субпродукты жвачных"],
  ["pork_fat", [33, 40], [42, 50], [8, 14], [0, 0],
    "свинина, сало, шпик, бекон, ветчина, колбаса, сосиски, свиные субпродукты"],
  ["poultry_fat", [26, 32], [40, 48], [18, 26], [0, 1],
    "курица, индейка, утка, гусь, куриная кожа, куриный жир, куриная печень"],
  ["egg_fat", [28, 34], [40, 48], [12, 18], [0, 2], "яйцо, яичный желток, омлет, яичница"],
  ["olive_avocado", [12, 18], [65, 75], [10, 16], [0, 0],
    "оливковое масло, оливки, маслины, авокадо, масло авокадо"],
  ["rapeseed_mustard", [6, 9], [58, 66], [26, 32], [0, 0],
    "рапсовое масло, каноловое масло, горчичное масло, семена горчицы"],
  ["peanut", [15, 20], [45, 55], [25, 35], [0, 0], "арахис, арахисовое масло, арахисовая паста"],
  ["seed_oils_linoleic", [10, 15], [18, 28], [55, 68], [0, 0],
    "подсолнечное масло, кукурузное масло, соевое масло, масло виноградных семечек, семечки подсолнуха, тыквенные семечки, кунжут, тахини, майонез, маргарин"],
  ["high_oleic_oils", [7, 12], [75, 85], [5, 12], [0, 0],
    "высокоолеиновое подсолнечное, высокоолеиновое сафлоровое, олеиновый подсолнечник"],
  ["flax_chia", [8, 11], [15, 22], [66, 75], [0, 0],
    "льняное масло, семена льна, семена чиа, рыжиковое масло"],
  ["walnut_hemp", [9, 12], [13, 22], [65, 75], [0, 0],
    "грецкий орех, конопляное семя, масло грецкого ореха"],
  ["fish_fatty_cold", [20, 25], [28, 42], [24, 34], [13, 20],
    "скумбрия, сельдь, лосось, форель, сардина, шпроты, анчоус, икра, рыбий жир, печень трески, голец, кета, чавыча, нерка, горбуша, сайра, мойва, палтус"],
  ["fish_lean_seafood", [18, 25], [12, 20], [30, 45], [25, 40],
    "треска, минтай, навага, судак, тунец, креветки, мидии, кальмар, гребешок, крабовые палочки"],
  ["fish_warm_low_n3", [25, 35], [25, 40], [20, 30], [3, 8],
    "тилапия, пангасиус, баса, дорадо, сибас, карп, толстолобик, сом — тепловодная и прудовая рыба"],
  ["nuts_tree", [7, 14], [55, 70], [15, 30], [0, 0],
    "миндаль, фундук, кешью, фисташки, кедровый орех, бразильский орех, макадамия"],
  ["cocoa_confectionery", [55, 65], [28, 35], [2, 5], [0, 0],
    "шоколад, какао, какао-масло, конфеты, вафли, печенье, торт, кондитерский жир"],
  ["grain_legume", [15, 22], [12, 25], [45, 60], [0, 0],
    "крупы, хлеб, макароны, рис, овсянка, бобовые, чечевица, нут, соя, тофу"],
  ["no_fat", [0, 0], [0, 0], [0, 0], [0, 0],
    "овощи, фрукты, зелень, сахар, мёд, вода, чай, кофе, алкоголь, желатин, специи — всё, где жира практически нет"],
];

if (process.env.ONLY) {
  const want = process.env.ONLY.split(",").map((x) => x.trim().toLowerCase());
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

const head = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n`;

// Копия `is_composite_dish`.
const compositePrompt = (name, identity) =>
  head(name, identity) +
  "Does the FAT of this food come from SEVERAL DIFFERENT SOURCES mixed together?\n\n" +
  "A food whose fat comes from ONE source is NOT a composite dish, however processed it is. " +
  "These are of one source:\n" +
  "— the flesh, organs, fat or skin of one animal, bird or fish, raw or cooked;\n" +
  "— one plant and what is pressed or ground from it: an oil, a nut, a seed, a grain, a " +
  "vegetable, a fruit;\n" +
  "— milk and everything made from milk alone: butter, cream, cheese, curd, yoghurt;\n" +
  "— eggs;\n" +
  "— a factory product built around one of these: sausage, ham, tinned fish, nut paste.\n\n" +
  "A food whose fat comes from SEVERAL sources IS a composite dish. Typically these are cooked " +
  "from a recipe: stuffed vegetables, cabbage rolls, cutlets and meatballs, casseroles and " +
  "bakes, pies and pastry with a filling, pilaf, stews and soups, salads with dressing, " +
  "sandwiches, ready meals.\n\n" +
  "ANYTHING COOKED FROM A RECIPE OF SEVERAL FOODS IS A COMPOSITE DISH — even when one of them " +
  "plainly carries most of the fat. A soup with meat is a composite dish: besides the meat " +
  "there is the oil it was fried in. The question is where the fat COMES FROM, not which " +
  "source of it is the largest.\n\n" +
  "Fill the reason FIRST — name what this food is made of and where its fat comes from — and " +
  "let the verdict follow from it.\n\n" +
  "Respond with ONLY a single minified JSON object.";

const compositeSchema = {
  type: "object",
  properties: {
    reason_where_the_fat_of_this_food_comes_from: { type: "string" },
    verdict_is_a_composite_dish: { type: "boolean" },
  },
  required: ["reason_where_the_fat_of_this_food_comes_from", "verdict_is_a_composite_dish"],
  additionalProperties: false,
};

// Копия `lookup_basic_fat_profile`.
const profilePrompt = (name, identity) =>
  head(name, identity) +
  "Report the FATTY-ACID PROFILE OF THIS FOOD'S FAT.\n\n" +
  "Every number you report is a PERCENT OF THIS FOOD'S TOTAL FAT, by weight — NOT grams per " +
  "100 g of food. A food's total fat is already known to us; you only describe how that fat is " +
  "composed. This means the profile does NOT depend on how much fat the food has: pure " +
  "sunflower oil and a sauce made from it share the same profile.\n\n" +
  "FIRST look for the food in the REFERENCE below. If it is there — or is plainly the same " +
  "food under another name, in another grammatical case or with a cut, grade or fat percentage " +
  "attached — put that entry's name into \"reference_key\", copied exactly.\n\n" +
  FAT_REFERENCE.map(([n, s, m, p, e]) =>
    `  ${n}: SFA ${s}, MUFA ${m}, PUFA ${p}, EPA+DHA ${e}`).join("\n") + "\n\n" +
  "Whether or not you found it, ALSO place the food in one row of the table below — rows are " +
  "by the TYPE OF FAT, not by the type of dish — and answer with that row's key. If the " +
  "reference had nothing, the values must fit that row's ranges.\n\n" +
  FAT_ROWS.map(([k, s, m, p, e, ex]) =>
    `  ${k.padEnd(20)} SFA ${s[0]}–${s[1]}, MUFA ${m[0]}–${m[1]}, PUFA ${p[0]}–${p[1]}, ` +
    `EPA+DHA ${e[0]}–${e[1]} — ${ex}`).join("\n") + "\n\n" +
  "Report:\n" +
  "- reference_key: the reference entry's name, copied exactly, or NONE\n" +
  "- category: the row key, copied EXACTLY as written above\n" +
  "- food_type: what this product is, in two or three words, in Russian\n" +
  "- sfa_pct / mufa_pct / pufa_pct: percent of total fat, each INSIDE its range in the chosen " +
  "row. The remaining weight is glycerol, so the three need not add up to 100 — never inflate " +
  "a fraction to make them.\n" +
  "- epa_dha_pct: EPA plus DHA together, percent of total fat, inside the row's range. Also " +
  "part of pufa_pct. Zero for everything that is not fish, seafood or fish oil — plants " +
  "contain NO EPA or DHA, however rich in omega-3 they are.\n" +
  "- reason: ONE short sentence (under 20 words) in Russian — why this row and this profile\n\n" +
  "Rules for choosing the row:\n" +
  "- Go by WHERE THE FAT COMES FROM. A dish fried in sunflower oil whose own fat is mostly " +
  "that oil belongs to the oil's row, not to the row of the vegetable.\n" +
  "- A mixed dish goes to the row of its DOMINANT fat source.\n" +
  "- Cooking, freezing, canning and packaging do not change the profile. The row does not " +
  "depend on them.\n" +
  "- A plain name goes to the ORDINARY variety of that oil. Only an explicit «высокоолеиновое» " +
  "/ «high-oleic» in the name moves sunflower or safflower oil to high_oleic_oils.\n" +
  "- Fish splits by WHERE IT LIVES, not by how fatty it is: cold-water sea fish carry EPA and " +
  "DHA, while warm-water and farmed pond fish carry mostly linoleic acid and very little n-3. " +
  "A fish you do not recognise as a cold-water sea species goes to fish_warm_low_n3.\n" +
  "- Anything with practically no fat goes to no_fat; its profile is irrelevant, report the " +
  "row and zeros.\n\n" +
  "Fill \"reason\" FIRST, then the two keys, then the numbers.\n\n" +
  "Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose.";

const profileSchema = {
  type: "object",
  properties: {
    reason: { type: "string" },
    reference_key: { type: "string" },
    category: { type: "string" },
    food_type: { type: "string" },
    sfa_pct: { type: "number" },
    mufa_pct: { type: "number" },
    pufa_pct: { type: "number" },
    epa_dha_pct: { type: "number" },
  },
  required: ["reason", "reference_key", "category", "food_type", "sfa_pct", "mufa_pct",
    "pufa_pct", "epa_dha_pct"],
  additionalProperties: false,
};

const b64 = (b) => Buffer.from(b).toString("base64url");
const uid = `fat-${Date.now()}`;
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
let group = "";
console.log("продукт                      развилка  откуда взяли              профиль");
for (const [g, name, want] of CASES) {
  if (g !== group) { group = g; console.log(`\n── ${g} ──`); }
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });

  const c = await ask(compositePrompt(name, top.definition), compositeSchema, "composite");
  if (c.err) { bad++; console.log(`FAIL ${name.padEnd(26)} развилка: ${c.err} ${c.raw ?? ""}`); continue; }
  const isDish = c.obj.verdict_is_a_composite_dish === true;

  if (want === "dish") {
    if (!isDish) { bad++; console.log(`MISS ${name.padEnd(26)} ждали блюдо, получили продукт`);
      console.log(`     ${c.obj.reason_where_the_fat_of_this_food_comes_from}`); }
    else console.log(`OK   ${name.padEnd(26)} блюдо`);
    continue;
  }
  if (isDish) {
    bad++;
    console.log(`MISS ${name.padEnd(26)} ждали продукт, ушло в ветку блюда`);
    console.log(`     ${c.obj.reason_where_the_fat_of_this_food_comes_from}`);
    continue;
  }

  const r = await ask(profilePrompt(name, top.definition), profileSchema, "fat");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} профиль: ${r.err} ${r.raw ?? ""}`); continue; }
  const hit = FAT_REFERENCE.find(([n]) =>
    n.toLowerCase() === String(r.obj.reference_key).trim().toLowerCase());
  const row = String(r.obj.category).trim().toLowerCase();
  const rowDef = FAT_ROWS.find(([k]) => k === row);
  const clamp = (x, [lo, hi]) => Math.min(Math.max(x, lo), hi);
  let profile;
  let src;
  if (hit) {
    profile = [hit[1], hit[2], hit[3], Math.min(hit[4], hit[3])];
    src = `справочник «${hit[0]}»`;
  } else if (rowDef) {
    const pufa = clamp(r.obj.pufa_pct, rowDef[3]);
    profile = [clamp(r.obj.sfa_pct, rowDef[1]), clamp(r.obj.mufa_pct, rowDef[2]), pufa,
      Math.min(clamp(r.obj.epa_dha_pct, rowDef[4]), pufa)];
    src = `строка ${row}`;
  } else {
    bad++;
    console.log(`MISS ${name.padEnd(26)} неизвестная строка «${r.obj.category}»`);
    continue;
  }
  const ok = want.ref ? hit && hit[0] === want.ref : (!hit && row === want.row);
  if (!ok) bad++;
  console.log(
    `${ok ? "OK  " : "MISS"} ${name.padEnd(26)} продукт   ${src.padEnd(24)} ` +
    profile.map((x) => Number(x).toFixed(0)).join("/")
  );
  if (!ok) {
    console.log(`     ждали ${want.ref ? `справочник «${want.ref}»` : `строку ${want.row}`}` +
      `, модель: ${r.obj.reason}`);
  }
}
console.log(`\nверно: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
