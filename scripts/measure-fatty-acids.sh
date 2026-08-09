#!/bin/bash
# Замер промпта на ЖИРНЫЕ КИСЛОТЫ: все значения одним запросом.
#
# Спрашивается ПРОФИЛЬ жира — доли от общего жира продукта, а не миллиграммы на
# 100 г. Граммы считает клиент: доля × `Food.fat`, который у нас уже есть и
# получен не от этой модели. Профиль — свойство типа жира: он не плывёт от
# разбавления, уварки и способа готовки, поэтому строки «сырое/готовое» не нужны.
#
# Меряется:
#   * РАЗБРОС — N прогонов одного продукта, min/медиана/max по каждому полю;
#   * УСТОЙЧИВОСТЬ КАТЕГОРИИ — в скольких прогонах из N выбрана та же строка;
#   * ВНУТРЕННЯЯ СВЕРКА — сумма трёх долей не больше ста, EPA+DHA ≤ ПНЖК. Это проверка без справочника, и она ловит бессмыслицу сразу.
#
# Промпт и схема здесь — ЕДИНСТВЕННЫЙ пока экземпляр: рабочего кода на жирные
# кислоты ещё нет. Когда он появится, промпт копировать в него дословно, иначе
# замер перестанет мерить то, что работает.
#
# Список продуктов — FOODS_FILE, строки «продукт|ориентир НЖК/МНЖК/ПНЖК».
set -u
TOK=$(cat "${TOKFILE:-/tmp/fa-tok}")
AI=${AI:-https://ai-worker-dev.vg-stavenko.workers.dev}
N=${N:-5}

# Строки — по ТИПУ ЖИРА, а не по типу продукта: профиль задаёт источник жира.
#
# Чисел в строках нет намеренно (§1.3 плейбука: увидев числа, модель отвечает ими).
# Вместо чисел — КАЧЕСТВЕННОЕ указание, какая фракция в строке главная: без него
# модель отдала обычное подсолнечное масло как высокоолеиновое (МНЖК вместо ПНЖК),
# а это разница втрое на самом частом масле в рационе.
# Диапазоны — в процентах от общего жира: НЖК / МНЖК / ПНЖК и EPA+DHA. Доли АЛК
# здесь нет: её считал единственный индикатор, который решено не выпускать.
# Именно диапазон строки, а не словесная подсказка, удерживает ответ: без него
# модель отдавала грецкий орех как мононенасыщенный, а чечевицу — с разбросом
# ПНЖК от 17 до 60. Клиент потом зажимает ответ в границы строки.
TABLE='  coconut_palm         SFA 82–92, MUFA 5–12, PUFA 1–3, EPA+DHA 0 — кокосовое масло, пальмовое масло, кокосовая стружка, кокосовое молоко, пальмоядровый жир
  dairy_fat            SFA 60–70, MUFA 22–30, PUFA 2–6, EPA+DHA 0 — сливочное масло, топлёное масло, сливки, сметана, сыр, молоко, творог, мороженое, кефир
  beef_lamb_fat        SFA 38–48, MUFA 40–50, PUFA 2–6, EPA+DHA 0 — говядина, телятина, баранина, курдючное сало, говяжий жир, субпродукты жвачных
  pork_fat             SFA 33–40, MUFA 42–50, PUFA 8–14, EPA+DHA 0 — свинина, сало, шпик, бекон, ветчина, колбаса, сосиски, свиные субпродукты
  poultry_fat          SFA 26–32, MUFA 40–48, PUFA 18–26, EPA+DHA 0–1 — курица, индейка, утка, гусь, куриная кожа, куриный жир, куриная печень
  egg_fat              SFA 28–34, MUFA 40–48, PUFA 12–18, EPA+DHA 0–2 — яйцо, яичный желток, омлет, яичница
  olive_avocado        SFA 12–18, MUFA 65–75, PUFA 10–16, EPA+DHA 0 — оливковое масло, оливки, маслины, авокадо, масло авокадо
  rapeseed_mustard     SFA 6–9, MUFA 58–66, PUFA 26–32, EPA+DHA 0 — рапсовое масло, каноловое масло, горчичное масло, семена горчицы
  peanut               SFA 15–20, MUFA 45–55, PUFA 25–35, EPA+DHA 0 — арахис, арахисовое масло, арахисовая паста
  seed_oils_linoleic   SFA 10–15, MUFA 18–28, PUFA 55–68, EPA+DHA 0 — подсолнечное масло, кукурузное масло, соевое масло, масло виноградных семечек, семечки подсолнуха, тыквенные семечки, кунжут, майонез, маргарин
  high_oleic_oils      SFA 7–12, MUFA 75–85, PUFA 5–12, EPA+DHA 0 — высокоолеиновое подсолнечное, высокоолеиновое сафлоровое, олеиновый подсолнечник
  flax_chia            SFA 8–11, MUFA 15–22, PUFA 66–75, EPA+DHA 0 — льняное масло, семена льна, семена чиа, рыжиковое масло
  walnut_hemp          SFA 9–12, MUFA 13–22, PUFA 65–75, EPA+DHA 0 — грецкий орех, конопляное семя, масло грецкого ореха
  fish_fatty_cold      SFA 20–28, MUFA 30–45, PUFA 20–30, EPA+DHA 12–25 — скумбрия, сельдь, лосось, форель, сардина, шпроты, анчоус, икра, рыбий жир, печень трески
  fish_lean_seafood    SFA 18–25, MUFA 12–20, PUFA 30–45, EPA+DHA 25–40 — треска, минтай, судак, тунец, креветки, мидии, кальмар, гребешок, крабовые палочки
  fish_warm_low_n3     SFA 25–35, MUFA 25–40, PUFA 20–30, EPA+DHA 3–8 — тилапия, пангасиус, баса, дорадо, сибас, карп, толстолобик, сом — тепловодная и прудовая рыба, у неё преобладает линолевая, а не n-3
  nuts_tree            SFA 7–14, MUFA 55–70, PUFA 15–30, EPA+DHA 0 — миндаль, фундук, кешью, фисташки, кедровый орех, бразильский орех, макадамия
  cocoa_confectionery  SFA 55–65, MUFA 28–35, PUFA 2–5, EPA+DHA 0 — шоколад, какао, какао-масло, конфеты, вафли, печенье, торт, кондитерский жир
  grain_legume         SFA 15–22, MUFA 12–25, PUFA 45–60, EPA+DHA 0 — крупы, хлеб, макароны, рис, овсянка, бобовые, чечевица, нут, соя, тофу
  no_fat               all zero — овощи, фрукты, зелень, сахар, мёд, вода, чай, кофе, алкоголь, желатин, специи — всё, где жира практически нет'

SCHEMA='{"$schema":"https://json-schema.org/draft/2020-12/schema","title":"FattyAcidProfile","type":"object","properties":{"category":{"description":"The category key from the table, copied EXACTLY as written there.","type":"string"},"food_type":{"description":"What this product actually is, in two or three words.","type":"string"},"sfa_pct":{"description":"SATURATED fatty acids as a PERCENT OF THIS FOOD'"'"'S TOTAL FAT, by weight. Not per 100 g of food.","type":"number","format":"double"},"mufa_pct":{"description":"MONOUNSATURATED fatty acids as a PERCENT OF THIS FOOD'"'"'S TOTAL FAT, by weight.","type":"number","format":"double"},"pufa_pct":{"description":"POLYUNSATURATED fatty acids as a PERCENT OF THIS FOOD'"'"'S TOTAL FAT, by weight.","type":"number","format":"double"},"epa_dha_pct":{"description":"EPA plus DHA together as a PERCENT OF THIS FOOD'"'"'S TOTAL FAT. Part of pufa_pct, so it can never exceed it. Zero for everything except fish, seafood and fish oil.","type":"number","format":"double"},"confidence":{"description":"How sure you are of this profile, from 0.0 to 1.0. Be honest: 1.0 means you know THIS food'"'"'s fat profile, 0.0 means you are guessing from the category alone.","type":"number","format":"double"},"reason":{"description":"One short sentence: why this category and this profile. Keep it under 20 words.","type":"string"}},"required":["category","food_type","sfa_pct","mufa_pct","pufa_pct","epa_dha_pct","confidence","reason"]}'

FOODS=()
while IFS= read -r l; do [ -n "$l" ] && FOODS+=("$l"); done < "${FOODS_FILE:?нужен FOODS_FILE}"

printf '%-34s %-14s %-22s %-22s %-22s %-9s %5s %-20s\n' \
  "продукт" "ориентир" "НЖК мед(min–max)" "МНЖК мед(min–max)" "ПНЖК мед(min–max)" "EPA+DHA" "ошиб" "категория"
printf '%s\n' "--------------------------------------------------------------------------------------------------------------------------------------------------------"

for row in "${FOODS[@]}"; do
  name="${row%%|*}"; ref="${row##*|}"
  prompt="You are a nutritional database. For the food item \"$name\", report the FATTY-ACID PROFILE OF ITS FAT.

Every number you report is a PERCENT OF THIS FOOD'S TOTAL FAT, by weight — NOT grams per 100 g of food. A food's total fat is already known to us; you only describe how that fat is composed. This means the profile does NOT depend on how much fat the food has: pure sunflower oil and a sauce made from it share the same profile.

First place the food in ONE row of this table — rows are by the TYPE OF FAT, not by the type of dish — then give values INSIDE that row's ranges:

$TABLE

Report:
- category: the row key, copied EXACTLY as written above
- food_type: what this product is, in two or three words, in Russian
- sfa_pct / mufa_pct / pufa_pct: percent of total fat, each INSIDE its range in the chosen row. The remaining weight is glycerol, so the three need not add up to 100 — never inflate a fraction to make them.
- epa_dha_pct: EPA plus DHA together, percent of total fat, inside the row's range. Also part of pufa_pct. Zero for everything that is not fish, seafood or fish oil — plants contain NO EPA or DHA, however rich in omega-3 they are.
- confidence: 0.0 to 1.0, how sure you are of THIS food's profile
- reason: ONE short sentence (under 20 words) in Russian — why this row and this profile

Rules for choosing the row:
- Go by WHERE THE FAT COMES FROM. A dish fried in sunflower oil whose own fat is mostly that oil belongs to the oil's row, not to the row of the vegetable.
- A mixed dish goes to the row of its DOMINANT fat source.
- Cooking, freezing, canning and packaging do not change the profile. The row does not depend on them.
- A plain name goes to the ORDINARY variety of that oil. Only an explicit «высокоолеиновое» / «high-oleic» in the name moves sunflower or safflower oil to high_oleic_oils.
- Fish splits by WHERE IT LIVES, not by how fatty it is: cold-water sea fish carry EPA and DHA, while warm-water and farmed pond fish carry mostly linoleic acid and very little n-3. A fish you do not recognise as a cold-water sea species goes to fish_warm_low_n3.
- Anything with practically no fat goes to no_fat; its profile is irrelevant, report the row and zeros.

Base the answer only on the food name \"$name\". Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose."

  body=$(jq -n --arg p "$prompt" --argjson s "$SCHEMA" \
    '{model:"@cf/qwen/qwen3-30b-a3b-fp8",messages:[{role:"user",content:$p}],response_format:{type:"json_schema",json_schema:{name:"response",schema:$s,strict:true}},stream:true,think:false,max_tokens:2000}')

  sfa=(); mufa=(); pufa=(); epa=(); cats=""; errs=0; bad=""
  for i in $(seq 1 "$N"); do
    raw=$(curl -s -X POST "$AI/chat/completions" -H 'Content-Type: application/json' \
      -H "Authorization: Bearer $TOK" -d "$body")
    c=$(echo "$raw" | grep '^data: ' | sed 's/^data: //' | grep -v '^\[DONE\]' \
        | jq -rj '.choices[0].delta.content // empty' 2>/dev/null)
    v=$(echo "$c" | jq -r '[.sfa_pct, .mufa_pct, .pufa_pct, .epa_dha_pct, .category] | @tsv' 2>/dev/null)
    if [ -z "$v" ] || [ "$(echo "$v" | cut -f1)" = "null" ]; then errs=$((errs+1)); continue; fi
    IFS=$'\t' read -r s m p e k <<< "$v"
    sfa+=("$s"); mufa+=("$m"); pufa+=("$p"); epa+=("$e"); cats="$cats$k "
    # Внутренняя сверка: сумма долей и вложенность EPA+DHA в ПНЖК.
    flag=$(jq -n --argjson s "$s" --argjson m "$m" --argjson p "$p" --argjson e "$e" '
      # Нижнего порога у суммы НЕТ: у постной рыбы и морепродуктов доли жирных
      # кислот в справочниках дают около семидесяти (остальное — фосфолипиды и
      # глицерин), а у беcжирных продуктов честный ответ — нули. Осмысленна только
      # верхняя граница: больше ста процентов жира не бывает.
      [ (if ($s+$m+$p) > 100 then "сумма=\(($s+$m+$p)*10|round/10)" else empty end),
        (if $e > $p then "EPA>ПНЖК" else empty end) ] | join(",")')
    flag=$(echo "$flag" | tr -d '"')
    [ -n "$flag" ] && bad="$bad$flag; "
  done

  if [ ${#sfa[@]} -eq 0 ]; then
    printf '%-34s %-14s %-22s %-22s %-22s %-9s %5d %-20s\n' "$name" "$ref" "—" "—" "—" "—" "—" "$errs" "—"
    continue
  fi
  st() { printf '%s\n' "$@" | sort -g | jq -s -r '
    (if (length%2)==1 then .[length/2|floor] else ((.[length/2-1]+.[length/2])/2) end) as $med
    | "\($med) (\(.[0])–\(.[-1]))"'; }
  med() { printf '%s\n' "$@" | sort -g | jq -s -r '
    if (length%2)==1 then .[length/2|floor] else ((.[length/2-1]+.[length/2])/2) end'; }
  topkey=$(echo "$cats" | tr ' ' '\n' | grep -v '^$' | sort | uniq -c | sort -rn | head -1 | awk '{print $2}')
  topn=$(echo "$cats" | tr ' ' '\n' | grep -v '^$' | sort | uniq -c | sort -rn | head -1 | awk '{print $1}')
  # Со зажимом в границы строки ответ определяется СТРОКОЙ, поэтому главное, что
  # мерим, — попала ли модель в ожидаемую строку.
  mark=""; [ "$topkey" != "$ref" ] && mark=" ← ЖДАЛИ $ref"
  top="$topkey ($topn/${#sfa[@]})$mark"
  printf '%-34s %-14s %-22s %-22s %-22s %-9s %5d %-20s\n' "$name" "$ref" \
    "$(st "${sfa[@]}")" "$(st "${mufa[@]}")" "$(st "${pufa[@]}")" \
    "$(med "${epa[@]}")" "$errs" "$top"
  [ -n "$bad" ] && printf '%34s СВЕРКА НЕ СОШЛАСЬ: %s\n' "" "$bad"
done
