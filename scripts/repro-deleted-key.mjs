// Воспроизведение: ключ УДАЛЁН НА СЕРВЕРЕ, а на устройстве он остался.
// Ровно это состояние даёт «Сбросить доступ» из админки. Пользователь жмёт
// «Войти», церемония проходит (ключ на устройстве есть), а сервер такого ключа
// не знает. Смотрим, что реально отвечает /authenticate/finish.
import { chromium } from "playwright";
const FE = process.env.FE || "https://renorma-fit-dev.pages.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";

const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
const p = await ctx.newPage();
p.on("console", (m) => console.log("[page]", m.text().slice(0, 200)));
await p.goto(FE, { waitUntil: "domcontentloaded" });
await p.waitForTimeout(3000);

const cdp = await ctx.newCDPSession(p);
await cdp.send("WebAuthn.enable");
const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
  options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
             hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
});

// Ключ, которого сервер никогда не видел (или уже забыл).
// rpId ключа обязан совпадать с тем, что выдаёт сервер (на смоуке домен и rpId
// различаются: renorma.app против fit-smoke.renorma.app).
const RP_ID = process.env.RP_ID || "";
const { credentialId, privateKey, rpId } = await p.evaluate(async (forced) => {
  const kp = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
  const b64 = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)));
  const id = crypto.getRandomValues(new Uint8Array(32));
  return { credentialId: b64(id), privateKey: b64(pkcs8), rpId: forced || location.hostname };
}, RP_ID);
await cdp.send("WebAuthn.addCredential", {
  authenticatorId,
  credential: { credentialId, isResidentCredential: true, rpId, privateKey,
                userHandle: btoa("ghost-user"), signCount: 0 },
});

// Ровно те же два запроса, что делает приложение в auth::authenticate().
const out = await p.evaluate(async (auth) => {
  const b64u = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)))
    .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  const dec = (s) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
  const r1 = await fetch(`${auth}/authenticate/begin`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ fingerprint: "repro-fp" }),
  });
  const beginStatus = r1.status;
  const opts = await r1.json();
  const pk = opts.publicKey || opts;
  pk.challenge = dec(pk.challenge);
  let cred;
  try {
    cred = await navigator.credentials.get({ publicKey: pk });
  } catch (e) {
    return { beginStatus, ceremony: `${e.name}: ${e.message}` };
  }
  const body = {
    credential: {
      id: cred.id, rawId: b64u(cred.rawId), type: cred.type,
      response: {
        clientDataJSON: b64u(cred.response.clientDataJSON),
        authenticatorData: b64u(cred.response.authenticatorData),
        signature: b64u(cred.response.signature),
      },
    },
    fingerprint: "repro-fp",
  };
  const r2 = await fetch(`${auth}/authenticate/finish`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return { beginStatus, ceremony: "ok", finishStatus: r2.status, finishBody: (await r2.text()).slice(0, 300) };
}, AUTH);

console.log(JSON.stringify(out, null, 2));
await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
await b.close();
