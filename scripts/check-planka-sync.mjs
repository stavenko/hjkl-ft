// Планки на ДВУХ устройствах одного человека.
//
// Планка — это то, по чему человека судят индикаторы. Если два его телефона
// разошлись в ней, они судят его по разным правилам, и он видит два разных
// ответа на один и тот же вопрос. `check-syncv2.mjs` гоняет два устройства, но
// только по дневнику: хранилище планок не проверял никто.
//
// Проверяется три утверждения:
//
//   1. Планка, поставленная на A, доезжает до B.
//   2. Кураторская планка — тоже: её ставит не приложение, а директива, и
//      второй телефон обязан узнать о ней тем же путём.
//   3. ЗАБЫВАНИЕ при отвязке доезжает до B. Это главное. Постоянную планку
//      куратора отвязка стирает, чтобы вернулось наше правило. Если отметка об
//      удалении не доедет, на втором телефоне кураторское число останется
//      навсегда — и человек будет судиться по правилу куратора, которого у него
//      больше нет.
//
// Запуск: node scripts/check-planka-sync.mjs
//   FE — каталог собранного фронтенда (по умолчанию ../frontend/dist)

import { serveWithProxy, launchBrowser } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
const SUPPORT = process.env.SUPPORT ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const CAL = 2500;
const FIBER = 32;

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

/// Что устройство знает о планках прямо сейчас.
const readPlankas = async () => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((res, rej) => {
    const q = indexedDB.open(`hjkl-ft-${uid}`);
    q.onsuccess = () => res(q.result); q.onerror = () => rej(q.error);
  });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], 'readonly').objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const [hist, flags] = [await all('planka_history'), await all('app_flags')];
  db.close();
  const last = (k) => hist.filter((h) => h.kind === k)
    .sort((a, b) => a.date.localeCompare(b.date)).at(-1)?.amount;

  return {
    ids: hist.map((h) => h.id).sort(),
    calories: last('calories'),
    steps: last('steps'),
    fiber: last('fiber'),
    peer: flags.find((f) => f.key === 'support_current_peer')?.value ?? null,
  };
};

/// Дождаться состояния на устройстве, перезагружая его: пул — событие запуска, а
/// не непрерывный поток. Ровно так же ждёт `check-syncv2.mjs`.
const settle = async (page, want, tries = 6) => {
  for (let i = 0; i < tries; i++) {
    const st = await page.evaluate(readPlankas).catch(() => ({}));
    if (want(st)) return st;
    await page.reload({ waitUntil: 'domcontentloaded' });
    await page.waitForTimeout(8000);
  }
  return await page.evaluate(readPlankas).catch(() => ({}));
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
console.log(`приложение на ${server.url}, воркеры через прокси`);
const b = await launchBrowser();

const { userId: uid } = await createPaidUser('planka-sync');
const token = mintToken(uid);

/// Устройство того же человека: своя вкладка, свой профиль браузера, тот же счёт.
const device = async (name) => {
  const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
  const page = await ctx.newPage();
  page.on('console', (m) => { if (/panicked at/.test(m.text())) console.log(`[${name}] ${m.text().slice(0, 160)}`); });
  await page.goto(server.url, { waitUntil: 'domcontentloaded' });
  await page.evaluate(({ uid, token }) => {
    localStorage.clear();
    localStorage.setItem('user_id', uid);
    localStorage.setItem('auth_token', token);
    localStorage.setItem('pwa_dismissed', 'true');
  }, { uid, token });
  await page.goto(server.url, { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(6000);
  return { ctx, page };
};

// ── 1. Планка, поставленную ПРИЛОЖЕНИЕМ, узнаёт второй телефон ─────────────
section('1. планка с первого телефона доезжает до второго');
const A = await device('A');
// Планку ставит САМО приложение — недельным циклом, ради которого якорь и
// отматывается на десять дней назад. Подложить готовую строку в базу было бы
// проще, но такая запись минует очередь отправки: синку о ней знать неоткуда, и
// проверка сверяла бы посев с посевом.
await A.page.evaluate(async (arg) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const now = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(['app_flags', 'profile', 'goals', 'planka_history',
      'foods', 'diary', 'step_entries', 'weight_entries', '_outbox'], 'readwrite');
    const f = tx.objectStore('app_flags');
    for (const [key, value] of [
      ['welcome_shown', 'true'], ['push_onboarding_dismissed', 'true'],
      ['planka_weekly_anchor', arg.day10], ['steps_planka_weekly_anchor', arg.day10],
      ['steps_gate_opened_at', arg.days[30]], ['activity_week_unlocked', 'true'],
      ['ft_subscription', JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
        active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' })],
    ]) f.put({ key, value });
    tx.objectStore('profile').put({ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1985,
      goal: 'lose', steps_planka: 9000, created_at: now, updated_at: now });
    tx.objectStore('goals').put({ id: 'g-cal', nutrient: 'Calories', key: 'calories', direction: 'AtMost',
      amount: arg.cal, unit: 'Kcal', period: 'Day', created_at: arg.iso10, updated_at: arg.iso10 });
    tx.objectStore('planka_history').put({ id: `calories:${arg.day10}`, kind: 'calories', date: arg.day10,
      amount: arg.cal, created_at: arg.iso10, updated_at: arg.iso10 });
    tx.objectStore('planka_history').put({ id: `steps:${arg.day10}`, kind: 'steps', date: arg.day10,
      amount: 9000, created_at: arg.iso10, updated_at: arg.iso10 });
    // Событие отправки к каждой посеянной строке. Прямая запись в базу минует
    // очередь, и синк о ней не узнаёт — а нам нужно, чтобы ДО начала проверки
    // оба телефона стояли на одном и том же.
    for (const id of [`calories:${arg.day10}`, `steps:${arg.day10}`]) {
      tx.objectStore('_outbox').put({
        seq: String(Date.now() * 1000 + Math.floor(Math.random() * 900)).padStart(20, '0'),
        store: 'planka_history', op: 'upsert', id, ts: Date.now(),
      });
    }
    tx.objectStore('foods').put({ id: 'f1', name: 'Овсяная каша', kcal: 100, protein: 3, fat: 2, carbs: 18,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
      created_at: now, updated_at: now });
    for (let i = 1; i <= 12; i++) {
      tx.objectStore('diary').put({ id: 'd' + i, food_id: 'f1', date: arg.days[i], time: null, grams: 2000,
        waste_grams: 0, meal_label: 'lunch', deleted: false, created_at: now, updated_at: now });
      tx.objectStore('step_entries').put({ id: 's' + i, date: arg.days[i], steps: 10500,
        created_at: now, updated_at: now });
    }
    for (let i = 0; i < 21; i++) {
      tx.objectStore('weight_entries').put({ id: 'w' + i, date: arg.days[i], weight_kg: 90 + i * 0.14,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: now, updated_at: now });
    }
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { day10: ymd(10), iso10: iso(10), cal: CAL, days: Array.from({ length: 25 }, (_, i) => ymd(i)) });
await A.page.reload({ waitUntil: 'domcontentloaded' });
// Двигается ШАГОВАЯ планка: калорийную цикл вправе и оставить (недоедающему её
// не поднимают), а шаги при перевыполнении поднимает — на них и смотрим.
const a1 = await settle(A.page, (s) => s.ids.includes(`steps:${ymd(0)}`));
check('первый телефон сам поднял шаговую планку', a1.steps > 9000, `9000 → ${a1.steps}`);

const B = await device('B');
const b1 = await settle(B.page, (s) => s.steps === a1.steps && s.calories === a1.calories);
check('второй телефон увидел планку первого', b1.steps === a1.steps && b1.calories === a1.calories,
  `A шаги ${a1.steps}, калории ${a1.calories} / B шаги ${b1.steps}, калории ${b1.calories}`);

// ── 2. Кураторская планка тоже доезжает ─────────────────────────────────────
section('2. кураторская планка доезжает до второго телефона');
const curatorId = `e2e-curator-${uuid().slice(0, 8)}`;
const curator = mintToken(curatorId);
let r = await api(curator, 'POST', '/curator/register', {});
check('куратор зарегистрировался', r.status === 200, `${r.status}`);
r = await api(curator, 'POST', '/curator/clients', { name: 'Двухтелефонный клиент' });
const clientId = r.json?.client?.id;
const code = r.json?.client?.invite_code;
check('слот клиента заведён', !!clientId && !!code, code ?? String(r.status));

await A.page.goto(`${server.url}/curator?c=${code}`, { waitUntil: 'domcontentloaded' });
const askShown = await A.page.waitForSelector('[data-testid="curator-invite-ask"]', { timeout: 25000 })
  .then(() => true).catch(() => false);
if (askShown) await A.page.click('[data-testid="curator-invite-accept"]');
const done = await A.page.waitForSelector('[data-testid="curator-invite-done"]', { timeout: 25000 })
  .then(() => true).catch(() => false);
check('первый телефон принял приглашение', done);
await A.page.goto(server.url, { waitUntil: 'domcontentloaded' });

r = await api(curator, 'POST', `/curator/clients/${clientId}/reply`, {
  client_id: uuid(), text: '', kind: 'set_planka_v2',
  payload: JSON.stringify({ key: 'fiber', amount: FIBER }),
});
check('директива по клетчатке принята', r.status === 200, `${r.status} ${r.text ?? ''}`);
const a2 = await settle(A.page, (s) => s.fiber === FIBER);
check('первый телефон применил кураторскую планку', a2.fiber === FIBER, `${a2.fiber}`);
const b2 = await settle(B.page, (s) => s.fiber === FIBER);
check('второй телефон узнал о кураторской планке', b2.fiber === FIBER, `${b2.fiber}`);

// ── 3. Забывание при отвязке доезжает до B ──────────────────────────────────
section('3. отвязка стирает кураторскую планку НА ОБОИХ');
r = await api(curator, 'POST', `/curator/clients/${clientId}/unbind`, {});
check('куратор прекратил работу', r.status === 200, `${r.status} ${r.text ?? ''}`);
const a3 = await settle(A.page, (s) => s.peer === 'admin' && s.fiber === undefined);
check('первый телефон вернулся к админу', a3.peer === 'admin', String(a3.peer));
check('первый телефон забыл кураторскую планку', a3.fiber === undefined, `клетчатка ${a3.fiber}`);
// Главное утверждение: удаление — не локальное событие. Второй телефон о нём
// узнаёт тем же путём, что и о самой планке.
const b3 = await settle(B.page, (s) => s.fiber === undefined);
check('второй телефон тоже забыл её', b3.fiber === undefined, `клетчатка ${b3.fiber}`);
check('подвижная планка на обоих осталась', a3.calories === b3.calories && b3.calories !== undefined,
  `A ${a3.calories} / B ${b3.calories}`);
check('перечни записей на обоих совпали', JSON.stringify(a3.ids) === JSON.stringify(b3.ids),
  `A [${a3.ids}] / B [${b3.ids}]`);

await A.ctx.close(); await B.ctx.close();
await b.close();
server.close();
console.log(fail ? `\n=== FAILURES: ${fail} ===` : '\n=== ALL OK ===');
process.exit(fail ? 1 : 0);
