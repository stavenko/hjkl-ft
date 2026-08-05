#!/bin/bash
# Насколько правдив промпт по кальцию: десять продуктов, по десять запросов на
# каждый. Промпт СЛОВО В СЛОВО тот, что строит lookup_nutrient, и схема та же —
# иначе мерили бы не то, что работает в приложении.
#
# Справочные значения — USDA / стандартные таблицы, мг на 100 г.
set -u
TOK=$(cat /tmp/iron-tok)
AI=${AI:-https://ai-worker-dev.vg-stavenko.workers.dev}
N=${N:-10}

SCHEMA='{"$schema":"https://json-schema.org/draft/2020-12/schema","title":"SingleNutrient","type":"object","properties":{"min_value":{"description":"Lowest reasonable amount per 100 g (a plain number in the requested unit).","type":"number","format":"double"},"max_value":{"description":"Highest reasonable amount per 100 g (a plain number in the requested unit).","type":"number","format":"double"},"recommended":{"description":"Most likely amount per 100 g (a plain number in the requested unit).","type":"number","format":"double"},"comment":{"description":"Short explanation of the value, in the requested language.","type":"string"}},"required":["min_value","max_value","recommended","comment"]}'

# Список можно подменить файлом (строки вида «продукт|справочное»), иначе берётся
# встроенный: FOODS_FILE=… bash measure-calcium-consistency.sh
if [ -n "${FOODS_FILE:-}" ]; then
  # bash 3.2 в macOS не знает mapfile — читаем построчно.
  FOODS=()
  while IFS= read -r l; do [ -n "$l" ] && FOODS+=("$l"); done < "$FOODS_FILE"
else
# продукт|справочное значение, мг/100 г
FOODS=(
  "Вкустаун Творог обезжиренный 0,5%|120"
  "Вкустаун Йогурт натуральный 3,2%|120"
  "Вкустаун Молоко пастеризованное 2,5%|120"
  "Сырный Дом Пармезан 40%|1180"
  "Сырный Дом Моцарелла 45%|505"
  "СояПлюс Тофу классический|350"
  "Зелёный Луг Кунжут семена неочищенные|975"
  "Зелёный Луг Шпинат свежий|99"
  "Зелёный Луг Рис белый шлифованный|10"
  "Доброфлот Сайра тихоокеанская натуральная|60"
)
fi

printf '%-46s %7s %7s %7s %7s %7s %6s\n' "продукт" "справ." "медиана" "min" "max" "разброс" "ошибок"
printf '%s\n' "--------------------------------------------------------------------------------------------"

for row in "${FOODS[@]}"; do
  name="${row%%|*}"; ref="${row##*|}"
  prompt="You are a nutritional database. For the food item \"$name\", give the amount of Кальций per 100 grams, as a plain number in мг.

For raw/dry as-sold products (grains, rice, pasta, flour, meat, fish, legumes) use the RAW value unless the name says cooked/boiled/fried/steamed.

Provide:
- min_value: lowest reasonable amount (number, in мг)
- max_value: highest reasonable amount (number, in мг)
- recommended: the most likely amount (number, in мг)
- comment: brief explanation in Russian

Base the answer only on the food name \"$name\". All values are per 100 g in мг — bare numbers, no unit text. Respond with ONLY a single minified JSON object and nothing else — no markdown, no prose."
  body=$(jq -n --arg p "$prompt" --argjson s "$SCHEMA" \
    '{model:"@cf/qwen/qwen3-30b-a3b-fp8",messages:[{role:"user",content:$p}],response_format:{type:"json_schema",json_schema:{name:"response",schema:$s,strict:true}},stream:true,think:false,max_tokens:2000}')

  vals=(); errs=0
  for i in $(seq 1 "$N"); do
    raw=$(curl -s -X POST "$AI/chat/completions" -H 'Content-Type: application/json' \
      -H "Authorization: Bearer $TOK" -d "$body")
    c=$(echo "$raw" | grep '^data: ' | sed 's/^data: //' | grep -v '^\[DONE\]' \
        | jq -rj '.choices[0].delta.content // empty' 2>/dev/null)
    v=$(echo "$c" | jq -r '.recommended // empty' 2>/dev/null)
    if [ -n "$v" ]; then vals+=("$v"); else errs=$((errs+1)); fi
  done

  if [ ${#vals[@]} -eq 0 ]; then
    printf '%-46s %7s %7s %7s %7s %7s %6d\n' "$name" "$ref" "—" "—" "—" "—" "$errs"
    echo "$name|$ref|нет данных" >> /tmp/calcium-raw.txt
    continue
  fi
  stats=$(printf '%s\n' "${vals[@]}" | sort -g | jq -s '
    { med: (if (length%2)==1 then .[length/2|floor] else ((.[length/2-1]+.[length/2])/2) end),
      lo: .[0], hi: .[-1] }')
  med=$(echo "$stats" | jq -r .med); lo=$(echo "$stats" | jq -r .lo); hi=$(echo "$stats" | jq -r .hi)
  spread=$(jq -n --argjson lo "$lo" --argjson hi "$hi" 'if $lo>0 then (($hi/$lo)*10|round)/10 else 0 end')
  printf '%-46s %7s %7s %7s %7s %6sx %6d\n' "$name" "$ref" "$med" "$lo" "$hi" "$spread" "$errs"
  echo "$name|$ref|$(printf '%s ' "${vals[@]}")" >> /tmp/calcium-raw.txt
done
