#!/bin/bash
# Тот же замер, но по НОВОМУ промпту с таблицей категорий (`ai::lookup_calcium`).
# Промпт и схема скопированы дословно, иначе мерилось бы не то, что работает.
# Список продуктов — FOODS_FILE, строки «продукт|справочное значение».
set -u
TOK=$(cat /tmp/iron-tok)
AI=${AI:-https://ai-worker-dev.vg-stavenko.workers.dev}
N=${N:-10}

TABLE='  cheese_hard          600–1300 мг — пармезан, грана падано, чеддер, гауда, эмменталь, российский, пекорино
  cheese_semi          350–700 мг — моцарелла, сулугуни, адыгейский, брынза, фета, эдам, косичка
  cheese_soft          80–400 мг — рикотта, камамбер, бри, маскарпоне, творожный сыр, филадельфия
  cheese_processed     200–900 мг — плавленый сыр, сырок плавленый, сыр в ванночке, хохланд, виола
  milk_liquid          90–150 мг — молоко, кефир, ряженка, айран, простокваша, питьевой йогурт
  yogurt_sour_cream    80–180 мг — йогурт, греческий йогурт, сметана, сливочный сыр без соли
  cottage_cheese       80–200 мг — творог, зернёный творог, творожная масса, сырники
  milk_concentrated    250–1300 мг — сухое молоко, сгущённое молоко, молочная сыворотка сухая
  cream_whey           50–110 мг — сливки, сыворотка, мороженое пломбир
  seeds_high           500–1500 мг — мак, кунжут, семена чиа
  sesame_paste         250–550 мг — тахини, халва тахинная, кунжутная паста, козинак кунжутный
  fish_with_bones      120–450 мг — сардины в масле, шпроты, лосось консервированный с костями, килька
  tofu                 100–700 мг — тофу, соевый творог
  nuts_high_calcium    200–300 мг — миндаль, бразильский орех
  nuts_other           40–150 мг — фундук, фисташки, кешью, грецкий орех, арахис, кедровый орех
  legumes_dry          60–300 мг — фасоль сухая, нут сухой, соевые бобы, маш
  greens_leafy         130–260 мг — петрушка, укроп, кале, руккола, базилик, кинза
  vegetables_other     25–90 мг — брокколи, цветная капуста, белокочанная капуста, пекинская капуста, стручковая фасоль, репа
  greens_oxalate       40–120 мг — шпинат, щавель, ревень, свекольная ботва
  dried_fruit          30–180 мг — инжир сушёный, курага, урюк, финики
  fortified            100–180 мг — обогащённое кальцием растительное молоко, сок с кальцием, хлопья с кальцием
  plant_milk_plain     5–40 мг — соевое молоко без добавок, овсяное молоко, миндальное молоко, кокосовое молоко
  other_none           0–0 мг — мясо, птица, рыба без костей, яйца, крупы, макароны, хлеб, овощи, фрукты, масло, сахар, сладости, вода, чай, кофе, алкоголь — всё, чего нет выше'

SCHEMA='{"$schema":"https://json-schema.org/draft/2020-12/schema","title":"CalciumDetail","type":"object","properties":{"category":{"description":"The category key from the table, copied EXACTLY as written there.","type":"string"},"food_type":{"description":"What this product actually is, in two or three words.","type":"string"},"min_value":{"description":"Lowest reasonable CALCIUM CONTENT per 100 g, in milligrams.","type":"number","format":"double"},"max_value":{"description":"Highest reasonable CALCIUM CONTENT per 100 g, in milligrams.","type":"number","format":"double"},"recommended":{"description":"Most likely CALCIUM CONTENT per 100 g, in milligrams.","type":"number","format":"double"},"reason":{"description":"One short sentence: why this category and this amount. Keep it under 20 words.","type":"string"}},"required":["category","food_type","min_value","max_value","recommended","reason"]}'

FOODS=()
while IFS= read -r l; do [ -n "$l" ] && FOODS+=("$l"); done < "${FOODS_FILE:?нужен FOODS_FILE}"

printf '%-46s %7s %7s %7s %7s %6s %-20s\n' "продукт" "справ." "медиана" "min" "max" "ошиб" "категория"
printf '%s\n' "----------------------------------------------------------------------------------------------------------"

for row in "${FOODS[@]}"; do
  name="${row%%|*}"; ref="${row##*|}"
  prompt="You are a nutritional database. For the food item \"$name\", report how much CALCIUM it contains per 100 grams, in MILLIGRAMS.

For raw/dry as-sold products (grains, seeds, legumes, flour) use the RAW value unless the name says cooked/boiled/soaked.

First place the food in ONE row of this table, then give a value INSIDE that row's range:

$TABLE

Report:
- category: the row key, copied EXACTLY as written above
- food_type: what this product is, in two or three words, in Russian
- min_value / max_value / recommended: milligrams per 100 g, min ≤ recommended ≤ max
- reason: ONE short sentence (under 20 words) in Russian — why this row and this amount

Rules for choosing the row:
- Cheese goes by HARDNESS: hard and dry → cheese_hard, semi-hard and brined → cheese_semi, soft and fresh → cheese_soft, melted/processed → cheese_processed.
- A plant drink counts as fortified ONLY if the name says so («обогащённое», «с кальцием», «fortified»). Otherwise it is plant_milk_plain, which has very little.
- Canned fish counts as fish_with_bones only when the bones are eaten (sardines, sprats, canned salmon). Fillet without bones is other_none.
- Anything not covered by a row above is other_none, and its value is 0. Meat, fish fillet, eggs, cereals, bread, pasta, vegetables, fruit, oils, sweets and drinks all go there — they carry some calcium, but too little to count.

Base the answer only on the food name \"$name\". Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose."
  body=$(jq -n --arg p "$prompt" --argjson s "$SCHEMA" \
    '{model:"@cf/qwen/qwen3-30b-a3b-fp8",messages:[{role:"user",content:$p}],response_format:{type:"json_schema",json_schema:{name:"response",schema:$s,strict:true}},stream:true,think:false,max_tokens:2000}')

  vals=(); cats=""; errs=0
  for i in $(seq 1 "$N"); do
    raw=$(curl -s -X POST "$AI/chat/completions" -H 'Content-Type: application/json' \
      -H "Authorization: Bearer $TOK" -d "$body")
    c=$(echo "$raw" | grep '^data: ' | sed 's/^data: //' | grep -v '^\[DONE\]' \
        | jq -rj '.choices[0].delta.content // empty' 2>/dev/null)
    v=$(echo "$c" | jq -r '.recommended // empty' 2>/dev/null)
    k=$(echo "$c" | jq -r '.category // empty' 2>/dev/null)
    if [ -n "$v" ]; then vals+=("$v"); cats="$cats$k "; else errs=$((errs+1)); fi
  done

  if [ ${#vals[@]} -eq 0 ]; then
    printf '%-46s %7s %7s %7s %7s %6d %-20s\n' "$name" "$ref" "—" "—" "—" "$errs" "—"
    continue
  fi
  stats=$(printf '%s\n' "${vals[@]}" | sort -g | jq -s '
    { med: (if (length%2)==1 then .[length/2|floor] else ((.[length/2-1]+.[length/2])/2) end),
      lo: .[0], hi: .[-1] }')
  top=$(echo "$cats" | tr ' ' '\n' | grep -v '^$' | sort | uniq -c | sort -rn | head -1 | awk '{print $2" ("$1"/'"${#vals[@]}"')"}')
  printf '%-46s %7s %7s %7s %7s %6d %-20s\n' "$name" "$ref" \
    "$(echo "$stats" | jq -r .med)" "$(echo "$stats" | jq -r .lo)" "$(echo "$stats" | jq -r .hi)" "$errs" "$top"
done
