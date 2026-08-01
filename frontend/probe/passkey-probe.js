// Диагностика passkey: показывает ровно те сигналы, по которым приложение решает,
// предлагать ключ или запасной вход по коду, плюс НАСТОЯЩУЮ попытку создания
// (её ошибка — единственное, что говорит правду о браузере).
// Ничего никуда не отправляет; созданный ключ (если создастся) не используется.
const rows = [];
const add = (k, v, good) => rows.push([k, v, good]);

const ua = navigator.userAgent;
add("User-Agent", ua);
add("Android в UA", /Android/.test(ua) ? "да" : "нет", /Android/.test(ua));
const hasPKC = typeof window.PublicKeyCredential !== "undefined";
add("PublicKeyCredential есть", hasPKC ? "да" : "нет", hasPKC);

const render = () => {
  document.getElementById("t").innerHTML = rows.map(([k, v, good]) =>
    `<tr><td>${k}</td><td class="v ${good === true ? "yes" : good === false ? "no" : ""}">${String(v)}</td></tr>`
  ).join("");
};
render();

(async () => {
  if (hasPKC) {
    try {
      const uv = await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
      add("Платформенный аутентификатор (IUVPAA)", uv ? "доступен" : "нет", uv);
    } catch (e) { add("IUVPAA", "ошибка: " + e.name, false); }
    try {
      const cm = PublicKeyCredential.isConditionalMediationAvailable
        ? await PublicKeyCredential.isConditionalMediationAvailable() : "метода нет";
      add("Автозаполнение ключом (conditional)", String(cm), cm === true);
    } catch (e) { add("conditional", "ошибка: " + e.name, false); }
    add("Приложение сочло бы ключ недоступным",
        (/Android/.test(ua) && !(await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable().catch(() => false)))
          ? "да — покажет вход по коду" : "нет — предложит ключ");
  }
  render();
})();

document.getElementById("try").onclick = async () => {
  const out = document.getElementById("out");
  out.textContent = "Пробуем…";
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
    out.textContent = `УСПЕХ за ${Date.now() - t0} мс\nтип: ${cred.type}\nid: ${cred.id.slice(0, 24)}…`;
  } catch (e) {
    out.textContent = `ОТКАЗ за ${Date.now() - t0} мс\nname: ${e.name}\nmessage: ${e.message}`;
  }
};
