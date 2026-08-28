// Локальный сервер для браузерных проверок: раздаёт собранный `dist` и
// ПРОКСИРУЕТ обращения приложения к живым dev-воркерам.
//
// Зачем прокси, а не прямой адрес воркеров. В части окружений (в том числе в
// контейнере, где эти проверки пишутся) браузер наружу не ходит, а node ходит.
// Приложение при этом обязано работать по-настоящему: подписка, синк, чат,
// кураторские ручки. Прокси разводит это: браузер знает только 127.0.0.1, а до
// воркеров дотягивается node.
//
// Побочная польза: видно КАЖДЫЙ запрос приложения — проверке есть что
// утверждать не только про экран, но и про то, с чем оно ходило на сервер.

import { createHmac } from 'node:crypto';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm',
  '.css': 'text/css', '.json': 'application/json', '.toml': 'text/plain',
  '.svg': 'image/svg+xml', '.png': 'image/png', '.gif': 'image/gif',
  '.webmanifest': 'application/manifest+json', '.woff2': 'font/woff2', '.ico': 'image/x-icon',
};

/// Воркеры, которые приложение зовёт по имени. Ключ — префикс пути у прокси.
export const DEV_WORKERS = {
  auth: 'https://auth-worker-dev.vg-stavenko.workers.dev',
  support: 'https://support-worker-dev.vg-stavenko.workers.dev',
  push: 'https://main-flow-dev.vg-stavenko.workers.dev',
  ai: 'https://ai-worker-dev.vg-stavenko.workers.dev',
  payment: 'https://payment-worker-dev.vg-stavenko.workers.dev',
  ocr: 'https://ocr-queue-dev.vg-stavenko.workers.dev',
  sync: 'https://sync-worker-dev.vg-stavenko.workers.dev',
  bug: 'https://bug-report-worker-dev.vg-stavenko.workers.dev',
};

/// Поднять сервер над `root` (каталог dist).
///
/// `configFor(origin)` — содержимое `config/frontend.toml`, которое увидит
/// приложение: адреса воркеров подменяются на прокси этого же сервера.
/// Возвращает `{ url, calls, close }`, где `calls` — журнал проксированных
/// запросов: `{ worker, method, path, status }`.
export async function serveWithProxy({ root, port = 0, configFor }) {
  const calls = [];
  const srv = createServer(async (req, res) => {
    const u = new URL(req.url, 'http://127.0.0.1');
    const path = decodeURIComponent(u.pathname);

    // CORS не нужен — всё с одного origin, — но preflight приложение всё равно
    // может послать: отвечаем, чтобы не ловить ложных отказов.
    if (req.method === 'OPTIONS') {
      res.writeHead(204, {
        'Access-Control-Allow-Origin': '*',
        'Access-Control-Allow-Methods': 'GET, POST, PUT, DELETE, OPTIONS',
        'Access-Control-Allow-Headers': 'Content-Type, Authorization',
      });
      return res.end();
    }

    const m = path.match(/^\/api\/([a-z]+)(\/.*)?$/);
    if (m) {
      const base = DEV_WORKERS[m[1]];
      if (!base) { res.writeHead(502).end('unknown worker'); return; }
      const target = base + (m[2] ?? '/') + u.search;
      const body = ['GET', 'HEAD'].includes(req.method)
        ? undefined
        : await new Promise((r) => { const c = []; req.on('data', (d) => c.push(d)); req.on('end', () => r(Buffer.concat(c))); });
      const headers = {};
      if (req.headers.authorization) headers.authorization = req.headers.authorization;
      if (req.headers['content-type']) headers['content-type'] = req.headers['content-type'];
      try {
        const up = await fetch(target, { method: req.method, headers, body });
        const buf = Buffer.from(await up.arrayBuffer());
        calls.push({ worker: m[1], method: req.method, path: m[2] ?? '/', status: up.status });
        res.writeHead(up.status, {
          'Content-Type': up.headers.get('content-type') ?? 'application/json',
          'Access-Control-Allow-Origin': '*',
        });
        return res.end(buf);
      } catch (e) {
        calls.push({ worker: m[1], method: req.method, path: m[2] ?? '/', status: 0, error: String(e) });
        res.writeHead(502).end(String(e));
        return;
      }
    }

    if (path === '/config/frontend.toml' && configFor) {
      res.writeHead(200, { 'Content-Type': 'text/plain' });
      return res.end(configFor(`http://127.0.0.1:${srv.address().port}`));
    }

    // Статика. `normalize` — против `..` в пути: сервер локальный, но пусть не
    // умеет отдавать что попало с диска.
    const rel = normalize(path === '/' ? '/index.html' : path).replace(/^(\.\.[/\\])+/, '');
    const file = join(root, rel);
    try {
      const buf = await readFile(file);
      res.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' });
      res.end(buf);
    } catch {
      // SPA: неизвестный путь — это маршрут приложения, а не пропажа файла.
      try {
        const buf = await readFile(join(root, 'index.html'));
        res.writeHead(200, { 'Content-Type': 'text/html' });
        res.end(buf);
      } catch { res.writeHead(404).end(); }
    }
  });
  await new Promise((r) => srv.listen(port, '127.0.0.1', r));
  const url = `http://127.0.0.1:${srv.address().port}`;
  return { url, calls, close: () => srv.close() };
}

/// Запустить браузер.
///
/// В части окружений установленный Chromium не совпадает с тем, который ждёт
/// закреплённая версия playwright: он лежит по фиксированному пути и скачивать
/// второй запрещено. Если такой есть — берём его, иначе обычный запуск.
export async function launchBrowser(opts = {}) {
  const { chromium } = await import('playwright');
  const { existsSync } = await import('node:fs');
  const pinned = process.env.PW_CHROMIUM ?? '/opt/pw-browsers/chromium';
  return chromium.launch({
    headless: true,
    ...(existsSync(pinned) ? { executablePath: pinned } : {}),
    ...opts,
  });
}

/// Токен пользователя тем же секретом, что у dev-воркеров. Это и есть «обход
/// авторизации»: паскей проверяет ЛИЧНОСТЬ, а проверкам нужна не она, а всё,
/// что происходит после входа.
export function mintToken(sub, secret = 'dev-secret-change-in-production') {
  const b64 = (x) => Buffer.from(x).toString('base64url');
  const si = `${b64(JSON.stringify({ alg: 'HS256', typ: 'JWT' }))}.` +
             `${b64(JSON.stringify({ sub, iat: 0, exp: 4102444800, caps: [], token_id: 'devtest' }))}`;
  return `${si}.${b64(createHmac('sha256', secret).update(si).digest())}`;
}
