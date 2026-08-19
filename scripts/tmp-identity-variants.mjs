// Три формулировки опознания на одном наборе: нынешняя, «пользователь мог ошибиться —
// не фантазируй» и «пользователь мог опечататься».
//
// Настоящая еда должна проходить порог 0.6, выдуманная — нет. Опечатки в настоящих
// словах должны опознаваться: человек торопился, а еда за словом стоит настоящая.
import { readFileSync } from "node:fs";
import { execSync } from "node:child_process";
const AI = "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const THRESHOLD = 0.6;

// Словарь берём ИЗ КОДА, чтобы мерить то, что работает.
const src = readFileSync("frontend/src/services/flags_pipeline.rs", "utf8");
const rareBlock = src.slice(src.indexOf("const RARE_NAMES"), src.indexOf("const SHORTHANDS"));
const rare = [...rareBlock.matchAll(/\("([^"]+)",\s*"([^"]+)",\s*&\[([^\]]*)\]\)/g)]
  .map((m) => [m[1], m[2], m[3].replace(/"/g, "")]);
const shBlock = src.slice(src.indexOf("const SHORTHANDS"));
const short = [...shBlock.slice(0, shBlock.indexOf("];")).matchAll(/\("([^"]+)",\s*"([^"]+)"\)/g)]
  .map((m) => [m[1], m[2]]);

const dictionary =
  "A DICTIONARY of names that are rare or easy to mistake. Each line stands on its own " +
  "and has nothing to do with its neighbours. Match the name you were given against it " +
  "YOURSELF — the word may stand in another grammatical case, in the middle of a phrase, in " +
  "Latin letters or misspelled, and it still counts as the same word.\n" +
  rare.map(([w, d, t]) => `  ${w}: ${d}, could be translated as [${t}]`).join("\n") +
  "\n\nABBREVIATIONS from Russian labels.\n" +
  short.map(([w, d]) => `  ${w}: ${d}`).join("\n") + "\n\n";

const HEAD = (name) => `A person wrote this into their food diary: ${name}\n\n`;
const TASK =
  "Answer three things about it.\n" +
  "1. \"from_own_knowledge\": the closest definition you can recall YOURSELF, without the " +
  "dictionary. If you do not know this word, say so plainly — that is a valid answer.\n" +
  "2. \"from_dictionary\": the closest definition from the dictionary below, or NONE if nothing " +
  "in it fits this name.\n" +
  "3. \"options\": the three most likely definitions of the food, the surest first, each a " +
  "sentence of five or six words, each with your confidence from 0.0 to 1.0.\n\n";

const STRICT =
  "THE NAME MAY BE NO FOOD AT ALL. People mistype, shorten and invent words, and a made-up " +
  "word must not become a food: answer only from what you KNOW or from the dictionary, and " +
  "invent nothing. When neither your own knowledge nor the dictionary holds this word, say so " +
  "and give every option a confidence below 0.2 — a low number is the right answer here, not " +
  "a failure.\n\n";
const TYPO =
  "THE PERSON MAY HAVE MISTYPED. A word that is one or two letters away from a food you know " +
  "is that food — «Гречкаа» is buckwheat, «Творг» is curd. But a word that resembles nothing " +
  "you know is not a food at all, and inventing a meaning for it is worse than admitting you " +
  "do not know it: give every option a confidence below 0.2.\n\n";

const VARIANTS = [
  ["нынешний", (n) => HEAD(n) + TASK + dictionary],
  ["+не фантазируй", (n) => HEAD(n) + TASK + STRICT + dictionary],
  ["+опечатка", (n) => HEAD(n) + TASK + TYPO + dictionary],
  ["оба", (n) => HEAD(n) + TASK + STRICT + TYPO + dictionary],
];

// [имя, должно ли опознаться]
const ALL = [
  ["Голец", true], ["Полба", true], ["Маш", true], ["Кыстыбый", true],
  ["Творог 5%", true], ["Скумбрия х/к", true], ["Fuet truffle", true],
  ["Мороженая вишня", true], ["Куриная грудка", true],
  ["Гречкаа", true], ["Творг 5%", true], ["Скумбия", true],
  ["Зюзюбряк ассорти", false], ["Хряпундель", false], ["Мормышка сладкая", false],
  ["Бубурек копчёный", false], ["Квазилапша", false],
];
const CASES = process.env.FAKE_ONLY
  ? ALL.filter(([, real]) => !real).concat([["Голец", true], ["Творог 5%", true], ["Маш", true]])
  : ALL;

const APP = JSON.parse(readFileSync("/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad/app-id2.json", "utf8"));
const schema = process.env.APP_SCHEMA ? APP.schema : { type: "object", properties: {
  from_own_knowledge: { type: "string" }, from_dictionary: { type: "string" },
  options: { type: "array", items: { type: "object", properties: {
    definition: { type: "string" }, confidence: { type: "number" } },
    required: ["definition", "confidence"], additionalProperties: false } } },
  required: ["from_own_knowledge", "from_dictionary", "options"], additionalProperties: false };

const b64 = (b) => Buffer.from(b).toString("base64url");
const now = Math.floor(Date.now() / 1000);
const data = b64(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64(JSON.stringify({ sub: `var-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const ask = async (text) => {
  const r = await fetch(`${AI}/chat/completions`, { method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: text }],
      response_format: { type: "json_schema", json_schema: { name: "identity", schema } },
      stream: true, think: false, max_tokens: 2000 }) });
  const t = await r.text();
  let out = "";
  for (const line of t.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const p = line.slice(5).trim();
    if (!p || p === "[DONE]") continue;
    try { out += JSON.parse(p)?.choices?.[0]?.delta?.content ?? ""; } catch {}
  }
  try { return JSON.parse(out.trim()); } catch { return null; }
};

const REPEATS = Number(process.env.REPEATS || 1);
if (process.env.DUMP_MINE) {
  const { writeFileSync } = await import("node:fs");
  writeFileSync(process.env.DUMP_MINE, VARIANTS[0][1]("Зюзюбряк ассорти"));
  console.log("промпт скрипта сохранён");
  process.exit(0);
}
for (const [label, build] of VARIANTS.filter(([l]) => !process.env.ONLY_VARIANT || l === process.env.ONLY_VARIANT)) {
  let okReal = 0, real = 0, okFake = 0, fake = 0;
  const misses = [];
  for (const [name, shouldPass] of CASES) {
   for (let rep = 0; rep < REPEATS; rep++) {
    const a = await ask(build(name));
    const opts = (a?.options ?? []).filter((o) => o?.definition);
    const top = opts.reduce((x, y) => (Number(y.confidence) > Number(x.confidence) ? y : x),
      opts[0] ?? { confidence: 0, definition: "(нет)" });
    const passed = Number(top.confidence) >= THRESHOLD;
    if (shouldPass) { real++; if (passed) okReal++; else misses.push(`${name} отсечён (${Number(top.confidence).toFixed(2)})`); }
    else { fake++; if (!passed) okFake++; else misses.push(`${name} прошёл (${Number(top.confidence).toFixed(2)}): ${top.definition.slice(0, 40)}`); }
   }
  }
  console.log(`\n${label}`);
  console.log(`  настоящая еда прошла: ${okReal}/${real} · выдумка отсечена: ${okFake}/${fake}`);
  for (const m of misses) console.log(`    ${m}`);
}
