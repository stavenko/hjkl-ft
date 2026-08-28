// Куратор ведёт планки — приложение их не трогает. А он трогает.
//
// Это ЗЕРКАЛО к `check-calorie-planka-weekly.mjs`: там ровно то же состояние —
// планку поставили десять дней назад, человек ест и взвешивается — и недельный
// цикл обязан сработать. Здесь единственное отличие в том, что у человека ЕСТЬ
// куратор, и цикл обязан промолчать. Один посев, два прогона: значит изменилось
// поведение, а не данные.
//
// Привязка НАСТОЯЩАЯ, а не нарисованный флаг. Первая версия этой проверки
// подставляла `support_current_peer` в базу — и приложение честно опросило
// воркер, услышало «куратора нет» и отработало ОТВЯЗКУ: сдвинуло якоря и
// написало письмо. Проверка поймала не то, что хотела, зато показала, что
// отвязка работает. Теперь куратор заводится по-настоящему, через те же ручки,
// какими пользуется его приложение, — и заодно проверяется доставка: директива
// планки и запрос данных доезжают до приложения худеющего и применяются в нём.
//
// Проверяется то, чего не поймать юнит-тестом: недельный цикл и применение
// директив живут в ЗАПУЩЕННОМ приложении и читают состояние из IndexedDB.
//
// Запуск: node scripts/check-curator-planka-freeze.mjs
//   FE — каталог собранного фронтенда (по умолчанию ../frontend/dist)

import { serveWithProxy, mintToken, launchBrowser } from './lib/devserver.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
const OLD_PLANKA = 2500;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};

const SUPPORT = process.env.SUPPORT ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const api = async (token, method, path, body) => {
  const r = await fetch(SUPPORT + path, {
    method,
    headers: { Authorization: `Bearer ${token}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const t = await r.text();
  try { return { status: r.status, json: JSON.parse(t) }; } catch { return { status: r.status, text: t }; }
};
const uuid = () => crypto.randomUUID();

/// Завести куратора и привязать к нему этого человека — тем же путём, каким это
/// делает кураторское приложение.
async function bindCurator(uid) {
  const curator = mintToken(`e2e-curator-${uuid().slice(0, 8)}`);
  const user = mintToken(uid);
  let r = await api(curator, 'POST', '/curator/register', {});
  if (r.status !== 200) throw new Error(`регистрация куратора: ${r.status} ${r.text ?? ''}`);
  r = await api(curator, 'POST', '/curator/clients', { name: 'Проверочный клиент' });
  if (r.status !== 200) throw new Error(`создание клиента: ${r.status} ${r.text ?? ''}`);
  const { id, invite_code } = r.json.client;
  r = await api(user, 'POST', `/curator/invite/${invite_code}/accept`, {});
  if (r.status !== 200) throw new Error(`согласие: ${r.status} ${r.text ?? ''}`);
  return { curator, user, clientId: id };
}

/// Ждать, пока `read` вернёт непустое. Возвращает его или `null`.
async function waitFor(page, tries, read) {
  for (let i = 0; i < tries; i++) {
    const v = await read().catch(() => null);
    if (v) return v;
    await page.waitForTimeout(1000);
  }
  return null;
}

const ymd = (back) => { const d = new Date(); d.setDate(d.getDate() - back); return d.toISOString().slice(0, 10); };
const iso = (back) => { const d = new Date(); d.setDate(d.getDate() - back); return d.toISOString(); };

/// Посев: планку поставили десять дней назад, всё это время человек ест и
/// взвешивается. `peer` — с кем приложение считает себя связанным: "admin" (нет
/// куратора) или "curator:<id>".
function seedScript() {
  return async ({ uid, OLD_PLANKA, ymd0, iso0 }) => {
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const nowIso = new Date().toISOString();
    const app_flags = [
      { key: 'push_onboarding_dismissed', value: 'true' },
      { key: 'welcome_shown', value: 'true' },
      { key: 'planka_weekly_anchor', value: ymd0[10] },
      { key: 'steps_planka_weekly_anchor', value: ymd0[10] },
      { key: 'ft_subscription', value: JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
          active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' }) },
    ];
    const profile = [{ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1985,
      goal: 'lose', steps_planka: 9000, created_at: nowIso, updated_at: nowIso }];
    const goals = [{ id: 'g-cal', nutrient: 'Calories', key: 'calories', direction: 'AtMost',
      amount: OLD_PLANKA, unit: 'Kcal', period: 'Day', created_at: iso0[10], updated_at: iso0[10] }];
    // История планок — источник действующего значения. Без неё калорийной планки
    // для приложения просто нет.
    const planka_history = [{ id: `calories:${ymd0[10]}`, kind: 'calories', date: ymd0[10],
      amount: OLD_PLANKA, created_at: iso0[10], updated_at: iso0[10] }];
    const foods = [{ id: 'f1', name: 'Овсяная каша', kcal: 100, protein: 3, fat: 2, carbs: 18,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
      created_at: nowIso, updated_at: nowIso }];
    const diary = [];
    for (let i = 1; i <= 10; i++) {
      diary.push({ id: 'd' + i, food_id: 'f1', date: ymd0[i], time: null, grams: 2000,
        waste_grams: 0, meal_label: 'lunch', deleted: false, created_at: nowIso, updated_at: nowIso });
    }
    // Вес падает — тренд, который двигает планку.
    const weight_entries = [];
    for (let i = 0; i < 21; i++) {
      weight_entries.push({ id: 'w' + i, date: ymd0[i], weight_kg: 90 + i * 0.14,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: nowIso, updated_at: nowIso });
    }
    for (const [store, rows] of Object.entries({ app_flags, profile, goals, planka_history, foods, diary, weight_entries })) {
      await new Promise((res, rej) => {
        const tx = db.transaction([store], 'readwrite');
        for (const r of rows) tx.objectStore(store).put(r);
        tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
      });
    }
    db.close();
  };
}

/// Что приложение имеет на руках после работы.
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
  const [goals, flags, history] = [await all('goals'), await all('app_flags'), await all('planka_history')];
  db.close();
  const f = (k) => flags.find((x) => x.key === k)?.value;
  const cal = history.filter((h) => h.kind === 'calories').sort((a, b) => a.date.localeCompare(b.date));
  return {
    goal: goals.find((g) => g.nutrient === 'Calories')?.amount,
    effective: cal.at(-1)?.amount,
    calorieDays: cal.map((h) => h.date),
    anchor: f('planka_weekly_anchor'),
    letters: JSON.parse(f('letters_v1') || '[]').map((l) => l.id),
  };
};

/// Один прогон: поднять приложение с посевом и дать ему поработать.
async function run(browser, server, { withCurator }) {
  const uid = `freeze-${withCurator ? 'bound' : 'free'}-${Math.floor(Math.random() * 1e6)}`;
  const bond = withCurator ? await bindCurator(uid) : null;
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

  // Ждём, пока приложение заведёт свою базу, — до этого сеять некуда.
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
  const ymd0 = Array.from({ length: 25 }, (_, i) => ymd(i));
  const iso0 = Array.from({ length: 25 }, (_, i) => iso(i));
  await page.evaluate(seedScript(), { uid, OLD_PLANKA, ymd0, iso0 });

  // Сперва дать приложению УЗНАТЬ адресата: до первого ответа сервера оно не
  // знает, ведёт ли человека куратор, и пересчёт в этот момент намеренно
  // откладывается. Иначе проверка утверждала бы про гонку, а не про правило.
  await page.reload({ waitUntil: 'domcontentloaded' });
  const peer = await waitFor(page, 25, async () => await page.evaluate(async () => {
    const uid = localStorage.getItem('user_id');
    const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
    const rows = await new Promise((r) => { const q = db.transaction(['app_flags'], 'readonly').objectStore('app_flags').getAll(); q.onsuccess = () => r(q.result); });
    db.close();
    return rows.find((x) => x.key === 'support_current_peer')?.value ?? null;
  }));
  if (!peer) throw new Error('сервер так и не назвал адресата — опрос не дошёл?');
  console.log(`   адресат: ${peer}`);

  // Теперь запуск, на котором пересчёт судит по полному знанию.
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(18000);
  const state = await page.evaluate(readState);
  return { state, panics, uid, bond, page, ctx };
}

const server = await serveWithProxy({
  root: DIST,
  configFor: (origin) => [
    `api_base_url = ""`,
    `auth_base_url = "${origin}/api/auth"`,
    `push_base_url = "${origin}/api/push"`,
    `ai_base_url = "${origin}/api/ai"`,
    `payment_base_url = "${origin}/api/payment"`,
    `ocr_queue_base_url = "${origin}/api/ocr"`,
    `sync_base_url = "${origin}/api/sync"`,
    `bug_report_base_url = "${origin}/api/bug"`,
    `support_base_url = "${origin}/api/support"`,
    `landing_url = "https://renorma.app"`,
    `app_origin = "${origin}"`,
  ].join('\n'),
});
console.log(`приложение поднято на ${server.url}, воркеры через прокси\n`);

const b = await launchBrowser();

console.log('── без куратора: недельный цикл обязан сработать ──');
const free = await run(b, server, { withCurator: false });
console.log(`   планка ${OLD_PLANKA} → ${free.state.effective}; якорь ${free.state.anchor}`);
// Утверждаем, что цикл ПРОШЁЛ, а не что число изменилось: держать планку — тоже
// его законное решение (недоедающему её не поднимают). Арифметику проверяют
// юнит-тесты крейта `plankas`, здесь важно, что цикл вообще работает.
check('якорь переставлен на сегодня', free.state.anchor === ymd(0), `${free.state.anchor}`);
check('письмо о недельной планке пришло', free.state.letters.some((id) => id.startsWith('planka-')),
  free.state.letters.join(', ') || 'писем нет');
check('запуск без паник', free.panics.length === 0, free.panics[0] ?? 'паник нет');
await free.ctx.close();

console.log('\n── с куратором: то же состояние, цикл обязан промолчать ──');
const bound = await run(b, server, { withCurator: true });
console.log(`   планка ${OLD_PLANKA} → ${bound.state.effective}; якорь ${bound.state.anchor}`);
check('планка не изменилась', bound.state.effective === OLD_PLANKA,
  `${OLD_PLANKA} → ${bound.state.effective}`);
check('в историю ничего не дописано', bound.state.calorieDays.length === 1,
  bound.state.calorieDays.join(', '));
// Якорь стоит намеренно: человек ждал ровно столько, сколько ждал, и после
// отвязки неделя не должна начинаться заново.
check('якорь не сдвинут', bound.state.anchor === ymd(10), `${bound.state.anchor} (ожидали ${ymd(10)})`);
check('письма о недельной планке нет', !bound.state.letters.some((id) => id.startsWith('planka-')),
  bound.state.letters.join(', ') || 'писем нет');
check('отвязку не отработали по ошибке',
  !bound.state.letters.some((id) => id.startsWith('curator-unbound')),
  bound.state.letters.join(', ') || 'писем нет');
check('запуск без паник', bound.panics.length === 0, bound.panics[0] ?? 'паник нет');
// Журнал прокси — на случай разбора: видно, с чем приложение ходило на сервер.
if (process.env.VERBOSE) {
  console.log('   запросы:', server.calls.map((c) => `${c.status} ${c.method} ${c.worker}${c.path}`).join('\n             '));
}

console.log('\n── куратор ставит планку: она обязана примениться ──');
const NEW_PLANKA = 1850;
const dir = await api(bound.bond.curator, 'POST', `/curator/clients/${bound.bond.clientId}/reply`, {
  client_id: uuid(), text: '', kind: 'set_planka_v2',
  payload: JSON.stringify({ key: 'calories', amount: NEW_PLANKA }),
});
check('директива принята воркером', dir.status === 200, `${dir.status} ${dir.text ?? ''}`);

// Приложение опрашивает тред при ЗАПУСКЕ и при возврате из фона, а не
// непрерывно: директиву человек получает пушем и открывает приложение. Проверка
// повторяет именно это, а не ждёт у моря погоды.
await bound.page.reload({ waitUntil: 'domcontentloaded' });
const applied = await waitFor(bound.page, 30, async () => {
  const v = (await bound.page.evaluate(readState)).effective;
  return v === NEW_PLANKA ? v : null;
});
const after = await bound.page.evaluate(readState);
console.log(`   планка ${OLD_PLANKA} → ${after.effective}; дни в истории: ${after.calorieDays.join(', ')}`);
check('кураторская планка применилась', after.effective === NEW_PLANKA,
  `${after.effective} (ожидали ${NEW_PLANKA})`);
check('запись легла в историю сегодняшним днём', after.calorieDays.includes(ymd(0)),
  after.calorieDays.join(', '));
check('письмо о правке пришло', after.letters.some((id) => id.startsWith('curator-planka-')),
  after.letters.join(', ') || 'писем нет');

console.log('\n── куратор запрашивает данные: запрос обязан доехать ──');
const req = await api(bound.bond.curator, 'POST', `/curator/clients/${bound.bond.clientId}/request`,
  { client_id: uuid(), scope: 'all' });
check('запрос принят воркером', req.status === 200, `${req.status} ${req.text ?? ''}`);
await bound.page.reload({ waitUntil: 'domcontentloaded' });
const seen = await waitFor(bound.page, 30, async () => {
  return await bound.page.evaluate(async () => {
    const uid = localStorage.getItem('user_id');
    const db = await new Promise((res, rej) => {
      const q = indexedDB.open(`hjkl-ft-${uid}`);
      q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
    });
    const rows = await new Promise((res) => {
      const rq = db.transaction(['support_msgs'], 'readonly').objectStore('support_msgs').getAll();
      rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
    });
    db.close();
    const m = rows.filter((r) => r.kind === 'data_request').at(-1);
    return m ? (JSON.parse(m.payload || '{}').scope ?? null) : null;
  });
});
check('запрос доехал до приложения худеющего', seen === 'all', `scope = ${seen}`);
await bound.ctx.close();

await b.close();
server.close();
console.log(fail === 0 ? '\n=== ALL OK ===' : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
