// НЕДЕЛЯ КЛЕТЧАТКИ — на живом приложении.
//
// Восьмая тема пути: один НЕДЕЛЬНЫЙ индикатор и ни одной шкалы. Проверяется ровно
// это — что значок появился, шкалы не завелось, цвет считается по недельной планке
// (14 г на 1000 ккал, не меньше 25 г/сут), а задание на неделе яиц обещает именно
// неделю клетчатки.
//
//   node scripts/check-fiber-week.mjs
import { chromium } from "playwright";
import { openSeeded, DEFAULT_URL } from "./harness.mjs";
import { sceneSeed } from "./widget-scene.mjs";

const BASE = process.env.FE || DEFAULT_URL;

let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

/// Открыть сцену и снять с виджета всё, что нужно проверке.
async function look(browser, opts) {
  const { context, page } = await openSeeded(browser, {
    baseUrl: BASE,
    context: { serviceWorkers: "block", viewport: { width: 430, height: 932 } },
    uid: `fiber-${opts.week}-${opts.target ?? "all"}-${Math.floor(Math.random() * 1e6)}`,
    seed: sceneSeed(opts),
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);
  const seen = await page.evaluate(() => ({
    row: [...document.querySelectorAll("[data-ind]")].map((el) => [
      el.getAttribute("data-ind"),
      getComputedStyle(el.querySelector("div")).backgroundColor,
    ]),
    gauges: [...document.querySelectorAll("[data-gauge]")].map((el) => el.getAttribute("data-gauge")),
    caption: document.querySelector('[data-testid="progress-widget"]')?.innerText.split("\n")[0] ?? "",
  }));
  await context.close();
  return seen;
}

const NAME = (rgb) =>
  /224, *48/.test(rgb) ? "красный"
    : /232, *133/.test(rgb) ? "оранжевый"
    : /31, *164/.test(rgb) ? "зелёный"
    : /154, *160/.test(rgb) ? "серый"
    : rgb;

const browser = await chromium.launch({ headless: true });

// 1. Неделя ЯИЦ: клетчатки ещё нет, но задание уже обещает её.
{
  const s = await look(browser, { week: "egg", target: "egg" });
  check("на неделе яиц значка клетчатки нет",
    !s.row.some(([n]) => n === "Клетчатка"), s.row.map(([n]) => n).join(", "));
  check("задание обещает неделю клетчатки",
    s.caption.includes("неделю клетчатки"), s.caption);
}

// 2. Неделя КЛЕТЧАТКИ, планка набрана: значок есть и он зелёный, шкалы нет.
{
  const s = await look(browser, { week: "fiber", target: null });
  const fiber = s.row.find(([n]) => n === "Клетчатка");
  check("значок клетчатки появился", !!fiber, s.row.map(([n]) => n).join(", "));
  check("набранная планка — зелёный", fiber && NAME(fiber[1]) === "зелёный",
    fiber ? NAME(fiber[1]) : "—");
  check("своей шкалы у клетчатки нет",
    !s.gauges.some((g) => /Клетчатк/i.test(g ?? "")), s.gauges.join(" · "));
  check("задание называет недельную планку в граммах",
    /Наберите за неделю \d+ г клетчатки/.test(s.caption), s.caption);
}

// 3. Неделя КЛЕТЧАТКИ, планка провалена: значок оранжевый — одна незакрытая неделя.
{
  const s = await look(browser, { week: "fiber", target: "fiber" });
  const fiber = s.row.find(([n]) => n === "Клетчатка");
  check("непобранная планка — оранжевый", fiber && NAME(fiber[1]) === "оранжевый",
    fiber ? NAME(fiber[1]) : "значка нет");
}

await browser.close();
process.exit(failed ? 1 : 0);
