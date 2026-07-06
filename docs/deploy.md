# Деплой

Авто-деплоя из git НЕТ (Pages-проекты и воркеры без git-интеграции). Всё катится вручную через `wrangler`. Билд из текущего рабочего дерева/чекаута — деплой clean-состояния = сначала `git checkout` нужного коммита (WIP не забудьте застешить).

Окружения: `*-dev` (тест, `renorma-fit-dev.pages.dev` и `*-dev.workers.dev`) и `*-prod` (`fit.renorma.app`, `*.renorma.app`). Конвенция: `renorma-<product>-dev|prod`, `<worker>-dev|prod`.

## Фронтенд (Leptos PWA → Cloudflare Pages)

**Prod** (`renorma-fit-prod` → `fit.renorma.app`):
```bash
frontend/scripts/deploy-prod.sh          # default project renorma-fit-prod
```
Скрипт: `trunk build --release` → `cp pwa-worker.js dist/_worker.js` (динамический per-user manifest) → подмена dev-конфига на прод (`config-prod/frontend.toml`) → переписывание CSP `connect-src` на `*.renorma.app` → `wrangler pages deploy`.

**Dev** (`renorma-fit-dev` → `renorma-fit-dev.pages.dev`) — отдельного скрипта нет:
```bash
cd frontend
trunk build --release
cp pwa-worker.js dist/_worker.js
npx wrangler pages deploy dist --project-name=renorma-fit-dev --branch main --commit-dirty=true
```
Dev-конфиг (`config/frontend.toml`, dev-URLs) trunk кладёт в `dist/config/` сам — подмена не нужна.

> Версия сборки = `sha256(init.js + sw.js + index.html)[:12]`, публикуется в `/version.json`. Приложение опрашивает его на резюме и показывает «Обновить» в Настройки → Версия. Shell-only фиксы (sw.js/index.html) тоже бампают версию.

## Admin (→ Cloudflare Pages)

**Prod** (`renorma-admin-prod` → `admin.renorma.app`):
```bash
admin/scripts/deploy-prod.sh
```
**Dev** (`renorma-admin-dev`):
```bash
cd admin && trunk build --release && npx wrangler pages deploy dist --project-name=renorma-admin-dev --branch main --commit-dirty=true
```

## Воркеры (Rust → Cloudflare Workers)

Каждый `cloudflare/<worker>/wrangler.toml`: `name = "<worker>-dev"` (по умолчанию) + `[env.production] name = "<worker>-prod"`. Билд (`worker-build --release`) запускается автоматически из `[build] command` при `wrangler deploy`.

**Dev:**
```bash
cd cloudflare/<worker> && npx wrangler deploy
```
**Prod:**
```bash
cd cloudflare/<worker> && npx wrangler deploy --env production
```

Воркеры: `ai-worker`, `auth-worker`, `bug-report-worker`, `main-flow` (push/reminders), `ocr-queue`, `payment-worker`, `receipt-worker`, `support-worker`, `sync-worker`, `telegram-worker`.

- **`lava-mock` — только dev** (нет `[env.production]`): мок lava.top, в прод не катить (money-safety).
- **`payment-worker` prod — деньги.** Катить осознанно.
- Прод-секреты — из CF Secrets Store (не per-worker). `CLOUDFLARE_API_TOKEN` — в репозиторном `.env`.

## Порядок

Обычный релиз: воркеры (если менялись) → фронтенд/admin. Прод — после проверки на dev.
