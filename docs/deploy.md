# Деплой

Авто-деплоя из git НЕТ (Pages-проекты и воркеры без git-интеграции). Всё катится вручную через `wrangler`. Билд из текущего рабочего дерева/чекаута — деплой clean-состояния = сначала `git checkout` нужного коммита (WIP не забудьте застешить).

Окружения: `*-dev` (тест, `renorma-fit-dev.pages.dev` и `*-dev.workers.dev`) и `*-prod` (`fit.renorma.app`, `*.renorma.app`). Конвенция: `renorma-<product>-dev|prod`, `<worker>-dev|prod`.

Pages-проекты: `renorma-fit-*` (приложение худеющего), `renorma-admin-*`, `renorma-curator-*`, `renorma-gym-*` (тренировки).

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

## Куратор (→ Cloudflare Pages)

**Prod** (`renorma-curator-prod` → `curator.renorma.app`):
```bash
curator/scripts/deploy-prod.sh
```
**Dev** (`renorma-curator-dev`):
```bash
cd curator && trunk build --release && npx wrangler pages deploy dist --project-name=renorma-curator-dev --branch main --commit-dirty=true
```

> Домен у кураторского приложения СВОЙ не для красоты: `rp_id` паскея берётся от
> него, и `curator.renorma.app` не является registrable suffix ни для
> `fit.renorma.app`, ни для `admin.renorma.app`. Три области ключей не
> пересекаются структурно, а не только по origin. Соответствующие переменные —
> `CURATOR_RP_ID`/`CURATOR_RP_ORIGIN` в `cloudflare/auth-worker/wrangler.toml`.

## Тренировки (→ Cloudflare Pages)

**Prod** (`renorma-gym-prod` → `gym.renorma.app`):
```bash
gym/scripts/deploy-prod.sh
```
**Dev** (`renorma-gym-dev`):
```bash
cd gym
trunk build --release
cp pwa-worker.js dist/_worker.js
npx wrangler pages deploy dist --project-name=renorma-gym-dev --branch main --commit-dirty=true
```
Dev-конфиг (`config/frontend.toml`) trunk кладёт в `dist/config/` сам — подмена не нужна.

> Версия сборки = `sha256(init.js + sw.js + index.html)[:12]`, публикуется в
> `/version.json` (штампует `scripts/build-shell.sh` из post_build-хука Trunk).
> Приложение опрашивает его при запуске и при возвращении на передний план и
> показывает «Обновить» в Настройках → Версия. Правка одной только оболочки
> (sw.js/index.html) тоже бампает версию — иначе обновление никогда бы не
> предложилось. Тот же хук выносит встроенный модуль Trunk в `/init.js` и
> заменяет в CSP `script-src 'unsafe-inline'` на хеши оставшихся встроенных
> скриптов, так что **править index.html и катить без пересборки нельзя**:
> хеши разъедутся, и браузер молча не выполнит ни регистрацию сервис-воркера,
> ни сторож зависшего обновления.


> Домен у приложения тренировок свой, а область ключей — НЕТ, и это ровно
> наоборот, чем у админки и куратора. Вход сюда идёт ТЕМ ЖЕ паскеем, что и в
> `fit.renorma.app`: в проде `GYM_RP_ID = "renorma.app"` — общий rp_id,
> registrable suffix обоих поддоменов, — а различается только origin церемонии
> (`GYM_RP_ORIGIN`). Переменные в `cloudflare/auth-worker/wrangler.toml`; после
> их правки auth-worker надо перекатить, иначе вход на gym отобьётся.
>
> **На dev общего ключа нет и быть не может**: `pages.dev` — публичный суффикс,
> и rp_id приложения худеющего (`renorma-fit-dev.pages.dev`) для gym-домена не
> является registrable suffix — браузер отверг бы церемонию. Поэтому на стенде у
> зала СВОЯ область (`renorma-gym-dev.pages.dev`) и свой ключ. «Тот же ключ» —
> это про прод.

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

- **`gym-sync-worker`** — журнал синхронизации приложения тренировок
  (`gym-sync.renorma.app`), копия `sync-worker` со своей DO-неймспейсой. Выкачен
  вместе с привязкой `GYM_SYNC_WORKER` в payment-worker: в одиночку его катить
  было нельзя — обход `WIPE_TARGETS` его бы не знал, и «забыть меня» оставило бы
  журнал тренировок. Приложение в него пока не пишет, но стирание уже доходит.

- **`lava-mock` — только dev** (нет `[env.production]`): мок lava.top, в прод не катить (money-safety).
- **`payment-worker` prod — деньги.** Катить осознанно.
- Прод-секреты — из CF Secrets Store (не per-worker). `CLOUDFLARE_API_TOKEN` — в репозиторном `.env`.

## Порядок

Обычный релиз: воркеры (если менялись) → фронтенд/admin/curator/gym. Прод — после проверки на dev.

Первый выкат приложения тренировок: сперва воркеры, в которых разрешается
gym-origin, и только потом сам Pages-проект. В обратном порядке приложение
выкатится в состояние, где запрос отбивает браузер (CORS) ещё до ответа сервера.

Их ТРИ, и катить надо все три — и на dev тоже:

| воркер | что в нём для зала | что отвалится без выката |
| --- | --- | --- |
| `auth-worker` | `GYM_RP_ID`/`GYM_RP_ORIGIN` + gym-origin | вход: церемония не начинается вовсе |
| `payment-worker` | gym-origin для `GET /subscription` | подписка: приложение показывает «Нет связи» вместо «не оплачена» |
| `ai-worker` | gym-origin | фраза восстановления в настройках |

На dev про это забыли дважды подряд — сперва про `auth-worker`, потом про
`payment-worker`, — и оба раза нашёл `scripts/check-gym-passkey.mjs`, а не
человек.
