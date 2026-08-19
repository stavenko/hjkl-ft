// СКОЛЬКО ЕДЫ МОДЕЛЬ НЕ СМОГЛА ОПОЗНАТЬ — сводка по Analytics Engine.
//
// Продукт, который не опознан, остаётся без признаков и без нутриентов: это честнее
// выдумки, но оставлять так нельзя — каждое имя в этом списке значит, что словарь
// редких имён пора пополнить.
//
// Считается по датасету определений (`CLIENT_DETECTIONS`), а не по логам воркера:
// там те же события, но полями, и хранятся дольше суток. Строка `ALERT
// unrecognised-food` в логе — для оповещения; здесь — разбор.
//
//   node scripts/unknown-foods.mjs [дней]        по умолчанию 7, прод: ENV=prod
//
// Нужен CLOUDFLARE_API_TOKEN в .env (право Account Analytics Read).
import { readFileSync } from "node:fs";

const DAYS = Number(process.argv[2] || 7);
const ENV = process.env.ENV === "prod" ? "" : "_dev";
const DATASET = `renorma_client_detections${ENV}`;
const ACCOUNT = process.env.CLOUDFLARE_ACCOUNT_ID || "a2809d140765b9e7915c5644c249863b";

const token = (() => {
  if (process.env.CLOUDFLARE_API_TOKEN) return process.env.CLOUDFLARE_API_TOKEN;
  const env = readFileSync(new URL("../.env", import.meta.url), "utf8");
  const m = env.match(/^CLOUDFLARE_API_TOKEN=(.+)$/m);
  if (!m) throw new Error("CLOUDFLARE_API_TOKEN не найден ни в окружении, ни в .env");
  return m[1].trim();
})();

const sql = async (query) => {
  const r = await fetch(
    `https://api.cloudflare.com/client/v4/accounts/${ACCOUNT}/analytics_engine/sql`,
    { method: "POST", headers: { Authorization: `Bearer ${token}` }, body: query },
  );
  const text = await r.text();
  if (!r.ok) throw new Error(`HTTP ${r.status}: ${text.slice(0, 200)}`);
  return JSON.parse(text).data ?? [];
};

const WHERE = `index1 = 'identity.unknown' AND timestamp > NOW() - INTERVAL '${DAYS}' DAY`;

const [total] = await sql(
  `SELECT count() AS events, count(DISTINCT blob6) AS people, count(DISTINCT blob1) AS foods
   FROM ${DATASET} WHERE ${WHERE}`,
);
console.log(`\n=== не опознано за ${DAYS} дн. (${DATASET}) ===`);
console.log(`событий ${total?.events ?? 0} · разных продуктов ${total?.foods ?? 0} · людей ${total?.people ?? 0}\n`);

const byFood = await sql(
  `SELECT blob1 AS food, count() AS n, count(DISTINCT blob6) AS people
   FROM ${DATASET} WHERE ${WHERE} GROUP BY food ORDER BY people DESC, n DESC LIMIT 40`,
);
if (!byFood.length) {
  console.log("ничего — вся еда опознавалась");
} else {
  // Людей важнее событий: один человек, добавивший продукт трижды, — не то же, что
  // трое разных, споткнувшихся об одно имя.
  console.log("продукт                             событий  людей");
  for (const r of byFood) {
    console.log(`  ${String(r.food).slice(0, 34).padEnd(34)} ${String(r.n).padStart(7)}  ${String(r.people).padStart(5)}`);
  }
}

const byDay = await sql(
  `SELECT toDate(timestamp) AS day, count() AS n
   FROM ${DATASET} WHERE ${WHERE} GROUP BY day ORDER BY day`,
);
if (byDay.length) {
  console.log("\nпо дням:");
  for (const r of byDay) console.log(`  ${r.day}  ${"▇".repeat(Math.min(40, Number(r.n)))} ${r.n}`);
}
