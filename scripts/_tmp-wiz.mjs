import { chromium } from "playwright";
const OUT = "/private/tmp/claude-501/-Users-vasilijstavenko-projects-hjkl-ft/56df53af-a1ed-4117-8e82-8a1f8aad90e8/scratchpad";
const b = await chromium.launch();
const p = await b.newPage({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2 });
const errs = []; p.on("pageerror", e => errs.push(String(e)));
await p.goto("http://localhost:8898/index.html", { waitUntil: "networkidle" });

// шаг 1: мужчина 180
await p.locator('[data-sex="male"]').click();
await p.locator("#height").fill("180"); await p.locator("#height").dispatchEvent("input");
await p.locator(".card").screenshot({ path: `${OUT}/wiz1.png` });
await p.locator("#next").click(); await p.waitForTimeout(200);

// шаг 2: возраст 45
await p.locator("#age").fill("45"); await p.locator("#age").dispatchEvent("input");
console.log("шаг2 note:", await p.locator("#ageNote").innerText());
await p.locator(".card").screenshot({ path: `${OUT}/wiz2.png` });
await p.locator("#next").click(); await p.waitForTimeout(200);

// шаг 3: тренируется много лет
await p.locator('[data-train="2"]').click();
await p.locator(".card").screenshot({ path: `${OUT}/wiz3.png` });
await p.locator("#next").click(); await p.waitForTimeout(200);

// шаг 4: идеальный + текущий 100
console.log("шаг4 идеал:", await p.locator("#ideal").innerText(), "|", (await p.locator("#facts").innerText()).replace(/\n/g, " · "));
await p.locator("#weight").fill("100"); await p.locator("#weight").dispatchEvent("input");
await p.locator(".card").screenshot({ path: `${OUT}/wiz4.png` });
await p.locator("#next").click(); await p.waitForTimeout(300);

// шаг 5: график
console.log("шаг5:", (await p.locator("#verdict").innerText()).replace(/\s+/g, " "));
console.log("svg:", await p.locator("#chart svg").count(), "| cta виден:", await p.locator("#cta").isVisible());
await p.locator(".card").screenshot({ path: `${OUT}/wiz5.png` });
await p.screenshot({ path: `${OUT}/wiz5-full.png`, fullPage: true });

// без тренировок для сравнения
await p.locator("#back2").click(); await p.waitForTimeout(200);
const noTrain = await p.evaluate(() => [
  W.ideal(180, 45, "male", 0).toFixed(1), W.ideal(180, 45, "male", 1).toFixed(1), W.ideal(180, 45, "male", 2).toFixed(1),
  W.ideal(165, 30, "female", 0).toFixed(1), W.ideal(165, 60, "female", 0).toFixed(1), W.ideal(165, 75, "female", 0).toFixed(1),
]);
console.log("идеал м180/45 (0/1/2 трен):", noTrain.slice(0,3).join(" / "), "| ж165 в 30/60/75 лет:", noTrain.slice(3).join(" / "));
console.log("ошибок:", errs.length, errs.join("; "));
await b.close();
