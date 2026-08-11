// Кнопка «Открыть приложение» в Mini App обязана вести на ПЕРСОНАЛЬНУЮ ссылку,
// по которой человек сразу ставит PWA: корень с аккаунтом — `/?u=<user_id>`.
//
// Раньше она вела на голый корень: общий манифест со `start_url:"/"`, и ярлык,
// поставленный с такой страницы, не знает аккаунта.
//
// Прогоняется живой путь на dev: телеграм-личность → оплата через мок → POST
// /miniapp/me с ПОДПИСАННОЙ initData (на dev токен бота — публичная заглушка из
// wrangler.toml) → сверка отданных ссылок.
import crypto from "node:crypto";

const TG = process.env.TG_URL || "https://telegram-worker-dev.vg-stavenko.workers.dev";
const AUTH = process.env.AUTH || "https://auth-worker-dev.vg-stavenko.workers.dev";
const PAY = process.env.PAY || "https://payment-worker-dev.vg-stavenko.workers.dev";
const MOCK = process.env.LAVA_MOCK || "https://lava-mock-dev.vg-stavenko.workers.dev";
const IKEY = process.env.INTERNAL_KEY || "dev-internal-push-key";
const BOT = process.env.BOT_TOKEN || "dev-telegram-bot-token";
const ONBOARD = process.env.ONBOARD_URL || "https://renorma-fit-dev.pages.dev/onboard";

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};
const j = async (r) => { const t = await r.text(); try { return JSON.parse(t); } catch { return { _raw: t }; } };
const post = (u, b, h = {}) => fetch(u, {
  method: "POST", headers: { "Content-Type": "application/json", ...h }, body: JSON.stringify(b) });

function initData(id, username) {
  const user = JSON.stringify({ id, username, first_name: "E2E" });
  const fields = { auth_date: String(Math.floor(Date.now() / 1000)), user };
  const dcs = Object.keys(fields).sort().map((k) => `${k}=${fields[k]}`).join("\n");
  const secret = crypto.createHmac("sha256", "WebAppData").update(BOT).digest();
  const hash = crypto.createHmac("sha256", secret).update(dcs).digest("hex");
  return new URLSearchParams({ ...fields, hash }).toString();
}

const tg = 870000 + Math.floor(Math.random() * 100000);
const username = `link-${tg}`;
const { userId } = await j(await post(`${AUTH}/internal/account-resolve`,
  { provider: "telegram", providerUid: String(tg), username }, { "X-Internal-Key": IKEY }));
check("аккаунт заведён", !!userId, String(userId));

const co = await j(await post(`${PAY}/internal/checkout`,
  { tgUserId: tg, tgUsername: username, currency: "RUB", paymentMethod: "SBP" },
  { "X-Internal-Key": IKEY }));
await post(`${MOCK}/pay/confirm`, { contractId: new URL(co.payUrl).searchParams.get("oid") });

const me = await j(await post(`${TG}/miniapp/me`, { initData: initData(tg, username) }));
check("статус оплачен", me.status === "paid" || me.status === "claimed", String(me.status));
const ROOT = ONBOARD.replace(/\/onboard$/, "");
check("appUrl — это корень с аккаунтом", me.appUrl === `${ROOT}/?u=${userId}`, String(me.appUrl));
check("appUrl не ведёт в онбординг", !String(me.appUrl).includes("/onboard"), String(me.appUrl));
check("регистрационная ссылка осталась прежней",
  typeof me.onboardUrl === "string" && me.onboardUrl.startsWith(`${ONBOARD}?u=${userId}`)
    && /#code=\d{6}$/.test(me.onboardUrl),
  String(me.onboardUrl));

// Ссылка обязана открываться персональным манифестом — иначе ярлык потеряет аккаунт.
const html = await (await fetch(me.appUrl)).text();
const link = (/<link rel="manifest"[^>]*>/.exec(html) || [""])[0];
check("страница по appUrl отдаёт персональный манифест",
  link.includes(`/manifest.json?u=${userId}`), link);

console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
