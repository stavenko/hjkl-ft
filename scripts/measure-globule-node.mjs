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
console.log("продукт                      ответ       глобула  опознание");
for (const [name, want] of CASES) {
  const idp = promptFor("flags", "identify", name);
  const id = await ask(idp.prompt, idp.schema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const np = promptFor("flags", "milk_globule", name, top.definition);
  const r = await ask(np.prompt, np.schema, "milk_globule");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} глобула: ${r.err} ${r.raw ?? ""}`); continue; }
  // Код спрашивает ВЕРДИКТ, а не категорию словом: эталон сводим к нему же.
  const got = r.obj.verdict === true;
  // Категории набора — наши, вердикт кода — булев. Истина здесь: жир заперт в целой молочно-жировой глобуле.
  const YES = ["INTACT"];
  const shouldBe = YES.includes(String(want).toUpperCase());
  const ok = got === shouldBe;
  if (!ok) bad++;
  // Наш признак: печень, сердце, мышца млекопитающего и страус тратят недельные граммы.
  const isIntact = got === "INTACT";
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${(got ? "да " : "нет").padEnd(16)} ${isIntact ? "да " : "нет"}           ${top.definition}`);
  if (!ok) console.log(`     ждали ${shouldBe ? "да" : "нет"} (${want}), модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
