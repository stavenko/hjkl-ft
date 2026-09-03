import { test, expect } from '@playwright/test';
import { registerAccount, seedFeature } from './helpers';

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

/** Register, land on the diary, open the "add food" picker. */
async function openFoodPicker(page: import('@playwright/test').Page) {
  await page.goto('/');
  await registerAccount(page);

  const navDiary = page.getByTestId('nav-diary');
  await navDiary.waitFor({ state: 'visible', timeout: 15_000 });
  await navDiary.click();

  const add = page.getByTestId('diary-btn-add');
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
    await page.goto('/');
    await registerAccount(page);

    // Флаг ставит куратор директивой; в испытании его кладут прямо в базу — засев
    // применяется ПОСЛЕ миграций, иначе они бы его стёрли (services/test_seed.rs).
    await seedFeature(page, 'lazy_food');

    const navDiary = page.getByTestId('nav-diary');
    await navDiary.waitFor({ state: 'visible', timeout: 15_000 });
    await navDiary.click();
    const add = page.getByTestId('diary-btn-add');
    await add.waitFor({ state: 'visible', timeout: 10_000 });
    await add.click();

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
