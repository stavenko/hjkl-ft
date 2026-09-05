import { test, expect } from '@playwright/test';
import type { Page, Route } from '@playwright/test';
import { signInSeeded } from './helpers';

/**
 * ВЕСЬ круг ленивой записи, от заведения еды до неудачи, — одним прогоном на
 * каждую ветку.
 *
 * Остальные испытания этой возможности берут разобранную запись готовой: кладут в
 * базу агрегатор с позициями и смотрят на интерфейс. Середина при этом не
 * проверяется ничем — а середина здесь и есть суть: описание уходит в модель,
 * ответ превращается в позиции, каждая позиция ищется в базе человека, ненайденная
 * ЗАВОДИТСЯ новой едой, и только потом запись становится разобранной.
 *
 * МОДЕЛЬ ПОДМЕНЕНА, и это не срезание угла. Живая модель на каждом прогоне стоит
 * денег, отвечает по-разному и падает не по нашей вине — испытание на ней меряло бы
 * её настроение, а не наш код. Подменяется РОВНО транспорт: ответы отдаются тем же
 * потоком SSE, каким отвечает ai-worker, и весь разбор ниже — настоящий.
 *
 * Что проверяет каждая ветка:
 *
 *   удача   — нераспознанная запись становится разобранной; знакомая еда находится
 *             в базе, а незнакомая заводится новой; в дневнике видны обе позиции;
 *   неудача — сбой модели не хоронит запись и не показывается человеку сырым: в
 *             строке стоит фраза с кодом, а не «LLM output error».
 */

/** Один ответ модели потоком SSE — так же, как его отдаёт ai-worker. */
function sse(payload: unknown): string {
  const content = JSON.stringify(payload);
  const chunk = {
    id: 'stub',
    object: 'chat.completion.chunk',
    created: 0,
    model: 'stub',
    choices: [{ index: 0, finish_reason: null, delta: { content } }],
  };
  return `data: ${JSON.stringify(chunk)}\n\ndata: [DONE]\n\n`;
}

/**
 * Ответ подбирается ПО ТЕКСТУ ЗАПРОСА, а не по порядку вызовов: порядок держится на
 * том, в каком месте кода стоит запрос, и первая же перестановка сделала бы
 * испытание врущим, а не падающим.
 */
async function stubModel(page: Page, merged: unknown | null): Promise<() => number> {
  let calls = 0;
  await page.route('**/ai-worker-dev.vg-stavenko.workers.dev/**', async (route: Route) => {
    const url = route.request().url();
    if (!url.endsWith('/chat/completions')) {
      // `/health` и прочее к модели отношения не имеют — пропускаем как есть.
      return route.continue();
    }
    const body = route.request().postData() ?? '';

    // Сбор списка из описания — единственный запрос, который умеет провалиться по
    // сценарию. 401, а не 500: ответ, который НЕ изменится от повтора, — именно тот
    // случай, ради которого правило «не долбиться» и написано.
    if (body.includes('Собери из всего этого')) {
      // Считаем ТОЛЬКО сбор списка. Рядом идёт своя фоновая работа — подбор
      // нутриентов у засеянной еды, разметка словами, — и она к правилу «не
      // долбиться в 401» отношения не имеет; счёт всего подряд мерил бы её.
      calls += 1;
      if (merged === null) {
        return route.fulfill({
          status: 401,
          contentType: 'application/json',
          body: '{"error":"Unauthorized"}',
        });
      }
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body: sse(merged) });
    }
    // Разметка словами для поиска — её просят при ЗАВЕДЕНИИ новой еды.
    if (body.includes('По каким словам')) {
      return route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body: sse({ keywords: ['мёд', 'сладкое'] }),
      });
    }
    // Всё прочее (выбор из кандидатов) — «не знаю»; код тогда заводит новую копию.
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body: sse({ id: null }) });
  });
  return () => calls;
}

/** Нераспознанная запись из одного описания, без снимков, и включённый флаг. */
async function seedPending(page: Page): Promise<void> {
  const now = new Date().toISOString();
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  await signInSeeded(page, {
    app_flags: [{ key: 'feature.lazy_food', value: 'true', updated_at: now }],
    // Овсянка в базе УЖЕ ЕСТЬ — её надо найти, а не завести второй раз.
    foods: [
      {
        id: 'f-oat', name: 'Овсянка на воде', kcal: 88, protein: 3.0, fat: 1.7, carbs: 15.0,
        nutrients: {}, keywords: ['овсянка', 'каша'], package_weight: null,
        is_recipe: false, recipe_id: null, archived: false, is_restaurant: false,
        created_at: now, updated_at: now,
      },
    ],
    diary: [
      {
        id: 'e2e-cycle-1', food_id: '', date: today, time: null, grams: 0, waste_grams: 0,
        meal_label: 'Завтрак', deleted: false, kind: 'pending',
        description: 'Овсянка на воде 200 г и ложка мёда',
        images: [], items: [], label: null, recognized_at: null,
        recognition_error: null, recognition_tries: 0, retry_after_wait: false,
        created_at: now, updated_at: now,
      },
    ],
  });
  await page.getByTestId('nav-diary').click();
}

/** Что лежит в хранилище — читаем базу, а не экран: заведение еды экрану не видно. */
async function storeRows(page: Page, store: string): Promise<Record<string, unknown>[]> {
  return page.evaluate(async (name) => {
    // База принадлежит человеку и названа его идентификатором — имя заранее
    // неизвестно, поэтому ищем её среди открытых.
    const names = await indexedDB.databases();
    const target = names.find((n) => /^hjkl-ft-/.test(n.name || ''));
    if (!target?.name) throw new Error('база приложения не найдена');
    const db: IDBDatabase = await new Promise((ok, no) => {
      const r = indexedDB.open(target.name as string);
      r.onsuccess = () => ok(r.result);
      r.onerror = () => no(r.error);
    });
    return new Promise((ok, no) => {
      const r = db.transaction(name).objectStore(name).getAll();
      r.onsuccess = () => ok(r.result);
      r.onerror = () => no(r.error);
    });
  }, store);
}

/**
 * КЭШ ШАГОВ: упавшая на середине цепочка дочитывается с места обрыва.
 *
 * Разбор одной записи — это N+1+M обращений к модели: снимок читается отдельно от
 * снимка, потом прочитанное сводится в список, потом каждая позиция ищется в базе.
 * Пока результаты шагов жили в памяти одного прохода, сбой на последнем шаге
 * выбрасывал все предыдущие, и следующая попытка платила за них заново — у записи
 * с тремя снимками это три обращения к vision-модели в сутки впустую, пока сбой не
 * починят.
 *
 * Проверяется это ровно так, как и стоит проверять экономию: СЧЁТОМ ЗАПРОСОВ.
 * Снимок читается успешно, сведение падает; страница перезагружается, разбор идёт
 * снова. Прочтение кадра обязано остаться ОДНИМ на оба захода — оно лежит в базе,
 * а не в памяти вкладки, — а сведение обязано случиться повторно, иначе испытание
 * ничего не доказывает: одного захода не было бы вовсе.
 */
test('прочитанный кадр не перечитывается после сбоя на следующем шаге', async ({ page }) => {
  let reads = 0;
  let merges = 0;
  await page.route('**/ai-worker-dev.vg-stavenko.workers.dev/**', async (route: Route) => {
    if (!route.request().url().endsWith('/chat/completions')) return route.continue();
    const body = route.request().postData() ?? '';

    if (body.includes('Разбери ЭТОТ ОДИН снимок')) {
      reads += 1;
      return route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body: sse({
          what_is_on_the_photo: 'этикетка творога',
          photo_kind: 'label',
          foods_on_the_photo: [],
          product_name_printed: 'Творог 5%',
          kcal_per_100g_printed: 121,
        }),
      });
    }
    if (body.includes('Собери из всего этого')) {
      merges += 1;
      // 500, а не 401: «сервер прилёг» очередь берёт снова, и второй заход
      // случится — без него сравнивать было бы нечего.
      return route.fulfill({ status: 500, contentType: 'text/plain', body: 'сервер прилёг' });
    }
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body: sse({ id: null }) });
  });

  const now = new Date().toISOString();
  const d = new Date();
  const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  // Картинка кладётся в свой store и адресуется хэшем — записи достаётся строка,
  // а не мегабайт base64 (`services::images`). Хэш здесь любой: приложение берёт
  // его как ключ, а не пересчитывает.
  const hash = 'e2e0000000000000000000000000000000000000000000000000000000cache1';
  await signInSeeded(page, {
    app_flags: [{ key: 'feature.lazy_food', value: 'true', updated_at: now }],
    images: [{
      hash,
      data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
      created_at: now,
    }],
    diary: [{
      id: 'e2e-cache-1', food_id: '', date: today, time: null, grams: 0, waste_grams: 0,
      meal_label: 'Завтрак', deleted: false, kind: 'pending',
      description: '', images: [hash], items: [], label: null, recognized_at: null,
      recognition_error: null, recognition_tries: 0, retry_after_wait: false,
      created_at: now, updated_at: now,
    }],
  });

  /** Дождаться, пока запросы перестанут идти: очередь проходит по записи не один
   *  раз за запуск, и мерить надо устоявшееся число, а не первое попавшееся. */
  const settle = async () => {
    let last = -1;
    for (let i = 0; i < 30 && last !== reads + merges; i += 1) {
      last = reads + merges;
      await page.waitForTimeout(1_000);
    }
  };

  await page.getByTestId('nav-diary').click();
  await expect(page.getByTestId('diary-row-pending')).toBeVisible({ timeout: 15_000 });
  await expect.poll(() => merges, { timeout: 20_000 }).toBeGreaterThan(0);
  await settle();
  const readsAfterFirst = reads;
  const mergesAfterFirst = merges;
  // Один снимок — одно прочтение, сколько бы раз очередь ни бралась за запись.
  expect(readsAfterFirst, 'кадр прочитан не один раз за первый запуск').toBe(1);

  // Второй заход: то же устройство, та же база, новый запуск приложения.
  await page.reload();
  await page.getByTestId('nav-diary').waitFor({ state: 'visible', timeout: 20_000 });
  await expect.poll(() => merges, { timeout: 25_000 }).toBeGreaterThan(mergesAfterFirst);
  await settle();

  // Сведение спрашивалось снова — значит второй заход действительно был, и
  // сравнивать есть что.
  expect(merges).toBeGreaterThan(mergesAfterFirst);
  // А кадр — нет: его прочтение лежит в базе, а не в памяти вкладки.
  expect(reads, `кадр перечитан ${reads} раз(а) вместо ${readsAfterFirst}`).toBe(readsAfterFirst);
});

test.describe('ленивая запись: круг целиком', () => {
  test('описание разбирается, знакомая еда находится, незнакомая заводится', async ({ page }) => {
    await stubModel(page, {
      how_frames_group: 'Снимков нет, список собран из описания.',
      items: [
        {
          name: 'Овсянка на воде', from_frames: null, where_grams_came_from: 'user_text',
          grams: 200, kcal_per_100g: null, protein_per_100g: null,
          fat_per_100g: null, carbs_per_100g: null,
        },
        {
          name: 'Мёд', from_frames: null, where_grams_came_from: 'user_text',
          grams: 20, kcal_per_100g: 329, protein_per_100g: 0.8,
          fat_per_100g: 0, carbs_per_100g: 81.5,
        },
      ],
    });
    await seedPending(page);

    // Запись перестаёт быть нераспознанной — это и есть исход круга.
    await expect(page.getByTestId('diary-row-aggregate')).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId('diary-row-pending')).toHaveCount(0);

    const foods = await storeRows(page, 'foods');
    const names = foods.map((f) => f.name);
    // Мёда в базе не было — он заведён разбором.
    expect(names, `в базе оказалось: ${names.join(', ')}`).toContain('Мёд');
    // А овсянка НЕ задвоилась: одноимённую надо было найти, а не завести снова.
    expect(names.filter((n) => n === 'Овсянка на воде')).toHaveLength(1);

    // Позиции легли в запись с теми граммами, что назвал человек.
    const diary = await storeRows(page, 'diary');
    const entry = diary.find((e) => e.id === 'e2e-cycle-1') as Record<string, unknown>;
    expect(entry.kind).toBe('aggregate');
    const items = entry.items as { food_id: string; grams: number }[];
    expect(items.map((i) => i.grams).sort((a, b) => a - b)).toEqual([20, 200]);
    // Найденная овсянка — та самая строка базы, а не новая с тем же именем.
    expect(items.map((i) => i.food_id)).toContain('f-oat');
  });

  test('сбой модели не хоронит запись и не показывается человеку сырым', async ({ page }) => {
    const calls = await stubModel(page, null);
    await seedPending(page);

    const row = page.getByTestId('diary-row-pending');
    await expect(row).toBeVisible({ timeout: 15_000 });

    const note = page.getByTestId('lazy-row-error');
    await expect(note).toBeVisible({ timeout: 30_000 });
    const text = (await note.innerText()).trim();

    // Человеку — фраза с кодом, по которому мы найдём причину.
    expect(text).toContain('Код ошибки');
    // И НИ СЛОВА из внутренностей: «LLM output error: ModelExecution("HTTP 503…")»
    // однажды уже доехало до дневника живому человеку.
    expect(text).not.toMatch(/HTTP|LLM|ModelExecution|503/);

    // Запись остаётся на месте: сбой на нашей стороне не повод её терять.
    await expect(row).toBeVisible();

    // И запрос был ОДИН. 401 не изменится от настойчивости, а три захода — это три
    // оплаченных запроса на каждый разбор у каждого человека.
    expect(calls(), 'на 401 ушло больше одного захода за разбором').toBe(1);
  });
});
