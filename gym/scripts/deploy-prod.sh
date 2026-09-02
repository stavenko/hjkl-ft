#!/usr/bin/env bash
# Собрать приложение тренировок и выкатить его на ПРОДОВЫЙ Pages-проект
# (gym.renorma.app).
#
# Сборка та же, что у dev; для прода после неё подменяются два артефакта:
#   - config/frontend.toml  → прод-адреса воркеров (config-prod/frontend.toml)
#   - _headers CSP connect-src → прод-origin'ы воркеров на *.renorma.app
# Dev-проект (renorma-gym-dev.pages.dev) остаётся на dev-конфиге и dev-CSP.
#
# Использование: gym/scripts/deploy-prod.sh [pages-project-name]
#                (по умолчанию: renorma-gym-prod)
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${1:-renorma-gym-prod}"

trunk build --release

# 0) Воркер Pages в advanced-режиме: уводит Mi/Samsung в Chrome ещё до того, как
#    отдан первый байт приложения. Само присутствие dist/_worker.js и включает
#    advanced-режим.
cp pwa-worker.js dist/_worker.js

# 1) прод-адреса воркеров
cp config-prod/frontend.toml dist/config/frontend.toml

# 2) прод-CSP: подменяем origin'ы воркеров в connect-src на renorma.app'овские.
python3 - <<'PY'
import re
p = "dist/_headers"
s = open(p).read()
prod = ("connect-src 'self' "
        "https://auth.renorma.app https://pay.renorma.app https://ai.renorma.app "
        "https://gym-sync.renorma.app;")
s, n = re.subn(r"connect-src [^;]*;", prod, s, count=1)
assert n == 1, "connect-src directive not found in dist/_headers"
open(p, "w").write(s)
print("deploy-prod: rewrote CSP connect-src to *.renorma.app workers")
PY

npx wrangler pages deploy dist --project-name="$PROJECT" --branch main --commit-dirty=true
