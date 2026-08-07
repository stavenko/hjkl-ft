// Конец онбординга: приложение поставлено.
//
// Установка PWA перезагружает вкладку (`appinstalled` в index.html), а
// перезагрузка стирает состояние страницы — и онбординг начинался заново, снова
// прося создать ключ, который человек создал минуту назад. Отметка переживает
// перезагрузку и уводит на конечный экран.
//
// Прогон: node check-onboard-installed.mjs
import { chromium } from "playwright";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
const UID = "00000000-1111-2222-3333-444444444444";

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

const b = await chromium.launch();

async function openOnboard(seedInstalled) {
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
  await ctx.addInitScript((installed) => {
    sessionStorage.setItem("update_auto_applied", "1");
    if (installed) localStorage.setItem("pwa_installed", "1");
  }, seedInstalled);
  const page = await ctx.newPage();
  await page.goto(`${BASE}/onboard?u=${UID}`, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);
  return { ctx, page };
}

// ── 1. Отметки нет — онбординг идёт как шёл ──
{
  const { ctx, page } = await openOnboard(false);
  console.log("\n1. Приложение ещё не поставлено");
  check(
    !(await page.getByTestId("onboard-installed").isVisible().catch(() => false)),
    "конечного экрана нет",
  );
  await ctx.close();
}

// ── 2. Отметка есть — конечный экран, и ничего кроме ──
{
  const { ctx, page } = await openOnboard(true);
  console.log("\n2. Приложение поставлено");
  check(
    await page.getByTestId("onboard-installed").isVisible().catch(() => false),
    "показан конечный экран",
  );
  const text = await page.innerText("body");
  check(
    /re:Norma установлено как приложение на ваш рабочий стол\./.test(text),
    "фраза про установку — дословно",
  );
  check(
    /Закройте браузер и откройте приложение, тапнув по иконке на рабочем столе\./.test(text),
    "фраза про запуск — дословно",
  );
  // «Ни ссылок, ни переходов»: на ЭКРАНЕ не должно быть ничего нажимаемого.
  // Считаем только видимое: нижнее меню приложения живёт в оболочке роутера под
  // этим экраном и человеку не показывается.
  const visible = (sel) =>
    page.$$eval(sel, (els) => els.filter((e) => e.offsetParent !== null).length);
  check((await visible("a")) === 0, "видимых ссылок нет");
  check((await visible("button")) === 0, "видимых кнопок нет");
  check(!/Создать|ключ|Установить/.test(text), "инструкции по установке нет");
  await ctx.close();
}

await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
