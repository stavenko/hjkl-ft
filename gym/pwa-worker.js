// Воркер Cloudflare Pages в advanced-режиме (кладётся как dist/_worker.js).
//
// Перенесён из приложения худеющего (frontend/pwa-worker.js); от него оставлено
// то, что имеет смысл здесь. Персонального манифеста тут НЕТ и не нужно: в
// приложение тренировок не ведёт ссылка с `?u=<user_id>` — сюда приходят своим
// ключом, а не по приглашению бота.
//
// Остаётся одна работа: браузеры, из которых человека уводят в Chrome, получают
// СВОЮ страницу прямо отсюда, а не экран внутри приложения.
//
// Mi Browser (Xiaomi): ключ доступа там создать нельзя — `PublicKeyCredential`
// отсутствует вовсе (замерено пробником на Redmi, Android 15, MiuiBrowser 14.60).
// Samsung Internet: intent в Chrome оттуда СРАБАТЫВАЕТ (проверено на устройстве,
// SamsungBrowser/30.0, Android 10) — а раз так, одна кнопка лучше инструкции по
// установке из самого Samsung Internet.
//
// Решение принимается ДО того, как отдан хоть байт: не грузится ни манифест, ни
// сервис-воркер, ни WASM. Ради одной кнопки качать приложение целиком незачем, а
// манифест вдобавок подбивает браузер предлагать ярлык на рабочий стол —
// ставить который тут ровно не надо.
//
// Экраны `android_mi` / `android_samsung` в приложении при этом ОСТАЮТСЯ: если
// сервис-воркер, живущий с прошлых визитов, отдаст навигацию из кэша, сюда
// управление не попадёт, и показать что-то осмысленное должно само приложение.
// Текст в двух местах — плата за это; он короткий и меняется вместе.
const MI_TITLE = "Тренировки re:Norma работают в браузере Chrome.";
const MI_BUTTON = "Открыть в Chrome";

function isHandoffBrowser(request) {
  const ua = request.headers.get("User-Agent") || "";
  // Samsung Internet держит в UA и «Chrome/143», и «SamsungBrowser/30» — ловим по
  // второму, иначе он не отличим от настоящего Chrome.
  return /miuibrowser|samsungbrowser/i.test(ua);
}

// Запрос на СТРАНИЦУ, а не на картинку или скрипт: подменять надо только то,
// что человек видит.
function isPageRequest(request) {
  return (
    request.method === "GET" &&
    (request.headers.get("Accept") || "").includes("text/html")
  );
}

function miPage(url) {
  const host = url.host;
  const rest = `${url.pathname}${url.search}`;
  const target = `https://${host}${rest}`;
  const intent =
    `intent://${host}${rest}#Intent;scheme=https;package=com.android.chrome;` +
    `S.browser_fallback_url=${encodeURIComponent(target)};end`;
  // Ни <link rel="manifest">, ни регистрации сервис-воркера — намеренно.
  return `<!doctype html>
<html lang="ru"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>re:Norma — тренировки</title>
<style>
 body{font:16px/1.5 system-ui,-apple-system,sans-serif;margin:0;min-height:100vh;display:flex;
      align-items:center;justify-content:center;padding:32px 24px;text-align:center;
      background:#0E1116;color:#E8EBF1;box-sizing:border-box}
 .w{max-width:24rem;width:100%}
 img{width:80px;height:80px;border-radius:18px;margin-bottom:20px}
 h1{font-size:24px;line-height:1.3;margin:0 0 28px}
 a{display:block;padding:18px;border-radius:12px;background:#14B884;color:#04130C;
   font-size:18px;font-weight:700;text-decoration:none}
</style></head>
<body><div class="w" data-testid="pwa-mi-page">
 <img src="/icons/icon-192.png" alt="re:Norma">
 <h1>${MI_TITLE}</h1>
 <a data-testid="pwa-btn-open-chrome" href="${intent}">${MI_BUTTON}</a>
</div>
<script>
// Сервис-воркер, оставшийся с прошлых визитов, перехватывает навигации и может
// отдать закэшированную оболочку приложения вместо этой страницы — тогда человек
// снова упрётся в то, из чего мы его уводим. Снимаем его здесь: в Mi приложение
// всё равно не работает, а в Chrome регистрация своя.
if ("serviceWorker" in navigator) {
  navigator.serviceWorker.getRegistrations().then(function (rs) {
    rs.forEach(function (r) { r.unregister(); });
  }).catch(function () {});
}
</script></body></html>`;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (isHandoffBrowser(request) && isPageRequest(request)) {
      return new Response(miPage(url), {
        headers: { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store" },
      });
    }

    // Всё остальное — статике (SPA-фолбэк держат Pages/_redirects).
    return env.ASSETS.fetch(request);
  },
};
