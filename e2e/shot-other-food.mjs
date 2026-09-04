// Снимок экрана «Другая еда» — нового пути записи еды, который живёт за
// кураторским флагом `lazy_food`. Не испытание: просто показать, как это выглядит.
//
// Сеанс кладётся засевом, без онбординга (`docs/testing.md`, раздел про e2e).
// Токен ненастоящий — экран сетевого счёта не касается.
import { existsSync } from 'node:fs';
import { chromium, devices } from '@playwright/test';

// Те же две поправки окружения, что в `playwright.config.ts`: этот скрипт —
// обычный node, конфигурацию Playwright он не читает, а без них в контейнере
// агента браузер не запустится и наружу не выйдет. Почему именно так — там же.
const IMAGE_CHROMIUM = '/opt/pw-browsers/chromium';
const proxyServer = process.env.HTTPS_PROXY || process.env.https_proxy;
const browser = await chromium.launch({
  ...(existsSync(IMAGE_CHROMIUM) ? { executablePath: IMAGE_CHROMIUM } : {}),
  ...(proxyServer ? { proxy: { server: proxyServer }, args: ['--ssl-version-max=tls1.2'] } : {}),
});
const ctx = await browser.newContext({
  ...devices['iPhone 13'],
  serviceWorkers: 'block',
  bypassCSP: true,
});
const page = await ctx.newPage();
page.on('console', m => { if (/засев|ошиб/i.test(m.text())) console.log(m.text().slice(0, 160)); });

const BASE = process.env.BASE || 'https://renorma-fit-dev.pages.dev';
const OUT = process.env.OUT || './';
await page.goto(BASE + '/', { waitUntil: 'load' });
const now = new Date().toISOString();
await page.evaluate((now) => {
  localStorage.setItem('user_id', 'e2e-shot-' + Date.now());
  localStorage.setItem('auth_token', 'e2e-not-a-real-token');
  localStorage.setItem('auth_ctx', 'browser');
  localStorage.setItem('pwa_dismissed', 'true');
  localStorage.setItem('ft_test_seed', JSON.stringify({
    profile: [{ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1990, goal: 'lose', updated_at: now }],
    app_flags: [
      { key: 'ft_subscription', value: JSON.stringify({ plan: 'e2e', end: Date.now() + 3e10, active: true }), updated_at: now },
      { key: 'ft_subscription_checked_at', value: String(Date.now()), updated_at: now },
      { key: 'push_onboarding_dismissed', value: 'true', updated_at: now },
      { key: 'feature.lazy_food', value: 'true', updated_at: now },
    ],
  }));
}, now);
await page.reload({ waitUntil: 'load' });

await page.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 25000 });
await page.getByTestId('nav-diary').click();
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 15000 });
await page.getByTestId('meal-add').first().click();

const btn = page.getByTestId('diary-add-btn-other-food');
await btn.waitFor({ state: 'visible', timeout: 15000 });
console.log('кнопка:', (await btn.textContent())?.trim());
await page.screenshot({ path: OUT + 'other-food-1-picker.png' });

await btn.click();
await page.getByTestId('other-food-panel').waitFor({ state: 'visible', timeout: 15000 });
await page.waitForTimeout(800);
await page.screenshot({ path: OUT + 'other-food-2-panel.png' });

// Со снимками: кнопка становится плиткой в их ряду. Кадры настоящие — те же, на
// которых мерили распознавание (тарелка и этикетка: два разных случая сразу).
await page.setInputFiles('#other-food-photo-input', [
  '../scripts/fixtures/food-liver-cauliflower.jpg',
  '../scripts/fixtures/label-smetana-1.jpg',
]);
await page.getByTestId('other-food-thumb').first().waitFor({ state: 'visible', timeout: 20000 });
await page.waitForTimeout(1200);
console.log('снимков:', await page.getByTestId('other-food-thumb').count());
await page.screenshot({ path: OUT + 'other-food-3-with-photos.png' });

// Нажатие на миниатюру открывает снимок: обрезка и удаление.
await page.getByTestId('other-food-thumb').first().click();
await page.getByTestId('photo-crop').waitFor({ state: 'visible', timeout: 15000 });
await page.waitForTimeout(700);
await page.screenshot({ path: OUT + 'other-food-4-crop.png' });

// Тот же экран во время жеста: сетка в треть показывается только пока тянут.
// Тянем правый нижний угол внутрь и снимаем ДО отпускания.
const se = await page.getByTestId('photo-crop-grip-se').boundingBox();
await page.mouse.move(se.x + se.width / 2, se.y + se.height / 2);
await page.mouse.down();
await page.mouse.move(se.x - 70, se.y - 90, { steps: 12 });
await page.waitForTimeout(300);
await page.screenshot({ path: OUT + 'other-food-5-crop-drag.png' });
await page.mouse.up();

// Закрываем просмотр и добавляем запись: она обязана появиться в дневнике сразу,
// нераспознанной, — разбор идёт фоном и требует сети.
await page.getByTestId('photo-crop-cancel').click();
await page.getByTestId('other-food-description').fill('Печень со сливками и цветная капуста, примерно по тарелке');
await page.getByTestId('other-food-add').click();
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 20000 });
await page.waitForTimeout(2500);
console.log('в дневнике строк:', await page.locator('[data-testid^="diary-"]').count());
await page.screenshot({ path: OUT + 'other-food-6-diary.png' });

// ── ВТОРАЯ запись через тот же интерфейс, с тремя снимками ──────────────────
//
// Нужна, чтобы в одном приёме пищи оказалось несколько фотографий: они лежат на
// записях, а не на приёме, и увидеть их вместе можно только так.
await page.getByTestId('meal-add').first().click();
await page.getByTestId('diary-add-btn-other-food').waitFor({ state: 'visible', timeout: 15000 });
await page.getByTestId('diary-add-btn-other-food').click();
await page.getByTestId('other-food-panel').waitFor({ state: 'visible', timeout: 15000 });
await page.setInputFiles('#other-food-photo-input', [
  '../scripts/fixtures/food-fish.jpg',
  '../scripts/fixtures/label-teos-1.jpg',
  '../scripts/fixtures/label-kartoshka-1.jpg',
]);
await page.getByTestId('other-food-thumb').first().waitFor({ state: 'visible', timeout: 20000 });
await page.waitForTimeout(1200);
await page.getByTestId('other-food-description').fill('Скумбрия из духовки, творожок и немного варёной картошки');
await page.getByTestId('other-food-add').click();
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 20000 });
await page.waitForTimeout(2000);

// ── Разбор первой записи + обычные записи из базы ───────────────────────────
//
// Настоящий разбор отсюда не запустить: токен ненастоящий, ai-воркер отвечает 401.
// Поэтому первую запись подменяем тем, чем она стала бы после разбора. Вторая
// остаётся нераспознанной — так на одном экране видно оба состояния сразу.
//
// Рядом кладём обычные записи, выбранные из базы, — тот путь никуда не делся, и
// важно видеть, что оба вида строк уживаются в одном приёме.
const rows = await page.evaluate(async () => {
  const names = await indexedDB.databases();
  const target = names.find((n) => /^hjkl-ft-/.test(n.name || ''));
  const db = await new Promise((res, rej) => {
    const r = indexedDB.open(target.name);
    r.onsuccess = () => res(r.result);
    r.onerror = () => rej(r.error);
  });
  return await new Promise((res) => {
    const r = db.transaction('diary').objectStore('diary').getAll();
    r.onsuccess = () => res(r.result);
  });
});
const pendings = rows.filter((e) => e.kind === 'pending').sort((a, b) =>
  String(a.created_at).localeCompare(String(b.created_at)));
if (pendings.length < 2) throw new Error(`ожидались две нераспознанные записи, а их ${pendings.length}`);
const first = pendings[0];

const nowIso = new Date().toISOString();
await page.evaluate(({ row, now }) => {
  const food = (id, name, kcal, p, f, c) => ({
    id, name, kcal, protein: p, fat: f, carbs: c,
    nutrients: {}, keywords: [], package_weight: null,
    is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
    created_at: now, updated_at: now,
  });
  const direct = (id, food_id, grams) => ({
    id, food_id, date: row.date, time: null, grams, waste_grams: 0,
    meal_label: row.meal_label, deleted: false, kind: 'direct',
    description: null, images: [], items: [], label: null, recognized_at: null,
    created_at: now, updated_at: now,
  });
  localStorage.setItem('ft_test_seed', JSON.stringify({
    foods: [
      food('f-liver', 'Печень говяжья со сливками', 165, 17.4, 9.1, 3.2),
      food('f-cauli', 'Капуста цветная варёная', 29, 2.3, 0.3, 4.1),
      food('f-smetana', 'Сметана 20%', 206, 2.8, 20.0, 3.2),
      food('f-plum', 'Слива', 49, 0.8, 0.3, 9.6),
      food('f-apple', 'Яблоко', 47, 0.4, 0.4, 9.8),
      food('f-cabbage', 'Капуста белокочанная', 28, 1.8, 0.1, 4.7),
    ],
    diary: [
      {
        ...row,
        kind: 'aggregate',
        label: 'Печень со сливками и цветная капуста',
        items: [
          { food_id: 'f-liver', grams: 300 },
          { food_id: 'f-cauli', grams: 400 },
          { food_id: 'f-smetana', grams: 30 },
        ],
        recognized_at: now,
        updated_at: now,
      },
      direct('d-plum', 'f-plum', 120),
      direct('d-apple', 'f-apple', 180),
      direct('d-cabbage', 'f-cabbage', 150),
    ],
  }));
}, { row: first, now: nowIso });
await page.reload({ waitUntil: 'load' });
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 25000 });
await page.waitForTimeout(2500);
await page.screenshot({ path: OUT + 'other-food-7-recognised.png', fullPage: true });

// ── Правка: сперва РАЗОБРАННОЙ записи, потом НЕРАСПОЗНАННОЙ ─────────────────
//
// Строки идут в том же порядке, что на снимке выше: первая — разобранная,
// вторая — нераспознанная.
async function openEdit(nth, out) {
  await page.getByTestId('lazy-row-menu').nth(nth).click();
  await page.getByTestId('lazy-row-edit').first().waitFor({ state: 'visible', timeout: 10000 });
  await page.getByTestId('lazy-row-edit').first().click();
  await page.getByTestId('lazy-food-edit').waitFor({ state: 'visible', timeout: 15000 });
  await page.waitForTimeout(1500);
  await page.screenshot({ path: OUT + out, fullPage: true });
  await page.getByTestId('lazy-edit-cancel').click();
  await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 15000 });
  await page.waitForTimeout(800);
}
await openEdit(0, 'other-food-8-edit-recognised.png');
await openEdit(1, 'other-food-9-edit-pending.png');

// Та же правка разобранной, но с открытыми зонами: нажимаем «Изменить» на снимках
// и на описании. Так видно, куда ведут кнопки на запертых зонах.
await page.getByTestId('lazy-row-menu').nth(0).click();
await page.getByTestId('lazy-row-edit').first().click();
await page.getByTestId('lazy-food-edit').waitFor({ state: 'visible', timeout: 15000 });
await page.getByTestId('other-food-photo-edit').click();
await page.waitForTimeout(600);
// Правка верха: позиции спрятаны, чтобы человек смотрел на то, что меняет.
if (await page.getByTestId('lazy-edit-item').count() !== 0) {
  throw new Error('позиции обязаны быть скрыты на время правки верха');
}
// Меняем состав: открываем кадр и удаляем его. В базу это НЕ должно попасть,
// пока не нажали «Сохранить».
await page.getByTestId('other-food-thumb').first().click();
await page.getByTestId('photo-crop').waitFor({ state: 'visible', timeout: 15000 });
await page.getByTestId('photo-crop-delete').click();
await page.getByTestId('lazy-food-edit').waitFor({ state: 'visible', timeout: 10000 });
await page.getByTestId('other-food-description').fill('Правка, которую мы сейчас отменим');
await page.waitForTimeout(600);
console.log('кадров после удаления:', await page.getByTestId('other-food-thumb').count());
await page.screenshot({ path: OUT + 'other-food-11-edit-unlocked.png', fullPage: true });

// «Отмена» обязана вернуть и кадры, и описание, и позиции.
await page.getByTestId('lazy-edit-cancel-top').click();
await page.waitForTimeout(800);
const backItems = await page.getByTestId('lazy-edit-item').count();
const backThumbs = await page.getByTestId('other-food-thumb-locked').count();
const backText = await page.getByTestId('other-food-description-locked').innerText();
console.log(`после отмены: позиций ${backItems}, кадров ${backThumbs}`);
if (backItems !== 3) throw new Error(`позиции не вернулись: ${backItems}`);
if (backThumbs !== 2) throw new Error(`кадры не вернулись: ${backThumbs}`);
if (!backText.includes('по тарелке')) throw new Error(`описание не вернулось: ${backText}`);
await page.screenshot({ path: OUT + 'other-food-12-edit-cancelled.png', fullPage: true });

await page.getByTestId('lazy-edit-cancel').click();
await page.getByTestId('meal-add').first().waitFor({ state: 'visible', timeout: 15000 });
await page.waitForTimeout(800);

// Меню строки — оно теперь одно и то же у ленивой и у обычной; снимем открытым.
await page.getByTestId('lazy-row-menu').first().click();
await page.getByTestId('lazy-row-move').waitFor({ state: 'visible', timeout: 10000 });
await page.waitForTimeout(400);
await page.screenshot({ path: OUT + 'other-food-10-menu.png' });
console.log('готово');

await browser.close();
