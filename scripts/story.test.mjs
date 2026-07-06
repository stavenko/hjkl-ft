// Automated tests for the story changes made this session, on top of the seed
// harness. Run headless against a deployed build:
//
//   cd scripts
//   npm install && npx playwright install chromium     # once
//   npm test                                           # or: node --test story.test.mjs
//   TEST_URL=https://<hash>.hjkl-ft.pages.dev npm test # a specific build
//
// Each test seeds a fresh, isolated browser context (own IndexedDB/localStorage),
// navigates to the relevant page, and asserts the resulting UI.

import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { chromium } from "playwright";
import { DEFAULT_URL, FLAG, planFor, openSeeded } from "./harness.mjs";

const BASE = process.env.TEST_URL || DEFAULT_URL;
let browser;

before(async () => {
  browser = await chromium.launch({ headless: true });
});
after(async () => {
  await browser?.close();
});

// Seed + open a page; runs `body(page)` then always tears the context down.
async function withPage(opts, body) {
  const { context, page } = await openSeeded(browser, { baseUrl: BASE, ...opts });
  try {
    return await body(page);
  } finally {
    await context.close().catch(() => {});
  }
}

const text = (page) => page.locator("body").innerText();
const hasLink = (page, href) => page.locator(`a[href="${href}"]`).count();

// ── Section page: heading size toned down (was is-size-1 → now is-size-3) ──
test("section heading uses is-size-3 (not the huge is-size-1)", async () => {
  await withPage({ plan: planFor("2.0"), landing: "/story/ch2-mistake" }, async (page) => {
    assert.equal(await page.locator("h1.is-size-3").count(), 1, "title should be is-size-3");
    assert.equal(await page.locator("h1.is-size-1").count(), 0, "title should not be is-size-1");
  });
});

// ── Story hub: per-section icons restored ──
test("story hub shows section icons", async () => {
  await withPage({ plan: planFor("3.0"), landing: "/" }, async (page) => {
    const body = await text(page);
    for (const icon of ["💰", "🥦", "🔍"]) {
      assert.ok(body.includes(icon), `hub should show the ${icon} section icon`);
    }
  });
});

// ── ch2-mistake: "keep the diary 7 MORE days", counted fresh from opening, NON-gating ──
test("ch2-mistake task is the fresh +7-day diary streak (0/7 on open), non-gating", async () => {
  await withPage({ plan: planFor("2.0"), landing: "/story/ch2-mistake" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("ещё 7 дней"), 'task should say "ещё 7 дней"');
    assert.ok(body.includes("0/7"), "fresh streak should read 0/7 right after opening");
    assert.ok(body.includes("Готово"), "section is non-gating → next section opens on open");
  });
});

// ── ch2-veg: weekly veg task with the sex-specific amount, NON-gating ──
test("ch2-veg task shows the male target (600 g) and does not gate progress", async () => {
  await withPage({ plan: planFor("2.1"), landing: "/story/ch2-veg", sex: "male" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("Каждый день съедайте и записывайте"), "daily veg task present");
    assert.ok(body.includes("600 г овощей"), "male target is 600 g");
    assert.ok(!body.includes("400 г"), "the other sex's number must NOT be shown");
    assert.ok(body.includes("0/7"), "streak starts fresh (0/7) on opening");
    assert.ok(body.includes("Готово"), "section is non-gating → marked done");
  });
});

test("ch2-veg task shows the female target (400 g)", async () => {
  await withPage({ plan: planFor("2.1"), landing: "/story/ch2-veg", sex: "female" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("400 г овощей"), "female target is 400 g");
    assert.ok(!body.includes("600 г"), "the other sex's number must NOT be shown");
  });
});

// ── ch2-protein: weekly task with the CALCULATED target (1.2 g/kg of weight) ──
test("ch2-protein task shows the calculated protein target and is non-gating", async () => {
  // Seeded weight = 80 kg → 1.2×80 = 96 g (exact, no rounding to tens).
  await withPage({ plan: planFor("2.2"), landing: "/story/ch2-protein" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("Каждый день съедайте и записывайте"), "daily protein task present");
    assert.ok(body.includes("96 г белка"), "target = 1.2 g/kg of weight = 96 g");
    assert.ok(body.includes("0/7"), "streak starts fresh (0/7) on opening");
    assert.ok(body.includes("Готово"), "section is non-gating → marked done");
  });
});

// ── ch2-drinks: week-long "no liquid calories" streak, fresh 0/7, non-gating ──
test("ch2-drinks shows the week-long liquid-calorie streak (0/7 on open), non-gating", async () => {
  await withPage({ plan: planFor("2.4"), landing: "/story/ch2-drinks" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("жидких калорий"), "weekly liquid-calorie task present");
    assert.ok(body.includes("0/7"), "fresh streak reads 0/7 right after opening");
    assert.ok(!body.includes("✅"), "task not marked done");
    assert.ok(body.includes("Готово"), "section is non-gating → next section opens on open");
  });
});

// ── ch2-night: "two protein-rich dinners" count, fresh 0/2, non-gating ──
test("ch2-night shows the two-dinners protein task (0/2 on open), non-gating", async () => {
  await withPage({ plan: planFor("2.6"), landing: "/story/ch2-night" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("Еда на ночь"), "night section title present");
    assert.ok(body.includes("треть или более дневной нормы белка"), "two-dinners protein task present");
    assert.ok(body.includes("0/2"), "fresh count reads 0/2 right after opening");
    assert.ok(!body.includes("✅"), "task not marked done");
    assert.ok(body.includes("Готово"), "section is non-gating → chapter advances on open");
  });
});

// ── ch3-deficit: calorie planka computed live (7-day avg), accept button sets it ──
test("ch3-deficit shows the computed calorie planka with an accept button", async () => {
  // Seeded diary = 2895 kcal/day; seeded weight rises → maintenance/surplus → −5%:
  // 2895 × 0.95 = 2750.25 → rounded to 10 = 2750 (balance-adjusted planka).
  await withPage({ plan: planFor("3.0"), landing: "/story/ch3-deficit" }, async (page) => {
    // The widget mounts via an async effect — wait for its accept button.
    await page.getByRole("button", { name: /Принять планку/ }).waitFor({ timeout: 8000 });
    const body = await text(page);
    // Formatted like other chapters: the standard "ЗАДАНИЕ" label (uppercased via CSS).
    assert.ok(body.toLowerCase().includes("задание"), "standard task label present");
    assert.ok(body.includes("Отныне необходимо есть"), "task intro text present");
    assert.ok(body.includes("Принять планку 2750 ккал"), "planka = 7-day avg adjusted for weight balance");
    assert.ok(!body.includes("Ваша ежедневная планка по калориям"), "confirmation not shown before accepting");
  });
});

test("ch3-deficit: pressing accept sets the planka", async () => {
  await withPage({ plan: planFor("3.0"), landing: "/story/ch3-deficit" }, async (page) => {
    await page.getByRole("button", { name: /Принять планку/ }).click();
    await page.waitForTimeout(900);
    const body = await text(page);
    assert.ok(body.includes("Ваша ежедневная планка по калориям"), "confirmation text appears after pressing accept");
    assert.ok(body.includes("приближаться к ней максимально"), "the instruction note appears too");
    // After accepting, the label, the big number card and the button are all gone.
    assert.ok(!body.toLowerCase().includes("дневная планка калорий"), "the label is removed after accepting");
    assert.ok(!body.includes("Принять планку"), "the accept button is removed after accepting");
    assert.ok(!/2750\s*ккал/.test(body), "the big number card is removed after accepting");
  });
});

// ── Hub "active tasks" list substitutes the {n} target (no raw placeholder) ──
test("hub active-tasks list fills the {n} target", async () => {
  // 2.3 → ch2-veg & ch2-protein are opened (seen) → their tasks are active.
  await withPage({ plan: planFor("2.3"), landing: "/", sex: "male" }, async (page) => {
    const body = await text(page);
    assert.ok(!body.includes("{n}"), "the {n} placeholder must be substituted");
    assert.ok(body.includes("600 г овощей"), "veg target filled in the active list");
    assert.ok(body.includes("96 г белка"), "protein target filled in the active list");
  });
});

// ── "Зачем вести дневник?" absorbs "Облегчаем подсчёт" + has the repeat task ──
test("diary section merges 'Облегчаем подсчёт' and has the repeat-food task", async () => {
  await withPage({ plan: planFor("1.8"), landing: "/story/diary" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("Облегчаем подсчёт"), "sub-heading merged into the diary section");
    assert.ok(body.includes("Повторите вчерашнюю еду"), "repeat-food task present");
  });
});

// ── Chapter 2 unlocks ONLY with weight+steps+diary 7-day streaks (+ sub) ──
test("chapter 2 unlocks when all three streaks + subscription are present", async () => {
  await withPage({ plan: planFor("2.0"), landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/ch2-mistake"), 1, "ch2 should be unlocked");
  });
});

test("chapter 2 stays locked without the diary streak", async () => {
  const p = planFor("2.0");
  const noDiary = { ...p, sensors: p.sensors.filter((s) => s !== "diaryStreak") };
  await withPage({ plan: noDiary, landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/ch2-mistake"), 0, "ch2 must be locked without diary streak");
  });
});

// ── Chapter 3 unlocks once the diary has a 7-day-in-a-row streak ──
test("chapter 3 unlocks with a 7-day diary streak", async () => {
  await withPage({ plan: planFor("3.0"), landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/ch3-deficit"), 1, "ch3 should be unlocked");
  });
});

test("chapter 3 stays locked without the diary streak", async () => {
  const p = planFor("3.0");
  const noDiary = { ...p, sensors: p.sensors.filter((s) => s !== "diaryStreak") };
  await withPage({ plan: noDiary, landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/ch3-deficit"), 0, "ch3 must be locked without a 7-day diary streak");
  });
});

// ── ch3-no-loss: weekly steps-planka task (7000/day), fresh 0/7, non-gating ──
test("ch3-no-loss shows the weekly steps-planka task (0/7 on open), non-gating", async () => {
  await withPage({ plan: planFor("3.1"), landing: "/story/ch3-no-loss" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("планку по шагам"), "steps-planka task present");
    assert.ok(body.includes("7000"), "planka target is 7000 steps");
    assert.ok(body.includes("0/7"), "fresh streak reads 0/7 right after opening");
    assert.ok(body.includes("Готово"), "section is non-gating → next section opens on open");
  });
});

// ── ch3-calorie: weekly 5/5-quality weigh-in task, fresh 0/7, non-gating ──
test("ch3-calorie shows the weekly 5/5 weigh-in task (0/7 on open), non-gating", async () => {
  await withPage({ plan: planFor("3.2"), landing: "/story/ch3-calorie" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("с качеством 5/5"), "5/5 weigh-in task present");
    assert.ok(body.includes("0/7"), "fresh streak reads 0/7 right after opening");
    assert.ok(body.includes("Готово"), "section is non-gating → next section opens on open");
  });
});

// ── ch3-friend: informational section renders its prose ──
test("ch3-friend renders the 'overestimate' lesson", async () => {
  await withPage({ plan: planFor("3.3"), landing: "/story/ch3-friend" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("переоценивают"), "friend section prose present");
    assert.ok(body.includes("проблема симметрична"), "symmetry paragraph present");
  });
});

// ── ch3-sleep: informational section renders its prose ──
test("ch3-sleep renders the sleep lesson", async () => {
  await withPage({ plan: planFor("3.4"), landing: "/story/ch3-sleep" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("фактор контроля аппетита"), "sleep section prose present");
    assert.ok(body.includes("7–9 часов"), "the 7–9 hours recommendation present");
  });
});

// ── ch3-walk: prose renders, and **bold**/*italic* markers are parsed (not literal) ──
test("ch3-walk renders the activity lesson with parsed markdown", async () => {
  await withPage({ plan: planFor("3.5"), landing: "/story/ch3-walk" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("важнейшая часть жизни"), "walk section prose present");
    assert.ok(body.includes("Низкая интенсивность"), "intensity classification present");
    assert.ok(!body.includes("**"), "bold markers parsed, not shown literally");
  });
});

// ── ch3-habits: prose renders with parsed markdown ──
test("ch3-habits renders the habits lesson", async () => {
  await withPage({ plan: planFor("3.6"), landing: "/story/ch3-habits" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("К ожирению приводит"), "habits section prose present");
    assert.ok(body.includes("по очереди"), "one-at-a-time paragraph present");
    assert.ok(!body.includes("**"), "bold markers parsed, not shown literally");
  });
});

// ── Settings: entering height yields a BMI from the latest weight ──
test("settings: entering height shows the BMI", async () => {
  // Seeded latest weight = 80 kg; height 180 cm → BMI = 80 / 1.8² = 24.7.
  await withPage({ plan: planFor("2.0"), landing: "/settings" }, async (page) => {
    const input = page.locator('[data-testid="settings-input-height"]');
    await input.waitFor({ timeout: 8000 });
    await input.fill("180");
    await input.blur();
    await page.waitForTimeout(400);
    const body = await text(page);
    assert.ok(body.includes("Ваш ИМТ: 24.7"), "BMI computed from height + latest weight");
  });
});

// ── Profile: birth year persists in the synced profile store ──
test("settings: birth year persists across reload (synced profile)", async () => {
  await withPage({ plan: planFor("2.0"), landing: "/settings" }, async (page) => {
    const sel = '[data-testid="settings-input-birth-year"]';
    await page.locator(sel).waitFor({ timeout: 8000 });
    await page.locator(sel).fill("1990");
    await page.locator(sel).blur();
    await page.waitForTimeout(600);
    await page.goto(page.url(), { waitUntil: "domcontentloaded" });
    await page.locator(sel).waitFor({ timeout: 8000 });
    await page.waitForTimeout(600);
    assert.equal(await page.locator(sel).inputValue(), "1990", "birth year persisted");
  });
});

// ── ch1 accounting carries the (non-gating) age task ──
test("accounting section shows the age task", async () => {
  await withPage({ plan: planFor("1.2"), landing: "/story/accounting" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("Укажите год рождения"), "age task present in accounting");
  });
});

// ── Weekly card: with no birth year it asks for it (no silent default, §7.4) ──
test("weekly card asks for the birth year when unset", async () => {
  await withPage({ plan: planFor("3.1"), landing: "/story/ch3-no-loss" }, async (page) => {
    const body = await text(page);
    assert.ok(body.includes("укажите год рождения"), "card asks for birth year (§7.4: no silent default)");
  });
});

// ── setup section now gates on ALL THREE (sex + lang + notif) ──
test("setup gates the next section on sex too", async () => {
  const base = { seen: ["intro", "setup"], sensors: [] };
  // lang + notif done, sex NOT → setup incomplete → accounting locked.
  await withPage({ plan: { ...base, flags: [FLAG.photos, FLAG.lang, FLAG.notif] }, landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/accounting"), 0, "accounting locked while sex is undone");
  });
  // + sex → setup complete → accounting unlocked.
  await withPage({ plan: { ...base, flags: [FLAG.photos, FLAG.lang, FLAG.notif, FLAG.sex] }, landing: "/" }, async (page) => {
    assert.equal(await hasLink(page, "/story/accounting"), 1, "accounting unlocks once sex is set");
  });
});
