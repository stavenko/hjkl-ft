import { chromium } from "playwright";
import { openSeeded } from "./harness.mjs";
const URL = process.argv[2];
const browser = await chromium.launch({ headless: true });
const { context, page } = await openSeeded(browser, {
  baseUrl: URL, landing: "/story/ch3-no-loss",
  seed: async (page, uid) => {
    await page.evaluate(async ({ uid }) => {
      const open=(n)=>new Promise((r,j)=>{const q=indexedDB.open(n);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error);});
      const db=await open(`hjkl-ft-${uid}`); const now=new Date(),iso=now.toISOString();
      const ymd=(o)=>{const d=new Date();d.setDate(d.getDate()-o);return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;};
      const put=(s,rows)=>new Promise((r,j)=>{const tx=db.transaction([s],"readwrite");const o=tx.objectStore(s);rows.forEach(x=>o.put(x));tx.oncomplete=()=>r();tx.onerror=()=>j(tx.error);});
      await put("app_flags",[{key:"push_onboarding_dismissed",value:"true"},{key:"paywall_skipped_date",value:ymd(0)},{key:"ft_subscription",value:JSON.stringify({plan:"monthly",end:now.getTime()+30*864e5,active:true,start:now.getTime(),status:"paid",no_renew:false,provider:"lava"})}]);
      await put("profile",[{key:"profile",sex:"male",height_cm:180,birth_year:1990,updated_at:iso}]);
      await put("foods",[{id:"cf",name:"Рацион дня",kcal:3300,protein:120,fat:90,carbs:300,nutrients:{},package_weight:null,is_recipe:false,recipe_id:null,archived:false,is_restaurant:false,is_snack:false,created_at:iso,updated_at:iso}]);
      const W=[],S=[],D=[];
      for(let i=0;i<14;i++){W.push({id:`w${i}`,date:ymd(i),weight_kg:Math.round((80.0-i*(1.0/13))*100)/100,no_water:false,no_food:false,no_wash:false,used_toilet:false,morning:true,created_at:iso,updated_at:iso});S.push({id:`s${i}`,date:ymd(i),steps:9000,created_at:iso,updated_at:iso});D.push({id:`d${i}`,food_id:"cf",date:ymd(i),time:null,grams:100,waste_grams:0,meal_label:null,deleted:false,created_at:iso,updated_at:iso});}
      await put("weight_entries",W);await put("step_entries",S);await put("diary",D);
      await put("goals",[{id:"g",nutrient:"Calories",key:"calories",direction:"AtMost",amount:3300,unit:"Kcal",period:"Day",created_at:iso,updated_at:iso}]);
      db.close();
    },{uid});
  },
});
await page.getByRole("button",{name:/Шаги:/}).click();
await page.waitForTimeout(900);
const goal = await page.evaluate(async () => {
  const open=(n)=>new Promise((r,j)=>{const q=indexedDB.open(n);q.onsuccess=()=>r(q.result);q.onerror=()=>j(q.error);});
  const uid=localStorage.getItem("user_id"); const db=await open(`hjkl-ft-${uid}`);
  const all=await new Promise((r)=>{const tx=db.transaction(["goals"],"readonly");const req=tx.objectStore("goals").getAll();req.onsuccess=()=>r(req.result);});
  return all.find(g=>g.nutrient==="Steps")||null;
});
console.log("Steps goal after click:", JSON.stringify(goal));
await context.close(); await browser.close();
