// СОГЛАСИЯ ПЕРЕД ПЕРЕХОДОМ В БОТА — на живых лендингах.
//
// Продаём мы через Telegram, и до кнопки «Открыть в Telegram» человек обязан
// принять оферту и политику конфиденциальности. Проверяется не разметка, а
// ПОВЕДЕНИЕ: пока хоть одна галочка снята, переход не должен происходить.
//
// Заодно смотрим сами документы: они должны открываться и называть Cloudflare —
// данные лежат у него, и в политике это сказано прямо.
//
//   node scripts/check-landing-consent.mjs
import { chromium } from "playwright";

const SITES = [
  { name: "renorma.app", url: "https://renorma.app/", cta: ".js-pay, #pay-btn",
    offer: "#pm-consent-offer", privacy: "#pm-consent-privacy", go: "#pm-go" },
  { name: "ig0.renorma.app", url: "https://ig0.renorma.app/", cta: ".js-buy",
    offer: "#buy-c-offer", privacy: "#buy-c-privacy", go: "#buy-go" },
];
const DOCS = ["https://renorma.app/offer", "https://renorma.app/privacy"];

let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

const browser = await chromium.launch({ headless: true });

for (const s of SITES) {
  console.log(`\n— ${s.name} —`);
  const ctx = await browser.newContext({ viewport: { width: 430, height: 932 } });
  const page = await ctx.newPage();
  await page.goto(s.url, { waitUntil: "domcontentloaded" });
  await page.waitForTimeout(1500);

  // Кнопок «купить» на странице несколько; первая может быть за пределами экрана
  // (в закреплённой шапке или ниже сгиба) — берём ту, что реально видна.
  const buy = async () => {
    const all = page.locator(s.cta);
    for (let i = 0; i < await all.count(); i++) {
      if (await all.nth(i).isVisible()) return all.nth(i);
    }
    return all.first();
  };
  await (await buy()).click();
  await page.waitForTimeout(500);
  const go = page.locator(s.go);
  check("модалка открылась и кнопка видна", await go.isVisible());

  // Обе галочки сняты: переход обязан быть закрыт.
  const state = async () => ({
    disabled: await go.evaluate((el) => el.classList.contains("is-disabled")),
    aria: await go.getAttribute("aria-disabled"),
  });
  let st = await state();
  check("без галочек переход закрыт", st.disabled && st.aria === "true", JSON.stringify(st));

  // Одной галочки мало.
  await page.locator(s.offer).check();
  await page.waitForTimeout(200);
  st = await state();
  check("одной галочки мало", st.disabled && st.aria === "true", JSON.stringify(st));

  // ПОВЕДЕНИЕ, а не только класс: клик по закрытой кнопке никуда не ведёт.
  const before = page.url();
  await go.click({ force: true }).catch(() => {});
  await page.waitForTimeout(600);
  check("клик по закрытой кнопке никуда не ведёт", page.url() === before,
    `${before} → ${page.url()}`);

  // Обе — открывается.
  await page.locator(s.privacy).check();
  await page.waitForTimeout(200);
  st = await state();
  check("с обеими галочками переход открыт", !st.disabled && st.aria === "false",
    JSON.stringify(st));
  check("ведёт в бота оплаты",
    (await go.getAttribute("href") || "").includes("renorma_payment_helper_bot"),
    await go.getAttribute("href"));

  // Ссылки на документы — из самой модалки, а не «где-то в подвале».
  for (const [sel, word] of [[s.offer, "оферт"], [s.privacy, "конфиденциальн"]]) {
    const href = await page.locator(`${sel} ~ span a`).getAttribute("href");
    check(`рядом с галочкой ссылка на документ (${word})`, !!href, href ?? "нет ссылки");
  }

  // Галочки сбрасываются при повторном открытии: согласие даётся заново.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(300);
  await (await buy()).click();
  await page.waitForTimeout(400);
  check("при повторном открытии галочки сброшены",
    !(await page.locator(s.offer).isChecked()) && !(await page.locator(s.privacy).isChecked()));

  await ctx.close();
}

// ── Сами документы ──────────────────────────────────────────────────────────
console.log("\n— документы —");
const ctx = await browser.newContext();
const page = await ctx.newPage();
for (const url of DOCS) {
  const res = await page.goto(url, { waitUntil: "domcontentloaded" });
  check(`${url} открывается`, res?.status() === 200, `HTTP ${res?.status()}`);
}
await page.goto("https://renorma.app/privacy", { waitUntil: "domcontentloaded" });
const text = (await page.locator("body").innerText()).replace(/\s+/g, " ");
check("политика называет Cloudflare как обработчика", /Cloudflare/.test(text));
check("политика говорит о трансграничной передаче",
  /за пределами Российской Федерации/i.test(text));
await ctx.close();

await browser.close();
process.exit(failed ? 1 : 0);
