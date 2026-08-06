// Экран «Персона» при первом запуске.
//
// Проверяемое: человеку объяснено, что это за экран; кнопка «Готово» есть сразу;
// нажатие на неё при незаполненной форме называет каждую беду словами и красит
// виноватые поля; заполненная форма уводит с экрана, и второй раз он не
// показывается.
import { chromium } from "playwright";
import { execSync } from "node:child_process";

const BASE = process.env.BASE || "https://renorma-fit-dev.pages.dev";
// Аккаунт заводится СВЕЖИЙ на каждый прогон: заполненный профиль синкается на
// сервер и возвращается в следующий чистый контекст — на использованном
// пользователе экран персоны больше не показывается.
const UID = `persona-form-${process.env.RUN_ID || Date.now().toString(36)}`;
const seeded = execSync(
  `node ../frontend/scripts/seed-test-subscription.mjs ${UID}`,
  { encoding: "utf8" },
);
const TOKEN = (seeded.match(/^token:\s+(\S+)$/m) || [])[1];
if (!TOKEN) {
  console.error("не удалось завести тестовый аккаунт:\n" + seeded);
  process.exit(1);
}

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "  ok  " : "  ПРОВАЛ "} ${what}`);
  if (!ok) failed++;
};

const b = await chromium.launch();
const ctx = await b.newContext({ viewport: { width: 390, height: 844 } });
// Сессия без прохождения входа: приложение открывает базу по user_id из
// localStorage — этого достаточно, чтобы дойти до экрана персоны.
await ctx.addInitScript(([uid, token]) => {
  // Гасим автообновление до входа: иначе перезагрузка посреди проверки
  // уносит введённое и подменяет экран.
  sessionStorage.setItem("update_auto_applied", "1");
  localStorage.setItem("user_id", uid);
  localStorage.setItem("auth_token", token);
  localStorage.setItem("token_id", "tok1");
  localStorage.setItem("auth_ctx", "browser");
}, [UID, TOKEN]);
const page = await ctx.newPage();
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
// Первый заход всегда перезагружается сам: index.html перезагружает страницу,
// когда впервые установленный сервис-воркер переходит в activated. Пока это не
// улеглось, введённое в форму уносит.
await page
  .waitForFunction(() => !!navigator.serviceWorker.controller, null, { timeout: 40000 })
  .catch(() => {});
await page.waitForTimeout(2500);

const dismiss = page.getByTestId("pwa-btn-dismiss");
if (await dismiss.isVisible({ timeout: 40000 }).catch(() => false)) {
  await dismiss.click();
}
await page.getByText("Персона").first().waitFor({ timeout: 60000 });

const body = () => page.innerText("body");
const closeVisible = () =>
  page.getByRole("button", { name: "Готово" }).isVisible().catch(() => false);


// 1. Первый запуск: объяснение и кнопка на месте.
check(/приложение для похудения/.test(await body()), "объяснено, что это за экран");
check(await page.getByTestId("persona-btn-done").isVisible(), "кнопка «Готово» есть сразу");
check(
  !(await page.getByTestId("persona-problems").isVisible().catch(() => false)),
  "до нажатия ничего не красное",
);

// 2. Нажатие на пустой форме называет каждую беду.
await page.getByTestId("persona-btn-done").click();
await page.waitForTimeout(600);
const problems = await page.getByTestId("persona-problems").innerText();
console.log("   разбор формы:", JSON.stringify(problems.replace(/\n/g, " / ")));
check(/Укажите пол/.test(problems), "сказано про пол");
check(/Добавьте свой рост/.test(problems), "сказано про рост");
check(/Введите год рождения/.test(problems), "сказано про год рождения");
const ring = async (id) =>
  (await page.getByTestId(id).getAttribute("style")).includes("inset 0 0 0 2px");
check(await ring("persona-sex"), "поле пола подсвечено");
check(await ring("persona-height"), "поле роста подсвечено");
check(await ring("persona-year"), "поле года подсвечено");

// 3. Негодный год не считается заполненным.
await page.getByTestId("persona-year").fill("90");
await page.getByTestId("persona-btn-done").click();
await page.waitForTimeout(600);
check(
  /Введите год рождения/.test(await page.getByTestId("persona-problems").innerText()),
  "«90» не сходит за год рождения",
);

// 4. Заполненная форма уводит с экрана.
await page.getByTestId("persona-height").fill("178");
await page.getByTestId("persona-year").fill("1990");
await page.getByTestId("persona-sex").selectOption("male");
await page.getByTestId("persona-btn-done").click();
await page.getByTestId("nav-diary").waitFor({ timeout: 20000 });
check(!/приложение для похудения/.test(await body()), "экран персоны закрылся");

// 5. Второй запуск того же аккаунта: экрана больше нет.
const page2 = await ctx.newPage();
await page2.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await page2.getByTestId("nav-diary").waitFor({ timeout: 60000 });
await page2.waitForTimeout(2000);
check(
  !/приложение для похудения/.test(await page2.innerText("body")),
  "второй раз экран не показывается",
);

await page.screenshot({ path: "/tmp/persona-form.png", fullPage: true });
await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
