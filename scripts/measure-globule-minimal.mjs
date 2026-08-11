// Сравнение двух промптов на признаке «молочно-жировая глобула»: НАШЕГО (длинного,
// с механической границей и списками) и МИНИМАЛЬНОГО — один вопрос без подсказок.
//
// Зачем: если модель и так знает предмет, длинное определение только связывает ей
// руки и множит наши собственные ошибки — как вышло с плавленым сыром, где мой
// список врал, а модель была права.
//
// Запрос идёт НАПРЯМУЮ в ai-worker, тем же способом, что и рабочий код (стрим,
// json_schema, think:false). Список продуктов и ожидания — те же, что в
// scripts/measure-milk-globule.mjs.
//
// Запуск: node scripts/measure-globule-minimal.mjs [N]   (N прогонов, по умолчанию 3)
const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
const N = Number(process.argv[2] || 3);

const CASES = [
  ["Молоко 3.2%", true], ["Сливки 20%", true], ["Сметана 15%", true], ["Кефир 1%", true],
  ["Ацидофилин", true], ["Творог обезжиренный", true], ["Йогурт 2,1%", true],
  ["Сыр российский", true], ["Моцарелла", true], ["Мороженое пломбир", true],
  ["Плавленый сыр", false], ["Сливочное масло", false], ["Топлёное масло", false],
  ["Шоколад молочный", false], ["Круассан", false], ["Кижуч без головы", false],
  ["Куриное бедро", false], ["Оливковое масло", false], ["Соевое молоко", false],
  ["Яйцо куриное", false],
];

// Минимальный промпт: вопрос и ничего больше. Ни списков, ни границы, ни правил.
const MINIMAL = (name) =>
  `Продукт: «${name}».\n\n` +
  `Вопрос: жир этого продукта заключён в ЦЕЛЫЕ молочно-жировые глобулы ` +
  `(milk fat globules с нативной мембраной MFGM)?\n\n` +
  `Ответь одним минифицированным JSON-объектом: {"verdict":true|false,"reason":"…"} — ` +
  `reason одной короткой фразой по-русски.`;

const SCHEMA = {
  type: "object",
  properties: { verdict: { type: "boolean" }, reason: { type: "string" } },
  required: ["verdict", "reason"],
  additionalProperties: false,
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const now = Math.floor(Date.now() / 1000);
const uid = `globmin-${Date.now()}`;
const data = b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64url(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
const claim = await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });
if (!claim.ok) {
  console.error(`подписку активировать не удалось: ${claim.status}`);
  process.exit(1);
}

// Тот же транспорт, что в рабочем коде: стрим, схема, размышления выключены.
async function ask(prompt) {
  const res = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: prompt }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 800,
    }),
  });
  const text = await res.text();
  let out = "";
  for (const line of text.split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const payload = line.slice(6).trim();
    if (payload === "[DONE]") continue;
    try { out += JSON.parse(payload).choices?.[0]?.delta?.content ?? ""; } catch { /* не JSON — пропускаем */ }
  }
  try { return JSON.parse(out); } catch { return null; }
}

let bad = 0;
console.log(`минимальный промпт, ${N} прогонов на продукт\n`);
console.log("продукт                        ответы     ждём   причина (первый прогон)");
for (const [name, want] of CASES) {
  const votes = [];
  let reason = "";
  for (let i = 0; i < N; i++) {
    const a = await ask(MINIMAL(name));
    if (!a) { votes.push(null); continue; }
    votes.push(a.verdict);
    if (!reason) reason = a.reason;
  }
  const yes = votes.filter((v) => v === true).length;
  const no = votes.filter((v) => v === false).length;
  // Вердикт большинством: разброс между прогонами — сам по себе результат замера.
  const verdict = yes === no ? null : yes > no ? true : false;
  const ok = verdict === want;
  if (!ok) bad++;
  const s = (x) => (x === true ? "да " : x === false ? "нет" : " — ");
  console.log(`${ok ? "OK  " : "MISS"} ${name.padEnd(28)} ${s(verdict)} ${yes}/${N}    ${s(want)}   ${reason.slice(0, 60)}`);
}
console.log(`\nпопаданий: ${CASES.length - bad}/${CASES.length}`);
