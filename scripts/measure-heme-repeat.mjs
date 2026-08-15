// Признак гема N раз подряд: сколько раз ошиблась КАЖДАЯ позиция.
//
// Один прогон говорит только «столько-то из 22» и ничего — про устойчивость:
// разброс одной и той же версии доходил до трёх позиций. Здесь видно, какие
// именно позиции шатаются и насколько, а какие держатся намертво.
//
// RUNS=5 (по умолчанию), остальное — как у measure-heme.mjs: FE, ONLY.
import { spawn } from "node:child_process";

const RUNS = Number(process.env.RUNS || 5);

const run = (i) =>
  new Promise((res, rej) => {
    const p = spawn("node", ["measure-heme.mjs"], {
      cwd: import.meta.dirname,
      env: process.env,
    });
    let out = "";
    p.stdout.on("data", (d) => { out += d; });
    p.stderr.on("data", (d) => process.stderr.write(d));
    p.on("close", (code) => {
      if (code !== 0) return rej(new Error(`прогон ${i}: measure-heme.mjs вышел с ${code}`));
      res(out);
    });
  });

const order = [];                 // позиции в порядке набора
const miss = new Map();           // сколько прогонов ошиблись
const said = new Map();           // что модель отвечала, когда ошибалась
const scores = [];

for (let i = 1; i <= RUNS; i++) {
  const out = await run(i);
  const total = out.match(/попаданий: (\d+)\/(\d+)/);
  if (!total) throw new Error(`прогон ${i}: в выводе нет строки «попаданий»`);
  scores.push(`${total[1]}/${total[2]}`);
  // Строка «модель: …» идёт сразу за промахом, к которому относится.
  let lastMiss = null;
  for (const line of out.split("\n")) {
    const m = line.match(/^(OK  |MISS) (.+?)\s{2,}/);
    if (m) {
      const name = m[2].trim();
      if (!order.includes(name)) order.push(name);
      if (m[1] === "MISS") {
        miss.set(name, (miss.get(name) ?? 0) + 1);
        lastMiss = name;
      } else {
        lastMiss = null;
      }
      continue;
    }
    const r = line.match(/^\s+модель: (.+)$/);
    if (r && lastMiss) {
      const prev = said.get(lastMiss) ?? [];
      if (!prev.includes(r[1])) prev.push(r[1]);
      said.set(lastMiss, prev);
    }
  }
  console.log(`прогон ${i}: ${total[1]}/${total[2]}`);
}

console.log(`\nпрогоны: ${scores.join(", ")}`);
console.log(`\nпозиция                        ошибок из ${RUNS}`);
for (const name of [...order].sort((a, b) => (miss.get(b) ?? 0) - (miss.get(a) ?? 0))) {
  const n = miss.get(name) ?? 0;
  console.log(`${name.padEnd(30)} ${n}`);
  for (const s of said.get(name) ?? []) console.log(`   модель: ${s}`);
}
