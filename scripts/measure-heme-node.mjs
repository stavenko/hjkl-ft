// Гемовое железо: печень, органы, мясо млекопитающих, моллюски — вторым шагом.
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

// [название, ожидаемое: LIVER | ORGAN | MAMMAL_MEAT | MOLLUSC | OTHER]
//
// Гем — про ЦЕЛЬ: три порции в неделю. Входят печень любого животного, птицы и рыбы,
// прочие внутренние органы, мясо млекопитающих и моллюски перечисленных видов.
const CASES = [
  // Печень — любого животного, птицы и рыбы.
  ["Печень говяжья", "LIVER"],
  ["Печень куриная", "LIVER"],
  ["Печень цыплят-бройлеров", "LIVER"],
  ["Печень индейки", "LIVER"],
  ["Печень трески", "LIVER"],
  ["Паштет из куриной печени", "LIVER"],
  ["Ливерная колбаса", "LIVER"],
  // Прочие внутренние органы.
  ["Сердечки куриные", "ORGAN"],
  ["Желудки куриные", "ORGAN"],
  ["Язык говяжий", "ORGAN"],
  ["Почки свиные", "ORGAN"],
  // Мясо млекопитающих, включая изделия из него.
  ["Говядина", "MAMMAL_MEAT"],
  ["Свинина", "MAMMAL_MEAT"],
  ["Баранина", "MAMMAL_MEAT"],
  ["Колбаса докторская", "MAMMAL_MEAT"],
  ["Салями", "MAMMAL_MEAT"],
  ["Крольчатина", "MAMMAL_MEAT"],
  // Моллюски — только перечисленных видов.
  ["Мидии варёно-мороженые", "MOLLUSC"],
  ["Устрицы", "MOLLUSC"],
  ["Осьминог", "MOLLUSC"],
  // Мимо: кальмар (моллюск, но не из списка), ракообразные, птица, рыба, растения.
  ["Кальмар", "OTHER"],
  ["Креветки", "OTHER"],
  ["Куриная грудка", "OTHER"],
  ["Бедро куриное печёное", "OTHER"],
  ["Индейка филе", "OTHER"],
  ["Голец", "OTHER"],
  ["Кижуч без головы", "OTHER"],
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
console.log("продукт                      ответ            гем   опознание");
for (const [name, want] of CASES) {
  const idp = promptFor("flags", "identify", name);
  const id = await ask(idp.prompt, idp.schema, "identity");
  if (id.err) { bad++; console.log(`FAIL ${name.padEnd(26)} опознание: ${id.err}`); continue; }
  const opts = (id.obj.options ?? []).filter((o) => o && o.definition);
  const top = opts.reduce((a, b) => (Number(b.confidence) > Number(a.confidence) ? b : a),
    opts[0] ?? { definition: "(нет)", confidence: 0 });
  const np = promptFor("flags", "heme", name, top.definition);
  const r = await ask(np.prompt, np.schema, "heme");
  if (r.err) { bad++; console.log(`FAIL ${name.padEnd(26)} гем: ${r.err} ${r.raw ?? ""}`); continue; }
  // Код спрашивает ВЕРДИКТ, а не категорию словом: эталон сводим к нему же.
  const got = r.obj.verdict === true;
  // Категории набора — наши, вердикт кода — булев. Истина здесь: богатый источник гемового железа.
  const YES = ["LIVER", "ORGAN", "MAMMAL_MEAT", "MOLLUSC"];
  const shouldBe = YES.includes(String(want).toUpperCase());
  const ok = got === shouldBe;
  if (!ok) bad++;
  // Наш признак: печень, сердце, мышца млекопитающего и страус тратят недельные граммы.
  
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(26)} ${(got ? "да " : "нет").padEnd(16)} ${got ? "да " : "нет"}           ${top.definition}`);
  if (!ok) console.log(`     ждали ${shouldBe ? "да" : "нет"} (${want}), модель: ${r.obj.reason}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
process.exit(bad ? 1 : 0);
