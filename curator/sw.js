// Сервис-воркер кураторского приложения.
//
// Он здесь ради ОДНОГО: принять пуш о сообщении клиента, когда приложение
// закрыто. Кэшированием он не занимается — консоль всегда работает по сети, и
// показывать в ней устаревшие данные хуже, чем не показывать ничего.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));

self.addEventListener('push', (event) => {
  let data = {};
  try { data = event.data ? event.data.json() : {}; } catch (_) { data = {}; }
  const title = data.title || 're:Norma';
  const options = {
    body: data.body || '',
    icon: '/icons/icon-192.png',
    tag: data.tag || 'curator',
    renotify: true,
    data: { url: data.url || '/' },
  };
  event.waitUntil(self.registration.showNotification(title, options));
});

// WebKit 252544: повторное использование существующего окна оставляет его
// «инертным» — работает только openWindow. То же самое выстрадано в приложении
// худеющего, и повторять там ошибку здесь незачем.
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const url = (event.notification.data && event.notification.data.url) || '/';
  event.waitUntil(self.clients.openWindow(url));
});
