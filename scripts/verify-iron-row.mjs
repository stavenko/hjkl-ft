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

// КАТЕГОРИЯ ЖЕЛЕЗА отдельным запросом: из неё берётся ДОЛЯ УСВОЕНИЯ.
//
// Долю у модели не спрашивают никогда: без таблицы она даёт чечевице и творогу те же
// 0.20, что и говядине, вчетверо завышая усвоенное. Спрашивается строка, доля берётся
// из неё.
//
// Раньше строку выбирал первый узел конвейера железа, и он же опознавал продукт
// заново — «Голец это говядина». Теперь опознание приходит готовым, и узлу остаётся
// одна работа.
//
// [название, ожидаемая строка]
const CASES = [
  ["Печень куриная", "liver_offal"],
  ["Сердечки куриные", "liver_offal"],
  ["Говядина", "meat_red"],
  ["Свинина", "meat_red"],
  ["Куриная грудка", "meat_poultry"],
  ["Индейка филе", "meat_poultry"],
  ["Колбаса докторская", "meat_processed"],
  ["Голец", "fish"],
  ["Скумбрия", "fish"],
  ["Мидии варёно-мороженые", "shellfish"],
  ["Креветки", "shellfish"],
  ["Икра красная", "roe"],
  ["Чечевица варёная", "legumes"],
  ["Фасоль красная сухая", "legumes"],
  ["Кунжут", "nuts_seeds"],
  ["Миндаль", "nuts_seeds"],
  ["Гречка варёная", "grains_whole"],
  // Бородинский — ржаной цельнозерновой, поэтому grains_whole. Сначала в эталоне
  // стояло grains_refined — это была ошибка эталона, модель отвечала верно.
  ["Хлеб бородинский", "grains_whole"],
  ["Батон нарезной", "grains_refined"],
  ["Шпинат", "spinach_oxalate"],
  ["Петрушка", "greens_herbs"],
  ["Брокколи", "vegetables"],
  ["Яблоко", "fruit_fresh"],
  ["Курага", "fruit_dried"],
  ["Яйцо куриное", "eggs"],
  ["Творог 5 %", "dairy"],
  ["Тофу", "soy"],
  ["Борщ с говядиной", "dish_with_meat"],
  ["Овощное рагу", "dish_meatless"],
  ["Кофе чёрный", "tea_coffee_cocoa"],
  ["Сок апельсиновый", "drinks"],
];

const IRON_ABSORPTION_ROWS = [
  ["liver_offal", 0.25, "печень куриная, печень говяжья, печень свиная, сердце, почки"],
  ["shellfish", 0.25, "мидии, устрицы, гребешки, кальмар, креветки"],
  ["roe", 0.20, "икра красная, икра чёрная"],
  ["meat_red", 0.20, "говядина, телятина, баранина, свинина, кролик"],
  ["meat_poultry", 0.15, "курица, индейка, утка, куриная грудка"],
  ["fish", 0.15, "лосось, треска, тунец, скумбрия, сельдь, голец, форель, горбуша, кета, минтай, судак, щука, палтус, камбала — ЛЮБАЯ рыба"],
  ["meat_processed", 0.15, "колбаса, сосиски, ветчина, бекон, паштет"],
  ["dish_with_meat", 0.12, "плов с мясом, борщ с говядиной, паста болоньезе, пельмени"],
  ["fruit_fresh", 0.10, "яблоко, апельсин, клубника, киви, черешня, арбуз, банан, виноград"],
  ["fruit_dried", 0.10, "курага, изюм, чернослив, инжир сушёный, финики"],
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

const IRON_REFERENCE = [
  ["печень свиная", 18.0],
  ["печень куриная", 9.0],
  ["печень говяжья", 6.9],
  ["печень индейки", 7.5],
  ["печень трески", 1.9],
  ["сердечки куриные", 5.9],
  ["сердце говяжье", 4.3],
  ["почки говяжьи", 5.9],
  ["желудки куриные", 3.2],
  ["язык говяжий", 4.1],
  ["говядина", 2.6],
  ["баранина", 1.9],
  ["крольчатина", 1.3],
  ["свинина", 0.9],
  ["телятина", 1.1],
  ["оленина", 3.4],
  ["куриная грудка", 1.0],
  ["куриное бедро", 1.3],
  ["индейка филе", 1.4],
  ["колбаса варёная", 1.7],
  ["сосиски", 1.8],
  ["салями", 1.5],
  ["ветчина", 1.3],
  ["устрицы", 9.2],
  ["мидии", 6.7],
  ["креветки", 1.8],
  ["гребешки", 0.6],
  ["кальмар", 0.7],
  ["скумбрия", 1.6],
  ["сельдь", 1.1],
  ["лосось", 0.8],
  ["тунец", 1.0],
  ["треска", 0.4],
  ["голец", 0.5],
  ["икра красная", 1.8],
  ["кунжут", 14.6],
  ["семена тыквы", 8.8],
  ["чечевица сухая", 7.5],
  ["фасоль сухая", 6.7],
  ["нут сухой", 6.2],
  ["соя", 15.7],
  ["семена подсолнечника", 5.3],
  ["кешью", 6.7],
  ["фисташки", 4.2],
  ["миндаль", 3.7],
  ["грецкий орех", 2.9],
  ["арахис", 4.6],
  ["чечевица варёная", 3.3],
  ["фасоль варёная", 2.9],
  ["тофу", 5.4],
  ["гречка сухая", 6.7],
  ["гречка варёная", 1.5],
  ["овсянка сухая", 4.3],
  ["овсянка на воде", 1.7],
  ["рис белый варёный", 0.5],
  ["хлеб ржаной", 3.9],
  ["хлеб пшеничный", 1.5],
  ["макароны варёные", 1.3],
  ["петрушка", 6.2],
  ["укроп", 6.6],
  ["шпинат", 2.7],
  ["руккола", 1.5],
  ["брокколи", 0.7],
  ["капуста белокочанная", 0.6],
  ["картофель", 0.9],
  ["морковь", 0.7],
  ["яблоко", 0.1],
  ["банан", 0.3],
  ["курага", 3.2],
  ["изюм", 1.9],
  ["чернослив", 0.9],
  ["яйцо куриное", 1.7],
  ["творог", 0.4],
  ["молоко", 0.1],
  ["сыр твёрдый", 0.7],
  ["шоколад тёмный", 11.9],
  ["какао порошок", 13.9],
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

const ironRowPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "Place this food in ONE row of the table below. The row decides how well its iron is " +
  "absorbed, so what matters is WHAT THE FOOD IS, not how much iron it holds.\n\n" +
  IRON_ABSORPTION_ROWS.map(([k, , ex]) => `  ${k}: ${ex}`).join("\n") + "\n\n" +
  "LIVER, HEART, KIDNEYS and GIZZARDS go to liver_offal whatever the creature they came " +
  "from: chicken liver is liver_offal, not meat_poultry, and so are chicken hearts and beef " +
  "tongue. The row is about the ORGAN, not about the animal.\n" +
  "A cooked dish goes to dish_with_meat when it contains meat and to dish_meatless when it " +
  "does not. Any fish goes to fish, including names you do not recognise — a fish is never " +
  "meat_red. Drinks go to drinks; only milk drinks go to dairy.\n\n" +
  "Fill \"reason\" FIRST — one short sentence — then the row key, copied exactly.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const ironRowSchema = {
  type: "object",
  properties: { reason: { type: "string" }, verdict: { type: "string" } },
  required: ["reason", "verdict"],
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
console.log("продукт                      строка             усвоение  опознание");
for (const [name, want] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(ironRowPrompt(name, top.definition), ironRowSchema, "iron_row");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} строка: ${r.err} ${r.raw ?? ""}`); continue; }
  const row = String(r.obj.verdict).trim();
  const def = IRON_ABSORPTION_ROWS.find(([k]) => k === row);
  const ok = row === want;
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${row.padEnd(18)} ${def ? def[1] : "—"}      ${top.definition}`);
  if (!ok) {
    const w = IRON_ABSORPTION_ROWS.find(([k]) => k === want);
    console.log(`     ждали ${want} (усвоение ${w ? w[1] : "?"}), модель: ${r.obj.reason}`);
  }
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
