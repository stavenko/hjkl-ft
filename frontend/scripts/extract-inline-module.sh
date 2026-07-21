#!/bin/bash
# Extract Trunk's inline <script type="module"> into a separate file
# so CSP doesn't need 'unsafe-inline' for script-src.
#
# Runs as a Trunk post_build hook.

set -euo pipefail

DIST="${TRUNK_STAGING_DIR:-dist}"
HTML="$DIST/index.html"

if [ ! -f "$HTML" ]; then
  echo "extract-inline-module: $HTML not found, skipping"
  exit 0
fi

# Extract the inline module script content (first <script type="module">...</script>)
MODULE_CONTENT=$(python3 -c "
import re, sys
html = open('$HTML').read()
m = re.search(r'<script type=\"module\">(.*?)</script>', html, re.DOTALL)
if m:
    print(m.group(1).strip())
else:
    sys.exit(1)
" 2>/dev/null) || {
  echo "extract-inline-module: no inline module script found, skipping"
  exit 0
}

# Write to separate file, gating the WASM instantiation behind the compatibility
# flag set by the classic script in index.html. Trunk's module is:
#   import init, * as bindings from '/<hash>.js';
#   const wasm = await init({ module_or_path: '/<hash>_bg.wasm' });
#   window.wasmBindings = bindings;
#   dispatchEvent(...);
# The static `import` must stay at the top; we wrap everything AFTER it in
# `if (!globalThis.__RN_INCOMPATIBLE__) { ... }` so a device without reference-types
# never fetches/instantiates the wasm (init() would throw there). Top-level await is
# still legal inside a top-level `if` block in a module.
MODULE_CONTENT="$MODULE_CONTENT" python3 -c "
import os, re
content = os.environ['MODULE_CONTENT'].strip()
m = re.search(r'^\s*import\b.*?;', content, re.DOTALL | re.MULTILINE)
if not m:
    raise SystemExit('extract-inline-module: no import statement in trunk module')
imp = content[m.start():m.end()].strip()
rest = (content[:m.start()] + content[m.end():]).strip()
out = imp + '\nif (!globalThis.__RN_INCOMPATIBLE__) {\n' + rest + '\n}\n'
open('$DIST/init.js', 'w').write(out)
"

# Replace inline script with external reference
python3 -c "
import re
html = open('$HTML').read()
html = re.sub(
    r'<script type=\"module\">.*?</script>',
    '<script type=\"module\" src=\"/init.js\"></script>',
    html,
    count=1,
    flags=re.DOTALL,
)
open('$HTML', 'w').write(html)
"

# Compute SHA-256 hash of the remaining inline script (SW registration)
SW_HASH=$(python3 -c "
import re, hashlib, base64
html = open('$HTML').read()
scripts = re.findall(r'<script>(.*?)</script>', html, re.DOTALL)
for s in scripts:
    h = base64.b64encode(hashlib.sha256(s.encode()).digest()).decode()
    print(f'sha256-{h}')
" 2>/dev/null)

echo "extract-inline-module: extracted init.js, SW hash: $SW_HASH"

# Update _headers with correct hash (use only first hash line)
FIRST_HASH=$(echo "$SW_HASH" | head -1)
if [ -f "$DIST/_headers" ] && [ -n "$FIRST_HASH" ]; then
  python3 -c "
import re
headers = open('$DIST/_headers').read()
headers = re.sub(
    r\"'sha256-[A-Za-z0-9+/=]+'\",
    \"'$FIRST_HASH'\",
    headers,
)
import re as re2
headers = re2.sub(r\"(script-src[^;]*)'unsafe-inline'\s*\", r'\\1', headers)
open('$DIST/_headers', 'w').write(headers)
"
  echo "extract-inline-module: updated _headers with hash $FIRST_HASH"
fi

# --- Build-version stamp for in-app auto-update ---
# init.js changes every build (it references the new hashed wasm/js), so its
# hash is a stable per-build id. We expose it to the running app as
# `globalThis.__APP_VERSION__` and publish the same id at /version.json, which
# the app polls (on resume) to detect a new deploy and reload itself.
VERSION=$(python3 -c "
import hashlib, os
# Hash init.js (changes with the WASM/JS bundle) PLUS the shell files that change
# independently of the WASM — sw.js and index.html (the SW-registration script).
# Without this a shell-only fix (e.g. an iOS PWA sw.js change) leaves the build id
# unchanged, so version.json never moves and the in-app updater never offers «Обновить».
h = hashlib.sha256()
for f in ['$DIST/init.js', '$DIST/sw.js', '$DIST/index.html']:
    if os.path.exists(f):
        h.update(open(f, 'rb').read())
print(h.hexdigest()[:12])
")
python3 -c "
p = '$DIST/init.js'
src = open(p).read()
open(p, 'w').write('globalThis.__APP_VERSION__=\"$VERSION\";\n' + src)
"
echo "{\"v\":\"$VERSION\"}" > "$DIST/version.json"
echo "extract-inline-module: build version $VERSION (version.json + __APP_VERSION__)"
