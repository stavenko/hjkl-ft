// ЯЙЦО ПТИЦЫ: шестой узел конвейера признаков.
//
// Признак тривиален по замыслу — «яйцо птицы, чьё бы ни было и как бы ни
// приготовлено», — но ровно поэтому у него две ловушки, и обе про слово, а не про
// еду: ИКРА (рыбьи яйца, по-русски даже слово другое, а по-английски то самое roe)
// и всё, что зовётся яйцом, не будучи им, — шоколадное яйцо, яйцо-сюрприз.
//
// ПРОМПТ И СХЕМА БЕРУТСЯ ИЗ КОДА (scripts/prompts.json, см. scripts/lib/prompts.mjs) —
// ровно те, что уходят в модель из приложения. Копий здесь больше нет: они дважды
// разошлись с кодом, и оба раза замер показывал не то, что работает.
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
  ["Салат с яйцом", false],
  ["Запеканка творожная", false],
  ["Яичный салат", false],
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

import { promptFor } from "./lib/prompts.mjs";

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
  const idp = promptFor("flags", "identify", name);
  const id = await ask(idp.prompt, idp.schema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const eggp = promptFor("flags", "egg", name, top.definition);
  const r = await ask(eggp.prompt, eggp.schema, "egg");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} яйцо: ${r.err} ${r.raw ?? ""}`); continue; }
  const got = r.obj.is_this_product_of_bird_eggs === true;
  const ok = got === want;
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${got ? "да " : "нет"}   ${top.definition}`);
  if (!ok) console.log(`     ждали ${want ? "да" : "нет"}, модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
