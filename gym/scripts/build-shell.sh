#!/bin/bash
# Trunk post_build: вытащить встроенный модуль в отдельный файл, снять с CSP
# `unsafe-inline` и проштамповать сборку версией.
#
# Перенесено из приложения худеющего (frontend/scripts/extract-inline-module.sh)
# вместе с причинами:
#
#  1. Trunk кладёт загрузчик WASM ВСТРОЕННЫМ `<script type="module">`. Пока он
#     встроенный, CSP обязана разрешать `script-src 'unsafe-inline'` — то есть
#     не защищать от внедрения скриптов вовсе. Выносим в /init.js и оставляем в
#     CSP только хеш оставшегося встроенного скрипта (регистрация сервис-воркера).
#
#  2. Инстанцирование WASM оборачивается в `if (!globalThis.__RN_INCOMPATIBLE__)`.
#     Приложение собрано с reference-types (Rust 1.82+ включает их по умолчанию,
#     wasm-bindgen 0.2.125 без них не умеет), а браузер без них не может
#     инстанцировать модуль: init() упал бы глубоко внутри, оставив человека на
#     вечном экране загрузки. Проверка стоит в index.html ДО этого файла.
#
#  3. Версия сборки = sha256(init.js + sw.js + index.html)[:12]. init.js меняется
#     каждую сборку (в нём имена нового хешированного wasm/js), а sw.js и
#     index.html меняются НЕЗАВИСИМО от wasm — правка одной только оболочки без
#     них не сдвинула бы версию, и «Обновить» никогда бы не предложилось.
#     Публикуется дважды: в `globalThis.__APP_VERSION__` (что запущено) и в
#     /version.json (что выложено). Приложение сравнивает их — см. update.rs.

set -euo pipefail

DIST="${TRUNK_STAGING_DIR:-dist}"
HTML="$DIST/index.html"

if [ ! -f "$HTML" ]; then
  echo "build-shell: $HTML не найден, пропускаем"
  exit 0
fi

# ── 1. Встроенный модуль → /init.js ──
MODULE_CONTENT=$(python3 -c "
import re, sys
html = open('$HTML').read()
m = re.search(r'<script type=\"module\">(.*?)</script>', html, re.DOTALL)
if m:
    print(m.group(1).strip())
else:
    sys.exit(1)
" 2>/dev/null) || {
  echo "build-shell: встроенного модуля нет, пропускаем"
  exit 0
}

# Статический `import` обязан остаться наверху модуля; всё, что ПОСЛЕ него,
# заворачивается в проверку совместимости. Top-level await внутри top-level `if`
# в модуле законен.
MODULE_CONTENT="$MODULE_CONTENT" python3 -c "
import os, re
content = os.environ['MODULE_CONTENT'].strip()
m = re.search(r'^\s*import\b.*?;', content, re.DOTALL | re.MULTILINE)
if not m:
    raise SystemExit('build-shell: в модуле Trunk нет оператора import')
imp = content[m.start():m.end()].strip()
rest = (content[:m.start()] + content[m.end():]).strip()
open('$DIST/init.js', 'w').write(imp + '\nif (!globalThis.__RN_INCOMPATIBLE__) {\n' + rest + '\n}\n')
"

python3 -c "
import re
html = open('$HTML').read()
html = re.sub(
    r'<script type=\"module\">.*?</script>',
    '<script type=\"module\" src=\"/init.js\"></script>',
    html, count=1, flags=re.DOTALL,
)
open('$HTML', 'w').write(html)
"

# ── 2. CSP: хеши оставшихся встроенных скриптов вместо 'unsafe-inline' ──
HASHES=$(python3 -c "
import re, hashlib, base64
html = open('$HTML').read()
out = []
for s in re.findall(r'<script>(.*?)</script>', html, re.DOTALL):
    h = base64.b64encode(hashlib.sha256(s.encode()).digest()).decode()
    out.append(f\"'sha256-{h}'\")
print(' '.join(out))
")

if [ -f "$DIST/_headers" ] && [ -n "$HASHES" ]; then
  HASHES="$HASHES" python3 -c "
import os, re
p = '$DIST/_headers'
s = open(p).read()
# Заменяем сам маркер 'unsafe-inline' в script-src на набор хешей: так директива
# остаётся одной строкой и не зависит от того, что в ней стояло раньше.
def fix(m):
    return m.group(0).replace(\"'unsafe-inline'\", os.environ['HASHES'])
s, n = re.subn(r'script-src[^;]*;', fix, s, count=1)
assert n == 1, 'в _headers нет директивы script-src'
open(p, 'w').write(s)
"
  echo "build-shell: CSP script-src → $HASHES"
fi

# ── 3. Штамп версии ──
VERSION=$(python3 -c "
import hashlib, os
h = hashlib.sha256()
for f in ['$DIST/init.js', '$DIST/sw.js', '$DIST/index.html']:
    if os.path.exists(f):
        h.update(open(f, 'rb').read())
print(h.hexdigest()[:12])
")
# Читаем ДО открытия на запись: `open(p,'w')` обрезает файл, и вложенное чтение
# вернуло бы пустоту — init.js остался бы одной строчкой со штампом.
python3 -c "
p = '$DIST/init.js'
src = open(p).read()
open(p, 'w').write('globalThis.__APP_VERSION__=\"$VERSION\";\n' + src)
"
echo "{\"v\":\"$VERSION\"}" > "$DIST/version.json"
echo "build-shell: версия сборки $VERSION (version.json + __APP_VERSION__)"
