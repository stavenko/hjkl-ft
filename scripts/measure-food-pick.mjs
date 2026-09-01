// ВЫБОР ИЗ ОТОБРАННЫХ: тот же это продукт или новая копия.
//
// Отбор (measure-food-search.mjs) намеренно щедр: лучше лишний кандидат, чем
// упущенный. Здесь из этой горстки выбирается один — или не выбирается никто.
//
// Правило спеки (docs/lazy-diary-entry.md §6.4) строже, чем «похоже»:
//
//   Сравниваем по имени И КБЖУ — и только по тем параметрам, которые распознаны
//   на этикетке. Если новая копия имеет какие-то отличия, даже небольшие,
//   создаём новую копию.
//
// Значит бо́льшая часть решения — арифметика, а не модель: совпадение чисел
// проверяется точно, и спорить тут не о чем. Модели остаётся один вопрос, который
// кодом не решается: одно ли это и то же по названию. Здесь и меряется, стоит ли
// такое разделение труда — против того, чтобы отдать модели всё сразу.
//
// Цена ошибок разная, и складывать их в одно число нельзя:
//   ложное согласие — в дневник уедет ЧУЖОЕ КБЖУ. Это порча данных;
//   ложный отказ    — в базе заведётся вторая копия того же. Это беспорядок.
// Второе неприятно, первое недопустимо.
//
//   node scripts/measure-food-pick.mjs [--model МОДЕЛЬ] [--n ПОВТОРОВ] [--raw]

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "@cf/qwen/qwen3.8-27b");
const N = Number(arg("n", 3));
const RAW = process.argv.includes("--raw");

const FOODS = JSON.parse(readFileSync("scripts/fixtures/user-foods.json", "utf8")).foods;
const byId = Object.fromEntries(FOODS.map((f) => [f.id, f]));

// ── случаи ──
//
// `seen` — то, что распознали: имя и те нутриенты, которые УДАЛОСЬ прочитать
// (остальные null, как и приходит с одного кадра). `pool` — кандидаты, которые
// вернул отбор. `want` — какой продукт базы это на самом деле, null = ни один.
const CASES = [
  {
    what: "та же пачка творога, что уже в базе",
    seen: { name: "Творог «Пластовой» обезжиренный", kcal: 96, protein: 18, fat: 1.2, carbs: 3.3 },
    pool: ["f08", "f09"], want: "f08",
  },
  {
    what: "творог с ДРУГИМ жиром — по спеке это новая копия, а не f08",
    seen: { name: "Творог обезжиренный", kcal: 110, protein: 17, fat: 3.0, carbs: 3.3 },
    pool: ["f08", "f09"], want: null,
  },
  {
    what: "тот же творог, но с этикетки прочлись только калории и белок",
    seen: { name: "Творог обезжиренный ВкусВилл", kcal: 96, protein: 18, fat: null, carbs: null },
    pool: ["f08", "f09"], want: "f08",
  },
  {
    what: "йогурт TEOS — имя написано иначе, числа те же",
    seen: { name: "Греческий йогурт ТЕОС натуральный 2%", kcal: 66.8, protein: 8, fat: 2, carbs: 4.2 },
    pool: ["f14", "f12", "f11"], want: "f14",
  },
  {
    what: "куриное филе из описания, чисел нет вовсе",
    seen: { name: "Куриная грудка", kcal: null, protein: null, fat: null, carbs: null },
    pool: ["f15", "f16", "f19", "f18"], want: "f15",
  },
  {
    what: "оливки: числа совпали, имя длиннее",
    seen: { name: "Оливки целые консервированные", kcal: 149, protein: 1.6, fat: 14.6, carbs: 0 },
    pool: ["f30", "f35", "f43", "f22"], want: "f30",
  },
  {
    what: "сметана той же жирности и с теми же числами",
    seen: { name: "Сметана 15%", kcal: 160, protein: 2.7, fat: 15, carbs: 3.6 },
    pool: ["f10", "f11"], want: "f10",
  },
  {
    what: "макароны: распознали ракушки, в базе род и бренд",
    seen: { name: "Ракушки", kcal: null, protein: null, fat: null, carbs: null },
    pool: ["f01", "f02"], want: "f01",
  },
  // ── почти-повторы: база зарастает именно ими ──
  //
  // «Макароны, макароны, макарошки, ма кароны» — четыре записи об одном и том же.
  // Регистр и описка обязаны решаться кодом; уменьшительное кодом не берётся, и
  // это честный вопрос к модели.
  {
    what: "ПОВТОР: то же слово с маленькой буквы",
    seen: { name: "макароны", kcal: 337, protein: 10.4, fat: 1.1, carbs: 71.5 },
    pool: ["f01", "f02"], want: "f01",
  },
  {
    what: "ПОВТОР: лишний пробел — «ма кароны»",
    seen: { name: "Ма кароны", kcal: 337, protein: 10.4, fat: 1.1, carbs: 71.5 },
    pool: ["f01", "f02"], want: "f01",
  },
  {
    what: "ПОВТОР: описка в одной букве — «макарони»",
    seen: { name: "Макарони", kcal: 337, protein: 10.4, fat: 1.1, carbs: 71.5 },
    pool: ["f01", "f02"], want: "f01",
  },
  {
    what: "ПОВТОР: уменьшительное «макарошки» — кодом не берётся",
    seen: { name: "Макарошки", kcal: 337, protein: 10.4, fat: 1.1, carbs: 71.5 },
    pool: ["f01", "f02"], want: "f01",
  },
  {
    what: "ПОВТОР: то же с хвостом — «макароны отварные»",
    seen: { name: "Макароны отварные", kcal: 337, protein: 10.4, fat: 1.1, carbs: 71.5 },
    pool: ["f01", "f02"], want: "f01",
  },
  {
    what: "ЛОВУШКА: «мясо» рядом с «маслом» — два шага правки, но и числа врозь",
    seen: { name: "Мясо говяжье", kcal: 187, protein: 18.9, fat: 12.4, carbs: 0 },
    pool: ["f34", "f17"], want: null,
  },
  {
    what: "ЛОВУШКА: «сельдерей» рядом с «сельдью»",
    seen: { name: "Сельдерей стеблевой", kcal: 13, protein: 0.9, fat: 0.1, carbs: 2.1 },
    pool: ["f21", "f24"], want: null,
  },

  // ── ловушки: в базе НЕТ этого продукта, правильный ответ — никто ──
  {
    what: "ЛОВУШКА: десерт «Картошка» против картофеля отварного",
    seen: { name: "Десерт «Картошка»", kcal: 377, protein: 5.9, fat: 16.5, carbs: 51.4 },
    pool: ["f28", "f39", "f40"], want: null,
  },
  {
    what: "ЛОВУШКА: кока-кола против кефира и молока",
    seen: { name: "Кока-кола", kcal: 42, protein: 0, fat: 0, carbs: 10.6 },
    pool: ["f12", "f11"], want: null,
  },
  {
    what: "ЛОВУШКА: масло сливочное против оливкового — имена близки, продукт другой",
    seen: { name: "Масло топлёное", kcal: 892, protein: 0.2, fat: 99, carbs: 0 },
    pool: ["f34", "f35"], want: null,
  },
  {
    what: "ЛОВУШКА: индейка против курицы — оба птица, продукт разный",
    seen: { name: "Индейка филе сырое", kcal: 84, protein: 19.2, fat: 0.7, carbs: 0 },
    pool: ["f19", "f15", "f16"], want: null,
  },
];

// ── арифметическая часть: что кодом ──

/// Совпадают ли числа ПО ТЕМ ПАРАМЕТРАМ, которые удалось прочитать. Допуск —
/// половина деления шкалы: на упаковке печатают округлённое, и 96 против 96.0 это
/// одно и то же, а 96 против 110 — разное. Ничего не прочитано — сравнивать
/// нечем, и код молчит (`null`), решать будет модель по имени.
function numbersAgree(seen, food) {
  const pairs = [["kcal", 1], ["protein", 0.05], ["fat", 0.05], ["carbs", 0.05]];
  let compared = 0;
  for (const [k, rel] of pairs) {
    if (seen[k] === null || seen[k] === undefined) continue;
    compared++;
    const tol = Math.max(k === "kcal" ? 1 : 0.1, food[k] * rel);
    if (Math.abs(seen[k] - food[k]) > tol) return false;
  }
  return compared === 0 ? null : true;
}

/// Кандидаты, пережившие арифметику. Те, у кого прочитанные числа расходятся,
/// отсеиваются молча: спека прямо велит на любое отличие заводить новую копию, и
/// спрашивать об этом модель незачем — она может согласиться.
const survivors = (seen, pool) => pool.filter((id) => numbersAgree(seen, byId[id]) !== false);

// ── совпадение имени: что ещё берётся кодом ──

/// Имя к сравнимому виду: регистр, ё, пунктуация и ПРОБЕЛЫ прочь. Пробелы убираем
/// целиком, потому что «ма кароны» — это те же макароны с промахом по клавише, а
/// не другой продукт.
const canonName = (s) =>
  s.toLowerCase().replace(/ё/g, "е").replace(/[^а-яa-z0-9]/g, "");

/// Расстояние Левенштейна — сколько правок отделяет одно написание от другого.
function distance(a, b) {
  if (a === b) return 0;
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const cur = [i];
    for (let j = 1; j <= b.length; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[b.length];
}

/// Одно и то же написание с точностью до описки. Порог зависит от длины, и
/// короткие слова не трогаем вовсе: «мясо» и «масло» отличаются на две правки из
/// пяти букв, а это совершенно разная еда. Уменьшительные («макарошки») сюда не
/// попадают — там правок больше, и это вопрос к модели, а не к строкам.
function sameName(a, b) {
  const x = canonName(a), y = canonName(b);
  if (!x || !y) return false;
  if (x === y) return true;
  const len = Math.min(x.length, y.length);
  if (len < 6) return false;
  return distance(x, y) <= (len >= 10 ? 2 : 1);
}

// ── вопрос к модели: только про имя ──

const SCHEMA = {
  type: "object",
  properties: {
    why: {
      description: "ПО-РУССКИ, одним предложением: почему выбран именно этот продукт или почему " +
        "не подошёл ни один.",
      type: "string",
    },
    match_id: {
      description: "Идентификатор продукта из списка — если это ТОТ ЖЕ САМЫЙ продукт, просто " +
        "названный иначе. Ни один не подходит — null. Null это нормальный, частый и НЕ плохой " +
        "ответ: у человека в базе может просто не быть этого продукта, и завести его заново " +
        "дешевле, чем приписать ему чужие нутриенты.",
      type: ["string", "null"],
    },
  },
  required: ["why", "match_id"],
};

const nutr = (f) => (f.kcal === null || f.kcal === undefined ? "нутриенты не прочитаны"
  : `${f.kcal} ккал, Б ${f.protein ?? "?"}, Ж ${f.fat ?? "?"}, У ${f.carbs ?? "?"} на 100 г`);

const PROMPT = (seen, pool) =>
  `У человека в дневнике питания есть своя база продуктов. Мы распознали продукт и ищем, нет ли ` +
  `его там уже, чтобы не заводить второй раз.\n\n` +
  `РАСПОЗНАЛИ: «${seen.name}» — ${nutr(seen)}\n\n` +
  `ЕСТЬ В БАЗЕ:\n` +
  pool.map((id) => `- id=${id}: «${byId[id].name}» — ${nutr(byId[id])}`).join("\n") + `\n\n` +
  `Тот же продукт — это тот же продукт, как бы его ни назвали: «ракушки» и «макароны», «сёмга» и ` +
  `«лосось», название с брендом и без. Разный вид, сорт или способ приготовления — это РАЗНЫЕ ` +
  `продукты, даже если слова похожи: топлёное масло не сливочное, индейка не курица, десерт ` +
  `«Картошка» не картофель. Сомневаешься — отвечай null.`;

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `pick-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function retry(what, attempts = 4) {
  let last;
  for (let i = 0; i < attempts; i++) {
    try { return await what(); } catch (e) { last = e; await new Promise((r) => setTimeout(r, 600 * 2 ** i)); }
  }
  throw last;
}

async function ask(token, seen, pool) {
  if (!pool.length) return { match_id: null, why: "кандидатов не осталось" };
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: PROMPT(seen, pool) }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema: SCHEMA, strict: true } },
      stream: true, think: false, max_tokens: 500,
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
  const j = JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
  // Уверенно названный несуществующий id бессмыслен — такой ответ это отказ.
  if (j.match_id && !pool.includes(j.match_id)) j.match_id = null;
  return j;
}

/// Порог `sameName` решает судьбу записи без всякой модели, поэтому проверяется
/// точно, парами. Короткие слова не трогаются намеренно: «мясо» и «масло»
/// отличаются на две правки из пяти букв, а это разная еда.
function checkSameName() {
  const same = [
    ["Макароны", "макароны"], ["Ма кароны", "Макароны"], ["Макарони", "Макароны"],
    ["макароны", "МАКАРОНЫ"], ["Творог 5%", "творог 5 %"], ["Йогурт", "Йогурт"],
    ["Сметана 15%", "сметана 15%"],
  ];
  const diff = [
    ["Мясо", "Масло"], ["Сельдь", "Сельдерей"], ["Макароны", "Макарошки"],
    ["Молоко", "Мороженое"], ["Лук", "Лук-порей"], ["Сыр", "Сырок"],
    ["Курица", "Куркума"], ["Икра", "Игра"],
  ];
  const bad = [];
  for (const [a, b] of same) if (!sameName(a, b)) bad.push(`не сошлись, а должны: «${a}» / «${b}»`);
  for (const [a, b] of diff) if (sameName(a, b)) bad.push(`СКЛЕИЛО разное: «${a}» / «${b}»`);
  console.log(bad.length ? `проверка имени: ${bad.length} расхождений` : "проверка имени: все пары верно");
  for (const b of bad) console.log(`   ${b}`);
  return bad.length === 0;
}

async function main() {
  checkSameName();
  const token = await mintToken();
  console.log(`модель ${MODEL}, повторов ${N}\n`);

  const ways = {
    "модели всё сразу": (c) => c.pool,
    "сначала арифметика": (c) => survivors(c.seen, c.pool),
    "арифметика + имя": (c) => survivors(c.seen, c.pool),
    "и полное совпадение чисел": (c) => survivors(c.seen, c.pool),
  };
  // «арифметика + имя» сначала пробует решить кодом: если среди переживших
  // арифметику есть продукт с ТЕМ ЖЕ написанием с точностью до описки — это он, и
  // модель не нужна. Так «Макароны», «макароны» и «ма кароны» перестают плодить
  // копии, не рискуя ложным согласием: числа уже сошлись.
  const byNameFirst = (c, pool) => pool.find((id) => sameName(c.seen.name, byId[id].name)) || null;

  /// Все четыре числа прочитаны, все четыре сошлись, кандидат остался один — и
  /// попал он в кандидаты не случайно, а по общему ключевому слову. Спека это и
  /// называет полной копией по КБЖУ; спрашивать здесь модель незачем, а её отказ
  /// стоит лишней копии продукта в базе — ровно того, чего мы избегаем.
  const allFourRead = (seen) =>
    ["kcal", "protein", "fat", "carbs"].every((k) => seen[k] !== null && seen[k] !== undefined);
  const byFullNumbers = (c, pool) =>
    pool.length === 1 && allFourRead(c.seen) && numbersAgree(c.seen, byId[pool[0]]) === true
      ? pool[0] : null;

  const score = {};
  for (const k of Object.keys(ways)) score[k] = { ok: 0, falseYes: 0, falseNo: 0, wrong: 0, n: 0 };

  for (const c of CASES) {
    console.log(`— ${c.what}`);
    for (const [way, poolOf] of Object.entries(ways)) {
      const pool = poolOf(c);
      const shortcut =
        way === "арифметика + имя" ? byNameFirst(c, pool)
        : way === "и полное совпадение чисел" ? (byNameFirst(c, pool) || byFullNumbers(c, pool))
        : null;
      const got = [];
      for (let i = 0; i < N; i++) {
        if (shortcut) { got.push({ match_id: shortcut, why: "то же написание — решено кодом" }); continue; }
        try { got.push(await retry(() => ask(token, c.seen, pool))); }
        catch (e) { got.push({ match_id: "СБОЙ", why: e.message }); }
      }
      const s = score[way];
      const marks = got.map((g) => {
        s.n++;
        if (g.match_id === "СБОЙ") return "сбой";
        if (g.match_id === c.want) { s.ok++; return "верно"; }
        if (c.want === null) { s.falseYes++; return `ЛОЖНОЕ СОГЛАСИЕ ${g.match_id}`; }
        if (g.match_id === null) { s.falseNo++; return "ложный отказ"; }
        s.wrong++; return `не тот: ${g.match_id}`;
      });
      console.log(`   ${way.padEnd(26)} кандидатов ${pool.length}${shortcut ? " (кодом)" : ""}: ${marks.join(", ")}`);
      if (RAW) for (const g of got) console.log(`      «${g.why}»`);
    }
  }

  console.log();
  for (const [way, s] of Object.entries(score)) {
    console.log(`${way.padEnd(26)} верно ${s.ok}/${s.n}   ЛОЖНЫХ СОГЛАСИЙ ${s.falseYes}   ` +
                `ложных отказов ${s.falseNo}   выбран не тот ${s.wrong}`);
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
