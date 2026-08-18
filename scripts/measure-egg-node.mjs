// ЯЙЦО ПТИЦЫ: шестой узел конвейера признаков.
//
// Признак тривиален по замыслу — «яйцо птицы, чьё бы ни было и как бы ни
// приготовлено», — но ровно поэтому у него две ловушки, и обе про слово, а не про
// еду: ИКРА (рыбьи яйца, по-русски даже слово другое, а по-английски то самое roe)
// и всё, что зовётся яйцом, не будучи им, — шоколадное яйцо, яйцо-сюрприз.
//
// ВНИМАНИЕ: промпт — копия шага `Step::Egg` из `flags_pipeline.rs`; итоговая
// проверка живым путём.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const THINK = process.env.THINK !== "0";

// [название, ждём ли «да»]
const CASES = [
  // Само яйцо — любой птицы, в любом виде.
  ["Яйцо куриное", true],
  ["Яйца перепелиные", true],
  ["Яйцо утиное", true],
  ["Яйцо страусиное", true],
  ["Яйцо варёное", true],
  ["Яйцо всмятку", true],
  ["Яичница глазунья", true],
  ["Омлет", true],
  ["Белок яичный", true],
  ["Желток яичный", true],
  ["Яичный порошок", true],
  ["Яйцо пашот", true],
  ["Яйцо жареное", true],
  ["Яйца копчёные", true],
  ["Меланж яичный", true],
  // Блюда, где яйцо лишь одно из многого.
  ["Блины", false],
  ["Майонез", false],
  ["Макароны", false],
  ["Сырники", false],
  ["Котлеты куриные", false],
  ["Бисквит", false],
  ["Оливье", false],
  // Слово «яйцо» без яйца.
  ["Киндер сюрприз", false],
  ["Шоколадное яйцо", false],
  // Икра — яйца рыб, а не птиц.
  ["Икра красная", false],
  ["Икра минтая", false],
  ["Молоки лососевые", false],
  // Просто не яйцо.
  ["Куриная грудка", false],
  ["Творог 5%", false],
  ["Гречка варёная", false],
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

const eggPrompt = (name, identity) =>
  `A person wrote this into their food diary: ${name}\n\n` +
  `Our automatic classifier says this product is: ${identity}\n\n` +
  "THE QUESTION: is this food a bird's egg, or a food made of eggs and almost nothing " +
  "else?\n\n" +
  "YES, whoever laid it — hen, quail, duck, goose, turkey, ostrich — and whatever was done to " +
  "it. An egg stays an egg after the shell is cracked and the yolk broken, and it stays an egg " +
  "after cooking, curing or drying: raw, boiled, poached, fried, baked, smoked, pickled, " +
  "salted, or dried into powder. Egg powder — яичный порошок, меланж — is YES: it is whole " +
  "eggs with the water taken out and nothing else. The YOLK and the WHITE on their own are " +
  "YES. So are яичница, глазунья, омлет and scrambled eggs, even with a spoon of milk, butter " +
  "or oil in them — the food is still eggs.\n\n" +
  "NO when eggs are merely ONE INGREDIENT AMONG MANY: pancakes, batter, mayonnaise, pasta, " +
  "cake, biscuit, meringue in a dessert, cutlets, casseroles, salads. NO for anything called " +
  "an egg without being one — a chocolate egg, an egg-shaped sweet.\n\n" +
  "ROE IS NOT AN EGG HERE: caviar, salmon roe, cod roe and fish milt come from fish, not from " +
  "birds.\n\n" +
  "Fill the reason field FIRST — name the ONE category that fits, or say that none does — and " +
  "let the verdict follow from it. Never list the categories that do not fit: running through " +
  "them turns into denying them all, the right one included.\n\n" +
  "Respond with ONLY a minified JSON object and nothing else.";

const eggSchema = {
  type: "object",
  properties: { reason: { type: "string" }, verdict: { type: "boolean" } },
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
console.log("продукт                      яйцо  опознание");
for (const [name, want] of CASES) {
  const id = await ask(identityPrompt(name), identitySchema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const r = await ask(eggPrompt(name, top.definition), eggSchema, "egg");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} яйцо: ${r.err} ${r.raw ?? ""}`); continue; }
  const got = r.obj.verdict === true;
  const ok = got === want;
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${got ? "да " : "нет"}   ${top.definition}`);
  if (!ok) console.log(`     ждали ${want ? "да" : "нет"}, модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
