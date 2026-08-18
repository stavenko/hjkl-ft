// Замер ТОЛЬКО узла опознания: что модель говорит о продукте, до всяких признаков.
//
// Признаки мерить полным конвейером долго — четверть часа на набор, — а опознание
// это один запрос. Здесь и лежит корень: «Голец» модель объявляла кониной, «Сайда»
// бараниной, «Черника с/м» — вишней в сиропе, и только потом верные правила честно
// выдавали неверный вердикт.
//
// ВНИМАНИЕ: промпт и словарь здесь — КОПИЯ того, что в `flags_pipeline.rs`. Копия
// нужна для скорости итераций; она может разойтись с кодом, поэтому итоговую
// проверку всё равно делает живой путь (measure-cross / measure-meat-flags).
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

// ONLY=подстрока[,…] — прогнать две-три позиции вместо всего набора. Итерация промпта
// идёт по одному-двум больным случаям; полный набор — только чтобы закрепить.
//
// [название, что ждём увидеть в опознании — подстрока, регистр не важен]
const CASES = [
  // Редкие рыбы: с них всё началось — «Голец» уходил в конину и говядину.
  ["Голец", "fish"],
  ["Сайда", "fish"],
  ["Пикша", "fish"],
  ["Муксун", "fish"],
  ["Зубатка", "fish"],
  ["Нельма", "fish"],
  ["Кижуч без головы", "fish"],
  // Сокращения и обработка: «с/м» читалось как сироп, «х/к» терялось.
  ["Черника с/м", "berry"],
  ["Скумбрия холодного копчения", "fish"],
  ["Сельдь солёная", "fish"],
  ["Лосось слабосолёный", "fish"],
  ["Икра трески копчёная", "roe"],
  // Мясные имена, на которых опознание врало.
  ["Бастурма", "beef"],
  ["Fuet truffle", "sausage"],
  ["Сосиски молочные", "sausage"],
  ["Ливерная колбаса", "liver"],
  ["Язык говяжий", "tongue"],
  ["Сердечки куриные", "heart"],
  ["Страусятина", "ostrich"],
  ["Буженина запечённая", "pork"],
  // Ягоды и крупы, шумевшие в перекрёстном наборе.
  ["Жимолость", "berry"],
  ["Ирга", "berry"],
  ["Полба", "grain"],
  ["Маш", "bean"],
  ["Хлеб бородинский", "bread"],
  // Сладкое против фрукта и молочное против сбитого.
  ["Вишня в сиропе", "syrup"],
  ["Варенье вишнёвое", "jam"],
  ["Сливочное масло", "butter"],
  // Контроль: то, что модель знает наверняка.
  ["Говядина", "beef"],
  ["Печень говяжья", "liver"],
  ["Яблоко", "apple"],
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
  ["зубатка", "a large northern sea fish", ["wolffish", "sea wolf"]],
  ["муксун", "a northern whitefish", ["muksun"]],
  ["омуль", "a northern whitefish", ["omul"]],
  ["нельма", "a northern whitefish", ["nelma", "Siberian white salmon"]],
  ["кижуч", "a Pacific salmon", ["coho salmon"]],
  ["кета", "a Pacific salmon", ["chum salmon"]],
  ["нерка", "a Pacific salmon", ["sockeye salmon"]],
  ["чавыча", "a Pacific salmon", ["chinook salmon", "king salmon"]],
  ["горбуша", "a Pacific salmon", ["pink salmon"]],
  ["сайра", "a small oily sea fish", ["saury"]],
  ["мойва", "a small oily northern fish", ["capelin"]],
  ["путассу", "a fish of the cod family", ["blue whiting"]],
  ["минтай", "a fish of the cod family", ["Alaska pollock", "walleye pollock"]],
  ["навага", "a northern fish of the cod family", ["navaga", "wachna cod"]],
  ["палтус", "a large flatfish", ["halibut"]],
  ["толстолобик", "a freshwater carp", ["silver carp"]],
  ["пангасиус", "a farmed freshwater catfish", ["pangasius", "basa"]],
  ["страусятина", "the flesh of an ostrich, that is of a BIRD", ["ostrich meat"]],
  ["бастурма", "air-dried cured BEEF, a whole muscle", ["basturma", "pastirma"]],
  ["суджук", "a dry cured sausage of beef or lamb", ["sujuk", "sucuk"]],
  ["буженина", "a whole piece of PORK, baked and not cured", ["buzhenina", "baked pork"]],
  ["fuet", "a Catalan dry cured PORK sausage", ["fuet", "fuet truffle"]],
  ["чоризо", "a Spanish dry cured pork sausage", ["chorizo"]],
];

const SHORT = [
  ["с/м", "fresh-frozen, nothing added"],
  ["св/м", "fresh-frozen, nothing added"],
  ["х/к", "cold-smoked"],
  ["г/к", "hot-smoked"],
  ["с/с", "lightly salted"],
  ["б/г", "headless, said of fish"],
  ["б/к", "skinless"],
  ["в/с", "top grade — a grade and nothing more"],
  ["п/ф", "a semi-prepared product"],
  ["ц/з", "wholegrain"],
];

const dictionary = () => {
  const names = RARE.map(([w, what, tr]) =>
    `  ${w}: ${what}, could be translated as [${tr.join(", ")}]`).join("\n");
  const short = SHORT.map(([w, what]) => `  ${w}: ${what}`).join("\n");
  return "A DICTIONARY of names that are rare or easy to mistake. Each line stands on its own " +
    "and has nothing to do with its neighbours. Match the name you were given against it " +
    "YOURSELF — the word may stand in another grammatical case, in the middle of a phrase, in " +
    "Latin letters or misspelled, and it still counts as the same word.\n" + names +
    "\n\nABBREVIATIONS from Russian labels. Every one of them is about storage, cut or grade — " +
    "none adds an ingredient.\n" + short + "\n\n";
};

const prompt = (name) =>
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

const schema = {
  type: "object",
  properties: {
    from_own_knowledge: { type: "string" },
    from_dictionary: { type: "string" },
    options: {
      type: "array",
      items: {
        type: "object",
        properties: { definition: { type: "string" }, confidence: { type: "number" } },
        required: ["definition", "confidence"],
        additionalProperties: false,
      },
    },
  },
  required: ["from_own_knowledge", "from_dictionary", "options"],
  additionalProperties: false,
};

const b64 = (b) => Buffer.from(b).toString("base64url");
const uid = `ident-${Date.now()}`;
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

// Ответ приходит потоком SSE — собираем content из delta, как это делает приложение.
const ask = async (name) => {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: prompt(name) }],
      response_format: { type: "json_schema", json_schema: { name: "identity", schema } },
      stream: true,
      think: process.env.THINK !== "0",
      max_tokens: Number(process.env.MAXTOK || 8000),
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
  try { return { obj: JSON.parse(clean) }; } catch (e) { return { err: `parse: ${e.message}`, raw: clean.slice(0, 90) }; }
};

let bad = 0;
console.log("продукт                      увер.  лучшее опознание");
for (const [name, want] of CASES) {
  const { obj, err, raw } = await ask(name);
  if (err) {
    bad++;
    console.log(`FAIL ${name.padEnd(28)} ${err} ${raw ?? ""}`);
    continue;
  }
  const opts = Array.isArray(obj.options) ? obj.options : [];
  const top = opts[0] ?? { definition: "(пусто)", confidence: 0 };
  // Судим по ЛУЧШЕМУ варианту: именно он определяет ответ признаков.
  const ok = String(top.definition).toLowerCase().includes(want.toLowerCase());
  if (!ok) bad++;
  const conf = Number(top.confidence ?? 0).toFixed(2);
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(28)} ${conf}  ${top.definition}`);
  console.log(`     память: ${obj.from_own_knowledge} | словарь: ${obj.from_dictionary}`);
  if (!ok) {
    console.log(`     ждали «${want}», прочие версии: ` +
      opts.slice(1).map((o) => `${o.definition} (${Number(o.confidence).toFixed(2)})`).join("; "));
  }
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
