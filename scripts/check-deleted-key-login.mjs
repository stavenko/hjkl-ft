// Ключ удалён НА СЕРВЕРЕ, на устройстве остался (состояние после «Сбросить
// доступ» из админки). Пользователь жмёт «Войти» на экране входа: церемония
// проходит, сервер отвечает 401 passkey_not_found. Приложение обязано назвать
// причину, а НЕ показывать «Проверьте интернет» — интернет тут ни при чём.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
// На боевом домене rpId (renorma.app) не равен хосту страницы — ключ обязан
// быть выпущен под тот же rpId, иначе церемония до сервера не дойдёт.
const RP_ID = process.env.RP_ID || "";
// Третий блок заводит ОПЛАЧЕННОГО пользователя через внутренние ручки — это
// делается только в dev; на боевом бэкенде блок пропускается.
const SEED = process.env.SKIP_SEED !== "1";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

// ── Ручка: неизвестный ключ — это отказ (401), а не сбой сервера (500) ──
{
  const b = await chromium.launch({ headless: true });
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
  const p = await ctx.newPage();
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(3000);
  const cdp = await ctx.newCDPSession(p);
  await cdp.send("WebAuthn.enable");
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
               hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  const ghost = await p.evaluate(async (forced) => {
    const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
    const pkcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
    const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)));
    return { credentialId: b64(crypto.getRandomValues(new Uint8Array(32))),
             privateKey: b64(pkcs8), rpId: forced || location.hostname };
  }, RP_ID);
  await cdp.send("WebAuthn.addCredential", {
    authenticatorId,
    credential: { credentialId: ghost.credentialId, isResidentCredential: true, rpId: ghost.rpId,
                  privateKey: ghost.privateKey, userHandle: btoa("ghost-user"), signCount: 0 },
  });
  const out = await p.evaluate(async (auth) => {
    const b64u = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)))
      .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    const dec = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
    const opts = await (await fetch(`${auth}/authenticate/begin`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fingerprint: "check-fp" }) })).json();
    const pk = opts.publicKey || opts;
    pk.challenge = dec(pk.challenge);
    const cred = await navigator.credentials.get({ publicKey: pk });
    const r = await fetch(`${auth}/authenticate/finish`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ fingerprint: "check-fp", credential: {
        id: cred.id, rawId: b64u(cred.rawId), type: cred.type,
        response: { clientDataJSON: b64u(cred.response.clientDataJSON),
                    authenticatorData: b64u(cred.response.authenticatorData),
                    signature: b64u(cred.response.signature) } } }),
    });
    return { status: r.status, body: await r.text() };
  }, AUTH);
  check("неизвестный ключ — 401, а не 500", out.status === 401, `HTTP ${out.status}`);
  check("код ошибки машиночитаемый", out.body.includes("passkey_not_found"), out.body.slice(0, 80));
  await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
  await b.close();
}

// ── Интерфейс: что видит пользователь на экране входа ──
{
  const b = await chromium.launch({ headless: true });
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const p = await ctx.newPage();
  const errors = [];
  p.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(3000);
  const cdp = await ctx.newCDPSession(p);
  await cdp.send("WebAuthn.enable");
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
               hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  const ghost = await p.evaluate(async (forced) => {
    const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
    const pkcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
    const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)));
    return { credentialId: b64(crypto.getRandomValues(new Uint8Array(32))),
             privateKey: b64(pkcs8), rpId: forced || location.hostname };
  }, RP_ID);
  await cdp.send("WebAuthn.addCredential", {
    authenticatorId,
    credential: { credentialId: ghost.credentialId, isResidentCredential: true, rpId: ghost.rpId,
                  privateKey: ghost.privateKey, userHandle: btoa("ghost-user"), signCount: 0 },
  });
  // В браузере (не в установленном PWA) приложение сперва показывает экран
  // установки — до экрана входа не добраться. Отмечаем его как показанный.
  await p.evaluate(() => localStorage.setItem("pwa_dismissed", "1"));
  await p.reload({ waitUntil: "domcontentloaded" });
  await p.waitForTimeout(9000);

  const btn = p.locator('[data-testid="auth-btn-try-passkey"]');
  check("экран входа показан", await btn.isVisible().catch(() => false),
    (await p.locator("body").innerText().catch(() => "")).replace(/\s+/g, " ").slice(0, 70));
  await btn.click();
  await p.waitForTimeout(4000);

  const box = p.locator('[data-testid="auth-error"]');
  const msg = (await box.innerText().catch(() => "")).replace(/\s+/g, " ").trim();
  check("сообщение об ошибке показано", await box.isVisible().catch(() => false), msg);
  check("сказано, что ключ не найден и надо зарегистрироваться",
    msg.includes("не можем найти вашего ключа на сервере") && msg.includes("зарегистрироваться"), msg);
  check("про интернет ничего не сказано", !/интернет/i.test(msg), msg);
  check("причина есть в консоли", errors.some((e) => /passkey_not_found/.test(e)),
    errors.filter((e) => /auth/i.test(e)).slice(0, 2).join(" | ").slice(0, 120));

  await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
  await b.close();
}

// ── Онбординг: «Уже есть аккаунт? Войти», а ключа на сервере нет ──
if (SEED)
// Человек больше ничего не делает — значит, ему нужно сказать, что ключ не
// найден, и вернуть форму регистрации, а не оставить его на пустом экране.
{
  const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
  const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
  const KEY = process.env.INTERNAL_KEY || "dev-internal-push-key";
  const j = async (r) => JSON.parse(await r.text());
  const post = (u, bd, h = {}) => fetch(u, { method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(bd) });

  const tg = 830000 + Math.floor(Math.random() * 100000);
  const { userId } = await j(await post(`${AUTH}/internal/account-resolve`,
    { provider: "telegram", providerUid: String(tg), username: `dk-${tg}` }, { "X-Internal-Key": KEY }));
  const co = await j(await post(`${PAY}/internal/checkout`,
    { tgUserId: tg, tgUsername: `dk-${tg}`, currency: "RUB", paymentMethod: "SBP" }, { "X-Internal-Key": KEY }));
  await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });
  const { code } = await j(await post(`${AUTH}/internal/code/mint`, { userId }, { "X-Internal-Key": KEY }));

  const b = await chromium.launch({ headless: true });
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 }, serviceWorkers: "block" });
  const p = await ctx.newPage();
  await p.goto(FE, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(3000);
  const cdp = await ctx.newCDPSession(p);
  await cdp.send("WebAuthn.enable");
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
               hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  const ghost = await p.evaluate(async (forced) => {
    const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
    const pkcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
    const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)));
    return { credentialId: b64(crypto.getRandomValues(new Uint8Array(32))),
             privateKey: b64(pkcs8), rpId: forced || location.hostname };
  }, RP_ID);
  await cdp.send("WebAuthn.addCredential", {
    authenticatorId,
    credential: { credentialId: ghost.credentialId, isResidentCredential: true, rpId: ghost.rpId,
                  privateKey: ghost.privateKey, userHandle: btoa("ghost-user"), signCount: 0 },
  });

  await p.goto(`${FE}/onboard?u=${userId}#code=${code}`, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(14000);
  const reg = p.locator('[data-testid="onboard-btn-register"]');
  const login = p.locator('[data-testid="onboard-btn-login"]');
  const nameInput = p.locator('[data-testid="onboard-input-name"]');
  check("онбординг: экран создания ключа показан", await reg.isVisible().catch(() => false));

  await login.click();
  await p.waitForTimeout(5000);

  const msg = (await p.locator(".notification.is-danger").innerText().catch(() => "")).replace(/\s+/g, " ").trim();
  check("онбординг: сказано, что ключа нет и надо зарегистрироваться",
    msg === "Мы не можем найти вашего ключа на сервере. Вам придётся зарегистрироваться.", msg);
  check("онбординг: поле имени вернулось", await nameInput.isVisible().catch(() => false));
  check("онбординг: кнопка регистрации вернулась", await reg.isVisible().catch(() => false));
  check("онбординг: спиннер убран",
    !(await p.locator('[data-testid="onboard-signing-in"]').isVisible().catch(() => false)));
  // Форма не просто вернулась, а работает: имя вводится, кнопка оживает.
  await nameInput.fill("Вася");
  check("онбординг: регистрация снова доступна", !(await reg.isDisabled().catch(() => true)),
    `disabled=${await reg.isDisabled().catch(() => "?")}`);

  await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
  await b.close();
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
