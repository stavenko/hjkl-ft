// Молочно-жировая глобула: цела, разрушена или молочного жира нет вовсе.
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

// [название, ожидаемое: INTACT | BROKEN | NOT_DAIRY]
//
// Молочный жир выходит из вымени в оболочке. Рвут её только три вещи: СБИВАНИЕ,
// ВЫТАПЛИВАНИЕ и плавление с солями-эмульгаторами. Ни закваска, ни нагрев, ни
// выдержка, ни соль, ни заморозка, ни сушка оболочку не трогают.
//
// Разница не косметическая: при одинаковых жирных кислотах жир в целой глобуле
// ведёт себя иначе (сыр против масла: −6.5 % ЛПНП), поэтому такой жир не участвует
// в индикаторе баланса.
const CASES = [
  // Оболочка цела.
  ["Молоко 3.2%", "INTACT"],
  ["Сливки 20%", "INTACT"],
  ["Сметана 15%", "INTACT"],
  ["Кефир 1%", "INTACT"],
  ["Ряженка", "INTACT"],
  ["Йогурт греческий", "INTACT"],
  ["Творог 5 %", "INTACT"],
  ["Творог обезжиренный", "INTACT"],
  ["Сыр российский", "INTACT"],
  ["Моцарелла", "INTACT"],
  ["Брынза", "INTACT"],
  ["Рикотта", "INTACT"],
  ["Молоко сгущённое", "INTACT"],
  ["Мороженое пломбир", "INTACT"],
  // Оболочка разрушена: сбили, вытопили или расплавили с солями.
  ["Сливочное масло", "BROKEN"],
  ["Топлёное масло гхи", "BROKEN"],
  ["Плавленый сыр", "BROKEN"],
  ["Сыр колбасный плавленый", "BROKEN"],
  ["Круассан", "BROKEN"],
  ["Печенье песочное", "BROKEN"],
  ["Молочный шоколад", "BROKEN"],
  // Молочного жира нет вовсе.
  ["Говядина", "NOT_DAIRY"],
  ["Голец", "NOT_DAIRY"],
  ["Яйцо куриное", "NOT_DAIRY"],
  ["Молоко овсяное", "NOT_DAIRY"],
  ["Молоко кокосовое", "NOT_DAIRY"],
  ["Масло подсолнечное", "NOT_DAIRY"],
  ["Чечевица варёная", "NOT_DAIRY"],
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

const globulePrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "Milk fat leaves the udder wrapped in a membrane. Only three things destroy that membrane: " +
  "CHURNING, RENDERING, and MELTING WITH EMULSIFYING SALTS. Nothing else does — not " +
  "fermenting, not souring, not heating, not ageing, not salting, not freezing, not drying, " +
  "not concentrating.\n\n" +
  "Answer with ONE word:\n" +
  "  NOT_DAIRY — there is no milk fat here at all: meat, fish, eggs, vegetable oils, nuts, " +
  "plants, or a plant «milk» made of soy, oat, almond or coconut;\n" +
  "  BROKEN — the milk fat was churned out (butter, spreads), rendered (ghee, clarified " +
  "butter, anhydrous milk fat) or melted with emulsifying salts (processed cheese, cheese " +
  "spread). The same when a cook or a factory added the milk fat AS BUTTER rather than as milk " +
  "or cream: croissants, shortcrust pastry, buttercream, milk chocolate, biscuits;\n" +
  "  INTACT — otherwise the fat is still in its native globules. This is the normal case for " +
  "dairy and it holds however the product was processed short of churning: milk of any fat " +
  "content, cream, sour cream, kefir, ryazhenka, yoghurt, cottage cheese including fat-free, " +
  "ricotta, natural cheeses hard or soft or brined, condensed and powdered milk, whey, ice " +
  "cream. A cheese or a curd that was not churned HAS intact globules, even though it was " +
  "pressed, salted, aged or heated.\n\n" +
  "Fill \"reason\" FIRST — one short sentence — and let the word follow from it.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const globuleSchema = {
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
console.log("продукт                      ответ       глобула  опознание");
for (const [name, want] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(globulePrompt(name, top.definition), globuleSchema, "globule");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} глобула: ${r.err} ${r.raw ?? ""}`); continue; }
  const got = String(r.obj.verdict).trim().toUpperCase();
  const ok = got === want;
  if (!ok) bad++;
  // Наш признак: печень, сердце, мышца млекопитающего и страус тратят недельные граммы.
  const isIntact = got === "INTACT";
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${got.padEnd(16)} ${isIntact ? "да " : "нет"}           ${top.definition}`);
  if (!ok) console.log(`     ждали ${want}, модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
