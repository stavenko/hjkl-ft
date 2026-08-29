// Дневник открыт неделю назад.
//
// Раньше прошлый день был закрыт: правки и удаления запрещены, панели приёмов
// пищи без «+», а на строке вместо меню — стрелка «повторить сегодня». Ради
// этого запрета сутки и переворачивались в 04:00, а не в полночь: засидевшийся
// до часу ночи должен был успеть дописать съеденное.
//
// Проверяется, что запрета больше нет и что взамен ничего не сломалось:
//
//   1. В дне недельной давности есть панели с «+», и еда в него ложится ИМЕННО
//      туда, а не в сегодня.
//   2. Кнопка вызова меню на строке — одна и та же во все дни, а «Повторить
//      сегодня» стало пунктом этого меню.
//   3. Правка и удаление в открытом дне работают.
//   4. Итоги прошлого дня ПЕРЕСУЖИВАЮТСЯ: замороженные значения индикаторов
//      сбрасываются, иначе шкалы показывали бы вчерашнее число.
//   5. День старше недели остаётся прожитым: меню там только «повторить».
//
// Запуск: node scripts/check-diary-week.mjs

import { serveWithProxy, launchBrowser } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};
const section = (n) => console.log(`\n── ${n} ──`);
const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString().slice(0, 10); };

const readDay = async ({ date, today }) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], 'readonly').objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const [diary, cal] = [await all('diary'), await all('ind_calories')];
  db.close();
  const on = (d) => diary.filter((e) => e.date === d && !e.deleted);
  return {
    rows: on(date).map((e) => ({ id: e.id, grams: e.grams, time: e.time, meal: e.meal_label })),
    today: on(today).length,
    frozen: cal.find((r) => r.date === date)?.value ?? null,
  };
};

const server = await serveWithProxy({
  root: DIST,
  configFor: (o) => [
    `api_base_url = ""`, `auth_base_url = "${o}/api/auth"`, `push_base_url = "${o}/api/push"`,
    `ai_base_url = "${o}/api/ai"`, `payment_base_url = "${o}/api/payment"`,
    `ocr_queue_base_url = "${o}/api/ocr"`, `sync_base_url = "${o}/api/sync"`,
    `bug_report_base_url = "${o}/api/bug"`, `support_base_url = "${o}/api/support"`,
    `landing_url = "https://renorma.app"`, `app_origin = "${o}"`,
  ].join('\n'),
});
console.log(`приложение на ${server.url}`);
const b = await launchBrowser();
const { userId: uid } = await createPaidUser('diary-week');
const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
const page = await ctx.newPage();
const panics = [];
const ownNavigation = (m) => /compilation aborted.*(aborted|cancel)/i.test(m);
page.on('pageerror', (e) => { if (!ownNavigation(e.message)) panics.push(e.message); });
page.on('console', (m) => { if (/panicked at/.test(m.text())) panics.push(m.text().slice(0, 200)); });

await page.goto(server.url, { waitUntil: 'domcontentloaded' });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem('user_id', uid);
  localStorage.setItem('auth_token', token);
  localStorage.setItem('pwa_dismissed', 'true');
}, { uid, token: mintToken(uid) });
await page.goto(server.url, { waitUntil: 'domcontentloaded' });
await page.waitForTimeout(6000);
// Три дня с едой: позавчера (внутри окна), шесть дней назад (край окна) и
// десять дней назад (за окном).
await page.evaluate(async (arg) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const now = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(['app_flags', 'profile', 'foods', 'diary'], 'readwrite');
    for (const [key, value] of [
      ['welcome_shown', 'true'], ['push_onboarding_dismissed', 'true'],
      ['ft_subscription', JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
        active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' })],
    ]) tx.objectStore('app_flags').put({ key, value });
    tx.objectStore('profile').put({ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1985,
      goal: 'lose', created_at: now, updated_at: now });
    tx.objectStore('foods').put({ id: 'f1', name: 'Овсяная каша', kcal: 100, protein: 3, fat: 2, carbs: 18,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
      created_at: now, updated_at: now });
    for (const [id, date] of [['e-in', arg.inWindow], ['e-edge', arg.edge], ['e-old', arg.old]]) {
      tx.objectStore('diary').put({ id, food_id: 'f1', date, time: '12:00', grams: 200,
        waste_grams: 0, meal_label: 'lunch', deleted: false, created_at: now, updated_at: now });
    }
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { inWindow: ymd(2), edge: ymd(6), old: ymd(10) });

/// Открыть дневник на нужной дате: кнопкой «назад» столько раз, сколько нужно.
const openDay = async (back) => {
  await page.goto(`${server.url}/diary`, { waitUntil: 'domcontentloaded' });
  await page.waitForSelector('[data-testid="diary-btn-prev-date"]', { timeout: 25000 });
  for (let i = 0; i < back; i++) {
    await page.click('[data-testid="diary-btn-prev-date"]');
    await page.waitForTimeout(250);
  }
  await page.waitForTimeout(1500);
};

// ── 1. День внутри окна выглядит как сегодняшний ────────────────────────────
section('1. день недельной давности открыт для записи');
await openDay(2);
const panels = await page.evaluate(() => ({
  add: document.querySelectorAll('[data-testid="meal-add"]').length,
  menu: document.querySelectorAll('[data-testid="diary-row-menu"]').length,
}));
check('на прошлом дне есть кнопки «+»', panels.add > 0, `${panels.add} шт.`);
check('кнопка меню на строке — та же, что сегодня', panels.menu > 0, `${panels.menu} шт.`);

// ── 2. Меню строки: те же пункты плюс «повторить сегодня» ───────────────────
section('2. меню строки одно на все дни');
await page.click('[data-testid="diary-row-menu"]');
await page.waitForTimeout(400);
const items = await page.evaluate(() => ({
  edit: !!document.querySelector('[data-testid="diary-menu-edit"]'),
  repeat: !!document.querySelector('[data-testid="diary-menu-repeat"]'),
  texts: [...document.querySelectorAll('button')].map((b) => b.innerText.trim()).filter(Boolean),
}));
check('в меню есть «Изменить»', items.edit);
check('в меню есть «Повторить сегодня»', items.repeat);
check('в меню есть «Дублировать» и «Удалить»',
  items.texts.some((t) => /Дублировать/i.test(t)) && items.texts.some((t) => /Удалить/i.test(t)),
  items.texts.filter((t) => /Дубл|Удал|Повтор|Измен/i.test(t)).join(' | '));

// ── 3. «Повторить сегодня» кладёт в СЕГОДНЯ, а не в открытый день ───────────
section('3. повтор кладёт в сегодня, запись — в открытый день');
await page.click('[data-testid="diary-menu-repeat"]');
await page.waitForTimeout(2500);
const afterRepeat = await page.evaluate(readDay, { date: ymd(2), today: ymd(0) });
check('в сегодняшнем дне появилась запись', afterRepeat.today === 1, `${afterRepeat.today}`);
check('в прошлом дне записей не прибавилось', afterRepeat.rows.length === 1,
  `${afterRepeat.rows.length}`);

// ── 4. Запись через «+» попадает в открытый день ────────────────────────────
section('4. «+» кладёт еду в открытый день');
await openDay(2);
await page.click('[data-testid="meal-add"]');
await page.waitForTimeout(1500);
const url = page.url();
check('экран добавления знает день', /[?&]date=/.test(url), url.split('?')[1] ?? url);
// Кладём в прошлый день ЧЕРЕЗ ПРИЛОЖЕНИЕ, а не в базу: проверяется весь путь.
const picked = await page.waitForSelector('[data-testid="diary-add-btn-pick-food"]', { timeout: 20000 })
  .then((h) => h).catch(() => null);
check('еда доступна для выбора', !!picked);
if (picked) {
  await picked.click();
  await page.waitForSelector('[data-testid="diary-add-weight-btn-confirm"]', { timeout: 15000 });
  await page.click('[data-testid="diary-add-weight-btn-confirm"]');
  await page.waitForTimeout(3500);
}
const added = await page.evaluate(readDay, { date: ymd(2), today: ymd(0) });
check('запись легла в ОТКРЫТЫЙ день, а не в сегодня', added.rows.length === 2,
  `в дне ${added.rows.length}, сегодня ${added.today}`);
// У посеянной записи время есть (12:00) — она из прошлого. У добавленной сейчас
// его быть не должно: в какой час человек ел позавчера, он не помнит.
check('у новой записи в прошлый день нет времени',
  added.rows.some((r) => !r.time), JSON.stringify(added.rows.map((r) => r.time)));
check('замороженный итог дня сброшен — день пересудят заново', added.frozen === null,
  `${added.frozen}`);

// ── 5. Правка и удаление в открытом дне ─────────────────────────────────────
section('5. правка и удаление работают в открытом дне');
await openDay(2);
await page.click('[data-testid="diary-row-menu"]');
await page.waitForTimeout(400);
const before = await page.evaluate(readDay, { date: ymd(2), today: ymd(0) });
const del = await page.evaluate(() => {
  const btns = [...document.querySelectorAll('button')].filter((b) => /^Удалить$/i.test(b.innerText.trim()));
  if (!btns.length) return false;
  btns[0].click();
  return true;
});
check('пункт «Удалить» нашёлся', del);
await page.waitForTimeout(2500);
const afterDel = await page.evaluate(readDay, { date: ymd(2), today: ymd(0) });
check('запись из прошлого дня удалилась', afterDel.rows.length === before.rows.length - 1,
  `${before.rows.length} → ${afterDel.rows.length}`);

// ── 6. День старше недели остаётся прожитым ─────────────────────────────────
section('6. день старше недели закрыт — только «повторить сегодня»');
await openDay(10);
await page.waitForSelector('[data-testid="diary-row-menu"]', { timeout: 20000 });
await page.click('[data-testid="diary-row-menu"]');
await page.waitForTimeout(400);
const oldMenu = await page.evaluate(() => ({
  repeat: !!document.querySelector('[data-testid="diary-menu-repeat"]'),
  edit: !!document.querySelector('[data-testid="diary-menu-edit"]'),
  add: document.querySelectorAll('[data-testid="meal-add"]:not([disabled])').length,
}));
check('в закрытом дне есть «Повторить сегодня»', oldMenu.repeat);
check('в закрытом дне нет правки', !oldMenu.edit);
check('в закрытом дне нельзя добавлять', oldMenu.add === 0, `${oldMenu.add} доступных «+»`);
check('запуск без паник', panics.length === 0, panics[0] ?? 'паник нет');

await ctx.close();
await b.close();
server.close();
console.log(fail ? `\n=== FAILURES: ${fail} ===` : '\n=== ALL OK ===');
process.exit(fail ? 1 : 0);
