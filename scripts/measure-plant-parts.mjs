// Части растения + примерная клетчатка — вторым шагом после опознания.
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

// [название, ожидаемая часть, справочная клетчатка г/100 г]
// Часть: root | leaf | fruit | seed | legume | grain | none
const CASES = [
  ["Яблоко", "fruit", 2.4],
  ["Огурец", "fruit", 0.7],
  ["Помидор", "fruit", 1.2],
  ["Черника с/м", "fruit", 3.6],
  ["Брокколи", "leaf", 2.6],
  ["Укроп свежий", "leaf", 2.8],
  ["Сельдерей стебель", "leaf", 1.6],
  ["Капуста белокочанная", "leaf", 2.0],
  ["Картофель отварной", "root", 1.8],
  ["Морковь", "root", 2.8],
  ["Свёкла варёная", "root", 2.5],
  ["Топинамбур", "root", 1.6],
  ["Редис", "root", 1.6],
  ["Чечевица варёная", "legume", 7.9],
  ["Фасоль красная варёная", "legume", 6.4],
  ["Нут варёный", "legume", 7.6],
  ["Соя", "legume", 9.3],
  ["Маш", "legume", 7.6],
  ["Грецкий орех", "seed", 6.7],
  ["Фисташки", "seed", 10.3],
  ["Кедровые орехи", "seed", 3.7],
  ["Семена подсолнечника", "seed", 8.6],
  ["Кунжут", "seed", 11.8],
  ["Полба", "grain", 10.7],
  ["Гречка варёная", "grain", 2.7],
  ["Хлеб бородинский", "grain", 7.0],
  ["Чипсы картофельные", "root", 4.4],
  ["Говядина", "none", 0.0],
  ["Творог 5 %", "none", 0.0],
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

const plantPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "First answer whether this food is a part of some plant at all. If it is, answer which part " +
  "of the plant it is — a root or tuber, a leaf or stalk, a fruit or berry, a seed or nut, a " +
  "legume, a grain. If the food is not from a plant, every part field is false.\n\n" +
  "Then give the dietary FIBRE of this food in grams per 100 g, as you know it. Zero is a " +
  "valid answer for food that has none.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const plantSchema = {
  type: "object",
  properties: {
    is_product_a_part_of_some_plant: { type: "boolean" },
    is_root: { type: "boolean" },
    is_leaf: { type: "boolean" },
    is_fruit: { type: "boolean" },
    is_seed: { type: "boolean" },
    is_legume: { type: "boolean" },
    is_grain: { type: "boolean" },
    fibre_g_per_100g: { type: "number" },
  },
  required: ["is_product_a_part_of_some_plant", "is_root", "is_leaf", "is_fruit", "is_seed",
    "is_legume", "is_grain", "fibre_g_per_100g"],
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

const partOf = (o) => {
  const set = [
    ["root", o.is_root], ["leaf", o.is_leaf], ["fruit", o.is_fruit],
    ["seed", o.is_seed], ["legume", o.is_legume], ["grain", o.is_grain],
  ].filter(([, v]) => v === true).map(([k]) => k);
  if (!o.is_product_a_part_of_some_plant && set.length === 0) return "none";
  return set.length ? set.join("+") : "none";
};

let badPart = 0;
let badFibre = 0;
console.log("продукт                      часть      клетчатка (наша / справка)  наш признак");
for (const [name, wantPart, refFibre] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { badPart++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const pl = await ask(plantPrompt(name, top.definition), plantSchema, "plant");
  if (pl.err) { badPart++; console.log(`FAIL ${name.padEnd(26)} части: ${pl.err} ${pl.raw ?? ""}`); continue; }
  const o = pl.obj;
  const part = partOf(o);
  const okPart = part === wantPart;
  if (!okPart) badPart++;
  // Клетчатка: судим мягко — вдвое туда или обратно от справочной. Нам нужен
  // порядок величины, а не точность граммов.
  const f = Number(o.fibre_g_per_100g);
  const okFibre = refFibre === 0
    ? f <= 0.5
    : f >= refFibre / 2 && f <= refFibre * 2;
  if (!okFibre) badFibre++;
  // Наш признак «овощи и фрукты»: растение, но НЕ корнеплод и НЕ бобовое.
  const vegFruit = o.is_product_a_part_of_some_plant && !o.is_root && !o.is_legume
    && !o.is_grain && !o.is_seed;
  console.log(
    `${okPart && okFibre ? "OK  " : "MISS"} ${name.padEnd(26)} ${part.padEnd(10)} ` +
    `${String(f).padEnd(6)} / ${String(refFibre).padEnd(6)} ${okFibre ? "  " : "≠ "}      ` +
    `${vegFruit ? "овощ/фрукт" : "—"}`
  );
  if (!okPart) console.log(`     ждали часть «${wantPart}», опознание: ${top.definition}`);
}
console.log(`\nчасть растения: ${CASES.length - badPart}/${CASES.length}`);
console.log(`клетчатка:      ${CASES.length - badFibre}/${CASES.length}`);
process.exit(badPart + badFibre ? 1 : 0);
