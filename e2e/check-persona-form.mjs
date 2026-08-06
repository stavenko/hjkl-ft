// Экран «Персона»: что видит человек, пока профиль не заполнен.
//
// Проверяемое: негодное значение не выбрасывается молча, отсутствие кнопки
// «Готово» объяснено списком незаполненного, а годное значение сохраняется
// сразу по вводу — без ухода с поля (на телефоне из числовой клавиатуры можно
// так и не «уйти»).
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


// 1. Пустой профиль: кнопки нет, и сказано, чего не хватает.
check(!(await closeVisible()), "кнопки «Готово» нет, пока профиль не заполнен");
check(/Чтобы продолжить, заполните/.test(await body()), "показано, чего не хватает");

const nums = page.locator('input[type="number"]');
const height = nums.nth(0);
const year = nums.nth(1);

// 2. Негодный год: молчания быть не должно.
await year.fill("90");
await page.waitForTimeout(600);
check(/Год целиком/.test(await body()), "негодный год объяснён, а не выброшен молча");

// 3. Негодный рост — то же самое.
await height.fill("16");
await page.waitForTimeout(600);
check(/Рост в сантиметрах/.test(await body()), "негодный рост объяснён");

// 4. Годные значения сохраняются по вводу, без ухода с поля.
await height.fill("178");
await year.fill("1990");
await page.selectOption("select", { index: 1 });
await page.waitForTimeout(1200);
check(!/Чтобы продолжить, заполните/.test(await body()), "список незаполненного пропал");
// Заполненный профиль уводит с экрана персоны сам — жать «Готово» не нужно и
// не приходится: кнопка нужна лишь когда экран открыт заново с дашборда.
await page.getByTestId("nav-diary").waitFor({ timeout: 20000 });
check(!/Год рождения/.test(await body()), "экран персоны закрылся сам");

await page.screenshot({ path: "/tmp/persona-form.png", fullPage: true });
await b.close();
console.log(failed ? `\nПРОВАЛОВ: ${failed}` : "\nвсё сошлось");
process.exit(failed ? 1 : 0);
