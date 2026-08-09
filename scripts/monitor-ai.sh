#!/bin/bash
# Пробник живости модели: раз в INTERVAL секунд реальный (крошечный) запрос к
# ai-воркеру, с записью времени, исхода и потраченных нейронов.
#
# Запрос настоящий, а не /health: проверяем именно то, что ломалось — доходит ли
# дело до модели. Цена пробы около 0.1 нейрона, то есть тысяча проб ≈ 100 нейронов.
#
# Токен добывается сам и обновляется каждый час (живёт 2 часа).
# Лог: LOG (по умолчанию /tmp/ai-monitor.log), одна строка на пробу.
set -u
cd "$(dirname "$0")/.."
AI=${AI:-https://ai-worker-dev.vg-stavenko.workers.dev}
INTERVAL=${INTERVAL:-300}
LOG=${LOG:-/tmp/ai-monitor.log}
TOKFILE=${TOKFILE:-/tmp/ai-monitor.tok}

mint() { node scripts/mint-ai-token.mjs > "$TOKFILE" 2>/dev/null; }
mint
minted=$(date +%s)

while true; do
  # Токен живёт два часа — обновляем с запасом.
  now=$(date +%s)
  if [ $(( now - minted )) -gt 3600 ]; then mint; minted=$now; fi

  body=$(curl -s --max-time 60 -X POST "$AI/chat/completions" \
    -H 'Content-Type: application/json' -H "Authorization: Bearer $(cat "$TOKFILE")" \
    -d '{"model":"@cf/qwen/qwen3-30b-a3b-fp8","messages":[{"role":"user","content":"1+1"}],"stream":true,"think":false,"max_tokens":4}')

  ts=$(date -u "+%Y-%m-%d %H:%M:%S")
  if echo "$body" | grep -q '"neurons"'; then
    n=$(echo "$body" | grep -o '"neurons":[0-9.]*' | head -1 | cut -d: -f2)
    echo "$ts UTC  ЖИВ    нейронов $n" >> "$LOG"
  elif echo "$body" | grep -q '4006'; then
    echo "$ts UTC  ЛИМИТ  4006 — суточная квота нейронов" >> "$LOG"
  else
    echo "$ts UTC  СБОЙ   $(echo "$body" | tr -d '\n' | head -c 200)" >> "$LOG"
  fi
  sleep "$INTERVAL"
done
