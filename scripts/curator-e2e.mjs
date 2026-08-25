// Сквозная проверка кураторского пути — по ручкам живого support-worker, без
// браузера. Проверяется то, что нельзя проверить юнит-тестом: договор между
// приложениями.
//
// Токены минтуются тем же секретом, что у dev-воркера, — как в остальных
// скриптах этой папки (см. admin-approval-e2e.mjs). Паскей здесь не нужен:
// проверяется НЕ вход, а всё, что происходит после него.
//
// Шаги:
//   1. куратор регистрируется свободно, без чьего-либо одобрения
//   2. заводит клиента и получает пригласительный код
//   3. худеющий открывает приглашение и соглашается — код гаснет
//   4. сообщения худеющего уходят КУРАТОРУ, а не в очередь админа
//   5. куратор просит данные за срок; худеющий шлёт отчёт; отчёт лёг в слот
//   6. куратор правит планку с запретом пересчёта — директива в треде
//   7. второй куратор перехватывает клиента: прежняя связь рвётся сама
//   8. отвязка: слот остаётся, ссылка новая
//
// Запуск: node scripts/curator-e2e.mjs
//   SUPPORT_BASE_URL, SUPPORT_JWT_SECRET (значения dev по умолчанию)

import { createHmac, randomUUID } from 'node:crypto';

const SUPPORT = process.env.SUPPORT_BASE_URL
  ?? 'https://support-worker-dev.vg-stavenko.workers.dev';
const JWT_SECRET = process.env.SUPPORT_JWT_SECRET ?? 'dev-secret-change-in-production';

const b64 = (b) => Buffer.from(b).toString('base64url');
function mint(sub) {
  const h = b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const c = b64(JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: 'e2e' }));
  const si = `${h}.${c}`;
  return `${si}.${b64(createHmac('sha256', JWT_SECRET).update(si).digest())}`;
}

function assert(cond, msg) {
  if (!cond) throw new Error(`ПРОВАЛ: ${msg}`);
}

async function call(token, method, path, body) {
  const res = await fetch(`${SUPPORT}${path}`, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    ...(body ? { body: JSON.stringify(body) } : {}),
  });
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* не JSON — вернём как есть */ }
  return { status: res.status, json, text };
}

async function main() {
  const run = randomUUID().slice(0, 8);
  const curator = mint(`e2e-curator-${run}`);
  const other = mint(`e2e-curator2-${run}`);
  const user = mint(`e2e-user-${run}`);

  // ── 1. Свободная регистрация ──
  let r = await call(curator, 'POST', '/curator/register', {});
  assert(r.status === 200, `регистрация куратора: ${r.status} ${r.text}`);
  assert(r.json.curator?.curator_id, 'регистрация не вернула профиль');
  // Идемпотентность: второй вызов не заводит второго куратора.
  const again = await call(curator, 'POST', '/curator/register', {});
  assert(again.json.created === false, 'повторная регистрация завела второй профиль');

  r = await call(curator, 'POST', '/curator/me', { name: `Куратор ${run}`, lang: 'ru' });
  assert(r.status === 200, `имя куратора: ${r.status} ${r.text}`);

  // Токен без профиля кураторские ручки НЕ открывает.
  r = await call(user, 'GET', '/curator/clients');
  assert(r.status === 403, `чужой токен получил список клиентов: ${r.status}`);

  // ── 2. Клиент и приглашение ──
  r = await call(curator, 'POST', '/curator/clients', { name: 'Тестовый клиент' });
  assert(r.status === 200, `создание клиента: ${r.status} ${r.text}`);
  const cid = r.json.client.id;
  const code = r.json.client.invite_code;
  assert(code, 'непривязанный слот обязан отдавать код');

  // Чужой клиент не находится — как несуществующий.
  r = await call(other, 'POST', '/curator/register', {});
  assert(r.status === 200, 'регистрация второго куратора');
  r = await call(other, 'GET', `/curator/clients/${cid}/report`);
  assert(r.status === 404, `чужой клиент виден второму куратору: ${r.status}`);

  // ── 3. Согласие ──
  r = await call(user, 'GET', `/curator/invite/${code}`);
  assert(r.status === 200 && r.json.found, `приглашение не читается: ${r.text}`);
  assert(r.json.curator_name === `Куратор ${run}`, 'имя куратора не доехало до экрана согласия');

  r = await call(user, 'POST', `/curator/invite/${code}/accept`, {});
  assert(r.status === 200, `согласие: ${r.status} ${r.text}`);

  // Код гаснет СОГЛАСИЕМ: второй раз он не работает.
  r = await call(user, 'POST', `/curator/invite/${code}/accept`, {});
  assert(r.status === 409, `погашенный код сработал второй раз: ${r.status}`);

  r = await call(user, 'GET', '/curator/binding');
  assert(r.json.bound === true, 'привязка не видна приложению худеющего');

  // ── 4. Сообщения идут КУРАТОРУ ──
  r = await call(user, 'POST', '/message', { client_id: randomUUID(), text: 'привет' });
  assert(r.status === 200, `отправка: ${r.status} ${r.text}`);
  assert(r.json.peer?.startsWith('curator:'), `сообщение ушло не куратору: ${r.json.peer}`);

  r = await call(curator, 'GET', `/curator/clients/${cid}/messages?after_seq=0&limit=50`);
  assert(r.status === 200, `чтение треда: ${r.status}`);
  assert(r.json.messages.some((m) => m.text === 'привет'), 'куратор не видит сообщение клиента');

  // ── 5. Запрос данных и отчёт ──
  r = await call(curator, 'POST', `/curator/clients/${cid}/request`,
    { client_id: randomUUID(), days: 21 });
  assert(r.status === 200, `запрос данных: ${r.status} ${r.text}`);

  r = await call(user, 'GET', '/messages?after_seq=0&limit=50');
  const req = r.json.messages.find((m) => m.kind === 'data_request');
  assert(req, 'запрос не дошёл до худеющего');
  assert(JSON.parse(req.payload).days === 21, 'срок запроса потерялся');

  const report = JSON.stringify({ report: { period: { from: 'a', to: 'b', days: 21 } } });
  r = await call(user, 'POST', '/message',
    { client_id: randomUUID(), text: 'Отчёт отправлен', kind: 'data_share', payload: report });
  assert(r.status === 200, `отчёт: ${r.status} ${r.text}`);

  r = await call(curator, 'GET', `/curator/clients/${cid}/report`);
  assert(r.json.report, 'отчёт не лёг в слот');
  assert(!r.json.request_days, 'выполненный запрос пережил свой ответ');

  // ── 6. Правка планки ──
  r = await call(curator, 'POST', `/curator/clients/${cid}/reply`, {
    client_id: randomUUID(),
    text: '',
    kind: 'set_planka_v2',
    payload: JSON.stringify({ key: 'fiber', amount: 32, locked: true }),
  });
  assert(r.status === 200, `правка планки: ${r.status} ${r.text}`);

  r = await call(user, 'GET', '/messages?after_seq=0&limit=50');
  const directive = r.json.messages.find((m) => m.kind === 'set_planka_v2');
  assert(directive, 'директива не дошла');
  const d = JSON.parse(directive.payload);
  assert(d.key === 'fiber' && d.amount === 32 && d.locked === true, 'директива приехала искажённой');
  assert(directive.sender_name === `Куратор ${run}`, 'подпись куратора не доехала');

  // ── 7. Перехват другим куратором ──
  r = await call(other, 'POST', '/curator/clients', { name: 'Тот же человек' });
  const cid2 = r.json.client.id;
  const code2 = r.json.client.invite_code;
  r = await call(user, 'POST', `/curator/invite/${code2}/accept`, {});
  assert(r.status === 200, `переход к другому куратору: ${r.status} ${r.text}`);
  assert(r.json.previous?.client_id === cid, 'прежняя связь не названа в ответе');

  // У прежнего куратора слот остался — и снова с кодом.
  r = await call(curator, 'GET', '/curator/clients');
  const old = r.json.clients.find((c) => c.id === cid);
  assert(old && !old.bound, 'слот прежнего куратора остался привязанным');
  assert(old.invite_code && old.invite_code !== code, 'прежний слот не получил новую ссылку');

  // ── 8. Отвязка по воле человека ──
  r = await call(user, 'POST', '/curator/unbind', {});
  assert(r.status === 200, `отвязка: ${r.status} ${r.text}`);
  r = await call(user, 'GET', '/curator/binding');
  assert(r.json.bound === false, 'привязка пережила отвязку');

  // Без куратора сообщения снова идут админу.
  r = await call(user, 'POST', '/message', { client_id: randomUUID(), text: 'снова в поддержку' });
  assert(r.json.peer === 'admin', `без куратора сообщение ушло не админу: ${r.json.peer}`);

  console.log('curator-e2e: все проверки пройдены');
}

main().catch((e) => {
  console.error(e.message);
  process.exit(1);
});
