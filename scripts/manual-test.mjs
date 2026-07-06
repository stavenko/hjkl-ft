// Story manual-test launcher (headed, hands-on).
//
//   node manual-test.mjs <chapter>.<section> [baseUrl]
//
// Seeds a fresh user so the given section is the current unlocked one (every
// earlier section complete, every chapter up to it open), then opens the APP
// (root) in a headed browser — you navigate the UI yourself. The target
// section's own task is left undone, so the "do the task → next unlocks" flow is
// testable.
//
//   <chapter> = 1 | 2 | 3   <section> = 0-based index in that chapter
//   e.g.  node manual-test.mjs 2.0
//   MANUAL_TEST_PATH=/story/ch2-veg node manual-test.mjs 2.1   # land on a page
//   MANUAL_TEST_SHOT=/tmp/x.png    node manual-test.mjs 2.0    # headless screenshot
//
// Setup (once):  cd scripts && npm install && npx playwright install chromium

import { chromium } from "playwright";
import { DEFAULT_URL, parseTarget, buildPlan, openSeeded } from "./harness.mjs";

async function main() {
  const target = parseTarget(process.argv[2]);
  const baseUrl = process.argv[3] || process.env.MANUAL_TEST_URL || DEFAULT_URL;
  const plan = buildPlan(target);
  const uid = `manualtest-${target.chapter}-${target.section}`;

  console.log(`▶ ${baseUrl}`);
  console.log(`▶ unlocked section ch${target.chapter}.${target.section} = ${plan.targetId}`);
  console.log(`▶ flags:   ${plan.flags.join(", ") || "(none)"}`);
  console.log(`▶ seen:    ${plan.seen.join(", ") || "(none)"}`);
  console.log(`▶ sensors: ${plan.sensors.join(", ") || "(none)"}`);

  const shot = process.env.MANUAL_TEST_SHOT;
  const browser = await chromium.launch({ headless: !!shot });
  const { page } = await openSeeded(browser, {
    baseUrl, uid, plan, landing: process.env.MANUAL_TEST_PATH ?? "/",
  });
  if (shot) {
    page.on("pageerror", (e) => console.log("  [pageerror]", String(e).split("\n")[0]));
    await page.screenshot({ path: shot, fullPage: true });
    console.log(`\n✅ Seeded. Screenshot → ${shot}`);
    await browser.close().catch(() => {});
    return;
  }
  console.log(`\n✅ Seeded; "${plan.targetId}" is the current unlocked section. Close the window to exit.`);
  await page.waitForEvent("close", { timeout: 0 }).catch(() => {});
  await browser.close().catch(() => {});
}

main().catch((e) => {
  console.error("✗", e.message);
  process.exit(1);
});
