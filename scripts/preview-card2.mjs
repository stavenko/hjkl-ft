import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";
const URL = process.argv[2], SCEN = process.argv[3], OUT = process.argv[4] || `card-${SCEN}.png`;
// scenario → [weightFn, intake, goal]
const flat = (i)=> 80.0 + (i%2===0? 0.05 : -0.05);
const rising = (i)=> 80.0 - i*(1.0/13);   // today heaviest
const cfg = {
  "plateau":        [flat,   2800, "lose"],
  "maintain-flat":  [flat,   2800, "maintain"],
  "maintain-rising":[rising, 3300, "maintain"],
}[SCEN];
const [wf, intake, goal] = cfg;
const b = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(b, { baseUrl: URL, landing: "/story/ch3-no-loss", seed: async (page, uid) => {
  await page.evaluate(async ({ uid, intake, goal, wvals }) => {
    const open=(n)=>new Promise((r,j)=>{const q=indexedDB.open(n);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error);});
    const db=await open(`hjkl-ft-${uid}`); const now=new Date(),iso=now.toISOString();
    const ymd=(o)=>{const d=new Date();d.setDate(d.getDate()-o);return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;};
    const put=(s,rows)=>new Promise((r,j)=>{const tx=db.transaction([s],"readwrite");const o=tx.objectStore(s);rows.forEach(x=>o.put(x));tx.oncomplete=()=>r();tx.onerror=()=>j(tx.error);});
    await put("app_flags",[{key:"push_onboarding_dismissed",value:"true"},{key:"paywall_skipped_date",value:ymd(0)},{key:"ft_subscription",value:JSON.stringify({plan:"monthly",end:now.getTime()+30*864e5,active:true,start:now.getTime(),status:"paid",no_renew:false,provider:"lava"})}]);
    await put("profile",[{key:"profile",sex:"male",height_cm:180,birth_year:1990,goal,updated_at:iso}]);
    await put("foods",[{id:"cf",name:"Рацион",kcal:intake,protein:120,fat:90,carbs:300,nutrients:{},package_weight:null,is_recipe:false,recipe_id:null,archived:false,is_restaurant:false,is_snack:false,created_at:iso,updated_at:iso}]);
    const W=[],S=[],D=[]; for(let i=0;i<14;i++){W.push({id:`w${i}`,date:ymd(i),weight_kg:Math.round(wvals[i]*100)/100,no_water:false,no_food:false,no_wash:false,used_toilet:false,morning:true,created_at:iso,updated_at:iso});S.push({id:`s${i}`,date:ymd(i),steps:9000,created_at:iso,updated_at:iso});D.push({id:`d${i}`,food_id:"cf",date:ymd(i),time:null,grams:100,waste_grams:0,meal_label:null,deleted:false,created_at:iso,updated_at:iso});}
    await put("weight_entries",W);await put("step_entries",S);await put("diary",D);
    await put("goals",[{id:"g",nutrient:"Calories",key:"calories",direction:"AtMost",amount:intake,unit:"Kcal",period:"Day",created_at:iso,updated_at:iso}]);
    db.close();
  }, { uid, intake, goal, wvals: Array.from({length:14},(_,i)=>wf(i)) });
}});
await page.waitForTimeout(1500);
await page.screenshot({ path: OUT, fullPage: true });
const body = await page.locator("body").innerText();
const m = body.match(/вес стоит[^\n]*|держится[^\n]*|немного растёт[^\n]*|снижается[^\n]*|растёт — вы в профиците[^\n]*/i);
const hasCal = body.includes("Калории:"); const hasSteps = body.includes("Шаги:");
console.log(SCEN, "=>", (m?m[0].slice(0,45):"(?)"), "| Калории:", hasCal, "| Шаги:", hasSteps);
await context.close(); await b.close();
