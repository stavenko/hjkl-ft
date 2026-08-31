// Универсальный разбор ФОТОГРАФИЙ ленивой записи: снимки → список еды с тем, что
// удалось прочесть с упаковки.
//
// Деления картинок на «этикетку» и «еду» больше нет (ТЗ §6.5), поэтому промпт
// один на оба случая: он сам решает, что перед ним. Здесь меряется самый
// неудобный подслучай — ОДИН продукт, снятый ДВАЖДЫ, у которого таблица разорвана
// между кадрами: на первом снимке есть название и калорийность, на втором — не
// обрезанные углеводы. Правильный ответ — ОДНА позиция со сведёнными цифрами, а
// не две половинки и не два продукта.
//
//   node scripts/measure-label-items.mjs [--n 3] [--single 1|2]
//
//   --single   отправить только один снимок из двух — видно, что даёт каждый
//              кадр поодиночке и что добавляет их сведение
//
// Разворачивать снимки не нужно: текст на них лежит на боку, и модель читает его
// как есть — проверено.
//
// Эталон вычитан со снимков вручную: сметана 15 %, на 100 г — 160 ккал, белки
// 2,7, жиры 15,0, углеводы 3,6.

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";
// Картиночная модель прямого пути — та, что стоит в `vision_model` дев-конфига.
const MODEL = process.env.MODEL || "qwen3-vl-plus";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const N = Number(arg("n", 3));
const SINGLE = arg("single", null);

const FIXTURES = ["scripts/fixtures/label-smetana-1.jpg", "scripts/fixtures/label-smetana-2.jpg"];

// Что должно получиться: ОДНА позиция, вот с такими цифрами на 100 г.
const WANT = {
  count: 1,
  nameKeys: ["сметан"],
  per100g: { kcal: 160, protein: 2.7, fat: 15.0, carbs: 3.6 },
};

const PROMPT =
  `Ты — nutrition vision assistant. На фотографиях — еда, которую съел человек. Это могут быть ` +
  `снимки упаковки с этикеткой, снимки самого продукта, снимки тарелки с готовой едой — или всё сразу.\n\n` +
  `Рассуждай про себя, затем выдай СТРОГИЙ JSON.\n` +
  `Шаги:\n` +
  `1. Пойми, что перед тобой на КАЖДОЙ фотографии: этикетка, продукт или тарелка с едой.\n` +
  `2. Собери список еды. ВАЖНО: несколько фотографий могут показывать ОДИН И ТОТ ЖЕ продукт с ` +
  `разных сторон или с разным приближением — это ОДНА позиция, а не несколько. Считай еду дважды ` +
  `только если это действительно разная еда.\n` +
  `3. Прочитай на упаковке пищевую ценность на 100 г. Таблица может быть РАЗОРВАНА между кадрами: ` +
  `название на одном снимке, цифры на другом, часть строки обрезана краем — СВЕДИ прочитанное со ` +
  `всех фотографий в одну таблицу. Текст может лежать на боку или вверх ногами — всё равно прочитай.\n` +
  `4. Заполняй только то, что ВИДНО. Не помнишь — не выдумывай: не прочитанное поле оставь null.\n\n` +
  `Правила:\n` +
  `- name — на РУССКОМ, короткое каноническое название продукта (1-3 слова), с жирностью или ` +
  `другой определяющей цифрой, если она есть на упаковке: «сметана 15%».\n` +
  `- kcal, protein, fat, carbs — на 100 г, числами. Энергию бери в ккал (число перед «ккал»), кДж игнорируй.\n` +
  `- sugar, fiber, saturated_fat — ТОЛЬКО если на упаковке есть ОТДЕЛЬНАЯ строка про них; иначе null. ` +
  `Сахар — это НЕ углеводы: не переписывай значение углеводов в сахар. Российские этикетки обычно НЕ ` +
  `выделяют сахар отдельно — нет строки «в том числе сахара» — значит sugar: null.\n` +
  `- package_weight_g — ТОЛЬКО если на снимке ВИДНА надпись массы нетто («450 г», «масса нетто 180 г»); ` +
  `иначе null. НЕ выводи её из типичного размера такой упаковки: по массе нетто мы считаем, что человек ` +
  `съел всю пачку, и выдуманное нетто станет выдуманным весом съеденного.\n` +
  `- grams — сколько человек съел, если это видно (порция на тарелке); для снимка упаковки null.\n` +
  `- confidence — 0..1, насколько ты уверен в позиции.\n\n` +
  `Верни ТОЛЬКО JSON, без прозы.`;

const NUM_OR_NULL = { type: ["number", "null"] };
const SCHEMA = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "PhotoItems",
  type: "object",
  properties: {
    items: {
      type: "array",
      items: {
        type: "object",
        properties: {
          name: { type: "string" },
          grams: NUM_OR_NULL,
          package_weight_g: NUM_OR_NULL,
          kcal: NUM_OR_NULL,
          protein: NUM_OR_NULL,
          fat: NUM_OR_NULL,
          carbs: NUM_OR_NULL,
          sugar: NUM_OR_NULL,
          fiber: NUM_OR_NULL,
          saturated_fat: NUM_OR_NULL,
          confidence: { type: "number" },
        },
        required: ["name", "grams", "package_weight_g", "kcal", "protein", "fat", "carbs",
                   "sugar", "fiber", "saturated_fat", "confidence"],
      },
    },
  },
  required: ["items"],
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const uid = `label-${Date.now()}`;
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: uid, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(SECRET),
    { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data)));
  const token = `${data}.${b64url(sig)}`;
  const co = await (await fetch(`${PAY}/test/guest-checkout`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ planId: "test" }),
  })).json();
  await fetch(`${PAY}/claim`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ claimId: co.claimId, secret: co.secret }),
  });
  return token;
}

/// Тот же формат запроса, что у `ai::vision_chat`: текст плюс image_url с data-URL.
async function ask(token, images) {
  const parts = [{ type: "text", text: PROMPT }];
  for (const b64 of images) {
    parts.push({ type: "image_url", image_url: { url: `data:image/jpeg;base64,${b64}` } });
  }
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: parts }],
      response_format: { type: "json_schema", json_schema: { name: "response", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 2000,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${(await r.text()).slice(0, 200)}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const payload = line.slice(6).trim();
    if (payload === "[DONE]") continue;
    try { content += JSON.parse(payload).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim()).items || [];
}

const near = (got, want) => got !== null && Math.abs(got - want) <= Math.max(0.15, want * 0.02);

async function main() {
  const token = await mintToken();
  const files = SINGLE ? [FIXTURES[Number(SINGLE) - 1]] : FIXTURES;
  const images = files.map((f) => readFileSync(f).toString("base64"));
  console.log(`модель ${MODEL}, снимков ${images.length}, повторов ${N}`);
  console.log(`эталон: сметана 15%, 100 г → ккал 160, Б 2,7, Ж 15,0, У 3,6\n`);

  let okCount = 0, okName = 0, okNums = 0;
  for (let i = 0; i < N; i++) {
    let items;
    try { items = await ask(token, images); }
    catch (e) { console.log(`  прогон ${i + 1}: сбой — ${e.message}`); continue; }

    const countOk = items.length === WANT.count;
    if (countOk) okCount++;
    for (const it of items) {
      const nums = ["kcal", "protein", "fat", "carbs"]
        .map((k) => `${k}=${it[k]}${near(it[k], WANT.per100g[k]) ? "" : " ✗"}`).join(" ");
      const extra = [
        it.package_weight_g !== null ? `нетто ${it.package_weight_g}` : null,
        it.sugar !== null ? `сахар ${it.sugar}` : null,
        it.fiber !== null ? `клетчатка ${it.fiber}` : null,
        it.saturated_fat !== null ? `НЖК ${it.saturated_fat}` : null,
        it.grams !== null ? `съедено ${it.grams}` : null,
      ].filter(Boolean).join(", ");
      console.log(`  прогон ${i + 1}: [${items.length}] «${it.name}»  ${nums}${extra ? `  (${extra})` : ""}`);
    }
    if (items.length === 1) {
      const it = items[0];
      if (WANT.nameKeys.some((k) => it.name.toLowerCase().includes(k))) okName++;
      if (["kcal", "protein", "fat", "carbs"].every((k) => near(it[k], WANT.per100g[k]))) okNums++;
    }
  }

  console.log(`\nодна позиция:      ${okCount}/${N}`);
  console.log(`название верное:   ${okName}/${N}`);
  console.log(`все четыре числа:  ${okNums}/${N}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
