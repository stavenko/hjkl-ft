// СРАВНЕНИЕ ВЕРСИЙ ГЕЙТА ОПОЗНАНИЯ на одних и тех же именах.
//
// Вопрос, ради которого это написано: раньше эти продукты проходили или падали
// так же? Ответ нельзя вычитать из кода — промпт менялся вместе с правилом, и
// решает ЖИВАЯ модель. Поэтому каждую версию спрашиваем её же промптом (из
// prompts.json того коммита) и считаем вес правилом ТОГО ЖЕ коммита.
//
//   node scripts/compare-identity-versions.mjs <повторов> <имя> [имя …]
//
// Версии задаются в VERSIONS: путь к prompts.json и правило веса.
import { readFileSync } from "node:fs";

const AI = "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const SC = process.env.SCRATCH || ".";
/// Модель замера: по умолчанию та, на которой работает приложение.
const MODEL = process.env.MODEL || "@cf/qwen/qwen3-30b-a3b-fp8";

/// Порог один во всех версиях — менялось только то, чем его сбивают.
const MIN = 0.6;

const VERSIONS = [
  {
    name: "a7bc81c до штрафа",
    prompts: `${SC}/wt-pre/scripts/prompts.json`,
    // Вес = уверенность лучшей версии, и всё.
    weight: (a, top) => top,
  },
  {
    name: "61fea80 штраф 0.7, два условия",
    prompts: `${SC}/wt-two/scripts/prompts.json`,
    weight: (a, top) => {
      const unknown = a.i_cannot_name_the_food_behind_this_name === true;
      const noDict = a.i_see_this_food_in_the_dictionary !== true;
      return top * (unknown && noDict ? 0.7 : 1.0);
    },
  },
  {
    name: "HEAD штраф 0.7, три условия",
    prompts: "scripts/prompts.json",
    weight: (a, top) => {
      const unknown = a.i_cannot_name_the_food_behind_this_name === true;
      const noDict = a.i_see_this_food_in_the_dictionary !== true;
      const fw = String(a.known_food_word_in_the_name || "").trim().toLowerCase();
      const noWord = !fw || fw.startsWith("none");
      return top * (unknown && noDict && noWord ? 0.7 : 1.0);
    },
  },
];

// ONLY=<подстрока> — прогнать только подходящие версии (замер стоит денег).
const PICKED = VERSIONS.filter((v) => !process.env.ONLY || v.name.includes(process.env.ONLY));

const b64 = (b) => Buffer.from(b).toString("base64url");
const now = Math.floor(Date.now() / 1000);
const head = b64(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64(JSON.stringify({ sub: `cmp-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${head}.${b64(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(head))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const promptOf = (file, food) => {
  const d = JSON.parse(readFileSync(file, "utf8")).flags.identify;
  return { prompt: d.prompt.replaceAll("{{FOOD}}", food), schema: d.schema };
};

const ask = async (file, food) => {
  const { prompt, schema } = promptOf(file, food);
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: prompt }],
      response_format: { type: "json_schema", json_schema: { name: "identity", schema } },
      stream: true, think: false, max_tokens: 2000,
    }),
  });
  const t = await r.text();
  let out = "";
  for (const line of t.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const p = line.slice(5).trim();
    if (!p || p === "[DONE]") continue;
    try { out += JSON.parse(p)?.choices?.[0]?.delta?.content ?? ""; } catch {}
  }
  try {
    const v = JSON.parse(out.trim());
    // Обёртку-массив вокруг единственного объекта снимаем так же, как приложение.
    return Array.isArray(v) ? (v[0] ?? null) : v;
  } catch { return null; }
};

const REPEATS = Number(process.argv[2] || 3);
const NAMES = process.argv.slice(3);
if (!NAMES.length) {
  console.error("нужны имена продуктов");
  process.exit(1);
}

console.log(`модель ${MODEL} · повторов на имя: ${REPEATS} · порог ${MIN}\n`);
const header = "продукт".padEnd(32) + PICKED.map((v) => v.name.padEnd(30)).join("");
console.log(header);
for (const food of NAMES) {
  const cells = [];
  for (const v of PICKED) {
    let passed = 0;
    const ws = [];
    for (let i = 0; i < REPEATS; i++) {
      const a = await ask(v.prompts, food);
      if (!a) { ws.push("—"); continue; }
      const opts = (a.options ?? []).filter((o) => o?.definition);
      const top = opts.reduce((m, o) => Math.max(m, Number(o.confidence) || 0), 0);
      const w = v.weight(a, top);
      ws.push(w.toFixed(2));
      if (w >= MIN) passed++;
    }
    cells.push(`${passed}/${REPEATS} ${ws.join(" ")}`.padEnd(30));
  }
  console.log(food.slice(0, 30).padEnd(32) + cells.join(""));
}
