// Смоук-фронт (fit-smoke.renorma.app) — второй фронт БОЕВОГО бэкенда.
// Проверяем: домен отдаёт прод-конфигурацию, воркеры пускают этот origin, и
// церемония создания ключа проходит (rpId = renorma.app, origin — смоук).
// Ключ создаётся НАСТОЯЩИЙ, через виртуальный аутентификатор CDP.
import { chromium } from "playwright";
const SMOKE = process.env.SMOKE || "https://fit-smoke.renorma.app";
const AUTH = "https://auth.renorma.app";

let fail = 0;
const check = (name, ok, extra = "") => { console.log(`${ok ? "OK " : "FAIL"} ${name}${extra ? " — " + extra : ""}`); if (!ok) fail++; };

// ── Конфигурация домена ──
{
  const r = await fetch(`${SMOKE}/config/frontend.toml`);
  const cfg = await r.text();
  check("смоук отдаёт конфигурацию", r.ok, `HTTP ${r.status}`);
  check("адреса воркеров — боевые", cfg.includes("https://auth.renorma.app") && cfg.includes("https://pay.renorma.app"));
  check("app_origin — смоук-домен", cfg.includes('app_origin = "https://fit-smoke.renorma.app"'));
  const head = await fetch(`${SMOKE}/`);
  const csp = head.headers.get("content-security-policy") || "";
  check("CSP разрешает боевые воркеры", csp.includes("https://auth.renorma.app") && csp.includes("https://sync.renorma.app"));
}

// ── Боевой auth-воркер пускает этот origin (CORS) ──
{
  const r = await fetch(`${AUTH}/register/begin`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Origin: SMOKE },
    body: JSON.stringify({ origin: SMOKE, display_name: "smoke probe" }),
  });
  const allow = r.headers.get("access-control-allow-origin");
  check("auth-воркер пускает смоук-origin", allow === SMOKE, String(allow));
  const body = await r.json().catch(() => ({}));
  const rp = body?.publicKey?.rp?.id || body?.rp?.id;
  check("церемония выдаётся с rpId renorma.app", rp === "renorma.app", String(rp));
}

// ── Настоящая регистрация ключа на смоук-домене ──
{
  const b = await chromium.launch({ headless: true });
  const ctx = await b.newContext({ viewport: { width: 430, height: 932 } });
  const p = await ctx.newPage();
  // Виртуальный аутентификатор: платформенный, с проверкой пользователя —
  // как экран блокировки на телефоне.
  const cdp = await ctx.newCDPSession(p);
  await cdp.send("WebAuthn.enable");
  const { authenticatorId } = await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: { protocol: "ctap2", transport: "internal", hasResidentKey: true,
               hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  await p.goto(SMOKE, { waitUntil: "domcontentloaded" });
  await p.waitForTimeout(9000);
  // Регистрация доступна только в оплаченном онбординге, поэтому дергаем ту же
  // функцию, что и приложение: begin → браузер создаёт ключ → finish.
  const result = await p.evaluate(async (auth) => {
    const b64u = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf)))
      .replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    const r = await fetch(`${auth}/register/begin`, {
      method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ origin: location.origin, display_name: "smoke probe" }),
    });
    if (!r.ok) return { stage: "begin", error: `HTTP ${r.status}` };
    const opts = await r.json();
    const pk = opts.publicKey || opts;
    pk.challenge = Uint8Array.from(atob(pk.challenge.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
    pk.user.id = Uint8Array.from(atob(pk.user.id.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
    try {
      const cred = await navigator.credentials.create({ publicKey: pk });
      return { stage: "created", rpId: pk.rp.id, id: b64u(cred.rawId).slice(0, 12) };
    } catch (e) {
      return { stage: "create", error: `${e.name}: ${e.message}` };
    }
  }, AUTH);
  console.log("  результат церемонии:", JSON.stringify(result));
  check("ключ создан на смоук-домене", result.stage === "created", result.error || "");
  check("ключ выпущен для renorma.app (общий с fit.renorma.app)", result.rpId === "renorma.app");
  await cdp.send("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
  await b.close();
}

console.log(fail === 0 ? "\n=== ALL OK ===" : `\n=== FAILURES: ${fail} ===`);
process.exit(fail === 0 ? 0 : 1);
