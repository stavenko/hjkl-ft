import { chromium } from "playwright";
const FE = process.argv[2];
const uid = "vf-" + Date.now();
const b = await chromium.launch({ headless: true });
const ctx = await b.newContext({ viewport: { width: 430, height: 920 }, serviceWorkers: "block", deviceScaleFactor: 2 });
const page = await ctx.newPage();
await page.goto(FE, { waitUntil: "domcontentloaded" });
await page.evaluate(async (uid) => { const del=(n)=>new Promise(r=>{const q=indexedDB.deleteDatabase(n);q.onsuccess=q.onerror=q.onblocked=()=>r();}); await del("hjkl-ft"); await del(`hjkl-ft-${uid}`); localStorage.clear(); localStorage.setItem("user_id",uid); localStorage.setItem("auth_token","x"); localStorage.setItem("pwa_dismissed","true"); localStorage.setItem("profile_sex","male"); }, uid);
await page.goto(FE, { waitUntil: "domcontentloaded" });
for (let i=0;i<40;i++){ const r=await page.evaluate(async(uid)=>{const dbs=await indexedDB.databases(); if(!dbs.some(d=>d.name===`hjkl-ft-${uid}`))return false; return await new Promise(res=>{const q=indexedDB.open(`hjkl-ft-${uid}`);q.onsuccess=()=>{const ok=q.result.objectStoreNames.contains("goals")&&q.result.objectStoreNames.contains("app_flags");q.result.close();res(ok);};q.onerror=()=>res(false);});},uid).catch(()=>false); if(r)break; await page.waitForTimeout(500);}
await page.evaluate(async(uid)=>{const db=await new Promise((res,rej)=>{const q=indexedDB.open(`hjkl-ft-${uid}`);q.onsuccess=()=>res(q.result);q.onerror=()=>rej(q.error);}); const now=new Date().toISOString();
  const flags=[{key:"push_onboarding_dismissed",value:"true"},{key:"paywall_skipped_date",value:now.slice(0,10)},{key:"ft_subscription",value:JSON.stringify({plan:"monthly",end:Date.now()+30*864e5,active:true,start:Date.now(),status:"paid",no_renew:false,provider:"lava"})},{key:"welcome_shown",value:"true"},{key:"calcium_week_unlocked",value:"true"}];
  await new Promise(res=>{const tx=db.transaction(["app_flags"],"readwrite");const os=tx.objectStore("app_flags");for(const f of flags)os.put(f);tx.oncomplete=res;});
  await new Promise(res=>{const tx=db.transaction(["profile"],"readwrite");tx.objectStore("profile").put({key:"profile",sex:"male",height_cm:180,birth_year:1990,goal:"lose",cycle_start:null,steps_planka:8000,updated_at:now});tx.oncomplete=res;});
  const goal={id:"g-cal-1",nutrient:"Кальций",key:"kalcij",direction:"AtLeast",amount:1000,unit:"Mg",period:"Day",created_at:now,updated_at:now};
  await new Promise(res=>{const tx=db.transaction(["goals"],"readwrite");tx.objectStore("goals").put(goal);tx.oncomplete=res;});
  db.close();},uid);
await page.goto(FE + "/diary/add", { waitUntil: "domcontentloaded" });
await page.waitForSelector("#splash",{state:"detached",timeout:15000}).catch(()=>{});
await page.waitForTimeout(2500);
// count nutrient rows / labels
const labels = await page.evaluate(() => {
  const nodes = [...document.querySelectorAll('*')].filter(n => n.children.length===0 && /^(Калории|Белки|Жиры|Углеводы|Кальций|Calcium|Calories|Protein|Fat|Carbs)/.test((n.textContent||'').trim()));
  return nodes.map(n=>n.textContent.trim()).slice(0,20);
});
console.log("LABELS:", JSON.stringify(labels));
await page.screenshot({ path: "verify-form.png", fullPage: true });
await b.close();
console.log("done");
