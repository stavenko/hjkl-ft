// ВЕС ПОРЦИИ — ДВА ОТДЕЛЬНЫХ ШАГА.
//
// Прямой вопрос «сколько тут граммов» все восемь доступных моделей отвечают на
// 50-73% ниже правды, и это не чинится ни промптом, ни размером модели (таблица в
// measure-per-image.mjs). Но в их же ответах видно противоречие: qwen3-vl-plus
// описывала мясо как «половина тарелки, слой 2-3 см» — около 700 см³ — и тут же
// называла 150 г. Геометрию описывает щедро, число называет скупо; значит число
// берётся не из описания.
//
// Отсюда всё построение: у модели спрашивают только то, что она умеет, а числа
// считает код. И спрашивают ПОРОЗНЬ — в этом проекте не раз подтверждалось, что два
// вопроса в одном запросе тянут друг друга вниз.
//
//   ШАГ 1. Размер посуды. Модель показывает рамку посуды и рамку предмета известного
//          размера, поперечник считает код. Это локализация, а не прикидка размера:
//          прикидывать модель не умеет (measure-plate-size.mjs — четыре способа
//          спросить, от 16% до 241% ошибки площади). Предмета в кадре нет — запасной
//          путь, прямой вопрос про сантиметры.
//
//   ШАГ 2. Что на посуде и сколько места занимает. Поперечник из шага 1 НАЗЫВАЕТСЯ
//          модели: замерено, что так лучше всего (13% против 35%, если промолчать, и
//          40%, если просить толщину в долях поперечника). Граммы не спрашиваются.
//
//   ШАГ 3. Граммы. Считает код: площадь дна на долю, на толщину, на заполнение и
//          плотность. Коэффициенты — в таблице DENSITY ниже, на виду.
//
// Тарелка с печенью (300 г) и капустой (400 г) при верном поперечнике: +8% и +13%.
//
//   node scripts/measure-portion-steps.mjs [--model МОДЕЛЬ] [--only ПОДСТРОКА]
//                                          [--plate СМ] [--raw]
//
// --plate задаёт поперечник вместо шага 1: дома посуда своя, её размер можно узнать
// один раз (снимок с пластиковой картой — она 85.6 мм по стандарту) и хранить.

import { readFileSync } from "node:fs";

const AI = process.env.AI || "https://ai-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.JWT_SECRET || "dev-secret-change-in-production";

const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > -1 && process.argv[i + 1] && !process.argv[i + 1].startsWith("--")
    ? process.argv[i + 1] : def;
};
const MODEL = arg("model", process.env.MODEL || "qwen3-vl-plus");
const ONLY = arg("only", null);
const PLATE_CM = Number(arg("plate", 0)) || null;
const RAW = process.argv.includes("--raw");

/// Эталоны — то, что человек взвесил, и размеры его посуды: маленькая 19 см, большая
/// 26. Какая на каком снимке — установлено, а не угадано: на рыбе вилка занимает
/// 1068 px при тарелке в 1150, то есть тарелка едва длиннее вилки; на печени 400 г
/// соцветий на маленькой лежали бы горкой почти в 6 см, а на большой — в 3.
const CASES = {
  "food-liver-cauliflower.jpg": {
    plate: 26,
    want: [
      { keys: ["печен", "мясо", "говяд", "субпрод"], grams: 300 },
      { keys: ["капуст"], grams: 400 },
    ],
  },
  "food-fish.jpg": { plate: 19, want: [] },
};

// ── ШАГ 1: размер посуды ──

/// Годные эталоны — только столовые приборы: они разбросаны по размеру мало (вилки и
/// ложки 19-21 см у всех), лежат в одной плоскости с посудой и не путаются между
/// собой. Телефон в списке есть, чтобы модели было что назвать, но мерить по нему
/// НЕЛЬЗЯ: телефоны 14-17 см, планшеты 25-28, а модель их не различает — на снимке с
/// печенью она назвала планшет телефоном и завысила тарелку на 14%, что превратилось
/// в 40% по весу. Запрет на частично видимый предмет в промпте её не остановил, а
/// null здесь останавливает: ответ уходит на запасной путь.
const REF_CM = { fork: 19.5, spoon: 19, knife: 21, phone: null, none: null };

const PLATE_SCHEMA = {
  type: "object",
  properties: {
    reference: {
      description: "Столовый прибор, видимый в кадре ЦЕЛИКОМ: \"fork\" — вилка, \"spoon\" — " +
        "ложка, \"knife\" — нож. Прибора нет или он виден лишь частично — \"none\". Если в " +
        "кадре лежит телефон или планшет — \"phone\"; мерить по нему не будут, но назвать стоит.",
      type: "string",
      enum: Object.keys(REF_CM),
    },
    plate_box: {
      description: "Рамка ПОСУДЫ в долях кадра: [x0, y0, x1, y1], где 0 — левый верхний угол, " +
        "1 — правый нижний. Охватывает посуду целиком, по внешнему краю.",
      type: "array", items: { type: "number" },
    },
    reference_box: {
      description: "Рамка предмета из поля reference, в тех же долях кадра. Предмета нет — [0,0,0,0].",
      type: "array", items: { type: "number" },
    },
    diameter_cm_guess: {
      description: "Поперечник посуды в сантиметрах на случай, если reference — \"none\" и мерить " +
        "не по чему. Тарелки бывают очень разные: десертная и закусочная заметно меньше обеденной, " +
        "миска бывает совсем небольшой. Не считай всякую тарелку обеденной по умолчанию.",
      type: "number",
    },
  },
  required: ["reference", "plate_box", "reference_box", "diameter_cm_guess"],
};

const PLATE_PROMPT =
  `На снимке еда в посуде. Нужен ТОЛЬКО размер самой посуды — про еду не думай вовсе.\n\n` +
  `Мерить будем по предмету известного размера, если он есть в кадре и виден целиком. Твоё дело — ` +
  `показать, где посуда и где этот предмет; сантиметры посчитают без тебя.`;

/// Доли кадра переводим в ПИКСЕЛИ прежде, чем мерить длины. Кадр не квадратный, и одна
/// и та же доля по горизонтали и по вертикали — разное число пикселей; считать длину
/// прямо в долях значит растянуть кадр в квадрат и получить вздор (проверено: 74%
/// ошибки против 2% после исправления).
///
/// У посуды берём большую сторону рамки: снятая под углом тарелка становится эллипсом,
/// и длинная ось ближе к настоящему поперечнику. У вытянутого предмета — диагональ
/// рамки: вилка лежит наискось, и её длина это диагональ, а не сторона.
function plateFromBoxes(a, W, H) {
  const cm = REF_CM[a.reference];
  if (!cm) return null;
  const [px0, py0, px1, py1] = a.plate_box || [];
  const [rx0, ry0, rx1, ry1] = a.reference_box || [];
  if ([px0, py0, px1, py1, rx0, ry0, rx1, ry1].some((v) => typeof v !== "number")) return null;
  const plate = Math.max(Math.abs(px1 - px0) * W, Math.abs(py1 - py0) * H);
  const ref = Math.hypot((rx1 - rx0) * W, (ry1 - ry0) * H);
  return ref ? (plate / ref) * cm : null;
}

// ── ШАГ 2: что на посуде и сколько места занимает ──

const REGIONS_SCHEMA = {
  type: "object",
  properties: {
    regions: {
      description: "Каждая отдельная еда на посуде. Граммы НЕ называй — их посчитают без тебя.",
      type: "array",
      items: {
        type: "object",
        properties: {
          name: { description: "Что это, по-русски, 1-3 слова.", type: "string" },
          share_of_plate: {
            description: "Какую долю ДНА посуды закрывает эта еда, если смотреть сверху: 0.5 — " +
              "половину, 0.25 — четверть. Считай площадь пятна, а не объём.",
            type: "number",
          },
          thickness_cm: {
            description: "Насколько высоко еда поднимается над дном в самом типичном месте, в " +
              "сантиметрах. Плоский ломтик тонкий, горка высокая.",
            type: "number",
          },
          how_dense: {
            description: "Насколько плотно уложена еда. \"solid\" — сплошной кусок без пустот " +
              "(филе, котлета, ломоть). \"packed\" — вязкая масса, пустот почти нет (каша, пюре, " +
              "тушёное с подливой). \"loose\" — куски с большими промежутками воздуха между ними " +
              "(соцветия, салат, ягоды горкой).",
            type: "string",
            enum: ["solid", "packed", "loose"],
          },
        },
        required: ["name", "share_of_plate", "thickness_cm", "how_dense"],
      },
    },
  },
  required: ["regions"],
};

const REGIONS_PROMPT = (plateCm) =>
  `Перед тобой снимок еды. Твоя задача — ЧТО на посуде и СКОЛЬКО МЕСТА это занимает.\n\n` +
  `Не думай о калорийности и не называй вес: граммы посчитают без тебя, по твоим размерам. ` +
  `Отвечай так, как если бы измерял линейкой, а не как если бы прикидывал порцию на глаз.\n\n` +
  `Поперечник посуды известен: ${plateCm.toFixed(0)} сантиметров. Отталкивайся от него, оценивая ` +
  `высоту слоя — сам размер посуды прикидывать не нужно.`;

// ── ШАГ 3: граммы ──

/// Сколько объёма реально занято едой и сколько весит кубический сантиметр. Числа
/// грубые и взяты из здравого смысла, а не из замера, — поэтому и стоят здесь, в коде:
/// их видно, их можно поправить, и они не спрятаны внутри модели. Подтверждены пока
/// только на loose (соцветия) и packed (тушёное мясо); solid не проверялся ни разу.
const DENSITY = {
  solid:  { fill: 0.90, g_per_cm3: 1.00 },
  packed: { fill: 0.80, g_per_cm3: 0.95 },
  loose:  { fill: 0.45, g_per_cm3: 0.90 },
};

function grams(diameterCm, r) {
  const area = Math.PI * (diameterCm / 2) ** 2;
  const { fill, g_per_cm3 } = DENSITY[r.how_dense] || DENSITY.packed;
  return area * r.share_of_plate * r.thickness_cm * fill * g_per_cm3;
}

// ── обвязка ──

/// Размеры JPEG из его заголовков — чтобы не тащить зависимость ради двух чисел.
function jpegSize(buf) {
  let i = 2;
  while (i < buf.length) {
    if (buf[i] !== 0xff) { i++; continue; }
    const m = buf[i + 1];
    if (m >= 0xc0 && m <= 0xcf && m !== 0xc4 && m !== 0xc8 && m !== 0xcc) {
      return [buf.readUInt16BE(i + 7), buf.readUInt16BE(i + 5)];
    }
    i += 2 + buf.readUInt16BE(i + 2);
  }
  throw new Error("не разобрать размеры JPEG");
}

const b64url = (buf) => Buffer.from(buf).toString("base64url");

async function mintToken() {
  const now = Math.floor(Date.now() / 1000);
  const data =
    b64url(JSON.stringify({ alg: "HS256", typ: "JWT" })) + "." +
    b64url(JSON.stringify({ sub: `geom-${Date.now()}`, iat: now, exp: now + 7200, caps: [], token_id: "t" }));
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

async function ask(token, image, prompt, schema) {
  const r = await fetch(`${AI}/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({
      model: MODEL,
      messages: [{ role: "user", content: [
        { type: "text", text: prompt },
        { type: "image_url", image_url: { url: `data:image/jpeg;base64,${image}` } },
      ] }],
      response_format: { type: "json_schema", json_schema: { name: "r", schema, strict: true } },
      stream: true, think: false, max_tokens: 1200,
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
  return JSON.parse(content.replace(/^```(json)?/, "").replace(/```$/, "").trim());
}

async function main() {
  const token = await mintToken();
  console.log(`модель ${MODEL}\n`);

  for (const [file, c] of Object.entries(CASES)) {
    if (ONLY && !file.includes(ONLY)) continue;
    const buf = readFileSync(`scripts/fixtures/${file}`);
    const image = buf.toString("base64");
    const [W, H] = jpegSize(buf);
    console.log(file);

    // ШАГ 1
    let plateCm = PLATE_CM, how = PLATE_CM ? "задан" : null;
    if (!plateCm) {
      try {
        const p = await ask(token, image, PLATE_PROMPT, PLATE_SCHEMA);
        const measured = plateFromBoxes(p, W, H);
        plateCm = measured ?? p.diameter_cm_guess;
        how = measured ? `по рамкам, эталон ${p.reference}` : "на глаз, эталона в кадре нет";
        if (RAW) console.log(`    ${JSON.stringify(p)}`);
      } catch (e) { console.log(`  шаг 1 сбой — ${e.message}\n`); continue; }
    }
    const err = ((plateCm / c.plate - 1) * 100).toFixed(0);
    console.log(`  посуда: ${plateCm.toFixed(1)} см (${how})   правда ${c.plate} см, ошибка ${err > 0 ? "+" : ""}${err}%`);

    // ШАГ 2 и 3
    let a;
    try { a = await ask(token, image, REGIONS_PROMPT(plateCm), REGIONS_SCHEMA); }
    catch (e) { console.log(`  шаг 2 сбой — ${e.message}\n`); continue; }

    for (const r of a.regions || []) {
      const g = grams(plateCm, r);
      const truth = c.want.find((t) => t.keys.some((k) => r.name.toLowerCase().includes(k)))?.grams;
      console.log(`  ${r.name}: доля ${r.share_of_plate}, слой ${r.thickness_cm} см, ${r.how_dense}` +
        `  →  ${g.toFixed(0)} г` +
        (truth ? `   (эталон ${truth}, ошибка ${((g / truth - 1) * 100).toFixed(0)}%)` : ""));
    }
    console.log();
  }
}

main().catch((e) => { console.error(e); process.exit(1); });
