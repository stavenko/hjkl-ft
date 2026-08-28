// Кураторский путь целиком, в ЗАПУЩЕННОМ приложении худеющего.
//
// Проверяет ровно четыре утверждения, и в таком порядке:
//
//   1. Без куратора приложение ведёт планки САМО — все, что оно ведёт: калории,
//      шаги и следующий за ними белок.
//   2. Привязка происходит ПО ССЫЛКЕ: человек открывает приглашение в своём
//      приложении и соглашается.
//   3. С куратором приложение планки НЕ ТРОГАЕТ — ни одну, пока не тронет он.
//   4. Передача работает в обе стороны: директива планки и запрос данных
//      доезжают до человека, отчёт доезжает до куратора.
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
// Девять постоянных планок (кальций, клетчатка, железо, гем, овощи, омега-3,
// баланс жиров, красное мясо, яйца) здесь не проверяются намеренно: они чистые
// функции от профиля и калорийной планки, и их считает `cargo test -p plankas`.
// Браузер нужен там, где правило зависит от ЗАПУЩЕННОГО приложения.
//
// Запуск: node scripts/check-curator-flow.mjs
//   FE      — каталог собранного фронтенда (по умолчанию ../frontend/dist)
//   VERBOSE — печатать все запросы приложения к воркерам

import { serveWithProxy, launchBrowser } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
const SUPPORT = process.env.SUPPORT ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const OLD_CAL = 2500;
const OLD_STEPS = 9000;
const NEW_CAL = 1850;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};
const section = (n) => console.log(`\n── ${n} ──`);

const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString().slice(0, 10); };
const iso = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString(); };
const uuid = () => crypto.randomUUID();

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
  page.on('pageerror', (e) => panics.push(e.message));
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
  await page.evaluate(seed, {
    uid, OLD_CAL, OLD_STEPS,
    ymd0: Array.from({ length: 35 }, (_, i) => ymd(i)),
    iso0: Array.from({ length: 35 }, (_, i) => iso(i)),
  });
  return { ctx, page, panics, uid };
}

/// Дать приложению узнать адресата, потом перезапустить: пересчёт судит по
/// полному знанию, а не по гонке с первым опросом.
async function settleAndRun(page) {
  await page.reload({ waitUntil: 'domcontentloaded' });
  const peer = await waitFor(page, 30, async () => (await page.evaluate(readState)).peer);
  if (!peer) throw new Error('сервер так и не назвал адресата — опрос не дошёл?');
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(18000);
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
const curator = mintToken(`e2e-curator-${uuid().slice(0, 8)}`);
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
const boundPeer = await settleAndRun(bound.page);
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
}
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
await bound.page.waitForTimeout(18000);
const back = await bound.page.evaluate(readState);
console.log(`   калории ${NEW_CAL}→${back.calories}, шаги ${back.steps}; якорь ${back.calAnchor}`);
check('калорийный цикл снова работает', back.calAnchor === ymd(0), back.calAnchor);
check('шаговый цикл снова работает', back.stepAnchor === ymd(0), back.stepAnchor);
check('письмо о недельной планке снова приходит',
  back.letters.some((i) => i.startsWith('planka-')), back.letters.join(', '));
// Кураторское число — отправная точка, а не помеха: цикл считает ОТ него.
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
