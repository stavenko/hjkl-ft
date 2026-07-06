import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";

const URL = process.argv[2];
const SCEN = process.argv[3] || "surplus"; // surplus | hard
const OUT = process.argv[4] || `card-${SCEN}.png`;
const DAYS = 14;

const browser = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(browser, {
  baseUrl: URL,
  landing: "/story/ch3-no-loss",
  seed: async (page, uid) => {
    await page.evaluate(async ({ uid, SCEN, DAYS }) => {
      const open = (n) => new Promise((res, rej) => { const r = indexedDB.open(n); r.onsuccess=()=>res(r.result); r.onerror=()=>rej(r.error); });
      const db = await open(`hjkl-ft-${uid}`);
      const now = new Date(); const nowIso = now.toISOString();
      const ymd = (o)=>{const d=new Date();d.setDate(d.getDate()-o);return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;};
      const put = (store, rows) => new Promise((res, rej) => { const tx=db.transaction([store],"readwrite"); const os=tx.objectStore(store); rows.forEach(r=>os.put(r)); tx.oncomplete=()=>res(); tx.onerror=()=>rej(tx.error); });

      const surplus = SCEN === "surplus";
      const intakeKcal = surplus ? 3300 : 900;
      // surplus: weight rises to today (today heaviest); hard: weight falls to today.
      const weight = (i) => surplus ? (80.0 - i*(1.0/13)) : (79.0 + i*(1.0/13));

      await put("app_flags", [
        { key:"push_onboarding_dismissed", value:"true" },
        { key:"paywall_skipped_date", value: ymd(0) },
        { key:"ft_subscription", value: JSON.stringify({plan:"monthly", end: now.getTime()+30*864e5, active:true, start:now.getTime(), status:"paid", no_renew:false, provider:"lava"}) },
      ]);
      await put("profile", [{ key:"profile", sex:"male", height_cm:180, birth_year:1990, updated_at: nowIso }]);
      await put("foods", [{ id:"cf", name:"Рацион дня", kcal:intakeKcal, protein:120, fat:90, carbs:300, nutrients:{}, package_weight:null, is_recipe:false, recipe_id:null, archived:false, is_restaurant:false, is_snack:false, created_at:nowIso, updated_at:nowIso }]);
      const W=[],S=[],D=[];
      for (let i=0;i<DAYS;i++){
        W.push({ id:`w${i}`, date:ymd(i), weight_kg:Math.round(weight(i)*100)/100, no_water:false,no_food:false,no_wash:false,used_toilet:false,morning:true, created_at:nowIso, updated_at:nowIso });
        S.push({ id:`s${i}`, date:ymd(i), steps:9000, created_at:nowIso, updated_at:nowIso });
        D.push({ id:`d${i}`, food_id:"cf", date:ymd(i), time:null, grams:100, waste_grams:0, meal_label:null, deleted:false, created_at:nowIso, updated_at:nowIso });
      }
      await put("weight_entries", W); await put("step_entries", S); await put("diary", D);
      if (surplus) await put("goals", [{ id:"g", nutrient:"Calories", key:"calories", direction:"AtMost", amount:3300, unit:"Kcal", period:"Day", created_at:nowIso, updated_at:nowIso }]);
      db.close();
    }, { uid, SCEN, DAYS });
  },
});
await page.waitForTimeout(1500);
await page.screenshot({ path: OUT, fullPage: true });
const body = await page.locator("body").innerText();
const m = body.match(/Сейчас ваш вес[^\n]*|проконсультироваться с врачом|Продолжайте вести дневник|укажите год рождения|Всё идёт/);
console.log(SCEN, "=>", m ? m[0].slice(0,60) : "(no card text)");
await context.close(); await browser.close();
