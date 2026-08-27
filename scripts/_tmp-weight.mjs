import { chromium } from "playwright";
const OUT = "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad";
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2 });
const errs = [];
p.on("pageerror", e => errs.push(String(e)));
await p.goto("http://localhost:8898/index.html", { waitUntil: "networkidle" });
const cases = [
  { sex: "female", age: 35, height: 165, weight: 82 },
  { sex: "female", age: 28, height: 170, weight: 60 },
  { sex: "male", age: 40, height: 180, weight: 100 },
  { sex: "male", age: 65, height: 175, weight: 95 },
  { sex: "male", age: 75, height: 172, weight: 88 },
  { sex: "female", age: 55, height: 160, weight: 120 },
];
const out = await p.evaluate((cs) => cs.map(c => {
  const r = W.calc(c);
  return { ...c, bf: +r.bodyFat.toFixed(1), ffm: +r.fatFree.toFixed(1), ideal: +r.ideal.toFixed(1),
           bmiNow: +r.bmi.toFixed(1), bmiGoal: +(r.ideal / Math.pow(c.height/100, 2)).toFixed(1),
           lose: +r.toLose.toFixed(1), weeksMid: r.mid.weeks, weeksFast: r.fast.weeks, weeksSlow: r.slow.weeks,
           already: r.already };
}), cases);
console.table(out);
// живой прогон интерфейса
await p.locator('[data-sex="male"]').click();
await p.locator("#weight").fill("100");
await p.locator("#weight").dispatchEvent("input");
await p.locator("#height").fill("180");
await p.locator("#height").dispatchEvent("input");
await p.locator("#age").fill("40");
await p.locator("#age").dispatchEvent("input");
await p.waitForTimeout(300);
console.log("verdict:", (await p.locator("#verdict").innerText()).replace(/\s+/g, " "));
console.log("ideal:", await p.locator("#ideal").innerText(), "| facts:", (await p.locator("#facts").innerText()).replace(/\n/g, " · "));
console.log("svg:", await p.locator("#chart svg").count(), "ошибок:", errs.length, errs.join("; "));
await p.screenshot({ path: `${OUT}/weight-top.png` });
await b.close();
