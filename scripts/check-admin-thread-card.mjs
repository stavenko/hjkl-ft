// Из переписки в админке должна открываться карточка того же человека — с кнопкой
// «Сбросить доступ (онбординг заново)».
//
// Проверяется живой путь: заводим НАСТОЯЩИЙ аккаунт в auth-worker, пишем от его
// имени в поддержку, открываем тред как эксперт, жмём «Карточка» и смотрим, что
// открылась карточка ИМЕННО этого user_id и в ней есть кнопка сброса.
//
// Аутентификация эксперта обходится вбросом JWT в localStorage — ровно как в
// scripts/admin-smoke.mjs; под проверкой не пасскей, а связка тред → карточка.
//
// Запуск из корня репозитория; admin/dist должен быть собран.
import { createHmac, randomUUID } from "node:crypto";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import pw from "../e2e/node_modules/@playwright/test/index.js";
const { chromium } = pw;

// Тот же воркер, что прописан в admin/config/frontend.toml, — иначе засеянный
// разговор уедет в другой и в очереди не появится.
const SUPPORT = process.env.SUPPORT_BASE_URL ?? "https://support-worker-dev.vg-stavenko.workers.dev";
const AUTH = process.env.AUTH ?? "https://auth-worker-dev.vg-stavenko.workers.dev";
const SECRET = process.env.SUPPORT_JWT_SECRET ?? "dev-secret-change-in-production";
const EXPERT_ID = process.env.SUPPORT_EXPERT_ID ?? "expert-1";
const IKEY = process.env.INTERNAL_KEY ?? "dev-internal-push-key";
const DIST = new URL("../admin/dist/", import.meta.url).pathname;
const PORT = 3702;

const b64 = (b) => Buffer.from(b).toString("base64url");
const mint = (sub) => {
  const h = b64(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const c = b64(JSON.stringify({ sub, iat: 0, exp: 4_102_444_800, caps: [], token_id: "check" }));
  const si = `${h}.${c}`;
  return `${si}.${b64(createHmac("sha256", SECRET).update(si).digest())}`;
};

const MIME = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm",
  ".toml": "text/plain", ".css": "text/css", ".json": "application/json" };
const staticServer = () => createServer(async (req, res) => {
  try {
    let p = decodeURIComponent(req.url.split("?")[0]);
    if (p === "/") p = "/index.html";
    const file = normalize(join(DIST, p));
    if (!file.startsWith(DIST)) { res.writeHead(403).end(); return; }
    const body = await readFile(file);
    res.writeHead(200, { "Content-Type": MIME[extname(file)] ?? "application/octet-stream" });
    res.end(body);
  } catch { res.writeHead(404).end("not found"); }
});

let fail = 0;
const check = (n, ok, extra = "") => {
  console.log(`${ok ? "OK  " : "FAIL"} ${n}${extra ? " — " + extra : ""}`);
  if (!ok) fail++;
};

const adminUrl = process.env.ADMIN_URL;
let server = null;
if (!adminUrl) {
  server = staticServer();
  await new Promise((r) => server.listen(PORT, r));
}
const baseUrl = adminUrl ?? `http://localhost:${PORT}/`;

// Настоящий аккаунт: карточку сервер собирает по существующему user_id.
const tg = 880000 + Math.floor(Math.random() * 100000);
const resolve = await fetch(`${AUTH}/internal/account-resolve`, {
  method: "POST",
  headers: { "Content-Type": "application/json", "X-Internal-Key": IKEY },
  body: JSON.stringify({ provider: "telegram", providerUid: String(tg), username: `card-${tg}` }),
});
const { userId } = await resolve.json();
check("аккаунт заведён", !!userId, String(userId));

const userMsg = `вопрос из проверки карточки ${Date.now()}`;
const seed = await fetch(`${SUPPORT}/message`, {
  method: "POST",
  headers: { Authorization: `Bearer ${mint(userId)}`, "Content-Type": "application/json" },
  body: JSON.stringify({ client_id: randomUUID(), text: userMsg }),
});
check("сообщение в поддержку отправлено", seed.ok, `${seed.status}`);

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 900, height: 900 } });
const page = await ctx.newPage();
await page.addInitScript((tok) => localStorage.setItem("auth_token", tok), mint(EXPERT_ID));
const errors = [];
page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });

await page.goto(baseUrl);
const row = page.getByTestId("conv").filter({ hasText: userMsg });
await row.waitFor({ state: "visible", timeout: 20_000 });
await row.click();
await page.getByTestId("msg").filter({ hasText: userMsg }).waitFor({ state: "visible", timeout: 15_000 });

const cardBtn = page.getByTestId("thread-user-card");
check("в шапке переписки есть кнопка карточки", await cardBtn.count() > 0);
await cardBtn.click();

const modal = page.getByTestId("user-modal");
await modal.waitFor({ state: "visible", timeout: 15_000 });
const modalText = (await modal.innerText()).replace(/\s+/g, " ");
check("карточка открылась на ЭТОМ пользователе", modalText.includes(userId), modalText.slice(0, 80));
check("в карточке есть сброс онбординга", await page.getByTestId("user-reset").count() > 0);
await page.getByTestId("user-reset").click();
check("сброс спрашивает подтверждение", await page.getByTestId("reset-confirm").count() > 0);
check("ошибок в консоли нет", errors.length === 0, errors.join(" | ").slice(0, 200));

await page.screenshot({ path: "/tmp/admin-thread-card.png", fullPage: true });
console.log("снято: /tmp/admin-thread-card.png");

await browser.close();
if (server) server.close();
console.log(fail ? `\n${fail} провалов` : "\nвсё сошлось");
process.exit(fail ? 1 : 0);
