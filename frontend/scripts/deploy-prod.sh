#!/usr/bin/env bash
# Build the frontend and deploy it to the PRODUCTION Pages project (fit.renorma.app).
#
# The build is identical to dev; only two artifacts differ for prod and are
# swapped into dist/ after the build:
#   - config/frontend.toml  → prod worker URLs (config-prod/frontend.toml)
#   - _headers CSP connect-src → the prod *.renorma.app worker origins
# The dev project (hjkl-ft.pages.dev) keeps the dev config/CSP.
#
# Usage: frontend/scripts/deploy-prod.sh [pages-project-name]   (default: renorma-app)
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT="${1:-renorma-app}"

trunk build --release

# 1) prod worker URLs
cp config-prod/frontend.toml dist/config/frontend.toml

# 2) prod CSP: swap the connect-src worker origins for the renorma.app ones
#    (the build already injected the script-src hash and stripped unsafe-inline).
python3 - <<'PY'
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
print("deploy-prod: rewrote CSP connect-src to *.renorma.app workers")
PY

npx wrangler pages deploy dist --project-name="$PROJECT" --branch main --commit-dirty=true
