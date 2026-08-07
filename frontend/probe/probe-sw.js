// Пустой обработчик fetch — без него браузер не считает страницу устанавливаемой
// и НЕ шлёт beforeinstallprompt, то есть проба показала бы ложное «нельзя».
self.addEventListener("install", function () { self.skipWaiting(); });
self.addEventListener("activate", function (e) { e.waitUntil(self.clients.claim()); });
self.addEventListener("fetch", function () {});
