// ГЕЙТ ОПОЗНАНИЯ: пускаем ли мы дальше то, чего модель не знает.
//
// Три режима:
//   RULE=1    — правило целиком: настоящая еда должна проходить, выдуманная нет;
//   JITTER=1  — дребезг: один продукт много раз, исход обязан быть один и тот же;
//   FIELD=1   — сами булевы поля: приходят ли всегда и согласуются ли с правдой.
//   без флагов — дословный ответ модели по именам из аргументов.
//
// Правило то же, что в коде (`FlagsCtx::recognised`): вес = уверенность лучшей версии,
// умноженная на 0.6, если модель призналась, что слова не знает, И словарь промолчал.
// Числа она ставит СВОЕЙ ФОРМУЛИРОВКЕ, а не еде: «Бубурек копчёный — a type of smoked
// bread» набирал 0.80 при честном «I do not know this word».
//
// Промпт и схема берутся из кода (scripts/prompts.json), копий здесь нет.
import { promptFor } from "./lib/prompts.mjs";
const AI = "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = "dev-secret-change-in-production";
const b64 = (b) => Buffer.from(b).toString("base64url");
const now = Math.floor(Date.now() / 1000);
const data = b64(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
  b64(JSON.stringify({ sub: `says-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
  { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
const token = `${data}.${b64(new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data))))}`;
const co = await (await fetch(`${PAY}/test/guest-checkout`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ planId: "test" }) })).json();
await fetch(`${PAY}/claim`, { method: "POST",
  headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
  body: JSON.stringify({ claimId: co.claimId, secret: co.secret }) });

const ask = async (name) => {
  const { prompt, schema } = promptFor("flags", "identify", name);
  const r = await fetch(`${AI}/chat/completions`, { method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ model: "@cf/qwen/qwen3-30b-a3b-fp8",
      messages: [{ role: "user", content: prompt }],
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

const REAL = ["Голец", "Полба", "Маш", "Кыстыбый", "Творог 5%", "Скумбрия х/к",
  "Fuet truffle", "Мороженая вишня", "Куриная грудка", "Гречка", "Хачапури", "Топинамбур"];
const FAKE = ["Зюзюбряк ассорти", "Бубурек копчёный", "Квазилапша", "Хряпундель", "Мормышка сладкая"];
if (process.env.JITTER) {
  // ДРЕБЕЗГ: один продукт много раз подряд. Исход должен быть один и тот же — иначе
  // человек получит признаки то так, то этак, а мы не сможем верить ни одному замеру.
  const REPEATS = Number(process.env.REPEATS || 6);
  console.log(`продукт                 прошёл/всего   веса`);
  for (const [names, isReal] of [[REAL, true], [FAKE, false]]) {
    for (const name of names) {
      let passed = 0;
      const weights = [];
      for (let i = 0; i < REPEATS; i++) {
        const a = await ask(name);
        if (!a) { weights.push("—"); continue; }
        const opts = (a.options ?? []).filter((o) => o?.definition);
        const top = opts.reduce((x, y) => (Number(y.confidence) > Number(x.confidence) ? y : x),
          opts[0] ?? { confidence: 0 });
        const unknown = a.i_dont_know_this_word === true;
        const noDict = a.i_see_this_food_in_the_dictionary !== true;
        const w = Number(top.confidence) * (unknown && noDict ? 0.6 : 1.0);
        weights.push(w.toFixed(2));
        if (w >= 0.6) passed++;
      }
      const stable = passed === 0 || passed === REPEATS;
      const want = isReal ? REPEATS : 0;
      const mark = passed === want ? "OK  " : (stable ? "СТАБ" : "ДРЕБ");
      console.log(`${mark} ${name.padEnd(22)} ${passed}/${REPEATS}          ${weights.join(" ")}`);
    }
  }
  process.exit(0);
}
if (process.env.FIELD) {
  // Проверка САМОГО ПОЛЯ: приходит ли оно всегда, булево ли, и совпадает ли с правдой.
  const REPEATS = Number(process.env.REPEATS || 3);
  let total = 0, present = 0, boolean = 0, agree = 0, cases = 0;
  const wrong = [];
  for (const [names, isReal] of [[REAL, true], [FAKE, false]]) {
    for (const name of names) {
      for (let i = 0; i < REPEATS; i++) {
        const a = await ask(name);
        total++;
        if (!a) continue;
        const has = Object.prototype.hasOwnProperty.call(a, "i_dont_know_this_word")
          && Object.prototype.hasOwnProperty.call(a, "i_see_this_food_in_the_dictionary");
        if (has) present++;
        if (typeof a.i_dont_know_this_word === "boolean"
            && typeof a.i_see_this_food_in_the_dictionary === "boolean") boolean++;
        cases++;
        const saysUnknown = a.i_dont_know_this_word === true;
        // Для настоящей еды ждём false ЛИБО находку в словаре (голец: сам не знает).
        const dictHit = a.i_see_this_food_in_the_dictionary === true;
        const right = isReal ? (!saysUnknown || dictHit) : saysUnknown;
        if (right) agree++;
        else wrong.push(`${name}: поле=${saysUnknown}, словарь=${dictHit ? "есть" : "NONE"}, память="${String(a.from_own_knowledge).slice(0,40)}"`);
      }
    }
  }
  console.log(`ответов ${total} · поле пришло ${present} · булево ${boolean} · согласуется с правдой ${agree}/${cases}`);
  for (const w of [...new Set(wrong)]) console.log("  " + w);
  process.exit(0);
}
if (process.env.RULE) {
  const REPEATS = Number(process.env.REPEATS || 2);
  let okReal = 0, real = 0, okFake = 0, fake = 0;
  const bad = [];
  for (const [names, isReal] of [[REAL, true], [FAKE, false]]) {
    for (const name of names) {
      for (let i = 0; i < REPEATS; i++) {
        const a = await ask(name);
        if (!a) continue;
        const opts = (a.options ?? []).filter((o) => o?.definition);
        const top = opts.reduce((x, y) => (Number(y.confidence) > Number(x.confidence) ? y : x),
          opts[0] ?? { confidence: 0 });
        const unknown = a.i_dont_know_this_word === true;
        const noDict = a.i_see_this_food_in_the_dictionary !== true;
        // Та же формула, что в коде: признание понижает вес втрое-с-небольшим.
        const weight = Number(top.confidence) * (unknown && noDict ? 0.6 : 1.0);
        const passed = weight >= 0.6;
        if (isReal) { real++; if (passed) okReal++; else bad.push(`${name}: отсечён (вес ${weight.toFixed(2)}, не знает: ${unknown}, словарь ${noDict ? "NONE" : "есть"})`); }
        else { fake++; if (!passed) okFake++; else bad.push(`${name}: ПРОШЁЛ (вес ${weight.toFixed(2)}, увер. ${Number(top.confidence).toFixed(2)}, не знает: ${unknown}) ${top.definition.slice(0, 34)}`); }
      }
    }
  }
  console.log(`настоящая еда прошла: ${okReal}/${real} · выдумка отсечена: ${okFake}/${fake}`);
  for (const b of [...new Set(bad)]) console.log("  " + b);
  process.exit(0);
}
for (const name of process.argv.slice(2)) {
  console.log(`\n=== ${name} ===`);
  for (let i = 0; i < 3; i++) {
    const a = await ask(name);
    if (!a) { console.log("  ответ не разобран"); continue; }
    const opts = (a.options ?? []).filter((o) => o?.definition);
    const top = opts.reduce((x, y) => (Number(y.confidence) > Number(x.confidence) ? y : x),
      opts[0] ?? { confidence: 0 });
    console.log(`  [${i + 1}] порог ${Number(top.confidence) >= 0.6 ? "ПРОЙДЕН" : "не пройден"}`);
    console.log(`      память:  ${a.from_own_knowledge}`);
    console.log(`      словарь: ${String(a.from_dictionary).slice(0, 60)}`);
    for (const o of opts) console.log(`        ${Number(o.confidence).toFixed(2)}  ${o.definition}`);
  }
}
