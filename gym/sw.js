// Сервис-воркер приложения тренировок.
//
// Перенесён из приложения худеющего (frontend/sw.js) вместе с выстраданными
// решениями; выброшено то, чего здесь пока нет, — пуш и его квитанции.
//
// БАМП ИМЕНИ = СНОС КЭША. `activate` удаляет все кэши с другими именами, и это
// единственный способ выбросить закэшированное под ПОСТОЯННЫМ адресом: у js/wasm
// хеш в имени, а гифки инструкций, шрифты и иконки живут по одному и тому же URL.
var CACHE_NAME = 'gym-v1';

// Оболочка с постоянными именами — прекэшируется на install, чтобы приложение
// открылось офлайн даже после единственного короткого сеанса в сети (iOS капризен
// к ленивому кэшированию на лету).
var SHELL = ['/', '/init.js', '/manifest.webmanifest', '/config/frontend.toml',
             '/icons/icon-192.png', '/icons/favicon.png',
             '/fonts/golos-latin.woff2', '/fonts/golos-cyrillic.woff2'];

// Положить адрес в кэш по мере сил (никогда не роняет вызывающего).
function precache(cache, u) {
    return fetch(u, { cache: 'no-cache' }).then(function (r) {
        if (r.ok) return cache.put(u, r.clone());
    }).catch(function () {});
}

// Хешированные JS-обвязка и WASM ТЕКУЩЕЙ сборки лежат ВНУТРИ init.js
// (`import … from '/gym-<hash>.js'` и `module_or_path:'/gym-<hash>_bg.wasm'`).
// Их обязательно прекэшировать на install: иначе они кэшируются только по факту
// обращения, и человек, который обновился, но ни разу не догрузился в сети, wasm
// потом уже не достанет — а на iOS кэш-первый статический fetch на отсутствующем
// wasm ВИСНЕТ навсегда, и приложение не открывается вовсе. Install идёт заведомо
// в сети (за самим sw.js уже ходили), так что сборка кэшируется целиком и связно.
function precacheHashedAssets(cache) {
    return fetch('/init.js', { cache: 'no-cache' }).then(function (r) {
        return r.ok ? r.text() : '';
    }).then(function (txt) {
        var urls = [];
        var js = txt.match(/from\s*['"](\/[^'"]+\.js)['"]/);
        var wasm = txt.match(/module_or_path\s*:\s*['"](\/[^'"]+\.wasm)['"]/);
        if (js) urls.push(js[1]);
        if (wasm) urls.push(wasm[1]);
        return Promise.all(urls.map(function (u) { return precache(cache, u); }));
    }).catch(function () {});
}

self.addEventListener('install', function (event) {
    event.waitUntil(
        caches.open(CACHE_NAME).then(function (cache) {
            return Promise.all([
                Promise.all(SHELL.map(function (u) { return precache(cache, u); })),
                precacheHashedAssets(cache),
            ]);
        }).then(function () { return self.skipWaiting(); })
    );
});

self.addEventListener('activate', function (event) {
    event.waitUntil(
        caches.keys().then(function (names) {
            return Promise.all(
                names.filter(function (n) { return n !== CACHE_NAME; })
                     .map(function (n) { return caches.delete(n); })
            );
        }).then(function () { return self.clients.claim(); })
    );
});

self.addEventListener('fetch', function (event) {
    var url = new URL(event.request.url);

    if (event.request.method !== 'GET') {
        return;
    }
    // Чужой origin (auth/payment/ai/sync-воркеры) — не перехватываем и не кэшируем
    // вовсе: закэшированный здесь ответ чужого API ломает запрос (ERR_FAILED).
    if (url.origin !== self.location.origin) {
        return;
    }
    // Проба версии обязана всегда идти в сеть — закэшируй мы её, вся проверка
    // обновления потеряла бы смысл.
    if (url.pathname === '/version.json') {
        return;
    }

    // Навигации, НЕхешированная точка входа (init.js) и конфигурация — сеть
    // вперёд. У init.js имя постоянное, а содержимое меняется каждую сборку (в
    // нём имена нового хешированного wasm/js): отдать его из кэша значит
    // загрузить прошлую сборку. frontend.toml — постоянное имя, чьё СОДЕРЖИМОЕ
    // различается между dev и прод-выкатами: устаревший бросит приложение на
    // чужие адреса воркеров, которые прод-CSP не пустит.
    if (event.request.mode === 'navigate'
        || url.pathname === '/init.js'
        || url.pathname === '/config/frontend.toml'
        || url.pathname === '/manifest.webmanifest') {
        // Офлайн-подстраховка: точный закэшированный ответ, иначе оболочка «/» —
        // приложение одностраничное, index.html отрисует нужный экран сам.
        var fallback = function () {
            return caches.match(event.request).then(function (cached) {
                if (cached) return cached;
                if (event.request.mode === 'navigate') return caches.match('/');
                return cached;
            });
        };
        var network = fetch(event.request, { cache: 'no-cache' }).then(function (response) {
            var clone = response.clone();
            caches.open(CACHE_NAME).then(function (cache) { cache.put(event.request, clone); });
            return response;
        });
        // Гонка сети с таймаутом. КРИТИЧНО для iOS: офлайн Safari внутри
        // сервис-воркера может ПОВИСНУТЬ на fetch вместо отказа, тогда `.catch`
        // не сработает никогда и `respondWith` останется висеть — приложение не
        // откроется. Таймаут гарантирует, что подстраховка выполнится.
        var timeout = new Promise(function (resolve) {
            setTimeout(function () { resolve('__timeout__'); }, 2500);
        });
        event.respondWith(
            Promise.race([network, timeout])
                .then(function (r) { return r === '__timeout__' ? fallback() : r; })
                .catch(fallback)
        );
        return;
    }

    // Остальная статика (js, wasm, шрифты, картинки) — кэш вперёд. Закэшированное
    // отдаётся мгновенно и БЕЗ сети: это и есть счастливый офлайн-путь.
    event.respondWith(
        caches.match(event.request).then(function (cached) {
            if (cached) return cached;
            // Не закэшировано → идём в сеть, но с ограничением: на iOS
            // заблокированное соединение оставляет fetch ВИСЕТЬ (не разрешается и
            // не отклоняется), что заморозило бы приложение на экране загрузки.
            // Щедрый таймаут — настоящая закачка wasm по медленному каналу в него
            // укладывается, а мёртвое соединение отваливается ошибкой, а не висит.
            var network = fetch(event.request).then(function (response) {
                if (response.ok) {
                    var clone = response.clone();
                    caches.open(CACHE_NAME).then(function (cache) { cache.put(event.request, clone); });
                }
                return response;
            });
            var timeout = new Promise(function (_, reject) {
                setTimeout(function () { reject(new Error('sw-static-timeout')); }, 20000);
            });
            return Promise.race([network, timeout]).catch(function () {
                return caches.match(event.request).then(function (c) { return c || Response.error(); });
            });
        })
    );
});
