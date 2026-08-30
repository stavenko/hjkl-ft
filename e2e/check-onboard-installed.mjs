// Конец онбординга: приложение поставлено.
//
// Установка PWA перезагружает вкладку (`appinstalled` в index.html), а
// перезагрузка стирает состояние страницы — и онбординг начинался заново, снова
// прося создать ключ, который человек создал минуту назад. Отметка переживает
// перезагрузку и уводит на конечный экран.
//
// Прогон: node check-onboard-installed.mjs
import { chromium, devices } from "playwright";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
const UID = "00000000-1111-2222-3333-444444444444";

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

const b = await chromium.launch();

async function openOnboard(seedInstalled) {
  // ТЕЛЕФОН, а не окно телефонного размера: без телефонного User-Agent экран
  // установки уходит в десктопную ветку, где вместо инструкции стоит «приложение
  // предназначено для мобильных устройств» — на компьютер его ставить незачем.
  const ctx = await b.newContext({ ...devices["iPhone 13"] });
  await ctx.addInitScript(() => sessionStorage.setItem("update_auto_applied", "1"));
  const page = await ctx.newPage();
  await page.goto(`${BASE}/onboard?u=${UID}`, { waitUntil: "domcontentloaded" });
  if (seedInstalled) {
    // Отметку ставим ОДИН раз и перезагружаем, а не через addInitScript: тот
    // возвращал бы её на каждую навигацию, и проверка «после сброса не вернулось»
    // проверяла бы саму себя.
    await page.evaluate(() => localStorage.setItem("pwa_installed", "1"));
    await page.reload({ waitUntil: "domcontentloaded" });
  }
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
  check(
    /могут занять несколько минут/.test(text),
    "предупреждение про ожидание на месте",
  );
  check(
    /Если приложение так и не появилось.*прервавшегося VPN/s.test(text),
    "сказано, что делать, если иконка не появилась",
  );
  // Считаем только видимое: нижнее меню приложения живёт в оболочке роутера под
  // этим экраном и человеку не показывается.
  const visible = (sel) =>
    page.$$eval(sel, (els) => els.filter((e) => e.offsetParent !== null).length);
  check((await visible("a")) === 0, "видимых ссылок нет");
  check((await visible("button")) === 1, "кнопка ровно одна — показать инструкцию");
  check(!/Создать ключ/.test(text), "инструкции по установке ещё нет");

  // ── Кнопка снимает отметку и возвращает к инструкции ──
  await page.getByTestId("onboard-btn-show-instructions").click();
  await page.waitForTimeout(1500);
  check(
    /Как установить/.test(await page.innerText("body")),
    "по кнопке показана инструкция по установке",
  );
  check(
    (await page.evaluate(() => localStorage.getItem("pwa_installed"))) === null,
    "отметка об установке снята",
  );
  // И это переживает перезагрузку: иначе человек снова упрётся в «всё готово».
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForTimeout(9000);
  check(
    !(await page.getByTestId("onboard-installed").isVisible().catch(() => false)),
    "после перезагрузки конечный экран не возвращается",
  );
  await ctx.close();
}

await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
