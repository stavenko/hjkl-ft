import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import { signInSeeded } from './helpers';

/**
 * The new food-logging path is gated by a CURATOR-set feature flag
 * (`features::LAZY_FOOD`). There is deliberately no user-facing switch: the
 * person should not have to know there are two ways to log food, and we must be
 * able to move them back if the new one disappoints.
 *
 * These two tests pin the gate itself, from opposite sides. They identify the
 * interface by the control that is actually rendered — not by a route, a title
 * or a screenshot — because the control is what the person taps.
 *
 *   flag off  →  «Новая еда» (`diary-add-btn-new-food`) opens the product form
 *   flag on   →  «Другая еда» (`diary-add-btn-other-food`) opens the photos +
 *                description screen (`other-food-panel`)
 *
 * Both assert the OTHER control is absent, not merely that theirs is present:
 * two buttons rendered at once would pass a one-sided check while being an
 * obvious bug.
 */

/** Сеанс с нужными флагами, дневник, открытый выбор «что добавить». */
async function openFoodPicker(
  page: import('@playwright/test').Page,
  features: string[] = [],
) {
  const now = new Date().toISOString();
  await signInSeeded(page, {
    app_flags: features.map((f) => ({ key: `feature.${f}`, value: 'true', updated_at: now })),
  });

  const navDiary = page.getByTestId('nav-diary');
  await navDiary.waitFor({ state: 'visible', timeout: 15_000 });
  await navDiary.click();

  // Добавление начинается с трапезы: «+» у Завтрака ведёт на /diary/add?meal=…
  const add = page.getByTestId('meal-add').first();
  await add.waitFor({ state: 'visible', timeout: 10_000 });
  await add.click();
}

test.describe('запись еды: старый и новый интерфейс за флагом куратора', () => {
  test('без флага открывается прежний путь — «Новая еда»', async ({ page }) => {
    await openFoodPicker(page);

    const oldButton = page.getByTestId('diary-add-btn-new-food');
    await expect(oldButton).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId('diary-add-btn-other-food')).toHaveCount(0);

    // Не только надпись: кнопка обязана вести в форму продукта. Иначе тест
    // прошёл бы и на кнопке, которая ничего не открывает.
    await oldButton.click();
    await expect(page.getByTestId('food-editor')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId('other-food-panel')).toHaveCount(0);
  });

  test('с флагом куратора открывается новый путь — «Другая еда»', async ({ page }) => {
    // Флаг ставит куратор директивой; в испытании он кладётся прямо в базу —
    // засев применяется ПОСЛЕ миграций, иначе они бы его стёрли
    // (frontend/src/services/test_seed.rs).
    await openFoodPicker(page, ['lazy_food']);

    const newButton = page.getByTestId('diary-add-btn-other-food');
    await expect(newButton).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId('diary-add-btn-new-food')).toHaveCount(0);

    await newButton.click();
    const panel = page.getByTestId('other-food-panel');
    await expect(panel).toBeVisible({ timeout: 10_000 });

    // Экран один, а не три вкладки: снимки и описание лежат рядом.
    await expect(page.getByTestId('other-food-add-photo')).toBeVisible();
    await expect(page.getByTestId('other-food-description')).toBeVisible();
    await expect(page.getByTestId('other-food-add')).toBeVisible();
    // Форма продукта отсюда не открывается.
    await expect(page.getByTestId('food-editor')).toHaveCount(0);
  });
});

/**
 * Вторая половина затвора — ФОН.
 *
 * Кнопку спрятать мало. Запись, сделанную под флагом, может привезти синхронизация
 * с другого устройства, а флаг куратор к тому времени уже снял — и тогда очередь
 * разбора не должна тронуться с места сама. Проверяется это по СЕТИ, а не по
 * экрану: единственное, что новый путь делает наружу, — запросы в ai-worker, и
 * их отсутствие и есть доказательство, что фон молчит.
 *
 * Ответ подменяется на 500, чтобы испытание не ходило к настоящей модели: нам важен
 * сам факт запроса, а не его исход.
 *
 * Две стороны, и вторая обязательна. Проверка «запросов нет» прошла бы и на
 * сломанном засеве, где разбирать попросту нечего, — поэтому та же самая запись с
 * включённым флагом обязана запрос породить.
 */
async function seedPendingEntry(page: Page, features: string[]): Promise<string[]> {
  const calls: string[] = [];
  await page.route('**/ai-worker-dev.vg-stavenko.workers.dev/**', (route) => {
    const url = route.request().url();
    // `/health` — общая проверка связи, её приложение делает всегда и к разбору
    // еды она отношения не имеет. Считаем только запросы к модели.
    if (!url.endsWith('/health')) {
      calls.push(url);
    }
    return route.fulfill({ status: 500, contentType: 'text/plain', body: 'нарочно' });
  });

  const now = new Date().toISOString();
  // Дата — МЕСТНАЯ, как её считает дневник. `toISOString` дал бы UTC, и под вечер
  // в плюсовом поясе запись легла бы во вчерашний день, где её никто не покажет.
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  await signInSeeded(page, {
    app_flags: features.map((f) => ({ key: `feature.${f}`, value: 'true', updated_at: now })),
    diary: [
      {
        id: 'e2e-pending-1',
        // Пусто, а не null: у `Pending` еды ещё нет, но поле в схеме — строка,
        // и на null разбор строки падает молча, вместе со всей записью.
        food_id: '',
        date: today,
        time: null,
        grams: 0,
        waste_grams: 0,
        meal_label: 'Завтрак',
        deleted: false,
        kind: 'pending',
        // Описание без снимков — законный повод для разбора: слов модели хватает.
        description: 'Овсянка на воде, столовая ложка мёда',
        images: [],
        items: [],
        label: null,
        recognized_at: null,
        recognition_error: null,
        recognition_tries: 0,
        retry_after_wait: false,
        created_at: now,
        updated_at: now,
      },
    ],
  });

  await page.getByTestId('nav-diary').click();
  await expect(page.getByTestId('diary-row-pending')).toBeVisible({ timeout: 15_000 });
  // Разбор уходит в фон, а не в тот же тик отрисовки: даём очереди время выстрелить.
  await page.waitForTimeout(4_000);
  return calls;
}

test.describe('очередь разбора за тем же флагом', () => {
  test('без флага готовая очередь остаётся нетронутой', async ({ page }) => {
    const calls = await seedPendingEntry(page, []);
    expect(calls, `фон постучался в ai-worker при снятом флаге: ${calls.join(', ')}`).toHaveLength(0);

    // И при этом запись НЕ прячется. Снятый флаг закрывает новый путь, а не
    // стирает у человека из дневника то, что он уже записал.
    await expect(page.getByTestId('diary-row-pending')).toBeVisible();
  });

  test('с флагом та же очередь уходит в разбор', async ({ page }) => {
    const calls = await seedPendingEntry(page, ['lazy_food']);
    expect(calls.length, 'засев не породил разбора — предыдущая проверка ничего не доказывает')
      .toBeGreaterThan(0);
  });
});
