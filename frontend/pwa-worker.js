// Cloudflare Pages advanced-mode worker (deployed as dist/_worker.js).
//
// Its ONLY job is to serve a per-user dynamic web-app manifest so an installed PWA carries the
// user's non-secret account id into its launch URL (`start_url=/?u=<user_id>`) — which is how
// the installed app, in its OWN storage context (separate from the browser on iOS), knows
// which account to log into. Everything else falls through to the static assets unchanged.
//
// `?u` empty → the plain app manifest (start_url "/"), so a normal install still works.

function manifest(u) {
  const m = {
    name: "Renorma",
    short_name: "Renorma",
    start_url: u ? `/?u=${encodeURIComponent(u)}` : "/",
    // Distinct `id` per user so a per-account install is its own app; empty → the base app.
    id: u ? `/app-${u}` : "/",
    display: "standalone",
    background_color: "#ffffff",
    theme_color: "#ffffff",
    orientation: "portrait",
    icons: [
      { src: "/icon-192.png", sizes: "192x192", type: "image/png" },
      { src: "/icon-512.png", sizes: "512x512", type: "image/png" },
    ],
  };
  return m;
}

// ── Браузеры, из которых уводим в Chrome ─────────────────────────────────────
//
// Mi Browser (Xiaomi): ключ доступа там создать нельзя — `PublicKeyCredential`
// отсутствует вовсе (замерено пробником на Redmi, Android 15, MiuiBrowser 14.60).
// Пользоваться приложением невозможно, человеку надо в Chrome — и больше ничего.
//
// Samsung Internet: intent в Chrome оттуда СРАБАТЫВАЕТ (проверено на устройстве,
// SamsungBrowser/30.0, Android 10) — а раз так, одна кнопка лучше инструкции по
// установке из самого Samsung Internet, которая была здесь раньше. Инструкцию
// человек увидит уже в Chrome.
//
// Поэтому такие браузеры получают СВОЮ страницу прямо отсюда, а не экран внутри
// приложения: решение принимается до того, как отдан хоть байт, так что не
// грузится ни манифест, ни сервис-воркер, ни WASM. Ради одной кнопки качать
// приложение целиком незачем, а манифест вдобавок подбивает браузер предлагать
// ярлык на рабочий стол — ставить который тут ровно не надо.
//
// Экраны `android_mi` / `android_samsung` в приложении при этом ОСТАЮТСЯ: если
// сервис-воркер, живущий с прошлых визитов, отдаст навигацию из кэша, сюда
// управление не попадёт, и показать что-то осмысленное должно само приложение.
// Текст в двух местах — плата за это; он короткий и меняется вместе.
const MI_TITLE = "Приложение re:Norma работает в браузере Chrome.";
const MI_BUTTON = "Открыть в Chrome";

function isHandoffBrowser(request) {
  const ua = request.headers.get("User-Agent") || "";
  // Samsung Internet держит в UA и «Chrome/143», и «SamsungBrowser/30» — ловим по
  // второму, иначе он не отличим от настоящего Chrome.
  return /miuibrowser|samsungbrowser/i.test(ua);
}

/// Запрос на СТРАНИЦУ, а не на картинку или скрипт: подменять надо только то,
/// что человек видит.
function isPageRequest(request) {
  return (
    request.method === "GET" &&
    (request.headers.get("Accept") || "").includes("text/html")
  );
}

function miPage(url) {
  // Путь сохраняем как есть. Ссылка из Telegram ведёт на `/onboard?u=…`, и это
  // ОСОБЫЙ вход: там приложение не накрывает страницу оверлеями, а ведёт свой
  // порядок — код, ключ, и лишь в конце установка. Уведи мы человека на корень,
  // он попал бы сразу на экран установки, не зарегистрировавшись.
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
<title>re:Norma</title>
<style>
 body{font:16px/1.5 system-ui,-apple-system,sans-serif;margin:0;min-height:100vh;display:flex;
      align-items:center;justify-content:center;padding:32px 24px;text-align:center;
      background:#f1f4f9;color:#0e1630;box-sizing:border-box}
 .w{max-width:24rem;width:100%}
 img{width:80px;height:80px;border-radius:16px;margin-bottom:20px}
 h1{font-size:24px;line-height:1.3;margin:0 0 28px}
 a{display:block;padding:18px;border-radius:12px;background:#3b4cf5;color:#fff;
   font-size:18px;font-weight:700;text-decoration:none}
</style></head>
<body><div class="w" data-testid="pwa-mi-page">
 <img src="/icon-192.png" alt="re:Norma">
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

    if (url.pathname === "/manifest.json") {
      const u = url.searchParams.get("u") || "";
      return new Response(JSON.stringify(manifest(u)), {
        headers: {
          "Content-Type": "application/manifest+json",
          "Cache-Control": "no-store",
        },
      });
    }

    const res = await env.ASSETS.fetch(request);

    // Inject `?u=<user_id>` into the served HTML's <link rel="manifest"> so the BROWSER reads
    // the per-user manifest (start_url=/?u=<u>) at page-load — before WASM runs. Without this
    // the link carries no ?u when «Add to Home Screen» snapshots the manifest, so the installed
    // PWA loses the account id (its start_url falls back to "/"). The page URL carries the id
    // (/onboard?u=… on install, /?u=… on launch), and we forward it into the manifest link.
    const u = url.searchParams.get("u") || "";
    const ct = res.headers.get("Content-Type") || "";
    if (u && ct.includes("text/html")) {
      const rewritten = new HTMLRewriter()
        .on('link[rel="manifest"]', {
          element(el) {
            el.setAttribute("href", `/manifest.json?u=${encodeURIComponent(u)}`);
          },
        })
        .transform(res);
      // User-specific → never let a shared cache hand this variant to another account.
      const headers = new Headers(rewritten.headers);
      headers.set("Cache-Control", "no-store");
      return new Response(rewritten.body, {
        status: rewritten.status,
        statusText: rewritten.statusText,
        headers,
      });
    }
    // Everything else → the static site (SPA index fallback handled by Pages/_redirects).
    return res;
  },
};
