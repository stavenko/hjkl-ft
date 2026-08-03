#!/usr/bin/env bash
# Сборка приложения и деплой в СМОУК-окружение (fit-smoke.renorma.app).
#
# Смоук ходит в ПРОД-воркеры — это второй фронт того же боевого бэкенда, нужный
# чтобы прогнать онбординг целиком на живом домене, не трогая fit.renorma.app.
# ВНИМАНИЕ: платежи здесь настоящие (реальная lava, реальные деньги).
#
# От прод-сборки отличается ровно одним артефактом: config/frontend.toml, где
# app_origin указывает на смоук-домен (по нему строится динамический манифест PWA).
# CSP — прод-шный, адреса воркеров те же.
#
# Ключи (passkey): rpId остаётся renorma.app, а смоук-origin добавлен в
# RP_ORIGINS_EXTRA прод-конфигурации auth-воркера, поэтому созданный здесь ключ
# работает и на fit.renorma.app — это один и тот же аккаунт.
#
# Usage: frontend/scripts/deploy-smoke.sh [pages-project-name]   (default: renorma-fit-smoke)
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${1:-renorma-fit-smoke}"

trunk build --release

# Pages advanced mode: наш воркер отдаёт пер-пользовательский манифест.
cp pwa-worker.js dist/_worker.js

# Адреса воркеров — прод-шные, отличается только app_origin.
cp config-smoke/frontend.toml dist/config/frontend.toml

# Прод-CSP: connect-src на *.renorma.app (сборка кладёт dev-список).
python3 - <<'PY2'
import re
p = "dist/_headers"
s = open(p).read()
prod = ("connect-src 'self' "
        "https://auth.renorma.app https://push.renorma.app https://ai.renorma.app "
        "https://pay.renorma.app https://ocr.renorma.app https://sync.renorma.app "
        "https://bug.renorma.app https://support.renorma.app;")
s, n = re.subn(r"connect-src [^;]*;", prod, s, count=1)
assert n == 1, "connect-src directive not found in dist/_headers"
open(p, "w").write(s)
print("deploy-smoke: CSP connect-src -> *.renorma.app")
PY2

npx wrangler pages deploy dist --project-name="$PROJECT" --branch main --commit-dirty=true
