var CACHE_NAME = 'ft-v32';
// Separate, long-lived cache holding the notification-receipt marker (the ntf code
// of a received push). Kept across SW activations (excluded from the cleanup below)
// so an app launched AFTER the push still consumes it. NOT usable as a live channel:
// on iOS the page's CacheStorage view detaches after a push subscribe/receipt —
// IndexedDB does live delivery instead (see docs/notification-receipt.md).
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

    event.waitUntil((function() {
        // ON RECEIPT: a "task complete" notification carries ntf=<kind>.<section>.<task>.<rand>
        // in its URL — receiving it confirms that task's milestone. The code is delivered
        // over three redundant channels (consumption is idempotent) because iOS breaks the
        // obvious ones — see docs/notification-receipt.md:
        //   1. IndexedDB — the LIVE channel. The page polls it (index.html) and hands the
        //      code to the WASM poll (lib.rs), which sets the task's story flag. The page's
        //      CacheStorage view detaches after a push subscribe/receipt on iOS (reads
        //      return empty until relaunch), so Cache can't do live; IndexedDB survives.
        //   2. Cache marker — next-launch fallback (a fresh page connection reads fine).
        //   3. postMessage to open windows — NOT delivered live on iOS, but query_notif
        //      (below) re-uses it as a page-initiated reply, which does deliver.
        // The SW can't touch localStorage — the page bridges everything (index.html).
        var m = (data.url || '').match(/[?&]ntf=([^&]+)/);
        if (m) {
            var code = decodeURIComponent(m[1]);
            return caches.open(NOTIF_CACHE)
                .then(function(c) { return c.put('/__notif_received__', new Response(code)); })
                .then(function() { return idbPutNotif(code); })
                .then(function() {
                    return self.clients.matchAll({ type: 'window', includeUncontrolled: true })
                        .then(function(cs) {
                            cs.forEach(function(cl) { cl.postMessage({ type: 'notif_received', code: code }); });
                        });
                })
                .then(function() { return self.registration.showNotification(data.title || 'Food Tracker', options); });
        }
        return self.registration.showNotification(data.title || 'Food Tracker', options);
    })());
});

self.addEventListener('notificationclick', function(event) {
    event.notification.close();
    var url = event.notification.data || '/';

    // Action buttons other than "open" do nothing; tapping the body gives an
    // empty action and proceeds.
    if (event.action && event.action !== 'open') {
        return;
    }

    // iOS/WebKit bug 252544: on a Home-Screen PWA, REUSING an already-open window
    // client (matchAll + client.navigate / postMessage / focus) leaves it INERT for
    // a short period — the deep link is dropped and the tap "does nothing". This is
    // exactly what bit us once auth v2 pushed users into the INSTALLED PWA (in a
    // plain Safari tab the routing works). The working path (per WebKit's Brady
    // Eidson) is clients.openWindow(url), which yields a NON-inert client that
    // routes correctly. So DON'T reuse an existing client — just openWindow(url).
    // (Task completion does NOT depend on this tap — it happens on receipt.)
    event.waitUntil(clients.openWindow(url).catch(function() {}));
});

// Receipt marker in IndexedDB — the LIVE delivery channel. Its storage stack is
// separate from CacheStorage, whose page-side view detaches on iOS after a push
// subscribe/receipt. The page polls and consumes it (index.html).
function idbPutNotif(code) {
    return new Promise(function(resolve) {
        try {
            var req = indexedDB.open('rn-notif', 1);
            req.onupgradeneeded = function() { req.result.createObjectStore('kv'); };
            req.onsuccess = function() {
                var db = req.result;
                var tx = db.transaction('kv', 'readwrite');
                tx.objectStore('kv').put(code, 'notif_received');
                tx.oncomplete = function() { db.close(); resolve(); };
                tx.onabort = function() { db.close(); resolve(); };
            };
            req.onerror = function() { resolve(); };
        } catch (e) { resolve(); }
    });
}

// The page asks the SW to read the receipt marker on its behalf (boot / resume).
// Needed because the PAGE's own CacheStorage reads return empty once its view
// detaches (iOS), while the SW's view keeps working. Consuming (delete) happens
// here so a queried code can't be double-delivered.
self.addEventListener('message', function(event) {
    var d = event.data || {};
    if (d.type !== 'query_notif') return;
    event.waitUntil(
        caches.open(NOTIF_CACHE).then(function(c) {
            return c.match('/__notif_received__').then(function(r) {
                if (!r) return;
                return r.text().then(function(code) {
                    return c.delete('/__notif_received__').then(function() {
                        if (event.source) event.source.postMessage({ type: 'notif_received', code: code });
                    });
                });
            });
        }).catch(function() {})
    );
});
