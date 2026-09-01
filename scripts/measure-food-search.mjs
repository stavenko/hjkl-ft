// ПОИСК ПОХОЖЕГО В БАЗЕ ПОЛЬЗОВАТЕЛЯ: замер отбора кандидатов.
//
// Распознали «Ракушки» — а у человека в базе лежат «Макароны». Это один и тот же
// продукт, но общих букв у слов нет, и никакая подстрока, нормализация или мера
// опечаток их не свяжет: связывать нечего, слово просто другое.
//
// Сейчас сопоставление обходится без поиска вовсе — ai::match_food вкладывает в
// промпт ВЕСЬ каталог, по вызову на распознанный продукт. С ростом базы это
// дорожает линейно, поэтому сначала нужен отбор: короткий список кандидатов,
// среди которых модель уже выбирает.
//
// Меряется здесь ровно отбор, а не выбор: попал ли нужный продукт в список. Если
// не попал — дальше выбирать не из чего, и никакая модель этого не спасёт.
//
//   node scripts/measure-food-search.mjs [--model МОДЕЛЬ] [--rebuild]
//
// --rebuild заново спрашивает у модели обобщающие слова для каталога (они кэшируются
// в scripts/fixtures/user-foods-generic.json, потому что готовятся ОДИН раз на
// продукт, а не на каждый поиск).

import { readFileSync, writeFileSync, existsSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "@cf/qwen/qwen3.8-27b");
const REBUILD = process.argv.includes("--rebuild");
const GENERIC_CACHE = "scripts/fixtures/user-foods-generic.json";

const FOODS = JSON.parse(readFileSync("scripts/fixtures/user-foods.json", "utf8")).foods;

// ── случаи ──
//
// `want` — что в базе ДОЛЖНО найтись; null значит, что подходящего нет и правильный
// ответ — не найти ничего. Несколько идентификаторов = годится любой.
const CASES = [
  { q: "Витые макароны",              want: ["f01", "f02"] },
  { q: "Ракушки",                     want: ["f01", "f02"] },
  { q: "Спиральные спагетти",         want: ["f01", "f02"] },
  { q: "Паста пенне",                 want: ["f01", "f02"] },
  { q: "Вермишель",                   want: ["f01", "f02"] },
  { q: "Творог «Пластовой» обезжиренный", want: ["f08", "f09"] },
  { q: "Греческий йогурт ТЕОС",       want: ["f14"] },
  { q: "Филе цыплёнка-бройлера",      want: ["f15"] },
  { q: "Куриная грудка",              want: ["f15"] },
  { q: "Оливки целые консервированные", want: ["f30"] },
  { q: "Сметана 15%",                 want: ["f10"] },
  { q: "Помидоры черри",              want: ["f25"] },
  { q: "Огурец",                      want: ["f24"] },
  { q: "Хлеб чёрный",                 want: ["f06"] },
  { q: "Гречневая каша",              want: ["f03"] },
  { q: "Красная фасоль в банке",      want: ["f43"] },
  { q: "Сёмга слабосолёная",          want: ["f20"] },
  { q: "Овсянка",                     want: ["f05"] },
  { q: "Сливочное масло",             want: ["f34"] },
  { q: "Куриный суп с вермишелью",    want: ["f45"] },
  // Ловушка: «Картошка» здесь — название десерта, а не корнеплод. Если отбор
  // притащит «Картофель отварной», это хуже промаха: модель может согласиться.
  { q: "Десерт «Картошка»",           want: [] },
  { q: "Кока-кола",                   want: [] },
  { q: "Авокадо",                     want: [] },
  { q: "Кабачки на гриле",            want: [] },
];

// ── отбор кандидатов: три способа ──

const norm = (s) => s.toLowerCase().replace(/ё/g, "е").replace(/[^а-яa-z0-9 ]/g, " ");
/// Грубая основа слова: русский словоизменяет хвостом, и нескольких букв хватает, чтобы
/// «макароны», «макаронами» и «макаронный» сошлись. Это не морфология, а дешёвая
/// замена ей — настоящий стеммер тут ничего не добавит, потому что промахи у нас
/// не на окончаниях.
const STEM_LEN = Number(arg("stem", 4));
const stem = (w) => (w.length > STEM_LEN ? w.slice(0, STEM_LEN) : w);
const stems = (s) => new Set(norm(s).split(/\s+/).filter((w) => w.length >= 3).map(stem));

/// Как ищет приложение сегодня: подстрока в обе стороны.
function bySubstring(q) {
  const nq = norm(q).trim();
  return FOODS.filter((f) => {
    const nf = norm(f.name).trim();
    return nf.includes(nq) || nq.includes(nf);
  });
}

/// Пословно, по грубым основам: чинит окончания и лишние слова, но не синонимы.
function byWords(q) {
  const a = stems(q);
  return FOODS.filter((f) => [...stems(f.name)].some((s) => a.has(s)));
}

/// По обобщающему слову: и у запроса, и у КАЖДОГО продукта базы оно заготовлено
/// заранее. Готовить надо с обеих сторон — иначе «ракушки», превращённые в
/// «макароны», всё равно не найдут лежащие в базе «Спагетти Barilla №5».
function byGeneric(q, generic, queryGeneric) {
  const wanted = stems(queryGeneric || "");
  const hits = FOODS.filter((f) => {
    const g = stems(generic[f.id] || "");
    return [...g].some((s) => wanted.has(s));
  });
  // Обобщение дополняет пословный отбор, а не заменяет: «Сметана 15%» находится
  // и без него, и терять такое попадание из-за неудачного обобщения незачем.
  const seen = new Set(hits.map((f) => f.id));
  return [...hits, ...byWords(q).filter((f) => !seen.has(f.id))];
}

// ── обобщающее слово от модели ──

const GENERIC_SCHEMA = {
  type: "object",
  properties: {
    generic: {
      description: "Одно-два слова: род продукта, к которому он принадлежит, в именительном падеже " +
        "единственного или множественного числа — так, как это слово написали бы в самом общем " +
        "списке покупок. Бренд, сорт, форма, жирность и способ приготовления отбрасываются: у " +
        "«Спагетти Barilla №5» род — макароны, у «Ракушек» тоже макароны, у «Куриного филе» — " +
        "курица. Если продукт сам себе род — повтори его.",
      type: "string",
    },
  },
  required: ["generic"],
};

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `search-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function askGeneric(token, name) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content:
        `Продукт: «${name}».\n\nК какому роду продуктов он относится? Ответь одним объектом JSON.` }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema: GENERIC_SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 200,
    }),
  });
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  let content = "";
  for (const line of (await r.text()).split("\n")) {
    if (!line.startsWith("data: ")) continue;
    const p = line.slice(6).trim();
    if (p === "[DONE]") continue;
    try { content += JSON.parse(p).choices?.[0]?.delta?.content ?? ""; } catch { /* пропуск */ }
  }
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim()).generic;
}

/// Собрать (или прочитать) обобщения для всего каталога. Готовятся ОДИН раз на
/// продукт: в приложении это делается при заведении еды, а не при каждом поиске.
async function catalogGeneric(token) {
  if (!REBUILD && existsSync(GENERIC_CACHE)) return JSON.parse(readFileSync(GENERIC_CACHE, "utf8"));
  const out = {};
  const failures = [];
  const queue = [...FOODS];
  const worker = async () => {
    for (;;) {
      const f = queue.shift();
      if (!f) return;
      try { out[f.id] = await askGeneric(token, f.name); }
      catch (e) { out[f.id] = null; failures.push(`${f.name}: ${e.message}`); }
    }
  };
  await Promise.all(Array.from({ length: 4 }, worker));
  if (failures.length) {
    console.error(`обобщение каталога не далось для ${failures.length} продуктов:`);
    for (const f of failures) console.error(`  ${f}`);
    throw new Error("каталог обобщён не полностью — мерить на нём нельзя");
  }
  writeFileSync(GENERIC_CACHE, JSON.stringify(out, null, 2) + "\n");
  return out;
}

async function main() {
  const token = await mintToken();
  const generic = await catalogGeneric(token);
  console.log(`модель ${MODEL}, каталог ${FOODS.length} продуктов\n`);

  const strategies = {
    "подстрока (как сейчас)": (c) => bySubstring(c.q),
    "по словам": (c) => byWords(c.q),
    "по обобщению": (c) => byGeneric(c.q, generic, c.queryGeneric),
  };

  // Обобщение запроса — тот же вопрос модели, что и для каталога.
  const qFailed = [];
  for (const c of CASES) {
    // Три попытки: сеть и лимиты подводят, а один потерянный запрос портит целую
    // строку сравнения — и портит незаметно, если подменить его чем попало.
    for (let i = 0; i < 3 && !c.queryGeneric; i++) {
      try { c.queryGeneric = await askGeneric(token, c.q); }
      catch (e) { if (i === 2) qFailed.push(`${c.q}: ${e.message}`); }
    }
  }
  if (qFailed.length) {
    console.error(`обобщение запроса не далось для ${qFailed.length} случаев:`);
    for (const f of qFailed) console.error(`  ${f}`);
    throw new Error("часть запросов не обобщена — сравнение вышло бы нечестным");
  }

  const score = {};
  for (const name of Object.keys(strategies)) score[name] = { hit: 0, miss: 0, falseHit: 0, size: 0, n: 0 };

  for (const c of CASES) {
    const line = [`${c.q}${c.want.length ? "" : "  (в базе нет)"}`.padEnd(38)];
    for (const [name, fn] of Object.entries(strategies)) {
      const got = fn(c);
      const ids = got.map((f) => f.id);
      const s = score[name];
      s.n++; s.size += ids.length;
      if (c.want.length === 0) {
        // Ничего подходящего нет: лишние кандидаты не ошибка сами по себе —
        // модель их отвергнет. Ошибка, только если притащило близкую ловушку.
        if (ids.length) s.falseHit++;
      } else if (c.want.some((w) => ids.includes(w))) s.hit++;
      else s.miss++;
      line.push(`${name}: ${ids.length ? `${ids.length} шт.` : "—"}${
        c.want.length && !c.want.some((w) => ids.includes(w)) ? " ПРОМАХ" : ""}`);
    }
    console.log(line.join("   "));
    console.log(`    обобщение запроса: «${c.queryGeneric}»`);
  }

  console.log();
  for (const [name, s] of Object.entries(score)) {
    const findable = s.hit + s.miss;
    console.log(`${name.padEnd(24)} нашлось ${s.hit}/${findable}, ` +
      `средний список ${(s.size / s.n).toFixed(1)}, ` +
      `лишний улов там, где искать нечего: ${s.falseHit}`);
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
