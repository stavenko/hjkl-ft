var CACHE_NAME = 'ft-v10';
// Separate, long-lived cache holding the tapped-notification deep link. Kept across
// SW activations (excluded from the cleanup below) so the page can consume it on
// foreground — the reliable channel on iOS standalone PWAs.
var NOTIF_CACHE = 'notif-deeplink';

// Fixed-name shell: precached on install so an offline launch works even after
// only a brief online session (iOS is finicky about lazy runtime caching). The
// hashed assets (wasm/js/css) still cache at runtime, cache-first.
var SHELL = ['/', '/init.js', '/manifest.json', '/config/frontend.toml',
             '/icon-192.png', '/favicon.ico',
             '/fonts/inter-latin.woff2', '/fonts/inter-cyrillic.woff2'];

self.addEventListener('install', function(event) {
    event.waitUntil(
        caches.open(CACHE_NAME).then(function(cache) {
            // Best-effort: never let one missing URL fail the whole install.
            return Promise.all(SHELL.map(function(u) {
                return fetch(u, { cache: 'no-cache' }).then(function(r) {
                    if (r.ok) return cache.put(u, r.clone());
                }).catch(function() {});
            }));
        }).then(function() { return self.skipWaiting(); })
    );
});

self.addEventListener('activate', function(event) {
    event.waitUntil(
        caches.keys().then(function(names) {
            return Promise.all(
                names.filter(function(name) { return name !== CACHE_NAME && name !== NOTIF_CACHE; })
                     .map(function(name) { return caches.delete(name); })
            );
        }).then(function() {
            return self.clients.claim();
        })
    );
});

self.addEventListener('fetch', function(event) {
    var url = new URL(event.request.url);

    // Non-GET requests and API calls — network only
    if (event.request.method !== 'GET') {
        return;
    }
    // Cross-origin requests (auth/ai/payment/push workers) — never intercept or
    // cache; let the browser handle them directly. Caching a cross-origin API
    // response here breaks the request (net::ERR_FAILED).
    if (url.origin !== self.location.origin) {
        return;
    }
    if (url.pathname.startsWith('/api/')) {
        return;
    }
    // Version probe must always hit the network — caching it stale would defeat
    // the in-app update check.
    if (url.pathname === '/version.json') {
        return;
    }

    // HTML navigations, the non-hashed module entry (init.js), AND the runtime
    // config — network first. init.js has a fixed filename but its content changes
    // every build (it references the new hashed wasm/js); serving it stale would load
    // the previous build's wasm. frontend.toml is the fixed-name config whose CONTENTS
    // differ between dev and prod deploys — serving it stale strands the app on the
    // wrong worker URLs (e.g. dev workers behind a prod CSP → blocked fetches). Both
    // must always be fresh online, with the cache only as an offline fallback.
    if (event.request.mode === 'navigate'
        || url.pathname === '/init.js'
        || url.pathname === '/config/frontend.toml'
        || url.pathname === '/manifest.json') {
        // Offline fallback: exact cached navigation, else the cached app shell
        // ("/") — this is an SPA, so index.html + the client router render the
        // right route. Covers opening/reloading offline on any route (/diary…).
        var fallback = function() {
            return caches.match(event.request).then(function(cached) {
                if (cached) return cached;
                if (event.request.mode === 'navigate') return caches.match('/');
                return cached;
            });
        };
        var network = fetch(event.request, { cache: 'no-cache' }).then(function(response) {
            var clone = response.clone();
            caches.open(CACHE_NAME).then(function(cache) { cache.put(event.request, clone); });
            return response;
        });
        // Race the network against a 2.5s timeout. CRITICAL for iOS: when offline,
        // Safari's SW `fetch` can HANG instead of rejecting, so a plain
        // `.catch(→cache)` never fires and `respondWith` stays pending forever —
        // the app never opens. The timeout guarantees the cache fallback runs.
        var timeout = new Promise(function(resolve) { setTimeout(function() { resolve('__timeout__'); }, 2500); });
        event.respondWith(
            Promise.race([network, timeout])
                .then(function(r) { return r === '__timeout__' ? fallback() : r; })
                .catch(fallback)
        );
        return;
    }

    // Static assets (JS, WASM, CSS, fonts) — cache first, update in background
    event.respondWith(
        caches.match(event.request).then(function(cached) {
            var fetchPromise = fetch(event.request).then(function(response) {
                if (response.ok) {
                    var clone = response.clone();
                    caches.open(CACHE_NAME).then(function(cache) {
                        cache.put(event.request, clone);
                    });
                }
                return response;
            }).catch(function() {
                return cached;
            });

            return cached || fetchPromise;
        })
    );
});

self.addEventListener('push', function(event) {
    var data = { title: 'Food Tracker', body: 'New notification', icon: '/icon-192.png' };

    if (event.data) {
        try {
            data = event.data.json();
        } catch (e) {
            data.body = event.data.text();
        }
    }

    var options = {
        body: data.body || '',
        icon: data.icon || '/icon-192.png',
        badge: '/icon-192.png',
        data: data.url || '/',
        tag: data.tag || 'default',
    };

    if (data.renotify) {
        options.renotify = true;
    }

    if (data.requireInteraction) {
        options.requireInteraction = true;
    }

    if (data.actions && Array.isArray(data.actions)) {
        options.actions = data.actions;
    }

    event.waitUntil(
        self.registration.showNotification(data.title || 'Food Tracker', options)
    );
});

self.addEventListener('notificationclick', function(event) {
    event.notification.close();
    var url = event.notification.data || '/';

    // Action buttons other than "open" do nothing; tapping the body gives an
    // empty action and proceeds.
    if (event.action && event.action !== 'open') {
        return;
    }

    event.waitUntil(
        // Durable deep-link marker FIRST. On iOS standalone PWAs the postMessage /
        // client.navigate / openWindow paths below are unreliable, but tapping the
        // notification always foregrounds the PWA — so the page consumes this marker
        // on visibilitychange/boot (see index.html) and navigates itself.
        caches.open(NOTIF_CACHE).then(function(cache) {
            return cache.put('/__tapped__', new Response(url));
        }).then(function() {
            return clients.matchAll({ type: 'window', includeUncontrolled: true });
        }).then(function(windowClients) {
            for (var i = 0; i < windowClients.length; i++) {
                var client = windowClients[i];
                if (client.url.indexOf(self.location.origin) === 0) {
                    // `client.navigate()` is a no-op on an already-focused iOS PWA
                    // window, so the deep link (e.g. `?notif=1`) never reaches the
                    // app. Hand the URL to the running page via postMessage instead;
                    // the page navigates itself (see index.html). Fall back to a
                    // best-effort navigate for engines that honour it.
                    client.postMessage({ type: 'notificationclick', url: url });
                    if (client.navigate) {
                        try { client.navigate(url); } catch (e) {}
                    }
                    if ('focus' in client) {
                        return client.focus();
                    }
                    return;
                }
            }
            return clients.openWindow(url);
        })
    );
});
