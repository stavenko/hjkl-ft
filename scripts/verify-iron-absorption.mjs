// ВЕРИФИКАЦИЯ УСВОЕНИЯ ЖЕЛЕЗА: долю называет МОДЕЛЬ, и вопрос ровно один — берёт
// ли она её оттуда, куда сама же отнесла продукт.
//
// Проверяем два разных дела:
//   ТОЧНО — совпало ли возвращённое число с тем, что стоит рядом с названной ею
//           записью справочника (а вне справочника — рядом с названной строкой);
//   ВЕРНО — совпало ли оно с долей, которая у этого продукта в справочнике на самом
//           деле. Модель может переписать своё число слово в слово и всё равно
//           ошибиться записью.
//
// ВНИМАНИЕ: промпт — копия шага `iron.absorption` из `iron_pipeline.rs`; итоговая
// проверка живым путём.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const THINK = process.env.THINK !== "0";

// [группа, название, справочная доля усвоения]
const CASES = [
  ["субпродукты", "Печень куриная", 0.25],
  ["субпродукты", "Печень говяжья", 0.25],
  ["субпродукты", "Печень свиная", 0.25],
  ["субпродукты", "Сердечки куриные", 0.25],
  ["мясо", "Говядина", 0.2],
  ["мясо", "Свинина", 0.2],
  ["мясо", "Баранина", 0.2],
  ["мясо", "Куриная грудка", 0.15],
  ["мясо", "Индейка филе", 0.15],
  ["рыба и морепродукты", "Голец", 0.15],
  ["рыба и морепродукты", "Скумбрия", 0.15],
  ["рыба и морепродукты", "Мидии варёно-мороженые", 0.25],
  ["рыба и морепродукты", "Кальмар", 0.25],
  ["рыба и морепродукты", "Креветки", 0.25],
  ["бобовые и орехи", "Чечевица варёная", 0.05],
  ["бобовые и орехи", "Фасоль красная сухая", 0.05],
  ["бобовые и орехи", "Нут сухой", 0.05],
  ["бобовые и орехи", "Кунжут", 0.04],
  ["бобовые и орехи", "Миндаль", 0.04],
  ["бобовые и орехи", "Семена тыквы", 0.04],
  ["крупы и хлеб", "Гречка варёная", 0.04],
  ["крупы и хлеб", "Овсянка на воде", 0.04],
  ["крупы и хлеб", "Хлеб бородинский", 0.04],
  ["зелень и овощи", "Шпинат", 0.02],
  ["зелень и овощи", "Петрушка", 0.08],
  ["зелень и овощи", "Брокколи", 0.08],
  ["прочее", "Яйцо куриное", 0.04],
  ["прочее", "Творог 5 %", 0.02],
  ["прочее", "Курага", 0.1],
  ["прочее", "Яблоко", 0.1],
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
  ["оливковое масло", 0.1, 0.08],
  ["подсолнечное масло", 0.1, 0.08],
  ["сливочное масло", 0.1, 0.02],
  ["сало свиное", 0.4, 0.15],
  ["сахар", 0.0, 0.08],
  ["мёд", 0.4, 0.08],
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

// [ключ строки, доля, примеры]
const IRON_ROWS = [
  ["liver_offal", 0.25, "печень куриная, печень говяжья, печень свиная, сердце, почки"],
  ["shellfish", 0.25, "мидии, устрицы, гребешки, кальмар, креветки"],
  ["roe", 0.2, "икра красная, икра чёрная"],
  ["meat_red", 0.2, "говядина, телятина, баранина, свинина, кролик"],
  ["meat_poultry", 0.15, "курица, индейка, утка, куриная грудка"],
  ["fish", 0.15, "лосось, треска, тунец, скумбрия, сельдь, голец, форель, горбуша, кета, минтай, судак, щука, палтус, камбала — ЛЮБАЯ рыба"],
  ["meat_processed", 0.15, "колбаса, сосиски, ветчина, бекон, паштет"],
  ["dish_with_meat", 0.12, "плов с мясом, борщ с говядиной, паста болоньезе, пельмени"],
  ["fruit_fresh", 0.1, "яблоко, апельсин, клубника, киви, черешня, арбуз, банан, виноград"],
  ["fruit_dried", 0.1, "курага, изюм, чернослив, инжир сушёный, финики"],
  ["vegetables", 0.08, "болгарский перец, помидор, огурец, брокколи, капуста, морковь, кабачок, лук, картофель"],
  ["greens_herbs", 0.08, "петрушка, укроп, руккола, базилик, кинза, зелёный лук"],
  ["grains_refined", 0.08, "белый хлеб, белый рис, макароны, манка"],
  ["fortified", 0.05, "хлопья с добавленным железом, каши быстрого приготовления, детские смеси"],
  ["dish_meatless", 0.05, "овощное рагу, вегетарианский суп, каша на воде"],
  ["legumes", 0.05, "фасоль, нут, чечевица, горох, маш (в сухом виде)"],
  ["grains_whole", 0.04, "гречка, овсянка, бурый рис, цельнозерновой хлеб, булгур"],
  ["nuts_seeds", 0.04, "кунжут, кешью, миндаль, тыквенные семечки, фундук"],
  ["eggs", 0.04, "яйцо куриное, омлет, яичница"],
  ["soy", 0.03, "тофу, соевое молоко, соевое мясо, эдамаме"],
  ["spinach_oxalate", 0.02, "шпинат, щавель, свёкла, ревень"],
  ["dairy", 0.02, "молоко, творог, сыр, йогурт, кефир, ряженка, ацидофилин, сливки"],
  ["drinks", 0.02, "вода, сок, морс, лимонад, пиво, безалкогольное пиво, вино, компот, квас"],
  ["tea_coffee_cocoa", 0.02, "чай, кофе, какао, шоколад"],
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

// Копия шага `iron.absorption`.
const absorptionPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "What FRACTION of this food's iron does the body actually take up? Heme iron — from liver, " +
  "offal, meat and shellfish — absorbs several times better than the non-heme iron of plants, " +
  "and tea, coffee and oxalates hold it back further.\n\n" +
  "FIRST look for the food in the REFERENCE below. If it is there — or is plainly the same " +
  "food under another name, in another grammatical case or with a cut or grade attached — put " +
  "that entry's name into \"reference_key\", copied exactly, and answer \"absorbed_fraction\" " +
  "with THAT ENTRY'S NUMBER, copied exactly. Do not round it, do not adjust it, do not replace " +
  "it with one you remember.\n\n" +
  IRON_REFERENCE.map(([n, , a]) => `  ${n}: ${a}`).join("\n") + "\n\n" +
  "Whether or not you found it, ALSO place the food in one row of the table below and answer " +
  "with that row's key. The number after the key is that row's fraction; if the reference had " +
  "nothing, answer \"reference_key\" with NONE and give THAT ROW'S number as the fraction.\n\n" +
  IRON_ROWS.map(([k, a, ex]) => `  ${k.padEnd(20)} ${String(a).padEnd(5)} — ${ex}`).join("\n") + "\n\n" +
  "Fill \"reason\" FIRST, then the two keys, then the fraction.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const absorptionSchema = {
  type: "object",
  properties: {
    reason: { type: "string" },
    reference_key: { type: "string" },
    category: { type: "string" },
    absorbed_fraction: { type: "number" },
  },
  required: ["reason", "reference_key", "category", "absorbed_fraction"],
  additionalProperties: false,
};

const b64 = (b) => Buffer.from(b).toString("base64url");
const uid = `absorb-${Date.now()}`;
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

let badCopy = 0;
let badValue = 0;
let group = "";
console.log("продукт                      откуда взяла              вернула  там стоит  справка");
for (const [g, name, wantFraction] of CASES) {
  if (g !== group) { group = g; console.log(`\n── ${g} ──`); }
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { badCopy++; badValue++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(absorptionPrompt(name, top.definition), absorptionSchema, "absorption");
  if (r.err) { badCopy++; badValue++; console.log(`FAIL ${name.padEnd(26)} усвоение: ${r.err} ${r.raw ?? ""}`); continue; }

  const got = Number(r.obj.absorbed_fraction);
  const hit = IRON_REFERENCE.find(([n]) => n.toLowerCase() === String(r.obj.reference_key).trim().toLowerCase());
  const row = IRON_ROWS.find(([k]) => k === String(r.obj.category).trim().toLowerCase());
  // Что стояло у той записи, которую она назвала сама.
  const shown = hit ? hit[2] : (row ? row[1] : null);
  const src = hit ? `справочник «${hit[0]}»` : (row ? `строка ${row[0]}` : `НЕИЗВЕСТНО «${r.obj.category}»`);
  const okCopy = shown !== null && Math.abs(got - shown) < 1e-9;
  const okValue = Math.abs(got - wantFraction) < 1e-9;
  if (!okCopy) badCopy++;
  if (!okValue) badValue++;
  console.log(
    `${okCopy && okValue ? "OK  " : "MISS"} ${name.padEnd(26)} ${src.padEnd(24)} ` +
    `${String(got).padEnd(8)} ${String(shown ?? "—").padEnd(10)} ${wantFraction}`
  );
  if (!okCopy || !okValue) console.log(`     ${r.obj.reason}`);
}
console.log(`\nпереписала точно: ${CASES.length - badCopy}/${CASES.length}`);
console.log(`доля верна:       ${CASES.length - badValue}/${CASES.length}`);
process.exit(badCopy + badValue ? 1 : 0);
