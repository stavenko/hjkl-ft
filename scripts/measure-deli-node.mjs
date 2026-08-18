// Мясная гастрономия: колбасы и окорока против домашней готовки и против рыбы.
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

// [название, ожидаемое: DELI | PLAIN | OTHER]
//
// Мясная гастрономия: колбаса, сосиски, салями, хамон, ветчина промышленного
// производства (диктовка владельца). Домашняя готовка — не гастрономия. Рыба, как
// бы её ни коптили и ни солили, здесь ни при чём: копчёная скумбрия однажды стала
// «86 г колбасы» в дневнике.
const CASES = [
  ["Колбаса докторская", "DELI"],
  ["Сосиски молочные", "DELI"],
  ["Салями", "DELI"],
  ["Хамон", "DELI"],
  ["Ветчина", "DELI"],
  ["Бекон", "DELI"],
  ["Бастурма", "DELI"],
  ["Fuet truffle", "DELI"],
  ["Ливерная колбаса", "DELI"],
  ["Сервелат", "DELI"],
  ["Карбонад копчёный", "DELI"],
  // Мясо, которое просто готовили.
  ["Говядина", "PLAIN"],
  ["Котлеты домашние", "PLAIN"],
  ["Буженина запечённая", "PLAIN"],
  ["Куриная грудка", "PLAIN"],
  ["Язык говяжий отварной", "PLAIN"],
  ["Фарш говяжий", "PLAIN"],
  ["Шашлык из свинины", "PLAIN"],
  // Не мясо вовсе — включая копчёную и солёную рыбу.
  ["Скумбрия холодного копчения", "OTHER"],
  ["Сельдь солёная", "OTHER"],
  ["Икра трески копчёная", "OTHER"],
  ["Крабовые палочки", "OTHER"],
  ["Творог 5 %", "OTHER"],
  ["Яйцо куриное", "OTHER"],
  ["Чечевица варёная", "OTHER"],
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

const deliPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "Is this a MEAT DELICATESSEN product — sausage, frankfurters, wieners, salami, jamon, " +
  "prosciutto, ham, bacon, pastrami, basturma, servelat, liver sausage, canned luncheon meat: " +
  "the kind a factory or a butcher makes for keeping, by curing with nitrite, smoking, long " +
  "salting, air-drying or fermenting?\n\n" +
  "Cooking is not this. Boiled, fried, baked, stewed and grilled meat, mince, cutlets, kebab " +
  "and a boiled tongue stay plain meat, however industrial the kitchen. FISH AND SEAFOOD are " +
  "never this, however they were smoked or salted — smoked mackerel is a smoked fish, not a " +
  "delicatessen meat.\n\n" +
  "Answer with ONE word:\n" +
  "  DELI — a meat delicatessen product;\n" +
  "  PLAIN — meat, but not a delicatessen product;\n" +
  "  OTHER — not meat at all.\n\n" +
  "Fill \"reason\" FIRST — one short sentence — and let the word follow from it.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const deliSchema = {
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
console.log("продукт                      ответ    колбасы  опознание");
for (const [name, want] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(deliPrompt(name, top.definition), deliSchema, "deli");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} гастрономия: ${r.err} ${r.raw ?? ""}`); continue; }
  const got = String(r.obj.verdict).trim().toUpperCase();
  const ok = got === want;
  if (!ok) bad++;
  // Наш признак: печень, сердце, мышца млекопитающего и страус тратят недельные граммы.
  const isDeli = got === "DELI";
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${got.padEnd(16)} ${isDeli ? "да " : "нет"}           ${top.definition}`);
  if (!ok) console.log(`     ждали ${want}, модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
