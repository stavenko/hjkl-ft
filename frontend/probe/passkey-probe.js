// Диагностика passkey: показывает ровно те сигналы, по которым приложение решает,
// предлагать ключ или запасной вход по коду, плюс НАСТОЯЩУЮ попытку создания
// (её ошибка — единственное, что говорит правду о браузере).
// Ничего никуда не отправляет; созданный ключ не используется.
const rows = [];
const add = (k, v, good) => { rows.push([k, v, good]); render(); };
const drows = [];
const dadd = (k, v, good) => { drows.push([k, v, good]); render(); };

const el = (id) => document.getElementById(id);
const tbl = (list) => list.map(([k, v, good]) =>
  `<tr><td>${k}</td><td class="v ${good === true ? "yes" : good === false ? "no" : ""}">${String(v)}</td></tr>`
).join("");

function render() {
  el("det").innerHTML = tbl(drows);
  el("t").innerHTML = rows.map(([k, v, good]) =>
    `<tr><td>${k}</td><td class="v ${good === true ? "yes" : good === false ? "no" : ""}">${String(v)}</td></tr>`
  ).join("");
  el("report").value = rows.map(([k, v]) => `${k}: ${v}`).join("\n")
    + "\n\nПопытка создания: " + el("out").textContent.replace(/\n/g, " | ");
}

const ua = navigator.userAgent;
const isAndroid = /Android/.test(ua);

// ── Ровно та же логика, что в приложении (pages/pwa_prompt.rs detect_platform) ──
{
  const u = ua.toLowerCase();
  const ios = u.includes("iphone") || u.includes("ipad") || u.includes("ipod");
  const android = u.includes("android");
  const mac = u.includes("macintosh") || u.includes("mac os");
  const chrome = u.includes("chrome") && !u.includes("edg") && !u.includes("opr");
  const firefox = u.includes("firefox");
  const edge = u.includes("edg/");
  const safari = u.includes("safari") && !u.includes("chrome") && !u.includes("chromium");
  const samsung = u.includes("samsungbrowser");
  // Яндекс.Браузер + встроенный браузер приложения Яндекс (в его UA нет
  // yabrowser, но есть chrome — без этих токенов он уходил бы в ветку Chrome).
  const yandex = u.includes("yabrowser") || u.includes("yasearchbrowser") || u.includes("yaapp_android");
  const platform =
    ios && safari ? "ios_safari" : ios && chrome ? "ios_chrome" : ios && firefox ? "ios_firefox" : ios ? "ios_safari" :
    android && samsung ? "android_samsung" : android && yandex ? "android_yandex" :
    android && firefox ? "android_firefox" : android ? "android_chrome" :
    mac && safari ? "macos_safari" : mac && chrome ? "macos_chrome" : mac && edge ? "macos_edge" :
    mac && firefox ? "macos_firefox" : mac ? "macos_chrome" :
    chrome ? "desktop_chrome" : edge ? "desktop_edge" : firefox ? "desktop_firefox" : "desktop_chrome";
  dadd("Токен yabrowser / yasearchbrowser / yaapp_android", yandex ? "найден" : "нет", yandex);
  dadd("Android определён", android ? "да" : "нет", android);
  dadd("Приложение определит платформу как", platform, platform === "android_yandex");
  dadd("Экран «как открыть в Chrome» покажется", platform === "android_yandex" ? "ДА" : "нет",
       platform === "android_yandex");
}
const hasPKC = typeof window.PublicKeyCredential !== "undefined";
add("User-Agent", ua);
add("Android в UA", isAndroid ? "да" : "нет", isAndroid);
add("Домен страницы", location.hostname);
add("PublicKeyCredential есть", hasPKC ? "да" : "нет", hasPKC);

const hasCreds = !!navigator.credentials;
const hasCreate = !!(navigator.credentials && typeof navigator.credentials.create === "function");
add("navigator.credentials есть", hasCreds ? "да" : "нет", hasCreds);
add("credentials.create — функция", hasCreate ? "да" : "нет", hasCreate);

(async () => {
  let uv = false;
  if (hasPKC) {
    try {
      uv = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
      add("Платформенный аутентификатор (IUVPAA)", uv ? "доступен" : "нет", uv);
    } catch (e) { add("IUVPAA", "ошибка: " + e.name, false); }
    try {
      const cm = PublicKeyCredential.isConditionalMediationAvailable
        ? await PublicKeyCredential.isConditionalMediationAvailable() : "метода нет";
      add("Автозаполнение ключом (conditional)", String(cm), cm === true);
    } catch (e) { add("conditional", "ошибка: " + e.name, false); }
  }
  // Два правила рядом: старое (только IUVPAA) и новое (плюс наличие самого
  // вызова create) — чтобы видеть, что именно меняется в этом браузере.
  add("СТАРОЕ правило (по IUVPAA)",
      (isAndroid && !uv) ? "ключ недоступен — вход по коду" : "ключ доступен — предложит ключ",
      !(isAndroid && !uv));
  add("НОВОЕ правило (+ credentials.create)",
      (isAndroid && (!uv || !hasCreate)) ? "ключ недоступен — вход по коду" : "ключ доступен — предложит ключ",
      !(isAndroid && (!uv || !hasCreate)));
})();

el("try").onclick = async () => {
  el("out").textContent = "Пробуем…"; render();
  const t0 = Date.now();
  try {
    const challenge = new Uint8Array(32); crypto.getRandomValues(challenge);
    const uid = new Uint8Array(16); crypto.getRandomValues(uid);
    const cred = await navigator.credentials.create({ publicKey: {
      challenge,
      rp: { name: "re:Norma probe", id: location.hostname },
      user: { id: uid, name: "probe@renorma", displayName: "probe" },
      pubKeyCredParams: [{ type: "public-key", alg: -7 }, { type: "public-key", alg: -257 }],
      authenticatorSelection: { authenticatorAttachment: "platform", userVerification: "required", residentKey: "required" },
      timeout: 60000,
      attestation: "none",
    }});
    el("out").textContent = `УСПЕХ за ${Date.now() - t0} мс\nтип: ${cred.type}\nid: ${cred.id.slice(0, 24)}…`;
  } catch (e) {
    el("out").textContent = `ОТКАЗ за ${Date.now() - t0} мс\nname: ${e.name}\nmessage: ${e.message}`;
  }
  render();
};

// Открыть ЭТУ ЖЕ страницу в системном Chrome (Android intent). Так видно, чем
// среда Chrome отличается от Яндекс.Браузера на том же телефоне. Если Chrome не
// установлен, browser_fallback_url возвращает на обычную https-ссылку.
{
  const target = location.href.split("#")[0];
  const host = location.host;
  const path = location.pathname + location.search;
  const fb = `S.browser_fallback_url=${encodeURIComponent(target)};`;
  // 1. Как в приложении: package + запасной адрес. Chromium отказывается
  //    запускать intent, если цель — обычный браузер, и тогда молча уходит по
  //    запасному адресу (выглядит как «открылось здесь же»).
  el("chrome").href = `intent://${host}${path}#Intent;scheme=https;package=com.android.chrome;${fb}end`;
  // 2. Явный компонент вместо package.
  el("chrome2").href = `intent://${host}${path}#Intent;scheme=https;` +
    `component=com.android.chrome/com.google.android.apps.chrome.Main;${fb}end`;
  // 3. Без запасного адреса: отказ виден как «ничего не произошло».
  el("chrome3").href = `intent://${host}${path}#Intent;scheme=https;action=android.intent.action.VIEW;` +
    `category=android.intent.category.BROWSABLE;package=com.android.chrome;end`;
  // 4. Собственная схема Chrome (официально iOS; на Android может не быть).
  el("chrome4").href = `googlechrome://navigate?url=${encodeURIComponent(target)}`;
  // 5. Последний кандидат: системный лист «Поделиться» — приложение выбирает
  //    пользователь, поэтому запрет Chromium на передачу другому браузеру здесь
  //    не действует.
  el("chrome5").href = `intent:#Intent;action=android.intent.action.SEND;type=text/plain;` +
    `S.android.intent.extra.TEXT=${encodeURIComponent(target)};end`;
  el("copylink").onclick = async () => {
    try { await navigator.clipboard.writeText(target); el("copylink").textContent = "Адрес скопирован"; }
    catch { el("copylink").textContent = target; }
  };
}

el("copy").onclick = async () => {
  try { await navigator.clipboard.writeText(el("report").value); el("copy").textContent = "Скопировано"; }
  catch { el("report").select(); el("copy").textContent = "Выделено — скопируйте вручную"; }
};
