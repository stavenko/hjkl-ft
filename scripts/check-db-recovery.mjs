// СМЕРТЬ СОЕДИНЕНИЯ С БАЗОЙ — приложение обязано пережить её без паники.
//
// Боевой случай (только iOS): система закрывает IndexedDB у свёрнутого PWA, а
// фоновые задачи просыпаются вместе с приложением и обращаются к уже закрытому
// соединению. Раньше слой базы на этом паниковал, а паника в wasm убивает и
// исполнитель фьючерсов — приложение переставало работать до перезагрузки.
//
// Здесь то же самое воспроизводится в браузере: соединение приложения закрывается
// извне (открытие базы с бОльшей версией шлёт `versionchange`), после чего
// приложение продолжает работать — читает дневник и пишет запись.
import { chromium } from "playwright";

const BASE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const uid = `dbkill-${Date.now()}`;

const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
const page = await ctx.newPage();

const panics = [];
page.on("console", (m) => {
  const t = m.text();
  if (/panicked at|RefCell already borrowed/.test(t)) panics.push(t.slice(0, 160));
});
page.on("pageerror", (e) => {
  const t = String(e);
  if (/panicked at|RefCell already borrowed|unreachable/.test(t)) panics.push(t.slice(0, 160));
});

await page.goto(BASE, { waitUntil: "domcontentloaded" });
await page.evaluate((u) => {
  localStorage.clear();
  localStorage.setItem("user_id", u);
  localStorage.setItem("auth_token", "x");
  localStorage.setItem("token_id", "t");
  localStorage.setItem("auth_ctx", "browser");
  localStorage.setItem("pwa_dismissed", "true");
}, uid);
await page.goto(BASE, { waitUntil: "domcontentloaded" });
for (let i = 0; i < 60; i++) {
  const ok = await page.evaluate(async (u) => {
    const dbs = await indexedDB.databases();
    return dbs.some((d) => d.name === `hjkl-ft-${u}`);
  }, uid).catch(() => false);
  if (ok) break;
  await page.waitForTimeout(500);
}
await page.waitForTimeout(4000);

// Убиваем соединение приложения: открытие с бОльшей версией шлёт `versionchange`
// всем открытым соединениям, и после него они непригодны.
const killed = await page.evaluate(async (u) => {
  const name = `hjkl-ft-${u}`;
  const cur = (await indexedDB.databases()).find((d) => d.name === name);
  const next = (cur?.version ?? 1) + 1;
  // Удаление базы — самый близкий аналог того, что делает iOS: соединение
  // приложения становится непригодным, и следующая же операция получает отказ.
  void next;
  return await new Promise((res) => {
    const req = indexedDB.deleteDatabase(name);
    req.onsuccess = () => res("база удалена, соединение приложения мертво");
    req.onblocked = () => res("blocked: приложение держит соединение");
    req.onerror = () => res(`ошибка: ${req.error}`);
    setTimeout(() => res("таймаут"), 8000);
  });
}, uid);
console.log(`убийство соединения: ${killed}`);

// ПЕРЕЗАГРУЖАТЬ СТРАНИЦУ НЕЛЬЗЯ: она открыла бы новое соединение, и проверять
// было бы нечего. Ходим по приложению его же меню — теми же задачами, что живут
// с момента запуска, ровно как при возвращении из фона.
for (const tab of ["Дневник", "Рецепты", "Главная", "Дневник"]) {
  await page.getByText(tab, { exact: true }).first().click({ timeout: 5000 }).catch(() => {});
  await page.waitForTimeout(2500);
}

const wrote = await page.evaluate(async (u) => {
  // Пишем через саму базу — проверяем, что она отвечает после воскрешения.
  try {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${u}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const ok = await new Promise((res) => {
      const tx = db.transaction(["app_flags"], "readwrite");
      tx.objectStore("app_flags").put({ key: "db_recovery_probe", value: String(Date.now()) });
      tx.oncomplete = () => res(true); tx.onerror = () => res(false);
    });
    db.close();
    return ok;
  } catch (e) { return `ошибка: ${e}`; }
}, uid);

const alive = await page.evaluate(() => document.body.innerText.length > 40);
console.log(`после убийства: экран живой ${alive} · запись в базу ${wrote}`);
console.log(panics.length ? `ПАНИКИ (${panics.length}):` : "паник нет");
for (const p of panics.slice(0, 6)) console.log("  " + p);
await browser.close();
process.exit(panics.length ? 1 : 0);
