// Воркер Cloudflare Pages в advanced-режиме (кладётся как dist/_worker.js).
//
// Перенесён из приложения худеющего (frontend/pwa-worker.js). Две работы.
//
// ПЕРВАЯ — персональный манифест. Установленное приложение уносит в своём
// `start_url` несекретный идентификатор аккаунта: `/?u=<user_id>`.
//
// Соблазн был решить, что залу это не нужно: вход здесь по discoverable-паскею,
// а тот сам покажет список ключей, и спрашивать «кто вы» не надо. Рассуждение
// неверное, и приложение питания уже за него платило. Ключ МОЖЕТ не сработать:
// не синхронизировалась связка, человек сменил телефон, система отказала. А у
// установленного приложения на iOS своё хранилище — сессии в нём нет. Без
// идентификатора в `start_url` такой человек упирается в экран входа, с
// которого нет выхода вообще: ни кода прислать некуда, ни даже сказать ему,
// оплачено у него или нет (см. account_state).
//
// Идентификатор попадает сюда так: человек входит ключом во вкладке, приложение
// переставляет `<link rel="manifest">` на `/manifest.json?u=<его id>`, и снимок
// манифеста, который браузер делает при установке, забирает `start_url` уже с
// ним. Ссылка бота, как в питании, для этого не нужна.
//
// ВТОРАЯ — браузеры, из которых человека уводят в Chrome: они получают СВОЮ
// страницу прямо отсюда, а не экран внутри приложения.
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
// Манифест собирается ЗДЕСЬ, а не берётся из статики: он зависит от `?u=`.
// Пустой `?u` отдаёт обычный манифест (start_url "/"), чтобы установка без
// входа тоже работала.
function manifest(u) {
  return {
    name: "re:Norma — тренировки",
    short_name: "re:Norma зал",
    description: "Тренировки re:Norma: вход тем же ключом, что и в приложение питания.",
    start_url: u ? `/?u=${encodeURIComponent(u)}` : "/",
    // Свой `id` на аккаунт: установка под конкретного человека — отдельное
    // приложение, а не переустановка поверх чужого.
    id: u ? `/app-${u}` : "/",
    scope: "/",
    display: "standalone",
    orientation: "portrait",
    background_color: "#0E1116",
    theme_color: "#0E1116",
    icons: [
      { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png", purpose: "any" },
      { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png", purpose: "any" },
      { src: "/icons/icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
    ],
  };
}

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

    // `?u=` подставляется в <link rel="manifest"> ПРЯМО В ОТДАВАЕМОЙ РАЗМЕТКЕ,
    // до того, как запустится wasm. Иначе браузер, снимая манифест в момент
    // «Добавить на экран Домой», прочитает ссылку без идентификатора, и
    // установленное приложение потеряет аккаунт (start_url свалится в "/").
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
      // Ответ персональный — общему кэшу его отдавать чужому человеку нельзя.
      const headers = new Headers(rewritten.headers);
      headers.set("Cache-Control", "no-store");
      return new Response(rewritten.body, {
        status: rewritten.status,
        statusText: rewritten.statusText,
        headers,
      });
    }

    // Всё остальное — статике (SPA-фолбэк держат Pages/_redirects).
    return res;
  },
};
