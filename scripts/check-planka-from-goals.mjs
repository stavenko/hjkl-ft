// Планка, оставшаяся в СТАРЫХ целях, переезжает в историю.
//
// Планка живёт в истории; запись в `goals` была её зеркалом и осталась у тех, чья
// планка старше самой истории. Приложение перестало это зеркало читать — и такой
// человек остался бы без планки вовсе. Хуже: приложение решило бы, что планку
// пора поставить ВПЕРВЫЕ, и заменило бы его число свежерассчитанным.
//
// Проверяется миграция `m025_planka_from_goals` на живом приложении: сеется
// человек со старой целью и ПУСТОЙ историей — как после переустановки на
// сборке, где истории ещё не было.
//
// Запуск: node scripts/check-planka-from-goals.mjs
//   DEV=1 — по ВЫКАЧЕННОМУ приложению, а не по локальному dist
// Как гонять проверки и почему вывод сохраняют в файл — `docs/testing.md`.

import { serveWithProxy, launchBrowser, isOwnNavigationError } from './lib/devserver.mjs';
import { createPaidUser, mintToken } from './lib/devuser.mjs';

const DIST = process.env.FE ?? new URL('../frontend/dist', import.meta.url).pathname;
const DEV = process.env.DEV ? (process.env.DEV_URL ?? 'https://renorma-fit-dev.pages.dev') : null;
const OLD_PLANKA = 2350;

let fail = 0;
const check = (n, ok, extra = '') => {
  console.log(`${ok ? 'OK  ' : 'FAIL'} ${n}${extra ? ' — ' + extra : ''}`);
  if (!ok) fail++;
};
const ymd = (b) => { const d = new Date(); d.setDate(d.getDate() - b); return d.toISOString().slice(0, 10); };

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
const b = await launchBrowser();
const { userId: uid } = await createPaidUser('goals-migr');
const ctx = await b.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block' });
const page = await ctx.newPage();
const panics = [];
page.on('pageerror', (e) => { if (!isOwnNavigationError(e.message, e.stack)) panics.push(e.message); });

await page.goto(server.url, { waitUntil: 'domcontentloaded' });
await page.evaluate(({ uid, token }) => {
  localStorage.clear();
  localStorage.setItem('user_id', uid);
  localStorage.setItem('auth_token', token);
  localStorage.setItem('pwa_dismissed', 'true');
}, { uid, token: mintToken(uid) });
await page.goto(server.url, { waitUntil: 'domcontentloaded' });
await page.waitForTimeout(6000);

// Старый человек: цель есть, истории НЕТ. Версия базы — до 25, чтобы миграция пошла.
await page.evaluate(async (arg) => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const now = new Date().toISOString();
  await new Promise((res, rej) => {
    const tx = db.transaction(['app_flags', 'profile', 'goals', 'planka_history'], 'readwrite');
    for (const [key, value] of [
      ['db_schema_version', '24'], ['welcome_shown', 'true'], ['push_onboarding_dismissed', 'true'],
      ['ft_subscription', JSON.stringify({ plan: 'monthly', end: Date.now() + 30 * 864e5,
        active: true, start: Date.now(), status: 'paid', no_renew: false, provider: 'lava' })],
    ]) tx.objectStore('app_flags').put({ key, value });
    tx.objectStore('profile').put({ key: 'profile', sex: 'male', height_cm: 180, birth_year: 1985,
      goal: 'lose', created_at: now, updated_at: now });
    tx.objectStore('goals').put({ id: 'g-cal', nutrient: 'Calories', key: 'calories',
      direction: 'AtMost', amount: arg.planka, unit: 'Kcal', period: 'Day',
      created_at: arg.made + 'T09:00:00Z', updated_at: now });
    for (const r of []) tx.objectStore('planka_history').put(r);
    tx.oncomplete = () => res(); tx.onerror = () => rej(tx.error);
  });
  db.close();
}, { planka: OLD_PLANKA, made: ymd(30) });

const read = async () => {
  const uid = localStorage.getItem('user_id');
  const db = await new Promise((r) => { const q = indexedDB.open(`hjkl-ft-${uid}`); q.onsuccess = () => r(q.result); });
  const all = (s) => new Promise((res) => {
    const rq = db.transaction([s], 'readonly').objectStore(s).getAll();
    rq.onsuccess = () => res(rq.result); rq.onerror = () => res([]);
  });
  const [hist, flags] = [await all('planka_history'), await all('app_flags')];
  db.close();
  const cal = hist.filter((h) => h.kind === 'calories').sort((a, b) => a.date.localeCompare(b.date));
  return {
    amount: cal.at(-1)?.amount,
    from: cal[0]?.date,
    ver: flags.find((f) => f.key === 'db_schema_version')?.value,
  };
};

await page.reload({ waitUntil: 'domcontentloaded' });
let st = { amount: undefined };
for (let i = 0; i < 30 && st.amount === undefined; i++) {
  await page.waitForTimeout(1000);
  st = await page.evaluate(read).catch(() => ({}));
}
check('миграция прошла', st.ver === '25', `версия ${st.ver}`);
check('планка из старой цели попала в историю', st.amount === OLD_PLANKA,
  `${st.amount} (ожидали ${OLD_PLANKA})`);
check('датирована днём создания цели', st.from === ymd(30), `${st.from} (ожидали ${ymd(30)})`);

// И главное: приложение не решило, что планку пора ставить впервые.
await page.reload({ waitUntil: 'domcontentloaded' });
await page.waitForTimeout(8000);
const after = await page.evaluate(read);
check('планку не переписали своей', after.amount === OLD_PLANKA, `${after.amount}`);
check('запуск без паник', panics.length === 0, panics[0] ?? 'паник нет');

await ctx.close(); await b.close(); server.close();
console.log(fail ? `\n=== FAILURES: ${fail} ===` : '\n=== ALL OK ===');
process.exit(fail ? 1 : 0);
