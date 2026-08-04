// Почему значение железа скачет: сравниваем ФОРМУ промпта на одном и том же
// продукте, дёргая AI-воркер напрямую по N раз на вариант.
//
//   A — как сейчас у железа: одно число, без вилки.
//   B — как у кальция: min / max / recommended + comment (вилка перед ответом).
//   C — вилка без comment (отделяем эффект вилки от эффекта свободного текста).
//
// Дев-воркер 402-гейтится по подписке, поэтому подписываем dev-JWT и活ируем
// фейковую оплату — тем же путём, что seed-test-subscription.mjs.
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const FOOD = process.env.FOOD || "Куриная печень";
const N = Number(process.env.N || 5);

const b64url = (buf) => Buffer.from(buf).toString("base64url");
async function jwt(sub) {
  const enc = new TextEncoder();
  const now = Math.floor(Date.now() / 1000);
  const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", enc.encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = await crypto.subtle.sign("HMAC", key, enc.encode(data));
  return `${data}.${b64url(new Uint8Array(sig))}`;
}

const uid = `iron-probe-${Date.now()}`;
const token = await jwt(uid);
const co = await (await fetch(`${PAY}/test/guest-checkout`, {
  method: "POST", headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ planId: "test" }),
})).json();
await fetch(`${PAY}/claim`, {
  method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }),
});

const RAW_CLAUSE =
  "For raw/dry as-sold products (grains, rice, pasta, flour, meat, fish, legumes) use the " +
  "RAW value unless the name says cooked/boiled/fried/steamed.";

// A — текущий железный: одно число, вилки нет.
const promptA = (f) =>
  `You are a nutritional database. For the food item "${f}", report its IRON.\n\n${RAW_CLAUSE}\n\n` +
  `Provide:\n` +
  `- iron_mg: total iron per 100 grams, in milligrams (a plain number)\n` +
  `- absorption: the FRACTION of that iron a human actually absorbs, between 0 and 1.\n` +
  `  Guidance: heme iron from red meat, liver, offal, shellfish and fish is absorbed at about ` +
  `0.15-0.35 (use ~0.25 for liver and red meat, ~0.20 for poultry and fish). Non-heme iron from ` +
  `legumes, seeds, nuts, greens, cereals, vegetables, fruit and dairy is absorbed at about ` +
  `0.02-0.20 (use ~0.05 for legumes and whole grains, ~0.08 when the food is rich in vitamin C, ` +
  `~0.03 when it is rich in phytates, tannins or calcium).\n\n` +
  `Base the answer only on the food name "${f}". Respond with ONLY a single minified JSON object.`;

// B — форма кальция, применённая к железу: вилка + comment.
const promptB = (f) =>
  `You are a nutritional database. For the food item "${f}", give the amount of Iron per 100 ` +
  `grams, as a plain number in mg.\n\n${RAW_CLAUSE}\n\n` +
  `Provide:\n` +
  `- min_value: lowest reasonable amount (number, in mg)\n` +
  `- max_value: highest reasonable amount (number, in mg)\n` +
  `- recommended: the most likely amount (number, in mg)\n` +
  `- comment: brief explanation in Russian\n\n` +
  `Base the answer only on the food name "${f}". All values are per 100 g in mg — bare numbers, ` +
  `no unit text. Respond with ONLY a single minified JSON object and nothing else.`;

// C — вилка, но без свободного текста.
const promptC = (f) =>
  `You are a nutritional database. For the food item "${f}", give the amount of Iron per 100 ` +
  `grams, as a plain number in mg.\n\n${RAW_CLAUSE}\n\n` +
  `Provide:\n` +
  `- min_value: lowest reasonable amount (number, in mg)\n` +
  `- max_value: highest reasonable amount (number, in mg)\n` +
  `- recommended: the most likely amount (number, in mg)\n\n` +
  `Base the answer only on the food name "${f}". Respond with ONLY a single minified JSON object.`;

const num = { type: "number" };
const schemaA = { type: "object", properties: { iron_mg: num, absorption: num },
  required: ["iron_mg", "absorption"], additionalProperties: false };
const schemaB = { type: "object",
  properties: { min_value: num, max_value: num, recommended: num, comment: { type: "string" } },
  required: ["min_value", "max_value", "recommended", "comment"], additionalProperties: false };
const schemaC = { type: "object", properties: { min_value: num, max_value: num, recommended: num },
  required: ["min_value", "max_value", "recommended"], additionalProperties: false };

// Ответ приходит потоком SSE — собираем content из delta.
async function ask(prompt, schema) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: prompt }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema } },
      stream: true, think: false, max_tokens: 8000,
    }),
  });
  if (!r.ok) return { err: `HTTP ${r.status}` };
  const text = await r.text();
  let out = "";
  for (const line of text.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const p = line.slice(5).trim();
    if (!p || p === "[DONE]") continue;
    try {
      const j = JSON.parse(p);
      out += j?.choices?.[0]?.delta?.content ?? "";
    } catch { /* частичный кадр — пропускаем */ }
  }
  const clean = out.trim().replace(/^```json\s*|^```\s*|```$/g, "").trim();
  try {
    return { obj: JSON.parse(clean), raw: clean };
  } catch (e) {
    return { err: `parse: ${e.message}`, raw: clean.slice(0, 80) };
  }
}

const stats = (xs) => {
  if (!xs.length) return "нет значений";
  const s = [...xs].sort((a, b) => a - b);
  const med = s[Math.floor(s.length / 2)];
  return `мин ${s[0]}, макс ${s[s.length - 1]}, медиана ${med}, разброс ×${(s[s.length - 1] / Math.max(s[0], 1e-9)).toFixed(1)}`;
};

// D — ДЕЙСТВУЮЩИЙ промпт кальция, слово в слово, на кальции. Нужен, чтобы
// понять: форма кальция устойчивее сама по себе или её просто никто не мерил.
const promptD = (f) =>
  `You are a nutritional database. For the food item "${f}", give the amount of Кальций per 100 ` +
  `grams, as a plain number in мг.\n\n${RAW_CLAUSE}\n\n` +
  `Provide:\n` +
  `- min_value: lowest reasonable amount (number, in мг)\n` +
  `- max_value: highest reasonable amount (number, in мг)\n` +
  `- recommended: the most likely amount (number, in мг)\n` +
  `- comment: brief explanation in Russian\n\n` +
  `Base the answer only on the food name "${f}". All values are per 100 g in мг — bare numbers, ` +
  `no unit text. Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose.`;

console.log(`продукт: «${FOOD}», по ${N} прогонов на вариант\n`);
for (const [name, prompt, schema, pick] of [
  ["A — как сейчас (одно число)", promptA, schemaA, (o) => o.iron_mg],
  ["B — как кальций (вилка + comment)", promptB, schemaB, (o) => o.recommended],
  ["C — вилка без comment", promptC, schemaC, (o) => o.recommended],
  ["D — действующий промпт КАЛЬЦИЯ (на кальции)", promptD, schemaB, (o) => o.recommended],
]) {
  const vals = [];
  const errs = [];
  for (let i = 0; i < N; i++) {
    const r = await ask(prompt(FOOD), schema);
    if (r.err) { errs.push(`${r.err}${r.raw ? ` | ${r.raw}` : ""}`); continue; }
    const v = pick(r.obj);
    vals.push(v);
  }
  console.log(`${name}`);
  console.log(`  значения: ${vals.join(", ") || "—"}`);
  console.log(`  ${stats(vals)}`);
  if (errs.length) console.log(`  сбоев: ${errs.length} — ${errs[0]}`);
  console.log("");
}
