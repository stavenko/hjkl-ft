// Экран установки, каким его видят Chrome и Safari на iPhone.
//
// Ветка выбирается по User-Agent, поэтому проверять надо ИМЕННО с этими UA: под
// обычным браузером рисуется другая инструкция. Заодно проверяется, что все
// снимки шагов реально отдаются (404 у картинки на экране установки — это молча
// пустая инструкция).
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const UID = "demo-user-123";

const CASES = [
  // Chrome на iOS: WebKit внутри, «CriOS» в UA — слова «chrome» там нет.
  { name: "chrome", out: "/tmp/ios-chrome-install.png", want: "адресной строки",
    ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 " +
        "(KHTML, like Gecko) CriOS/137.0.7151.107 Mobile/15E148 Safari/604.1" },
  { name: "safari", out: "/tmp/ios-safari-install.png", want: "нижней панели",
    ua: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15 " +
        "(KHTML, like Gecko) Version/18.5 Mobile/15E148 Safari/604.1" },
];

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const b = await chromium.launch({ headless: true });
for (const c of CASES) {
  const context = await b.newContext({
    userAgent: c.ua, viewport: { width: 390, height: 844 },
    deviceScaleFactor: 3, isMobile: true, hasTouch: true, serviceWorkers: "block",
  });
  const page = await context.newPage();
  const missing = [];
  page.on("response", (r) => {
    if (r.url().includes("/onboard-img/") && r.status() >= 400) missing.push(`${r.status()} ${r.url()}`);
  });
  await page.goto(`${BASE}/?u=${UID}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(10000);

  const text = (await page.locator("body").innerText()).replace(/\s+/g, " ");
  check(`${c.name}: своя инструкция`, text.includes(c.want), text.slice(0, 140));
  const shots = await page.$$eval(".step-shot", (els) => els.map((e) => e.getAttribute("src")));
  check(`${c.name}: у всех шагов есть снимок`, shots.length === 3, JSON.stringify(shots));
  check(`${c.name}: снимки отдаются`, missing.length === 0, JSON.stringify(missing));
  await page.screenshot({ path: c.out, fullPage: true });
  console.log("      снято:", c.out);
  await context.close();
}
await b.close();

console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
