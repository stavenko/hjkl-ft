// Прогнать РЕГРЕССИЮ — накопленные проверки репозитория — по одному приложению.
//
// Каждая из них умеет `FE=<адрес>`, но в этом окружении браузер наружу не ходит,
// а живой адрес отдаёт настоящие адреса воркеров, до которых он не дотянется.
// Поэтому здесь поднимается ОДИН сервер (`lib/devserver.mjs`) и адрес его
// подставляется всем: статика — с dev (или из локального `dist`), обращения к
// воркерам — через прокси.
//
// Запуск:
//   node scripts/run-regression.mjs              — по локальной сборке
//   DEV=1 node scripts/run-regression.mjs        — по выкаченному приложению
//   node scripts/run-regression.mjs check-fiber  — только совпавшие по имени
//   JOBS=1 node scripts/run-regression.mjs       — по одной, как раньше
//   FAILED=regression-dev-….log node scripts/run-regression.mjs  — только упавшее
//
// По умолчанию проверки идут ЧЕТВЁРКАМИ: полный прогон занимает минуты, а не полчаса.
// Отдельная проверка — самый быстрый способ проверить починку: `run-regression.mjs
// iron-week` поднимает сервер и гоняет одну.
//
// Проверка считается пройденной по КОДУ ВОЗВРАТА: так они и написаны.
//
// Полный вывод пишется в `regression-local.log` (или `-dev`). НЕ пропускайте
// прогон через `| tail`: итог печатается перед подробностями, и хвост срежет
// именно его. Подробнее — `docs/testing.md`.

import { spawn } from 'node:child_process';
import { appendFileSync, existsSync, readdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { serveWithProxy } from './lib/devserver.mjs';

const DIST = process.env.FE_DIST ?? new URL('../frontend/dist', import.meta.url).pathname;
const DEV = process.env.DEV ? (process.env.DEV_URL ?? 'https://renorma-fit-dev.pages.dev') : null;
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS ?? 300_000);
/// Сколько проверок идёт одновременно. Четыре — по числу ядер; браузеры почти всё
/// время ждут, поэтому упираемся не в процессор.
const JOBS = Math.max(1, Number(process.env.JOBS ?? 4));
const filter = process.argv[2] ?? '';

// Полный лог пишется САМ, а не зависит от того, как вызвали команду: вывод у
// прогона длинный, итог печатается перед подробностями, и любой `| tail` его
// срезает.
//
// Имя со ВРЕМЕНЕМ, а не одно на все прогоны. С постоянным именем чужой (или свой
// прошлый) лог выглядит как результат текущего прогона: пока идёт сборка, в файле
// лежит вчерашний итог, и его успеваешь принять за сегодняшний. Метка времени
// снимает вопрос совсем — а `LOG=` остаётся, когда имя нужно назвать самому.
const stamp = new Date().toISOString().slice(0, 19).replace(/[-:]/g, '').replace('T', '-');
const LOG = process.env.LOG ?? `regression-${DEV ? 'dev' : 'local'}-${stamp}.log`;
writeFileSync(LOG, '');
const say = (line = '') => {
  console.log(line);
  appendFileSync(LOG, line + '\n');
};

// Свои четыре набора поднимают сервер сами — здесь они лишние. Остальные
// исключения названы поимённо: у каждой своя причина, и молчаливого пропуска
// быть не должно.
const SKIP = new Map([
  ['check-curator-flow.mjs', 'поднимает свой сервер (и второй, кураторский)'],
  ['check-fresh-bind-unbind.mjs', 'поднимает свой сервер'],
  ['check-planka-sync.mjs', 'поднимает свой сервер, два устройства'],
  ['check-diary-week.mjs', 'поднимает свой сервер'],
  ['check-planka-from-goals.mjs', 'поднимает свой сервер'],
  ['check-detections.mjs', 'ходит в ИИ-воркер: медленно и платно'],
  ['check-identity-cache.mjs', 'ходит в ИИ-воркер'],
  ['check-identity-fallback.mjs', 'ходит в ИИ-воркер'],
  ['check-iron-lookup.mjs', 'ходит в ИИ-воркер'],
  ['check-kbju-lookup.mjs', 'ходит в ИИ-воркер'],
  ['check-food-sweep.mjs', 'ходит в ИИ-воркер'],
  ['check-vision-direct.mjs', 'ходит в ИИ-воркер'],
  ['check-usage-by-model.mjs', 'читает статистику ИИ-воркера'],
  ['check-full-pass.mjs', 'ходит в ИИ-воркер по всему списку еды'],
  ['check-telemetry.mjs', 'e2e: читает аналитику по ключам из ../.env'],
  // Сверки со справочником: каждая гоняет по списку продуктов настоящую модель.
  ['verify-calcium.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  ['verify-fat.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  ['verify-iron.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  ['verify-iron-row.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  ['verify-iron-absorption.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  ['verify-macros.mjs', 'ходит в ИИ-воркер по всему справочнику'],
  // Кода возврата не ставит вовсе — по нему судить нельзя, а прогон судит по нему.
  ['verify-form.mjs', 'ничего не утверждает: смотрит форму глазами'],
  // Не приложение худеющего: другое происхождение, наш сервер им не подставить.
  ['check-admin-thread-card.mjs', 'проверяет АДМИНКУ'],
  ['check-admin-wipe-ui.mjs', 'проверяет АДМИНКУ'],
  ['verify-oldshare.mjs', 'проверяет АДМИНКУ'],
  // Сквозные: половина сценария играется в АДМИНКЕ (экспертом), а она на другом
  // происхождении — свой сервер ей не подставить.
  ['verify-food-share-indicators.mjs', 'вторую половину играет АДМИНКА'],
  ['verify-open-week-directive.mjs', 'вторую половину играет АДМИНКА'],
  ['verify-planka-directive.mjs', 'вторую половину играет АДМИНКА'],
  ['verify-share-all.mjs', 'вторую половину играет АДМИНКА'],
  ['verify-share-parity.mjs', 'вторую половину играет АДМИНКА'],
  ['check-landing-consent.mjs', 'проверяет ЛЕНДИНГ'],
  ['check-miniapp-app-link.mjs', 'проверяет мини-приложение Telegram'],
  ['check-miniapp-access-button.mjs', 'проверяет мини-приложение Telegram'],
  ['check-miniapp-consent.mjs', 'проверяет мини-приложение Telegram'],
  ['check-miniapp-dev-entry.mjs', 'проверяет мини-приложение Telegram'],
  ['check-smoke-front.mjs', 'ходит на отдельный смоук-стенд fit-smoke'],
  // Ключ-паспорт привязан к ПРОИСХОЖДЕНИЮ страницы: `rpId` выдаёт сервер по
  // домену, а браузер принимает его, только если он и правда домен страницы.
  // С нашего 127.0.0.1 церемония не начнётся ни при каком проксировании — это
  // и есть смысл привязки, а не помеха.
  ['check-deleted-key-login.mjs', 'ключ-паспорт привязан к домену: с 127.0.0.1 церемонии нет'],
  // Вшили адрес намертво — подставить свой нечем.
  ['check-chat-input.mjs', 'вшит адрес конкретной старой выкладки'],
  ['check-recipe-finalize.mjs', 'вшит адрес конкретной старой выкладки'],
  ['check-syncv2.mjs', 'вшит живой адрес, и по нему же блокирует синк'],
  ['check-syncv2-migration.mjs', 'вшит живой адрес'],
  ['check-user-reset.mjs', 'вшит живой адрес'],
  ['check-user-wipe.mjs', 'вшит живой адрес'],
]);

// Проверки, которым нужен САМ ВОРКЕР: `_worker.js` выкладки перехватывает навигацию
// по браузеру (Mi и Samsung уводит в Chrome) и раздаёт персональный манифест. Наш
// сервер его не исполняет — он раздаёт файлы, — поэтому по локальной сборке они
// падают не по делу. По выкладке (`DEV=1`) идут как все.
const WORKER_ONLY = [
  'check-app-link.mjs',
  'check-samsung-hop.mjs',
  'check-browser-screens.mjs',
];
if (!DEV) {
  for (const f of WORKER_ONLY) {
    SKIP.set(f, 'нужен `_worker.js` выкладки — гоняйте с DEV=1');
  }
}

// `ONLY` — именной список через запятую. Нужен ровно для одного дела: прогнать по
// СБОРКЕ main те проверки, что упали в ветке, и по разнице понять, что сломано
// здесь, а что приехало с той стороны. Гонять по main весь набор ради этого —
// впустую занятый час.
//
// `FAILED=<лог>` — то же самое, но список берётся из прошлого прогона: перегнать
// только упавшее. Это главный способ итерации — починил, перегнал десяток, а не
// полсотни.
const only = (process.env.ONLY ?? '').split(',').map((x) => x.trim()).filter(Boolean);
if (process.env.FAILED) {
  const log = readFileSync(process.env.FAILED, 'utf8');
  for (const m of log.matchAll(/^(?:FAIL|ВЫШЛО ВРЕМЯ) +(?:e2e\/)?(\S+\.mjs)/gm)) {
    if (!only.includes(m[1])) only.push(m[1]);
  }
  console.log(`из ${process.env.FAILED}: перегоняем ${only.length}`);
}
// Проверок ДВА КАТАЛОГА, и второй до сих пор не гонялся вовсе: `e2e/` — своя
// сборка, свои соглашения (адрес в `BASE`, запуск из своего каталога, модули
// оттуда же). Здесь они сведены в один список: у каждой записи сказано, откуда
// она и как её звать.
const scriptsDir = new URL('.', import.meta.url).pathname;
const e2eDir = new URL('../e2e/', import.meta.url).pathname;
// Берутся ОБА семейства: `check-` и `verify-`. Второе годами не гонялось вовсе, а
// это такие же проверки — с утверждениями и кодом возврата, просто названные
// иначе: они писались как разовые сверки поведения с эталоном и такими и остались.
const pick = (dir, where) => readdirSync(dir)
  .filter((f) => /^(check|verify)-/.test(f) && f.endsWith('.mjs'))
  .map((f) => ({ file: f, where }));
const all = [...pick(scriptsDir, 'scripts'), ...pick(e2eDir, 'e2e')]
  .filter(({ file }) => (only.length ? only.includes(file) : file.includes(filter)))
  .sort((a, b) => a.file.localeCompare(b.file));
const run = all.filter(({ file }) => !SKIP.has(file));
const skipped = all.filter(({ file }) => SKIP.has(file));

// У `e2e/` своя `package.json`, поэтому node ищет модули в `e2e/node_modules` —
// а ставить второй playwright ради этого незачем. Одна ссылка на уже
// установленный, и каталог оживает. Идемпотентно: есть — не трогаем.
if (!existsSync(e2eDir + 'node_modules')) {
  try {
    symlinkSync('../scripts/node_modules', e2eDir + 'node_modules', 'dir');
  } catch (e) {
    console.log(`не удалось связать e2e/node_modules: ${e.message}`);
  }
}

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
say(`лог: ${LOG}`);
say(`${DEV ? `выкаченное приложение (${DEV})` : `локальная сборка (${DIST})`} на ${server.url}`);
say(`проверок: ${run.length}, пропущено: ${skipped.length}\n`);

const one = ({ file, where }) => new Promise((resolve) => {
  const started = Date.now();
  // `e2e/` запускается ИЗ СВОЕГО каталога: его проверки зовут соседние скрипты
  // относительным путём (`../frontend/scripts/...`), и из корня он бьёт мимо.
  // Адрес приложения там называется `BASE`, а не `FE`.
  const p = spawn('node', [where === 'e2e' ? file : `scripts/${file}`], {
    cwd: where === 'e2e' ? e2eDir : undefined,
    env: {
      ...process.env,
      FE: server.url,
      BASE: server.url,
      // Накопленные проверки зовут `chromium.launch()` без пути к браузеру —
      // подставляем установленный, не трогая сами проверки.
      NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --import ${scriptsDir}lib/pin-browser.mjs`.trim(),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let out = '';
  p.stdout.on('data', (d) => { out += d; });
  p.stderr.on('data', (d) => { out += d; });
  const kill = setTimeout(() => p.kill('SIGKILL'), TIMEOUT_MS);
  p.on('close', (code, signal) => {
    clearTimeout(kill);
    resolve({ file, where, code, signal, secs: Math.round((Date.now() - started) / 1000), out });
  });
});

// Проверки идут ПАЧКАМИ по `JOBS`. По одной прогон занимал полчаса при том, что
// почти всё это время браузеры СТОЯТ: в проверках десятки `waitForTimeout` —
// ожидание бутстрапа, синка, фоновых проходов, — а не работа процессора.
//
// Мешать друг другу им нечем: у каждой свой браузер, своя база (имя от случайного
// `uid`) и свой пользователь на dev-воркерах; сервер здесь один, но он только
// раздаёт статику и проксирует.
//
// Больше ядер брать смысла нет, а меньше — тоже: одна проверка идёт до трёх минут,
// и она же становится дном прогона. `JOBS=1` возвращает прежний порядок по одной —
// пригодится, если понадобится ловить проверку, которая не терпит соседей.
const results = [];
let next = 0;
const worker = async () => {
  while (next < run.length) {
    const item = run[next++];
    const r = await one(item);
    const name = item.where === 'e2e' ? `e2e/${r.file}` : r.file;
    appendFileSync(LOG, `\n═══ ${name} ═══\n${r.out}\n`);
    const mark = r.signal ? 'ВЫШЛО ВРЕМЯ' : r.code === 0 ? 'OK  ' : 'FAIL';
    results.push(r);
    say(`${mark} ${name} (${r.secs} с)   [${results.length}/${run.length}]`);
  }
};
await Promise.all(Array.from({ length: Math.min(JOBS, run.length) }, worker));
// Порядок завершения случаен — итог печатается по именам, чтобы два прогона можно
// было сравнить построчно.
results.sort((a, b) => a.file.localeCompare(b.file));
server.close();

const bad = results.filter((r) => r.code !== 0);
say(`\n── итог ──\nпрошло ${results.length - bad.length} из ${results.length}`);
for (const { file } of skipped) say(`  пропущено ${file} — ${SKIP.get(file)}`);
for (const r of bad) {
  say(`\n── ${r.where === 'e2e' ? 'e2e/' : ''}${r.file} ${r.signal ? '(убита по времени)' : `(код ${r.code})`} ──`);
  say(r.out.split('\n').filter((l) => /❌|FAIL|Error|error/i.test(l)).slice(0, 12).join('\n')
    || r.out.slice(-800));
}
say(`\nполный вывод каждой проверки — в ${LOG}`);
process.exit(bad.length ? 1 : 0);
