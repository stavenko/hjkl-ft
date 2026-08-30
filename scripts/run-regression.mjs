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
//
// Проверка считается пройденной по КОДУ ВОЗВРАТА: так они и написаны.
//
// Полный вывод пишется в `regression-local.log` (или `-dev`). НЕ пропускайте
// прогон через `| tail`: итог печатается перед подробностями, и хвост срежет
// именно его. Подробнее — `docs/testing.md`.

import { spawn } from 'node:child_process';
import { appendFileSync, readdirSync, writeFileSync } from 'node:fs';
import { serveWithProxy } from './lib/devserver.mjs';

const DIST = process.env.FE_DIST ?? new URL('../frontend/dist', import.meta.url).pathname;
const DEV = process.env.DEV ? (process.env.DEV_URL ?? 'https://renorma-fit-dev.pages.dev') : null;
const TIMEOUT_MS = Number(process.env.TIMEOUT_MS ?? 240_000);
const filter = process.argv[2] ?? '';

// Полный лог пишется САМ, а не зависит от того, как вызвали команду: вывод у
// прогона длинный, итог печатается перед подробностями, и любой `| tail` его
// срезает. На этом я уже один раз потерял результат часового прогона.
const LOG = process.env.LOG ?? `regression-${DEV ? 'dev' : 'local'}.log`;
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
  ['check-detections.mjs', 'ходит в ИИ-воркер: медленно и платно'],
  ['check-identity-cache.mjs', 'ходит в ИИ-воркер'],
  ['check-identity-fallback.mjs', 'ходит в ИИ-воркер'],
  ['check-iron-lookup.mjs', 'ходит в ИИ-воркер'],
  ['check-kbju-lookup.mjs', 'ходит в ИИ-воркер'],
  ['check-food-sweep.mjs', 'ходит в ИИ-воркер'],
  ['check-vision-direct.mjs', 'ходит в ИИ-воркер'],
  ['check-usage-by-model.mjs', 'читает статистику ИИ-воркера'],
  ['check-full-pass.mjs', 'ходит в ИИ-воркер по всему списку еды'],
  // Не приложение худеющего: другое происхождение, наш сервер им не подставить.
  ['check-admin-thread-card.mjs', 'проверяет АДМИНКУ'],
  ['check-admin-wipe-ui.mjs', 'проверяет АДМИНКУ'],
  ['check-landing-consent.mjs', 'проверяет ЛЕНДИНГ'],
  ['check-miniapp-app-link.mjs', 'проверяет мини-приложение Telegram'],
  ['check-miniapp-consent.mjs', 'проверяет мини-приложение Telegram'],
  ['check-miniapp-dev-entry.mjs', 'проверяет мини-приложение Telegram'],
  ['check-smoke-front.mjs', 'ходит на отдельный смоук-стенд fit-smoke'],
  // Вшили адрес намертво — подставить свой нечем.
  ['check-chat-input.mjs', 'вшит адрес конкретной старой выкладки'],
  ['check-recipe-finalize.mjs', 'вшит адрес конкретной старой выкладки'],
  ['check-syncv2.mjs', 'вшит живой адрес, и по нему же блокирует синк'],
  ['check-syncv2-migration.mjs', 'вшит живой адрес'],
  ['check-user-reset.mjs', 'вшит живой адрес'],
  ['check-user-wipe.mjs', 'вшит живой адрес'],
]);

const all = readdirSync(new URL('.', import.meta.url).pathname)
  .filter((f) => f.startsWith('check-') && f.endsWith('.mjs'))
  .filter((f) => f.includes(filter))
  .sort();
const run = all.filter((f) => !SKIP.has(f));
const skipped = all.filter((f) => SKIP.has(f));

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
say(`${DEV ? `выкаченное приложение (${DEV})` : `локальная сборка (${DIST})`} на ${server.url}`);
say(`проверок: ${run.length}, пропущено: ${skipped.length}\n`);

const one = (file) => new Promise((resolve) => {
  const started = Date.now();
  const p = spawn('node', [`scripts/${file}`], {
    env: {
      ...process.env,
      FE: server.url,
      // Накопленные проверки зовут `chromium.launch()` без пути к браузеру —
      // подставляем установленный, не трогая сами проверки.
      NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --import ./scripts/lib/pin-browser.mjs`.trim(),
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let out = '';
  p.stdout.on('data', (d) => { out += d; });
  p.stderr.on('data', (d) => { out += d; });
  const kill = setTimeout(() => p.kill('SIGKILL'), TIMEOUT_MS);
  p.on('close', (code, signal) => {
    clearTimeout(kill);
    resolve({ file, code, signal, secs: Math.round((Date.now() - started) / 1000), out });
  });
});

const results = [];
for (const file of run) {
  const r = await one(file);
  appendFileSync(LOG, `\n═══ ${file} ═══\n${r.out}\n`);
  const mark = r.signal ? 'ВЫШЛО ВРЕМЯ' : r.code === 0 ? 'OK  ' : 'FAIL';
  say(`${mark} ${file} (${r.secs} с)`);
  results.push(r);
}
server.close();

const bad = results.filter((r) => r.code !== 0);
say(`\n── итог ──\nпрошло ${results.length - bad.length} из ${results.length}`);
for (const [f, why] of skipped.map((f) => [f, SKIP.get(f)])) say(`  пропущено ${f} — ${why}`);
for (const r of bad) {
  say(`\n── ${r.file} ${r.signal ? '(убита по времени)' : `(код ${r.code})`} ──`);
  say(r.out.split('\n').filter((l) => /❌|FAIL|Error|error/i.test(l)).slice(0, 12).join('\n')
    || r.out.slice(-800));
}
say(`\nполный вывод каждой проверки — в ${LOG}`);
process.exit(bad.length ? 1 : 0);
