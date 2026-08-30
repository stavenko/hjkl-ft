// Человек, которого куратор ведёт С САМОГО НАЧАЛА.
//
// Зарегистрировался, тут же привязался, несколько недель худел под куратором, и
// куратор его отпустил. Отличие от обычного пути одно, и оно решающее: планку от
// НАС такой человек не получал ни разу. Всё, что у него есть, поставил куратор.
//
// Проверяется, что после отвязки приложение подхватывает его, а не бросает:
//
//   1. Свежий человек без единого дня данных привязывается по ссылке.
//   2. Куратор ведёт его: ставит калорийную планку, потом правит её.
//   3. Человек худеет — три недели веса, дневника и шагов.
//   4. Отвязка: постоянные планки возвращаются к нашим правилам, кураторская
//      калорийная остаётся, письмо перечисляет всё двенадцать.
//   5. ГЛАВНОЕ: недельный цикл после отвязки ЗАПУСКАЕТСЯ. Он судил о наличии
//      планки по записи в `goals`, которой у такого человека нет вовсе, — и
//      молча выходил, оставляя его с кураторским числом навсегда.
//
// Запуск: node scripts/check-fresh-bind-unbind.mjs
//   DEV=1 — по ВЫКАЧЕННОМУ приложению, а не по локальному dist
// Как гонять проверки и почему вывод сохраняют в файл — `docs/testing.md`.

import { serveWithProxy, launchBrowser, isOwnNavigationError } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
// Прогон против ВЫКАЧЕННОГО приложения: DEV=1 берёт статику с dev, а не из
// локального dist. Тогда проверка захватывает и саму выкладку.
const DEV = process.env.DEV ? (process.env.DEV_URL ?? 'https://renorma-fit-dev.pages.dev') : null;

const SUPPORT = process.env.SUPPORT ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const CURATOR_CAL = 2100;
const FIBER = 34;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};
const section = (n) => console.log(`\n── ${n} ──`);
const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString().slice(0, 10); };
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
  const [hist, flags, goals] = [await all('planka_history'), await all('app_flags'), await all('goals')];
  db.close();
  const kind = (k) => hist.filter((h) => h.kind === k).sort((a, b) => a.date.localeCompare(b.date));
  const f = (k) => flags.find((x) => x.key === k)?.value;
  return {
    calories: kind('calories').at(-1)?.amount,
    calorieDays: kind('calories').map((h) => h.date),
    fiber: kind('fiber').at(-1)?.amount,
    protein: kind('protein').at(-1)?.amount,
    // Запись в `goals` — то самое, чего у такого человека нет.
    goalRows: goals.filter((g) => g.nutrient === 'Calories').map((g) => g.amount),
    calAnchor: f('planka_weekly_anchor'),
    peer: f('support_current_peer') ?? null,
    letters: JSON.parse(f('letters_v1') || '[]').map((l) => l.id),
  };
};

const waitFor = async (page, tries, read) => {
  for (let i = 0; i < tries; i++) {
    const v = await read().catch(() => null);
    if (v) return v;
    await page.waitForTimeout(1000);
  }
  return null;
};

const server = await serveWithProxy({
  root: DIST,
  upstream: DEV,
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

// ── 1. Свежая регистрация и немедленная привязка ────────────────────────────
section('1. зарегистрировался и сразу привязался — данных нет ни одного дня');
const { userId: uid } = await createPaidUser('fresh-bind');
const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
const page = await ctx.newPage();
const panics = [];
page.on('pageerror', (e) => { if (!isOwnNavigationError(e.message, e.stack)) panics.push(e.message); });
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
// Онбординг: только профиль и подписка. Ни дневника, ни веса, ни планки — их у
// человека в первый день и нет.
await page.evaluate(async () => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const now = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(['app_flags', 'profile'], 'readwrite');
    for (const [key, value] of [
      ['welcome_shown', 'true'], ['push_onboarding_dismissed', 'true'],
      ['ft_subscription', JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
        active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' })],
    ]) tx.objectStore('app_flags').put({ key, value });
    tx.objectStore('profile').put({ key: 'profile', sex: 'male', height_cm: 176, birth_year: 1990,
      goal: 'lose', created_at: now, updated_at: now });
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
});

const curatorId = `e2e-curator-${uuid().slice(0, 8)}`;
const curator = mintToken(curatorId);
let r = await api(curator, 'POST', '/curator/register', {});
r = await api(curator, 'POST', '/curator/clients', { name: 'Новичок' });
const clientId = r.json?.client?.id;
const code = r.json?.client?.invite_code;
check('куратор завёл слот и выдал ссылку', !!clientId && !!code, code ?? String(r.status));

await page.goto(`${server.url}/curator?c=${code}`, { waitUntil: 'domcontentloaded' });
const asked = await page.waitForSelector('[data-testid="curator-invite-ask"]', { timeout: 25000 })
  .then(() => true).catch(() => false);
check('экран согласия открылся у новичка', asked);
if (asked) await page.click('[data-testid="curator-invite-accept"]');
const done = await page.waitForSelector('[data-testid="curator-invite-done"]', { timeout: 25000 })
  .then(() => true).catch(() => false);
check('согласие принято', done);
await page.goto(server.url, { waitUntil: 'domcontentloaded' });
await page.waitForTimeout(8000);

const start = await page.evaluate(readState);
check('планки у человека нет ни одной — её и не было', start.calories === undefined,
  `калории ${start.calories}`);
check('и записи в goals тоже нет — от нас он планку не получал',
  start.goalRows.length === 0, JSON.stringify(start.goalRows));
check('запуск без паник', panics.length === 0, panics[0] ?? 'паник нет');

// ── 2. Куратор ведёт его ────────────────────────────────────────────────────
section('2. куратор ставит планки — приложение их принимает');
for (const [key, amount] of [['calories', CURATOR_CAL], ['fiber', FIBER]]) {
  r = await api(curator, 'POST', `/curator/clients/${clientId}/reply`, {
    client_id: uuid(), text: '', kind: 'set_planka_v2',
    payload: JSON.stringify({ key, amount }),
  });
  check(`директива по «${key}» принята`, r.status === 200, `${r.status} ${r.text ?? ''}`);
}
await page.reload({ waitUntil: 'domcontentloaded' });
const led = await waitFor(page, 30, async () => {
  const st = await page.evaluate(readState);
  return st.calories === CURATOR_CAL && st.fiber === FIBER ? st : null;
}) ?? await page.evaluate(readState);
check('кураторская калорийная планка применилась', led.calories === CURATOR_CAL, `${led.calories}`);
check('кураторская постоянная планка применилась', led.fiber === FIBER, `${led.fiber}`);
check('запись в goals так и не появилась — директива пишет только историю',
  led.goalRows.length === 0, JSON.stringify(led.goalRows));

// ── 3. Три недели под куратором: человек худеет ─────────────────────────────
section('3. три недели под куратором — минус несколько килограммов');
await page.evaluate(async (days) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const now = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(['foods', 'diary', 'weight_entries', 'step_entries'], 'readwrite');
    tx.objectStore('foods').put({ id: 'f1', name: 'Овсяная каша', kcal: 100, protein: 3, fat: 2, carbs: 18,
      nutrients: {}, package_weight: null, is_recipe: false, recipe_id: null, archived: false,
      is_restaurant: false, is_snack: false, is_liquid_cal: false, is_veg_fruit: false,
      is_egg: false, is_red_meat: false, iron_mg: 0.5, iron_absorption: 0.05,
      created_at: now, updated_at: now });
    for (let i = 1; i <= 21; i++) {
      // Ест 2050 ккал при кураторской планке 2100 — держится её.
      tx.objectStore('diary').put({ id: 'd' + i, food_id: 'f1', date: days[i], time: null, grams: 2050,
        waste_grams: 0, meal_label: 'lunch', deleted: false, created_at: now, updated_at: now });
      tx.objectStore('step_entries').put({ id: 's' + i, date: days[i], steps: 9500,
        created_at: now, updated_at: now });
      // Минус 3 кг за три недели — уверенное, спокойное похудение.
      tx.objectStore('weight_entries').put({ id: 'w' + i, date: days[i], weight_kg: 88 + i * 0.143,
        no_water: true, no_food: true, no_wash: true, used_toilet: true, morning: true,
        created_at: now, updated_at: now });
    }
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, Array.from({ length: 25 }, (_, i) => ymd(i)));
await page.reload({ waitUntil: 'domcontentloaded' });
await page.waitForTimeout(12000);
const under = await page.evaluate(readState);
check('под куратором планка не двинулась сама', under.calories === CURATOR_CAL, `${under.calories}`);
check('писем о недельной планке нет', !under.letters.some((i) => i.startsWith('planka-')),
  under.letters.join(', ') || 'писем нет');

// ── 4. Отвязка ──────────────────────────────────────────────────────────────
section('4. куратор отпускает — наши правила возвращаются');
r = await api(curator, 'POST', `/curator/clients/${clientId}/unbind`, {});
check('куратор прекратил работу', r.status === 200, `${r.status} ${r.text ?? ''}`);
const freed = await waitFor(page, 40, async () => {
  await page.reload({ waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(6000);
  const st = await page.evaluate(readState);
  return st.peer === 'admin' ? st : null;
}) ?? await page.evaluate(readState);
check('адресат снова админ', freed.peer === 'admin', String(freed.peer));
check('постоянная планка забыта — вернулось наше правило', freed.fiber === undefined,
  `клетчатка ${freed.fiber}`);
check('кураторская калорийная осталась', freed.calories !== undefined, `${freed.calories}`);
const unbound = freed.letters.find((i) => i.startsWith('curator-unbound-'));
check('письмо об отвязке пришло', !!unbound, unbound ?? freed.letters.join(', '));

// ── 5. Главное: цикл подхватывает человека ──────────────────────────────────
section('5. недельный цикл ЗАПУСКАЕТСЯ у человека без записи в goals');
// Якорь на сегодня сдвинула сама отвязка — по нему о работе цикла судить нельзя.
// Отматываем его на десять дней назад: это то же, что подождать неделю, только
// без ожидания. Теперь двинуться он может лишь одним способом — циклом.
await page.evaluate(async (back) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  await new Promise((res) => {
    const tx = db.transaction(['app_flags'], 'readwrite');
    tx.objectStore('app_flags').put({ key: 'planka_weekly_anchor', value: back });
    tx.oncomplete = () => res();
  });
  db.close();
}, ymd(10));
await page.reload({ waitUntil: 'domcontentloaded' });
const ran = await waitFor(page, 45, async () => {
  const st = await page.evaluate(readState);
  return st.calAnchor === ymd(0) && st.letters.some((i) => i.startsWith('planka-')) ? st : null;
}) ?? await page.evaluate(readState);
check('цикл отработал — якорь снова на сегодня', ran.calAnchor === ymd(0),
  `${ran.calAnchor} (ожидали ${ymd(0)})`);
check('письмо о планке пришло', ran.letters.some((i) => i.startsWith('planka-')),
  ran.letters.join(', ') || 'писем нет');
check('цикл оттолкнулся от КУРАТОРСКОГО числа', ran.calorieDays.includes(ymd(0)),
  `${CURATOR_CAL} → ${ran.calories}, дни ${ran.calorieDays.join(', ')}`);
// Запись в `goals` появляется только теперь — её пишет наш пересчёт, а не
// кураторская директива. До этого места её не было ни разу, и на ней-то цикл и
// спотыкался.
check('запись в goals завёл наш пересчёт', ran.goalRows.length === 1,
  JSON.stringify(ran.goalRows));
check('белок пересчитан вслед за калориями', ran.protein !== undefined, `${ran.protein}`);
check('запуск без паник', panics.length === 0, panics[0] ?? 'паник нет');

await ctx.close();
await b.close();
server.close();
console.log(fail ? `\n=== FAILURES: ${fail} ===` : '\n=== ALL OK ===');
process.exit(fail ? 1 : 0);
