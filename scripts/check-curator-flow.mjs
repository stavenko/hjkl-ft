// Кураторский путь целиком, в ЗАПУЩЕННЫХ приложениях — и худеющего, и куратора.
//
// Проверяет шесть утверждений, и в таком порядке:
//
//   1. Без куратора приложение ведёт планки САМО — все, что оно ведёт: калории,
//      шаги и следующий за ними белок.
//   2. Привязка происходит ПО ССЫЛКЕ: человек открывает приглашение в своём
//      приложении и соглашается.
//   3. С куратором приложение планки НЕ ТРОГАЕТ — ни одну, пока не тронет он.
//   4. Передача работает в обе стороны: директива планки и запрос данных
//      доезжают до человека, отчёт доезжает до куратора — и везёт ИМЕННО ТО,
//      что человек у себя записал, а кураторский экран показывает те же числа.
//   5. Отвязка возвращает НАШИ правила: девять постоянных планок забываются,
//      три подвижные остаются кураторскими до первого пересчёта, и человек
//      получает письмо с перечнем того, что теперь соблюдать.
//   6. Подсчёт ВОЗОБНОВЛЯЕТСЯ: недельный цикл, молчавший при кураторе, снова
//      считает — и забирает кураторское число себе как отправную точку.
//
// Первый и третий пункты — один и тот же посев: планку поставили десять дней
// назад, человек всё это время ест, ходит и взвешивается. Отличается только
// наличие куратора, поэтому разница в поведении и есть доказательство.
//
// Про отчёт мало знать, что он доехал: потерянный ряд, сдвиг дат на день или
// чужая единица измерения доедут так же успешно. Поэтому часть 4в сверяет его
// содержимое с посевом, а 4г открывает кураторское приложение и сверяет то, что
// оно НАРИСОВАЛО, с числами человека: экран не показывает payload как есть, он
// пересчитывает его заново — цвета, график, тренд, — и разойтись может там.
//
// Девять постоянных планок (кальций, клетчатка, железо, гем, овощи, омега-3,
// баланс жиров, красное мясо, яйца) здесь не проверяются намеренно: они чистые
// функции от профиля и калорийной планки, и их считает `cargo test -p plankas`.
// Браузер нужен там, где правило зависит от ЗАПУЩЕННОГО приложения.
//
// Запуск: node scripts/check-curator-flow.mjs
//   FE      — каталог собранного фронтенда (по умолчанию ../frontend/dist)
//   CUR     — каталог собранного куратора (по умолчанию ../curator/dist)
//   VERBOSE — печатать все запросы приложения к воркерам

import { serveWithProxy, launchBrowser } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
const CUR_DIST = process.env.CUR ?? new URL('../curator/dist', import.meta.url).pathname;
const SUPPORT = process.env.SUPPORT ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const OLD_CAL = 2500;
const OLD_STEPS = 9000;
const NEW_CAL = 1850;
// То же, что кладёт `seed`: проверке нужны те же числа, что и посеву, иначе она
// сверяет отчёт сама с собой.
const SEED_STEPS = OLD_STEPS + 1500;
const SEED_KCAL = 2000; // 2000 г каши по 100 ккал на 100 г
const BIRTH_YEAR = 1985;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};
const section = (n) => console.log(`\n── ${n} ──`);

const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString().slice(0, 10); };
const iso = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString(); };
const uuid = () => crypto.randomUUID();

const SEED_WEIGHT_DAYS = Array.from({ length: 21 }, (_, i) => ymd(i));
const SEED_WEIGHT = Object.fromEntries(SEED_WEIGHT_DAYS.map((d, i) => [d, 90 + i * 0.14]));
const SEED_STEP_DAYS = Array.from({ length: 12 }, (_, i) => ymd(i + 1));
const AGE = new Date().getFullYear() - BIRTH_YEAR;

const api = async (token, method, path, body) => {
  const r = await fetch(SUPPORT + path, {
    method,
    headers: { Authorization: `Bearer ${token}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const t = await r.text();
  try { return { status: r.status, json: JSON.parse(t) }; } catch { return { status: r.status, text: t }; }
};

async function waitFor(page, tries, read) {
  for (let i = 0; i < tries; i++) {
    const v = await read().catch(() => null);
    if (v) return v;
    await page.waitForTimeout(1000);
  }
  return null;
}

// ── Посев: человек с историей, планку поставили десять дней назад ────────────
const seed = async ({ uid, OLD_CAL, OLD_STEPS, ymd0, iso0 }) => {
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const now = new Date().toISOString();
  const app_flags = [
    { key: 'push_onboarding_dismissed', value: 'true' },
    { key: 'welcome_shown', value: 'true' },
    { key: 'planka_weekly_anchor', value: ymd0[10] },
    { key: 'steps_planka_weekly_anchor', value: ymd0[10] },
    { key: 'steps_gate_opened_at', value: ymd0[30] },
    { key: 'activity_week_unlocked', value: 'true' },
    { key: 'ft_subscription', value: JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
        active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' }) },
  ];
  const profile = [{ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1985,
    goal: 'lose', steps_planka: OLD_STEPS, created_at: now, updated_at: now }];
  const goals = [{ id: 'g-cal', nutrient: 'Calories', key: 'calories', direction: 'AtMost',
    amount: OLD_CAL, unit: 'Kcal', period: 'Day', created_at: iso0[10], updated_at: iso0[10] }];
  // История планок — источник действующего значения.
  const planka_history = [
    { id: `calories:${ymd0[10]}`, kind: 'calories', date: ymd0[10], amount: OLD_CAL, created_at: iso0[10], updated_at: iso0[10] },
    { id: `steps:${ymd0[10]}`, kind: 'steps', date: ymd0[10], amount: OLD_STEPS, created_at: iso0[10], updated_at: iso0[10] },
  ];
  const foods = [{ id: 'f1', name: 'Овсяная каша', kcal: 100, protein: 3, fat: 2, carbs: 18,
    nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
    is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
    is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
    created_at: now, updated_at: now }];
  const diary = [], weight_entries = [], step_entries = [];
  for (let i = 1; i <= 12; i++) {
    diary.push({ id: 'd' + i, food_id: 'f1', date: ymd0[i], time: null, grams: 2000,
      waste_grams: 0, meal_label: 'lunch', deleted: false, created_at: now, updated_at: now });
    // Шаги ВЫШЕ планки — индикатор зелёный, и недельная ступенька обязана подняться.
    step_entries.push({ id: 's' + i, date: ymd0[i], steps: OLD_STEPS + 1500, created_at: now, updated_at: now });
  }
  for (let i = 0; i < 21; i++) {
    weight_entries.push({ id: 'w' + i, date: ymd0[i], weight_kg: 90 + i * 0.14,
      no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
      created_at: now, updated_at: now });
  }
  for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, foods, diary, weight_entries, step_entries })) {
    await new Promise((res, rej) => {
      const tx = db.transaction([store], 'readwrite');
      for (const r of rows) tx.objectStore(store).put(r);
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
  }
  db.close();
};

// ── Что приложение имеет на руках ───────────────────────────────────────────
const readState = async () => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], 'readonly').objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const [flags, history, msgs] = [await all('app_flags'), await all('planka_history'), await all('support_msgs')];
  db.close();
  const f = (k) => flags.find((x) => x.key === k)?.value;
  const kind = (k) => history.filter((h) => h.kind === k).sort((a, b) => a.date.localeCompare(b.date));
  return {
    calories: kind('calories').at(-1)?.amount,
    fiber: kind('fiber').at(-1)?.amount,
    fiberDays: kind('fiber').map((h) => h.date),
    calorieDays: kind('calories').map((h) => h.date),
    steps: kind('steps').at(-1)?.amount,
    stepDays: kind('steps').map((h) => h.date),
    proteinDays: kind('protein').map((h) => h.date),
    calAnchor: f('planka_weekly_anchor'),
    stepAnchor: f('steps_planka_weekly_anchor'),
    peer: f('support_current_peer') ?? null,
    letters: JSON.parse(f('letters_v1') || '[]').map((l) => l.id),
    requests: msgs.filter((m) => m.kind === 'data_request')
      .map((m) => JSON.parse(m.payload || '{}').scope ?? null),
  };
};

/// Поднять приложение с посевом и дождаться, пока оно узнает адресата.
async function boot(browser, server, prefix) {
  // Пользователь НАСТОЯЩИЙ и с оплаченной подпиской: без неё приложение
  // показывает замок, и до проверяемого дело не доходит вовсе.
  const { userId: uid } = await createPaidUser(prefix);
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const page = await ctx.newPage();
  const panics = [];
  // Проверка сама перезагружает страницу — иногда посреди докачки wasm. Браузер
  // сообщает об этом ошибкой загрузки, и она про НАШУ навигацию, а не про
  // приложение: настоящая поломка сборки выглядит иначе и на перезагрузку не
  // проходит. Всё остальное копим как есть.
  const ownNavigation = (m) => /compilation aborted.*(aborted|cancel)/i.test(m);
  page.on('pageerror', (e) => { if (!ownNavigation(e.message)) panics.push(e.message); });
  page.on('console', (m) => { if (/panicked at/.test(m.text())) panics.push(m.text().slice(0, 200)); });

  await page.goto(server.url, { waitUntil: 'domcontentloaded' });
  await page.evaluate(async ({ uid, token }) => {
    await new Promise((r) => { const q = indexedDB.deleteDatabase(`hjkl-ft-${uid}`); q.onsuccess = q.onerror = q.onblocked = () => r(); });
    localStorage.clear();
    localStorage.setItem('user_id', uid);
    localStorage.setItem('auth_token', token);
    localStorage.setItem('pwa_dismissed', 'true');
  }, { uid, token: mintToken(uid) });
  await page.goto(server.url, { waitUntil: 'domcontentloaded' });
  for (let i = 0; i < 40; i++) {
    const ok = await page.evaluate(async (uid) => {
      const dbs = await indexedDB.databases();
      if (!dbs.some((d) => d.name === `hjkl-ft-${uid}`)) return false;
      return await new Promise((res) => {
        const r = indexedDB.open(`hjkl-ft-${uid}`);
        r.onsuccess = () => { const has = r.result.objectStoreNames.contains('planka_history'); r.result.close(); res(has); };
        r.onerror = () => res(false);
      });
    }, uid).catch(() => false);
    if (ok) break;
    await page.waitForTimeout(500);
  }
  const sow = () => page.evaluate(seed, {
    uid, OLD_CAL, OLD_STEPS,
    ymd0: Array.from({ length: 35 }, (_, i) => ymd(i)),
    iso0: Array.from({ length: 35 }, (_, i) => iso(i)),
  });
  await sow();
  // Посев может не пережить первую синхронизацию: усыновление устройства чистит
  // локальные хранилища и берёт всё с сервера, а у свежего человека там пусто.
  // Гонку с ним не выиграть — можно только присмотреть за посеянным и посеять
  // снова, если его смыло. Иначе проверка падает на «планок нет», рассказывая о
  // поломке, которой не было.
  //
  // Без перезагрузки: она запустила бы недельный цикл ЗДЕСЬ, до привязки
  // куратора, и третья часть проверяла бы уже сдвинутые якоря.
  for (let i = 0; i < 8; i++) {
    await page.waitForTimeout(1000);
    const st = await page.evaluate(readState).catch(() => ({}));
    if (st.steps === undefined || st.calories === undefined) await sow();
  }
  return { ctx, page, panics, uid };
}

/// Вернуть планки к посеянному состоянию: планка стоит десять дней, цикл ещё не
/// считал, писем нет.
///
/// Нужно ровно один раз — после привязки. До неё приложение вправе считать само,
/// и иногда успевает: посев ложится в живую базу, приложение это видит и честно
/// отрабатывает недельный цикл. Гонку с ним не выиграть, но её и не надо
/// выигрывать — посылку третьей части можно просто восстановить, когда куратор
/// уже на месте.
const resetPlankas = async (page, { OLD_CAL, OLD_STEPS, day, iso }) => {
  await page.evaluate(async (arg) => {
    const uid = localStorage.getItem('user_id');
    const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
    const rows = await new Promise((res) => {
      const rq = db.transaction(['planka_history'], 'readonly').objectStore('planka_history').getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    await new Promise((res, rej) => {
      const tx = db.transaction(['planka_history', 'app_flags'], 'readwrite');
      const hist = tx.objectStore('planka_history');
      for (const r of rows) hist.delete(r.id);
      hist.put({ id: `calories:${arg.day}`, kind: 'calories', date: arg.day, amount: arg.OLD_CAL,
        created_at: arg.iso, updated_at: arg.iso });
      hist.put({ id: `steps:${arg.day}`, kind: 'steps', date: arg.day, amount: arg.OLD_STEPS,
        created_at: arg.iso, updated_at: arg.iso });
      const flags = tx.objectStore('app_flags');
      flags.put({ key: 'planka_weekly_anchor', value: arg.day });
      flags.put({ key: 'steps_planka_weekly_anchor', value: arg.day });
      flags.put({ key: 'letters_v1', value: '[]' });
      tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
    });
    db.close();
  }, { OLD_CAL, OLD_STEPS, day, iso });
};

/// Дать приложению узнать адресата, потом перезапустить: пересчёт судит по
/// полному знанию, а не по гонке с первым опросом.
async function settleAndRun(page, expectRun = true) {
  await page.reload({ waitUntil: 'domcontentloaded' });
  const peer = await waitFor(page, 30, async () => (await page.evaluate(readState)).peer);
  if (!peer) throw new Error('сервер так и не назвал адресата — опрос не дошёл?');
  await page.reload({ waitUntil: 'domcontentloaded' });
  if (expectRun) {
    // Ждём СОБЫТИЯ, а не времени. Фиксированной паузы иногда не хватало, и
    // проверка падала на том, что цикл ещё не добежал, — то есть врала о
    // поломке. Якорь встаёт ровно тогда, когда цикл отработал.
    await waitFor(page, 45, async () =>
      (await page.evaluate(readState)).calAnchor === ymd(0) ? true : null);
    // Белок и шаги идут следом за калориями — короткий добор, а не пауза.
    await waitFor(page, 15, async () =>
      (await page.evaluate(readState)).stepAnchor === ymd(0) ? true : null);
    await page.waitForTimeout(2000);
  } else {
    // Здесь проверяется, что НЕ произошло ничего, — ждать нечего, кроме времени.
    await page.waitForTimeout(18000);
  }
  return peer;
}

// ═══════════════════════════════════════════════════════════════════════════
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
console.log(`приложение на ${server.url}, воркеры через прокси`);
const b = await launchBrowser();

// ── 1. Без куратора приложение ведёт планки само ────────────────────────────
section('1. без куратора: приложение ведёт планки САМО');
const free = await boot(b, server, 'flow-free');
const freePeer = await settleAndRun(free.page);
const f = await free.page.evaluate(readState);
console.log(`   адресат ${freePeer}; калории ${OLD_CAL}→${f.calories}, шаги ${OLD_STEPS}→${f.steps}`);
// Утверждается, что цикл ПРОШЁЛ: держать планку — тоже его законное решение
// (недоедающему её не поднимают). Арифметику проверяет `cargo test -p plankas`.
check('калорийный цикл сработал (якорь на сегодня)', f.calAnchor === ymd(0), f.calAnchor);
check('письмо о калорийной планке пришло', f.letters.some((i) => i.startsWith('planka-')),
  f.letters.join(', ') || 'писем нет');
check('шаговый цикл сработал (якорь на сегодня)', f.stepAnchor === ymd(0), f.stepAnchor);
check('шаговая планка поднялась', f.steps > OLD_STEPS, `${OLD_STEPS} → ${f.steps}`);
check('белок пересчитан сегодняшним днём', f.proteinDays.includes(ymd(0)),
  f.proteinDays.join(', ') || 'записей нет');
check('запуск без паник', free.panics.length === 0, free.panics[0] ?? 'паник нет');
await free.ctx.close();

// ── 2. Привязка по ссылке ───────────────────────────────────────────────────
section('2. привязка ПО ССЫЛКЕ: экран согласия в приложении');
const bound = await boot(b, server, 'flow-bound');
const curatorId = `e2e-curator-${uuid().slice(0, 8)}`;
const curator = mintToken(curatorId);
let r = await api(curator, 'POST', '/curator/register', {});
check('куратор зарегистрировался', r.status === 200, `${r.status}`);
r = await api(curator, 'POST', '/curator/clients', { name: 'Проверочный клиент' });
check('слот клиента заведён', r.status === 200, `${r.status}`);
const clientId = r.json?.client?.id;
const code = r.json?.client?.invite_code;
check('ссылка приглашения выдана', !!code, code ?? 'кода нет');

await bound.page.goto(`${server.url}/curator?c=${code}`, { waitUntil: 'domcontentloaded' });
const askShown = await bound.page.waitForSelector('[data-testid="curator-invite-ask"]', { timeout: 25000 })
  .then(() => true).catch(() => false);
check('экран согласия открылся по ссылке', askShown,
  askShown ? '' : await bound.page.evaluate(() => document.body.innerText.slice(0, 120)));
if (askShown) {
  await bound.page.click('[data-testid="curator-invite-accept"]');
  const done = await bound.page.waitForSelector('[data-testid="curator-invite-done"]', { timeout: 25000 })
    .then(() => true).catch(() => false);
  check('согласие принято приложением', done);
}
r = await api(curator, 'GET', '/curator/clients');
check('сервер видит привязку', r.json?.clients?.some((c) => c.id === clientId && c.bound),
  JSON.stringify(r.json?.clients?.map((c) => ({ id: c.id, bound: c.bound })) ?? []));

// ── 3. С куратором приложение планки не трогает ─────────────────────────────
section('3. с куратором: приложение планки НЕ ТРОГАЕТ');
await bound.page.goto(server.url, { waitUntil: 'domcontentloaded' });
await resetPlankas(bound.page, { OLD_CAL, OLD_STEPS, day: ymd(10), iso: iso(10) });
const boundPeer = await settleAndRun(bound.page, false);
const s = await bound.page.evaluate(readState);
console.log(`   адресат ${boundPeer}; калории ${OLD_CAL}→${s.calories}, шаги ${OLD_STEPS}→${s.steps}`);
check('адресат — куратор', boundPeer.startsWith('curator:'), boundPeer);
check('калорийная планка не изменилась', s.calories === OLD_CAL, `${OLD_CAL} → ${s.calories}`);
check('шаговая планка не изменилась', s.steps === OLD_STEPS, `${OLD_STEPS} → ${s.steps}`);
check('белок не переписан', !s.proteinDays.includes(ymd(0)), s.proteinDays.join(', ') || 'записей нет');
// Якоря стоят намеренно: человек ждал ровно столько, сколько ждал, и после
// отвязки неделя не должна начинаться заново.
check('калорийный якорь не сдвинут', s.calAnchor === ymd(10), `${s.calAnchor} (ожидали ${ymd(10)})`);
check('шаговый якорь не сдвинут', s.stepAnchor === ymd(10), `${s.stepAnchor} (ожидали ${ymd(10)})`);
check('писем о недельных планках нет', !s.letters.some((i) => i.startsWith('planka-') || i.startsWith('steps-')),
  s.letters.join(', ') || 'писем нет');
check('запуск без паник', bound.panics.length === 0, bound.panics[0] ?? 'паник нет');

// ── 4. Передача в обе стороны ───────────────────────────────────────────────
section('4а. куратор ставит планку — она обязана примениться');
r = await api(curator, 'POST', `/curator/clients/${clientId}/reply`, {
  client_id: uuid(), text: '', kind: 'set_planka_v2',
  payload: JSON.stringify({ key: 'calories', amount: NEW_CAL }),
});
check('директива принята воркером', r.status === 200, `${r.status} ${r.text ?? ''}`);
// Приложение опрашивает тред при ЗАПУСКЕ и возврате из фона, а не непрерывно:
// человек получает пуш и открывает приложение. Повторяем именно это.
await bound.page.reload({ waitUntil: 'domcontentloaded' });
await waitFor(bound.page, 30, async () =>
  (await bound.page.evaluate(readState)).calories === NEW_CAL ? true : null);
let a = await bound.page.evaluate(readState);
check('кураторская планка применилась', a.calories === NEW_CAL, `${a.calories} (ожидали ${NEW_CAL})`);
check('запись легла в историю сегодняшним днём', a.calorieDays.includes(ymd(0)), a.calorieDays.join(', '));
check('письмо о правке пришло', a.letters.some((i) => i.startsWith('curator-planka-')),
  a.letters.join(', ') || 'писем нет');

section('4б. куратор запрашивает данные — запрос обязан доехать');
r = await api(curator, 'POST', `/curator/clients/${clientId}/request`, { client_id: uuid(), scope: 'all' });
check('запрос принят воркером', r.status === 200, `${r.status} ${r.text ?? ''}`);
await bound.page.reload({ waitUntil: 'domcontentloaded' });
const scope = await waitFor(bound.page, 30, async () =>
  (await bound.page.evaluate(readState)).requests.at(-1) ?? null);
check('запрос доехал до приложения', scope === 'all', `scope = ${scope}`);

section('4в. человек отправляет отчёт — он обязан доехать до куратора');
const widget = await bound.page.waitForSelector('[data-testid="dash-report-widget"]', { timeout: 25000 })
  .then((h) => h).catch(() => null);
check('виджет отчёта виден на главной', !!widget);
if (widget) {
  await widget.click();
  await bound.page.waitForSelector('[data-testid="report-panel"]', { timeout: 15000 });
  await bound.page.click('[data-testid="report-send"]');
  await bound.page.waitForSelector('[data-testid="report-choice"]', { timeout: 10000 });
  // Прошлого отчёта нет — предлагается только «все данные», и это тоже правило.
  const onlyAll = await bound.page.evaluate(() =>
    !document.querySelector('[data-testid="report-send-new"]')
    && !!document.querySelector('[data-testid="report-send-all"]'));
  check('без прошлого отчёта предложены только «все данные»', onlyAll);
  await bound.page.click('[data-testid="report-send-all"]');
  const sent = await bound.page.waitForSelector('[data-testid="report-sent"]', { timeout: 25000 })
    .then(() => true).catch(() => false);
  check('приложение отчиталось об отправке', sent);
}
const got = await waitFor(bound.page, 20, async () => {
  const rep = await api(curator, 'GET', `/curator/clients/${clientId}/report`);
  return rep.json?.report ? rep.json : null;
});
check('отчёт доехал до куратора', !!got, got ? '' : 'слот пуст');
if (got) {
  const rep = JSON.parse(got.report).report;
  console.log(`   период отчёта: ${rep.period.from} … ${rep.period.to}`);
  check('последний день отчёта — ВЧЕРА (сегодняшний не едет)', rep.period.to === ymd(1),
    `${rep.period.to} (вчера ${ymd(1)})`);
  check('выполненный запрос погашен', !got.request_scope, `request_scope = ${got.request_scope}`);

  // ── Содержимое отчёта сверяется С ПОСЕВОМ ─────────────────────────────────
  // Доехавший отчёт — ещё не верный отчёт. Потерянный ряд, сдвиг дат на день,
  // чужая единица измерения доедут ровно так же успешно, и до сих пор ни одна
  // проверка на это не смотрела. Здесь утверждается не «что-то пришло», а
  // «пришло ровно то, что человек у себя записал».
  const wRows = rep.weight?.series ?? [];
  const sRows = rep.steps?.series ?? [];
  const wDates = wRows.map((r) => r.date);
  const sDates = sRows.map((r) => r.date);
  // Правило «сегодняшнее не едет» — про ДАННЫЕ, а не только про границу
  // периода: день ещё заполняется, и судить по нему нельзя.
  check('в ряду веса нет сегодняшнего дня', !wDates.includes(ymd(0)),
    wDates.includes(ymd(0)) ? `есть ${ymd(0)}` : `последний ${wDates.at(-1) ?? 'ряд пуст'}`);
  check('в ряду шагов нет сегодняшнего дня', !sDates.includes(ymd(0)),
    sDates.includes(ymd(0)) ? `есть ${ymd(0)}` : `последний ${sDates.at(-1) ?? 'ряд пуст'}`);

  const wantW = SEED_WEIGHT_DAYS.filter((d) => d >= rep.period.from && d <= rep.period.to);
  check('ряд веса — все дни периода, ни одного лишнего',
    JSON.stringify([...wDates].sort()) === JSON.stringify([...wantW].sort()),
    `${wDates.length} из ${wantW.length}`);
  const wBad = wRows.find((r) => Math.abs(r.kg - SEED_WEIGHT[r.date]) > 1e-9);
  check('вес в отчёте — тот, что человек записывал', !wBad,
    wBad ? `${wBad.date}: ${wBad.kg} вместо ${SEED_WEIGHT[wBad.date]}` : `${wRows.length} записей сошлись`);

  const wantS = SEED_STEP_DAYS.filter((d) => d >= rep.period.from && d <= rep.period.to);
  const sBad = sRows.find((r) => r.steps !== SEED_STEPS);
  check('ряд шагов — все дни периода с теми же числами',
    JSON.stringify([...sDates].sort()) === JSON.stringify([...wantS].sort()) && !sBad,
    sBad ? `${sBad.date}: ${sBad.steps} вместо ${SEED_STEPS}` : `${sRows.length} из ${wantS.length}`);

  // Среднее едет ради расчёта на стороне куратора: по нему он отличает «планка
  // велика» от «планку не держат». Ошибка здесь тихо испортит его решение.
  check('среднее за 7 завершённых дней — то, что съедено',
    Math.abs((rep.avg_kcal_7d ?? 0) - SEED_KCAL) < 1,
    `${rep.avg_kcal_7d} (ожидали ${SEED_KCAL})`);

  const body = rep.body ?? {};
  check('тело — из профиля человека',
    body.sex === 'male' && body.height_cm === 180 && body.age_years === AGE,
    `${body.sex}, ${body.height_cm} см, ${body.age_years} лет (ожидали male, 180, ${AGE})`);

  // Планки в отчёте берутся из того же `plankas::current`, что рисует человеку
  // его собственные шкалы: сойтись они обязаны до числа.
  const mine = await bound.page.evaluate(readState);
  check('планки в отчёте — те же, что действуют у человека',
    rep.targets?.calories === mine.calories && rep.targets?.steps === mine.steps,
    `калории ${rep.targets?.calories} / ${mine.calories}, шаги ${rep.targets?.steps} / ${mine.steps}`);
  // Индикаторы судят дни по съеденному. Сегодняшний день ещё пуст — «съедено
  // ноль» прочитается как «планка соблюдена», и куратор увидит зелёный там, где
  // не произошло ничего.
  const indPts = (rep.indicators ?? []).flatMap((i) => (i.points ?? []).map((p) => p.date));
  check('в рядах индикаторов нет сегодняшнего дня', !indPts.includes(ymd(0)),
    indPts.includes(ymd(0)) ? `есть ${ymd(0)}` : `дней ${new Set(indPts).size}`);

  check('история планок доехала', (rep.plankas?.calories ?? []).some((p) => p.amount === NEW_CAL),
    JSON.stringify(rep.plankas?.calories ?? []));
}
let curatorCalc = null;
section('4г. кураторское приложение показывает ТО ЖЕ, что у человека');
// Отчёт, доехавший до воркера, — ещё не отчёт, увиденный куратором. Кураторский
// экран показывает не payload как есть: он пересчитывает его заново — цвета
// индикаторов, график веса, подсказку. Расхождение с числами человека заводится
// именно там, и до сих пор туда не смотрела ни одна проверка: тест сам ходил в
// воркер и сам разбирал JSON, а приложение куратора не открывалось ни разу.
const curServer = await serveWithProxy({
  root: CUR_DIST,
  configFor: (o) => [
    `auth_base_url = "${o}/api/auth"`, `support_base_url = "${o}/api/support"`,
    `push_base_url = "${o}/api/push"`, `app_origin = "${o}"`,
  ].join('\n'),
});
const cctx = await b.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
// Кураторское приложение работает только установленным: во вкладке оно
// показывает экран «поставьте на домашний экран» и дальше не идёт. Проверке
// нужен не этот заслон, а то, что за ним, — представляемся установленным ровно
// тем полем, по которому приложение и судит на iOS.
await cctx.addInitScript(() => {
  Object.defineProperty(navigator, 'standalone', { get: () => true, configurable: true });
});
const cpage = await cctx.newPage();
const cpanics = [];
// Со стеком: ошибка в кураторском приложении показалась однажды и больше не
// повторилась, а по одному сообщению не отличить сбой приложения от заминки
// самого браузера при клике. Стек ответит на это сразу, а не через прогон.
cpage.on('pageerror', (e) => {
  cpanics.push(e.message);
  console.log('[куратор, ошибка страницы]', (e.stack ?? e.message).slice(0, 400));
});
cpage.on('console', (m) => { if (/panicked at/.test(m.text())) cpanics.push(m.text().slice(0, 200)); });
// Вход тем же способом, что и у человека: паскей проверяет личность, а проверке
// нужна не она, а всё, что происходит после входа.
await cpage.goto(curServer.url, { waitUntil: 'domcontentloaded' });
await cpage.evaluate(({ id, token }) => {
  localStorage.clear();
  localStorage.setItem('curator_id', id);
  localStorage.setItem('curator_token', token);
}, { id: curatorId, token: curator });
await cpage.goto(curServer.url, { waitUntil: 'domcontentloaded' });

const row = await cpage.waitForSelector('[data-testid="client-row"]', { timeout: 30000 })
  .then((h) => h).catch(() => null);
check('куратор вошёл и видит клиента', !!row,
  row ? '' : (await cpage.evaluate(() => document.body.innerText.slice(0, 160))));
if (row && got) {
  const rep = JSON.parse(got.report).report;
  await row.click();
  const shown = await cpage.waitForSelector('[data-testid="client-report"]', { timeout: 30000 })
    .then(() => true).catch(() => false);
  check('отчёт открылся на экране куратора', shown,
    shown ? '' : (await cpage.evaluate(() => document.body.innerText.slice(0, 160))));

  const seen = await cpage.evaluate(() => {
    const root = document.querySelector('[data-testid="client-report"]');
    if (!root) return null;
    return {
      text: root.innerText,
      // Столбик шага подписан «дата · число» — это ровно те числа, по которым
      // куратор судит, и их можно сверить, а не разглядывать. Берём столбики
      // именно из карточки шагов: полоски индикаторов подписаны так же.
      steps: (() => {
        const card = [...root.querySelectorAll('.card')].find(
          (c) => !c.dataset.testid && c.querySelector('p')?.textContent.trim() === 'Шаги');
        return card ? [...card.querySelectorAll('span[title]')].map((e) => e.getAttribute('title')) : [];
      })(),
      indicators: [...root.querySelectorAll('[data-testid="indicator-row"]')].map((e) => e.innerText),
    };
  });

  check('период на экране — тот же, что в отчёте',
    seen.text.includes(rep.period.from) && seen.text.includes(rep.period.to),
    seen.text.split('\n')[0]);

  const wantSteps = rep.steps.series.map((r) => `${r.date} · ${r.steps}`);
  check('столбики шагов — те же дни и те же числа',
    JSON.stringify(seen.steps) === JSON.stringify(wantSteps),
    seen.steps.length === wantSteps.length ? 'ряды совпали' : `${seen.steps.length} против ${wantSteps.length}`);

  const slope = rep.weight.slope_kg_per_week;
  const wantSlope = slope == null ? 'тренд не определён'
    : `${slope >= 0 ? '+' : ''}${slope.toFixed(2)} кг/нед`;
  check('тренд веса на экране — из данных человека, а не выдуман',
    seen.text.includes(wantSlope), `ждали «${wantSlope}»`);

  // Планка на карточке индикатора — то самое число, по которому судится человек
  // у себя. Разойтись они не имеют права: судят-то одного и того же.
  const mine = await bound.page.evaluate(readState);
  const calRow = seen.indicators.find((t) => /Калори/i.test(t));
  check('планка калорий на экране куратора — та же, что у человека',
    !!calRow && calRow.includes(String(mine.calories)),
    calRow ? calRow.replace(/\n/g, ' ') : 'строки калорий нет');
  check('история планок на экране — с кураторским числом',
    seen.text.includes(String(NEW_CAL)), `${NEW_CAL} в тексте`);
  // «Рассчитать» — единственное место, где куратор ВИДИТ наше правило: кнопка
  // считает планку тем же крейтом, что и недельный цикл приложения, но по
  // данным ОТЧЁТА. Если в отчёт не доехало поле, нужное правилу, число молча
  // разойдётся с тем, к чему пришло бы приложение само. Сверяется это в части 6,
  // где цикл считает то же самое у человека.
  const calRowIdx = seen.indicators.findIndex((t) => /Калори/i.test(t));
  if (calRowIdx >= 0) {
    await cpage.evaluate((i) => {
      document.querySelectorAll('[data-testid="indicator-row"]')[i]
        .querySelector('[data-testid="indicator-edit"]').click();
    }, calRowIdx);
    const editor = await cpage.waitForSelector('[data-testid="planka-editor"]', { timeout: 15000 })
      .then(() => true).catch(() => false);
    check('редактор планки открылся', editor);
    if (editor) {
      const hasCalc = await cpage.$('[data-testid="planka-calc"]');
      check('«Рассчитать» предложено по калориям', !!hasCalc,
        hasCalc ? '' : 'кнопки нет — правилу не хватило данных отчёта');
      if (hasCalc) {
        await hasCalc.click();
        curatorCalc = await cpage.$eval('[data-testid="planka-value"]', (e) => e.value);
        check('расчёт дал число', /^\d+(\.\d+)?$/.test(curatorCalc), curatorCalc);
      }
      // Закрываем, ничего не отправляя: планку тут ставит не проверка.
      await cpage.click('[data-testid="planka-editor"]', { position: { x: 5, y: 5 } });
    }
  }
  check('кураторское приложение без паник', cpanics.length === 0, cpanics[0] ?? 'паник нет');
}

section('4д. «только новое» — от границы прошлого отчёта, а не с начала');
// Первый отчёт ушёл «за всё» и покрыл всё по вчерашний день. От чего
// отсчитывается ВТОРОЙ — до сих пор не проверял никто.
const openChoice = async () => {
  await bound.page.reload({ waitUntil: 'domcontentloaded' });
  const w = await bound.page.waitForSelector('[data-testid="dash-report-widget"]', { timeout: 25000 })
    .then((h) => h).catch(() => null);
  if (!w) return null;
  await w.click();
  await bound.page.waitForSelector('[data-testid="report-panel"]', { timeout: 15000 });
  await bound.page.click('[data-testid="report-send"]');
  await bound.page.waitForSelector('[data-testid="report-choice"]', { timeout: 10000 });
  return await bound.page.evaluate(() => ({
    new: !!document.querySelector('[data-testid="report-send-new"]'),
    all: !!document.querySelector('[data-testid="report-send-all"]'),
    text: document.querySelector('[data-testid="report-choice"]')?.innerText ?? '',
  }));
};
const sendAndCatch = async (testid) => {
  const before = JSON.stringify(got?.report ?? '');
  await bound.page.click(`[data-testid="${testid}"]`);
  await bound.page.waitForSelector('[data-testid="report-sent"]', { timeout: 25000 }).catch(() => {});
  const fresh = await waitFor(bound.page, 20, async () => {
    const rep = await api(curator, 'GET', `/curator/clients/${clientId}/report`);
    return rep.json?.report && JSON.stringify(rep.json.report) !== before ? rep.json : null;
  });
  return fresh ? JSON.parse(fresh.report).report : null;
};

// (а) Нового нет: прошлый отчёт дошёл до вчера, а сегодняшний день не едет.
// Кнопки «только новое» быть не должно — иначе уходит ПУСТОЙ отчёт с вывернутым
// периодом «с завтра по вчера», и он затирает у куратора прошлый, содержательный.
r = await api(curator, 'POST', `/curator/clients/${clientId}/request`, { client_id: uuid(), scope: 'new' });
check('запрос «только новое» принят', r.status === 200, `${r.status} ${r.text ?? ''}`);
const nothingNew = await openChoice();
check('нового нет — «только новое» не предлагается', nothingNew && !nothingNew.new && nothingNew.all,
  nothingNew ? nothingNew.text.replace(/\n/g, ' | ') : 'шторка не открылась');
// Передумать человек вправе: шторка накрывает экран целиком, и выход из неё
// обязан быть.
await bound.page.click('[data-testid="report-send-cancel"]');
const closed = await bound.page.waitForSelector('[data-testid="report-choice"]', { state: 'detached', timeout: 5000 })
  .then(() => true).catch(() => false);
check('шторку можно закрыть, ничего не отправив', closed);

// (б) Прошлый отчёт был давно. Отматываем его границу на пять дней назад — то же
// самое состояние, что у человека, отчитавшегося в прошлые выходные.
const rewound = await bound.page.evaluate(async ({ through }) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const rows = await new Promise((res) => {
    const rq = db.transaction(['support_msgs'], 'readonly').objectStore('support_msgs').getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const rep = rows.filter((m) => m.kind === 'data_share' && m.sender === 'user').pop();
  if (!rep) { db.close(); return null; }
  const payload = JSON.parse(rep.payload);
  payload.report.period.to = through;
  rep.payload = JSON.stringify(payload);
  await new Promise((res, rej) => {
    const tx = db.transaction(['support_msgs'], 'readwrite');
    tx.objectStore('support_msgs').put(rep);
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
  return rep.id;
}, { through: ymd(5) });
check('граница прошлого отчёта отмотана на пять дней назад', !!rewound, rewound ?? 'отчёта в базе нет');

const choice = await openChoice();
check('теперь предложены ОБА варианта', choice && choice.new && choice.all,
  choice ? choice.text.replace(/\n/g, ' | ') : 'шторка не открылась');
check('подсказка называет дату прошлого отчёта', (choice?.text ?? '').includes(ymd(5)), ymd(5));

const fresh = await sendAndCatch('report-send-new');
check('отчёт «только новое» доехал', !!fresh, fresh ? '' : 'слот не обновился');
if (fresh) {
  console.log(`   период: ${fresh.period.from} … ${fresh.period.to}`);
  // Со СЛЕДУЮЩЕГО дня за границей: день, уже уехавший в прошлом отчёте, второй
  // раз не едет.
  check('начало — следующий день за границей прошлого отчёта', fresh.period.from === ymd(4),
    `${fresh.period.from} (ожидали ${ymd(4)})`);
  check('конец — вчера', fresh.period.to === ymd(1), fresh.period.to);
  const days = fresh.weight.series.map((w) => w.date);
  check('в отчёте только новые дни, старых нет',
    days.length > 0 && days.every((d) => d >= ymd(4) && d <= ymd(1)),
    days.join(', ') || 'ряд пуст');
  check('шаги — тоже только новые дни',
    fresh.steps.series.every((x) => x.date >= ymd(4) && x.date <= ymd(1)),
    fresh.steps.series.map((x) => x.date).join(', ') || 'ряд пуст');
}

section('4е. чат в обе стороны: человек ↔ куратор');
// Переписка — единственный канал, где стороны говорят СВОИМИ словами, а не
// директивами. В сквозной проверке он до сих пор не проверялся ни разу: ездили
// планки, запросы и отчёты, а простой текст — нет.
const fromPerson = `от человека ${uuid().slice(0, 8)}`;
const fromCurator = `от куратора ${uuid().slice(0, 8)}`;
await bound.page.goto(`${server.url}/chat`, { waitUntil: 'domcontentloaded' });
const composer = await bound.page.waitForSelector('[data-testid="chat-input"]', { timeout: 25000 })
  .then((h) => h).catch(() => null);
check('чат человека открылся', !!composer);
if (composer) {
  await composer.fill(fromPerson);
  await bound.page.click('[data-testid="chat-send"]');
  const inThread = await waitFor(bound.page, 20, async () =>
    (await bound.page.evaluate(() =>
      document.querySelector('[data-testid="chat-messages"]')?.innerText ?? '')).includes(fromPerson) || null);
  check('сообщение видно в своей переписке', !!inThread);
}

// Куратор открывает ту же переписку у себя.
await cpage.goto(curServer.url, { waitUntil: 'domcontentloaded' });
const crow = await cpage.waitForSelector('[data-testid="client-row"]', { timeout: 30000 })
  .then((h) => h).catch(() => null);
if (crow) {
  await crow.click();
  await cpage.waitForSelector('[data-testid="client-chat"]', { timeout: 20000 });
  await cpage.click('[data-testid="client-chat"]');
}
const seenByCurator = await cpage.waitForSelector('[data-testid="curator-chat"]', { timeout: 20000 })
  .then(() => waitFor(cpage, 25, async () =>
    (await cpage.evaluate(() =>
      document.querySelector('[data-testid="curator-chat"]')?.innerText ?? '')).includes(fromPerson) || null))
  .catch(() => null);
check('куратор прочитал сообщение человека', !!seenByCurator,
  seenByCurator ? fromPerson : 'в переписке куратора его нет');

// И отвечает.
const cinput = await cpage.$('[data-testid="chat-input"]');
check('у куратора есть поле ответа', !!cinput);
if (cinput) {
  await cinput.fill(fromCurator);
  await cpage.click('[data-testid="chat-send"]');
  const backToCurator = await waitFor(cpage, 20, async () =>
    (await cpage.evaluate(() =>
      document.querySelector('[data-testid="curator-chat"]')?.innerText ?? '')).includes(fromCurator) || null);
  check('ответ виден у куратора', !!backToCurator);
}

// Человек получает ответ — тем же опросом, каким получал директивы.
await bound.page.goto(`${server.url}/chat`, { waitUntil: 'domcontentloaded' });
const gotReply = await waitFor(bound.page, 30, async () =>
  (await bound.page.evaluate(() =>
    document.querySelector('[data-testid="chat-messages"]')?.innerText ?? '')).includes(fromCurator) || null);
check('ответ куратора доехал до человека', !!gotReply,
  gotReply ? fromCurator : 'в переписке человека его нет');
check('кураторское приложение без паник (чат)', cpanics.length === 0, cpanics[0] ?? 'паник нет');

await cctx.close();
curServer.close();

section('5. отвязка возвращает наши правила');
// Сперва куратор ставит ПОСТОЯННУЮ планку: на ней и видно, что при отвязке
// возвращается наше правило. У девяти констант запись в истории может появиться
// ТОЛЬКО от него — приложение их не пишет, — поэтому её исчезновение и есть
// доказательство, а не совпадение.
const FIBER = 32;
r = await api(curator, 'POST', `/curator/clients/${clientId}/reply`, {
  client_id: uuid(), text: '', kind: 'set_planka_v2',
  payload: JSON.stringify({ key: 'fiber', amount: FIBER }),
});
check('директива по клетчатке принята', r.status === 200, `${r.status}`);
await bound.page.reload({ waitUntil: 'domcontentloaded' });
await waitFor(bound.page, 30, async () =>
  (await bound.page.evaluate(readState)).fiber === FIBER ? true : null);
let u = await bound.page.evaluate(readState);
check('постоянная планка встала от куратора', u.fiber === FIBER, `клетчатка ${u.fiber}`);

r = await api(curator, 'POST', `/curator/clients/${clientId}/unbind`, {});
check('куратор прекратил работу', r.status === 200, `${r.status} ${r.text ?? ''}`);
// Приложение узнаёт об отвязке ПО СМЕНЕ АДРЕСАТА в опросе — других признаков у
// него нет, кто бы её ни начал.
await bound.page.reload({ waitUntil: 'domcontentloaded' });
await waitFor(bound.page, 30, async () =>
  (await bound.page.evaluate(readState)).peer === 'admin' ? true : null);
await bound.page.waitForTimeout(4000);
u = await bound.page.evaluate(readState);
console.log(`   адресат ${u.peer}; калории ${u.calories}, клетчатка ${u.fiber ?? '—'}`);
check('адресат снова админ', u.peer === 'admin', u.peer);
check('постоянная планка забыта — вернулось наше правило', u.fiber === undefined,
  u.fiberDays.join(', ') || 'записей нет');
// Стирается ЗАПИСЬ, а не пишется наше число: клетчатка обязана и дальше следовать
// за калорийной планкой, а замороженное число ходило бы за ней не умея.
check('подвижная планка осталась кураторской', u.calories === NEW_CAL,
  `${u.calories} (ожидали ${NEW_CAL})`);
check('письмо об отвязке пришло', u.letters.some((i) => i.startsWith('curator-unbound-')),
  u.letters.join(', ') || 'писем нет');
const body = await bound.page.evaluate(async () => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const rows = await new Promise((r) => { const q = db.transaction(['app_flags'], 'readonly').objectStore('app_flags').getAll(); q.onsuccess = () => r(q.result); });
  db.close();
  const ls = JSON.parse(rows.find((x) => x.key === 'letters_v1')?.value || '[]');
  return ls.find((l) => l.id.startsWith('curator-unbound-'))?.body ?? '';
});
// Перечень обязан называть планки ИМЕНЕМ, а не оборотом из фразы: в столбце
// «• планку по яйцам — 7 шт» читается нелепо.
const listed = body.split('\n').filter((l) => l.startsWith('•'));
check('в письме перечислены планки с числами', listed.length >= 10 && body.includes(String(NEW_CAL)),
  `строк ${listed.length}` + (listed[0] ? `, первая: ${listed[0]}` : ''));
check('подписи в перечне — имена, а не обороты',
  listed.every((l) => !/планку по|предел по|your |target/i.test(l)),
  listed.find((l) => /планку по|предел по/i.test(l)) ?? 'все именами');
check('оба якоря сдвинуты на сегодня', u.calAnchor === ymd(0) && u.stepAnchor === ymd(0),
  `${u.calAnchor} / ${u.stepAnchor}`);

section('6. подсчёт возобновляется');
// Якоря отвязка поставила на сегодня — цикл сработает через неделю. Отматываем
// их назад: это то же самое, что подождать неделю, только без ожидания.
await bound.page.evaluate(async (back) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  await new Promise((res) => {
    const tx = db.transaction(['app_flags'], 'readwrite');
    tx.objectStore('app_flags').put({ key: 'planka_weekly_anchor', value: back });
    tx.objectStore('app_flags').put({ key: 'steps_planka_weekly_anchor', value: back });
    tx.oncomplete = () => res();
  });
  db.close();
}, ymd(10));
await bound.page.reload({ waitUntil: 'domcontentloaded' });
await waitFor(bound.page, 45, async () =>
  (await bound.page.evaluate(readState)).calAnchor === ymd(0) ? true : null);
await waitFor(bound.page, 15, async () =>
  (await bound.page.evaluate(readState)).stepAnchor === ymd(0) ? true : null);
await bound.page.waitForTimeout(2000);
const back = await bound.page.evaluate(readState);
console.log(`   калории ${NEW_CAL}→${back.calories}, шаги ${back.steps}; якорь ${back.calAnchor}`);
check('калорийный цикл снова работает', back.calAnchor === ymd(0), back.calAnchor);
check('шаговый цикл снова работает', back.stepAnchor === ymd(0), back.stepAnchor);
check('письмо о недельной планке снова приходит',
  back.letters.some((i) => i.startsWith('planka-')), back.letters.join(', '));
// Кураторское число — отправная точка, а не помеха: цикл считает ОТ него.
// Куратор нажал «Рассчитать» на том же отчёте — и число обязано совпасть с тем,
// к чему цикл пришёл сам: правило одно, крейт один, а данные к нему пришли двумя
// разными путями — из базы человека и из отчёта.
check('«Рассчитать» у куратора дало то же число, что и цикл',
  curatorCalc !== null && Math.abs(Number(curatorCalc) - Number(back.calories)) < 0.5,
  `куратор ${curatorCalc}, цикл ${back.calories}`);
check('цикл оттолкнулся от кураторского числа', back.calorieDays.includes(ymd(0)),
  back.calorieDays.join(', '));
await bound.ctx.close();

if (process.env.VERBOSE) {
  console.log('\nзапросы приложения:\n  ' +
    server.calls.map((c) => `${c.status} ${c.method} ${c.worker}${c.path}`).join('\n  '));
}
await b.close();
server.close();
console.log(fail === 0 ? '\n=== ALL OK ===' : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
