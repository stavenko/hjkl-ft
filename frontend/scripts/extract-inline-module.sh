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

# Write to separate file
echo "$MODULE_CONTENT" > "$DIST/init.js"

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
