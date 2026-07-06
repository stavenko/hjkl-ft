# Durable Objects — карта хранилищ

Пофайловый разбор всех Durable Object'ов проекта: назначение, адресация, схема хранилища,
операции и инварианты. Собрано из кода (не спекуляция). Закрывает TODO из
[scenarios.md](scenarios.md).

## Сводка

| DO | Воркер | Биндинг | Инстансы | Хранилище |
|----|--------|---------|----------|-----------|
| **SubscriptionDO** | payment-worker | `SUBSCRIPTION_DO` | per-user `v2:<userId>` | KV (ключ `sub`) |
| **ClaimDO** | payment-worker | `CLAIM_DO` | глобальный `claims-v2` | SQLite (`claims`, `refunds`) |
| **PaymentIndexDO** | payment-worker | `PAYMENT_INDEX_DO` | глобальный `index-v2` | KV (`order:`/`contract:`/`claim-contract:`) |
| **TgSessionDO** | telegram-worker | `TG_SESSION_DO` | глобальный `global-v2` | SQLite (`sessions`, `claims`, `miniapp_claims`) |
| **ConversationDO** | support-worker | `CONVERSATION_DO` | per-user `<userId>` | SQLite (`messages`, `meta`) |
| **ConversationIndexDO** | support-worker | `CONVERSATION_INDEX_DO` | глобальный `index` | SQLite (`conversations`, `meta`, `admins`, `admin_requests`) |
| **AuthDO** | auth-worker | `AUTH_DO` | глобальный `global` | KV (`cred:`/`user:`/`pairing:`/`token:`/…) |
| **UserDO** | auth-worker | `USER_DO` | — (deprecated) | нет |
| **BugReportDO** | bug-report-worker | `BUG_REPORT_DO` | глобальный `global` | SQLite (`reports`) |
| **SyncDO** | sync-worker | `SYNC_DO` | per-user `<sub>` | KV (по коллекциям, JSON-map) |
| **PushDO** | main-flow | `PUSH_DO` | глобальный `global` | KV (`push_sub:`/`user_push_subs:`) |
| **ScheduleDO** | main-flow | `SCHEDULE_DO` | per-user `<userId>` | KV (`schedule`/`next_slot`), alarm |
| **QueueDO** | ocr-queue | `QUEUE_DO` | глобальный `global` (weur) | KV (`queue`/`job:`/`img:`) |

> `DO_EPOCH = "v2"` есть только в payment-worker и telegram-worker (обнуление инстансов без
> delete-class миграции — префикс имени). В остальных воркерах epoch-префикса нет.

---

## payment-worker

### ClaimDO (`CLAIM_DO`, worker `payment-worker`)
- **Назначение:** SQLite-хранилище гостевого реестра оплаченных подписок и единая атомарная точка compare-and-set привязки claim к аккаунту (MONEY-SAFETY #3, #5).
- **Адресация:** одна глобальная инстанция через `idFromName("claims-v2")` (`DO_EPOCH = "v2"`), под single-threaded input gate DO чтение+запись claim атомарны.
- **Хранилище:** SQLite.
  - Таблица `claims`: `claim_id TEXT PRIMARY KEY`, `secret_hash TEXT NOT NULL` (хэш секрета, наружу не отдаётся), `provider TEXT NOT NULL`, `plan_id TEXT NOT NULL`, `status TEXT NOT NULL` (`pending`|`paid`|`claimed`|`void`), `claimed_by TEXT` (userId привязки), `contract_id TEXT`, `email TEXT`, `amount INTEGER`, `currency TEXT`, `period_end INTEGER`, `paid_event_key TEXT` (ключ обработанного paid-события для дедупа), `created_at INTEGER NOT NULL`, `paid_at INTEGER`, `claimed_at INTEGER`, `voided_at INTEGER`, `tg_user_id INTEGER`, `tg_username TEXT`, `pay_url TEXT` (сохранённая lava-ссылка чекаута для переиспользования — F-2). Индексы: `idx_claims_status(status)`, `idx_claims_contract(contract_id)`, `idx_claims_tg_user(tg_user_id)`. Колонки `tg_user_id`, `tg_username`, `pay_url` добавляются идемпотентным `ALTER` через `PRAGMA table_info`.
  - Таблица `refunds`: `user_id TEXT PRIMARY KEY` (один открытый запрос на пользователя), `amount INTEGER NOT NULL`, `currency TEXT NOT NULL`, `contract_id TEXT`, `email TEXT`, `days_left INTEGER`, `created_at INTEGER NOT NULL`, `status TEXT NOT NULL DEFAULT 'requested'`.
- **Операции** (`ensure_schema()` вызывается в начале каждого `fetch`):
  - `POST /create-pending` — `INSERT OR IGNORE` новой pending-строки claim (со `pay_url`).
  - `POST /mark-paid` — по `contractId`: перевод `pending`→`paid` с provider-anchored period; дедуп/tombstone-guard.
  - `POST /claim` — атомарный CAS `paid`→`claimed` с проверкой `secret_hash`.
  - `POST /status` — публичный опрос lifecycle-статуса по `claimId` (без секрета); неизвестный → `none`.
  - `POST /active-by-tg` — новейший неterminal claim по `tgUserId` (статус + `payUrl`), для дедупа повторного чекаута (F-2); `void` пропускается.
  - `GET /unbound` — админ: оплаченные, но не привязанные (`status='paid'`) для ручного возврата.
  - `POST /refund-add` — запись/замена запроса на возврат (upsert по `user_id`).
  - `GET /refunds` — админ: все запросы возврата, новейшие первыми.
  - `POST /tg-for-user` — Telegram id, привязанный к аккаунту (новейший claim по `claimed_by`).
  - `POST /claimed-by` — статус claim + `claimed_by` по `claimId`; неизвестный → `none`.
  - `POST /by-tg` — админ: все claim по `tg_username` (без учёта регистра, срезается `@`) или числовому `tg_user_id` (без секрета).
  - `POST /void` — админ: пометить claim `void` по `claimId`; отказ если уже `claimed`.
  - `POST /void-by-contract` — то же по `contractId` (гостевой refund-webhook).
  - `POST /test-activate` — TEST-ONLY: вставка строки сразу `paid` (доступно только через guarded `/test/*`, в проде невозможно).
- **Инварианты денег/безопасности:** жизненный цикл `pending`→`paid` (только подписанный webhook)→`claimed` (CAS); `void` — необратимый tombstone (MONEY-SAFETY #4): mark-paid на `void`-строке игнорируется (логируется). Дедуп: тот же `paid_event_key` → без перезаписи. Для уже `paid`-строки при другом event key бэкфилл только null-полей контакта; `paid_at`/`period_end` НИКОГДА не сдвигаются вперёд. CAS в `claim` строгого порядка веток: not_found(404)→bad_secret(403)→void(409)→not_paid_yet(409)→claimed(тот же user → idempotent ok; другой → claimed_by_other 403)→paid→CAS. Одна подписка = один аккаунт. `void`/`void_by_contract` отказывают для `claimed` (MONEY-SAFETY #7). `secret_hash` наружу не отдаётся. Nullable-колонки — реальные SQLite NULL для корректности COALESCE.

### SubscriptionDO (`SUBSCRIPTION_DO`, worker `payment-worker`)
- **Назначение:** per-user запись подписки; шлюзовой контракт, из которого ai-worker / ocr-queue / story читают флаг `active`.
- **Адресация:** одна инстанция на пользователя через `idFromName("v2:<userId>")` (формат `{DO_EPOCH}:{user_id}`).
- **Хранилище:** KV — единственный ключ `"sub"`, значение сериализуется из `SubRecord`: `plan: String` (planId, или `"paid"`/`"none"`), `status: String` (`paid`|`cancelled`|`expired`), `start: i64`, `end: i64`, опциональные `provider`, `contract_id`, `email`, `no_renew: bool`, `activate_key: String`. Default-запись (`plan="none"`, `status="expired"`, `start=0`, `end=0`) НИКОГДА не персистится — используется только при чтении отсутствующей записи.
- **Операции** (`load()` из ключа `"sub"` в начале каждого `fetch`):
  - `GET /subscription` — вернуть статус; `active` пересчитывается на каждом чтении как `now_ms() < end`.
  - `POST /activate` — provider-driven: `paid`, `plan="paid"`, `start=now`, `end` привязан к provider `periodEnd` (иначе `now + 30 дней`); проставить `provider`/`contractId`/`email`, `no_renew=false`, `activate_key`.
  - `POST /cancel` — отмена авто-продления: `no_renew=true`, `status="cancelled"`, активен до `end`; терпит пустое тело.
  - `POST /refund` — немедленный отзыв: `end=now`, `status="expired"`.
- **Инварианты денег/безопасности:** нет trial (MONEY-SAFETY #6) — never-paid аккаунт имеет `end=0` → `active:false`, default не персистится. `/activate` идемпотентен по `activateKey` (MONEY-SAFETY #4): реплей той же активации при `status=="paid"` → no-op. `end` анкорится к provider-reported `periodEnd`, а не к wall-clock (используется только если `pe > now`). GATE CONTRACT: поля `active`/`end` переименовывать нельзя; `active` всегда пересчитывается против wall-clock.

### PaymentIndexDO (`PAYMENT_INDEX_DO`, worker `payment-worker`)
- **Назначение:** единый глобальный индекс, отображающий ключи `order:<id>` / `contract:<id>` / `claim-contract:<id>` в userId (или гостевой claimId).
- **Адресация:** одна глобальная инстанция через `idFromName("index-v2")` (`DO_EPOCH = "v2"`).
- **Хранилище:** KV (DO storage key→value); ключ произвольный (передаётся в запросе), значение — строка `userId`. Нет фиксированной схемы.
- **Операции:** `POST /put` (`storage().put(key, userId)`); `POST /delete` (`storage().delete(key)`); `GET /get?key=<k>` → `{ "userId": <string|null> }`.
- **Инварианты:** в коде специальных money-safety-инвариантов нет.

**wrangler.toml (payment-worker):** биндинги одинаковы в dev и `[env.production]`. Единственная миграция — tag `v1`: `new_sqlite_classes = ["SubscriptionDO", "PaymentIndexDO", "ClaimDO"]`.

---

## telegram-worker

### TgSessionDO (`TG_SESSION_DO`, worker `telegram-worker`)
- **Назначение:** SQLite-хранилище Telegram-сессий: промокод чата и mapping claimId → секрет для paid-push (обычный бот и Mini App).
- **Адресация:** глобальный единственный инстанс `id_from_name("global-v2")` (`DO_EPOCH = "v2"`). Migration tag `v1`, `new_sqlite_classes = ["TgSessionDO"]`.
- **Хранилище:** три SQLite-таблицы:
  - `sessions`: `chat_id INTEGER PRIMARY KEY`, `promo_code TEXT` (nullable), `updated_at INTEGER NOT NULL`.
  - `claims`: `claim_id TEXT PRIMARY KEY`, `chat_id INTEGER NOT NULL`, `secret TEXT NOT NULL` (plaintext, живёт только здесь), `created_at INTEGER NOT NULL`, `notified_at INTEGER` (nullable, идемпотентность paid-push).
  - `miniapp_claims`: `claim_id TEXT PRIMARY KEY`, `tg_user_id INTEGER NOT NULL` (владение = WebApp user.id, не chat_id), `secret TEXT NOT NULL`, `created_at INTEGER NOT NULL`, `notified_at INTEGER` (nullable). `notified_at` добавляется идемпотентным `ALTER` через `PRAGMA table_info`.
- **Операции** (все POST, `ensure_schema()` в начале каждого `fetch`):
  - `/session/set-promo` — UPSERT промокода (last-typed wins); `/session/get-promo` — чтение (NULL/пусто → JSON null).
  - `/claims/put` — `INSERT OR IGNORE` claim→{chat_id, secret}; `/claims/get` → `{found}`/`{chatId, secret, notifiedAt}`; `/claims/mark-notified` — `notified_at = now`.
  - `/miniapp/claims/put` — `INSERT OR IGNORE` claim→{tg_user_id, secret}; `/miniapp/claims/get`; `/miniapp/claims/by-user` — `ORDER BY created_at DESC LIMIT 10` → `{claimId, secret}`; `/miniapp/claims/mark-notified`.
- **Инварианты:** один глобальный инстанс — все операции под input gate → UPSERT атомарны. Plaintext claim-секрет хранится ТОЛЬКО здесь и не логируется; выдаётся только in-process на owner-gated путях. put-операции идемпотентны (`INSERT OR IGNORE`), mark-notified идемпотентен.

---

## support-worker

### ConversationDO (`CONVERSATION_DO`, worker `support-worker`)
- **Назначение:** per-user диалог: append-only лог сообщений плюс read-курсоры пользователя и эксперта.
- **Адресация:** один инстанс на пользователя `id_from_name(user_id)` (user_id = JWT sub). Epoch-префикса нет. Migration tag `v1`, `new_sqlite_classes = ["ConversationDO", "ConversationIndexDO"]`.
- **Хранилище:** две SQLite-таблицы:
  - `messages`: `seq INTEGER PRIMARY KEY`, `client_id TEXT NOT NULL UNIQUE` (идемпотентность), `sender TEXT NOT NULL`, `expert_id TEXT` (nullable), `text TEXT NOT NULL`, `created_at TEXT NOT NULL` (RFC3339, для отображения).
  - `meta`: `k TEXT PRIMARY KEY`, `v TEXT NOT NULL`; начальные строки: `next_seq='1'`, `user_read_seq='0'`, `expert_read_seq='0'`, `user_meta='{}'`.
- **Операции** (`ensure_schema()` в начале `fetch`; роутинг по path):
  - `/append` — идемпотентный append: `client_id` уже есть → вернуть seq/created_at с `deduped:true`; иначе `seq = next_seq`, серверный timestamp, INSERT, advance `next_seq`.
  - `/list` — `WHERE seq > after_seq ORDER BY seq ASC`, limit+1 для `has_more`; default limit 50, clamp 1..200.
  - `/read` — сдвинуть read-курсор (`user`/`expert`) вперёд, только если `seq > current` (монотонно); неизвестная роль → 400.
- **Инварианты:** single-threaded DO делает SELECT-then-INSERT в `/append` race-free; `UNIQUE(client_id)` — backstop идемпотентности. Только UNIQUE-конфликт трактуется как дубликат; прочие ошибки surface'ятся (repo policy: не глушить). Read-курсоры монотонны.

### ConversationIndexDO (`CONVERSATION_INDEX_DO`, worker `support-worker`)
- **Назначение:** глобальный индекс-очередь по одной строке на диалог для эксперта плюс runtime-авторизация экспертов.
- **Адресация:** глобальный singleton `id_from_name("index")`. Epoch-префикса нет. Migration tag `v1` (общий с ConversationDO).
- **Хранилище:** четыре SQLite-таблицы:
  - `conversations`: `user_id TEXT PRIMARY KEY`, `preview TEXT`, `last_ts TEXT`, `last_seq INTEGER`, `pending_since TEXT` (nullable; NULL = отвечено), `pending_seq INTEGER` (монотонный порядок прибытия). Индекс `idx_pending(pending_seq)`.
  - `meta`: `k TEXT PRIMARY KEY`, `v TEXT NOT NULL`; строка `next_pending_seq='1'`.
  - `admins`: `sub TEXT PRIMARY KEY`, `approved_at TEXT NOT NULL`.
  - `admin_requests`: `code TEXT PRIMARY KEY`, `sub TEXT NOT NULL UNIQUE`, `created_at TEXT NOT NULL`.
- **Операции** (`ensure_schema()` в начале `fetch`; роутинг по path):
  - `/touch-user` — USER-сообщение: нет строки → INSERT с новым `pending_seq`; есть и `pending_since IS NULL` → новый pending-run с fresh `pending_seq`; advance preview/last_* только если `last_seq > existing`.
  - `/clear-pending` — EXPERT-ответ: очистить `pending_since/pending_seq` только если `existing_last <= reply_seq`.
  - `/conversations` — листинг `status=pending` (`pending_since IS NOT NULL ORDER BY pending_seq ASC`, курсор = pending_seq) или `status=answered` (`ORDER BY user_id ASC`); limit+1 для `has_more`.
  - `/admin-request` — кандидат запрашивает код (идемпотентно, retry до 8 при коллизии PK); `/admin-approve` — резолв `code → sub`, `INSERT OR REPLACE INTO admins`, DELETE запроса (код одноразовый); `/admin-is-approved`; `/admin-get` (`{approved, code|null}`).
- **Инварианты:** single-threaded DO → read-modify-write счётчика `next_pending_seq` и SELECT-then-write атомарны. touch/clear идемпотентны + монотонны (worker может всегда их звать, retry self-heal'ится). Очередь — строго возрастающий целочисленный `pending_seq` (без ms-ties); re-opened диалог (`last_seq > reply_seq`) не очищается stale-ответом. `sub` — из аутентифицированного хендлера, не из тела. `admin_requests.sub UNIQUE`; код 8 симв., 32-симв. алфавит без I/O/0/1 (40 бит), CSPRNG.

---

## auth-worker

### AuthDO (`AUTH_DO`, worker `auth-worker`)
- **Назначение:** глобальный сервер аутентификации: passkey-регистрация/логин (WebAuthn discoverable), pairing устройств, метаданные токенов и recovery-ключи.
- **Адресация:** единственный глобальный инстанс `id_from_name("global")`. Epoch-префикса нет.
- **Хранилище:** НЕ SQLite. KV с префиксами ключей:
  - `cred:{cred_id}` — `StoredPasskey` (user_id, cred_id, public_key, name, created_at, last_used_at, counter).
  - `user_creds:{user_id}` — массив cred_id пользователя.
  - `user:{user_id}` — `UserMetadata` (`recovery_hash_data: Option<{salt_b64, hash_b64}>`, `created_at`).
  - `pairing:{pairing_id}` — `PairingSession` (pairing_id, secret_hash, user_id, created_at, expires_at, status).
  - `pk_state:{id}` — `PasskeyState` (временное состояние ceremony, TTL по `expires_at`).
  - `token:{token_id}` — `TokenMetadata` (token_id, user_id, fingerprint, created_at, last_used_at); `user_tokens:{user_id}` — массив token_id.
- **Операции** (все POST, по path): `/register/begin|finish`, `/authenticate/begin|finish` (discoverable, без username), `/pair/create|request|approve|claim|finish|status|check`, `/token/store|validate|list`, `/recovery/set|verify`.
- **Инварианты:** recovery-ключ — HMAC-SHA256(key, random 32-byte salt), проверка константным `verify_slice`. `select_is_admin` — чистое равенство origin против env `ADMIN_RP_ORIGIN`; клиентский origin — только селектор, НИКОГДА не копируется в `PasskeyConfig` (origin фиксирован env). Пустой ceremony-origin → ошибка (без fallback). Passkey-state TTL 300с. Pairing — строгая машина состояний (Pending→Approved→Claimed→Completed/Expired): нельзя claim без user_id, mismatched secret → 403, expired → 410, повтор → 409.

### UserDO (`USER_DO`, worker `auth-worker`)
- **Назначение:** пустая заглушка (deprecated); вся auth-логика в AuthDO.
- **Адресация:** биндинг `USER_DO`; в коде не адресуется (стаб не создаётся).
- **Хранилище:** нет.
- **Операции:** `fetch` на любой запрос → `404 "UserDO is deprecated — use AuthDO"`. Комментарий: «kept as an empty shell for future sync data purposes».

---

## bug-report-worker

### BugReportDO (`BUG_REPORT_DO`, worker `bug-report-worker`)
- **Назначение:** единый глобальный append-only лог всех баг-репортов.
- **Адресация:** единственный глобальный инстанс `id_from_name("global")`. Epoch нет.
- **Хранилище:** SQLite, таблица `reports`: `id TEXT PRIMARY KEY` (`bug_{uuid_v4}`), `user TEXT NOT NULL` (JWT sub), `received_at INTEGER NOT NULL` (ms), `title TEXT NOT NULL`, `area TEXT NOT NULL` (default `"other"`), `steps_to_reproduce TEXT NOT NULL`, `expected TEXT NOT NULL`, `actual TEXT NOT NULL`, `severity TEXT NOT NULL` (default `"medium"`), `app_version TEXT NOT NULL`.
- **Операции:** `POST /report` (INSERT, `user` обязателен, → `{id}`); `GET /reports` (`ORDER BY received_at DESC, id DESC LIMIT 500`).
- **Инварианты:** append-only (INSERT/SELECT, без update/delete); newest-first, лимит 500.

---

## sync-worker

### SyncDO (`SYNC_DO`, worker `sync-worker`)
- **Назначение:** per-user хранилище данных приложения с синхронизацией last-writer-wins.
- **Адресация:** один инстанс на пользователя `id_from_name(sub)` (JWT sub). Epoch нет.
- **Хранилище:** НЕ SQLite. KV: каждая коллекция под своим именем как JSON-map (`BTreeMap<String, Value>`):
  - keyed by `id`: `foods`, `diary_entries`, `recipes`, `recipe_ingredients`, `goals`, `weight_entries`, `step_entries`, `deletions` (append-only tombstone-аккумулятор).
  - `story` — keyed by `key`; `profile` — singleton (ключ `"profile"`).
- **Операции:** `POST /sync/dump` (все коллекции в массивы; `diary_entries` исключает `deleted:true`); `POST /sync/push` (мёрж по `is_newer`/LWW, → `{conflicts: null}`).
- **Инварианты:** LWW по RFC3339 `updated_at` (лексикографически); tombstone `deleted:true` сохраняется при более новом `updated_at`, чтобы `dump` его опускал. `profile`/`story` — whole-object LWW. Строки без `id`/`key` пропускаются.

---

## main-flow

### PushDO (`PUSH_DO`, worker `main-flow`)
- **Назначение:** хранит Web Push subscription-объекты и выдаёт их по пользователю или целиком для рассылки.
- **Адресация:** единый глобальный инстанс `id_from_name("global")`. Epoch нет.
- **Хранилище:** KV: `push_sub:<user_id>:<hash>` → `PushSubscription` (`<hash>` = base64url первых 16 байт SHA-256 от endpoint); `user_push_subs:<user_id>` → `Vec<String>` хешей.
- **Операции:** `/subscribe` (сохранить + добавить hash в индекс); `/unsubscribe` (по user_id+endpoint); `/unsubscribe-by-endpoint` (по всем пользователям); `/list` (по user_id); `/list-all` (broadcast). `alarm()` — нет.
- **Инварианты:** дедуп по `hash` эндпоинта; индекс-список и подписки обновляются согласованно.

### ScheduleDO (`SCHEDULE_DO`, worker `main-flow`)
- **Назначение:** расписание напоминаний одного пользователя; по alarm рассылает web-push ближайшего включённого слота.
- **Адресация:** per-user `id_from_name(user_id)`. Epoch нет.
- **Хранилище:** KV: `schedule` → `UserSchedule` (`utc_offset_minutes: i32`; слоты `weigh_in/breakfast/lunch/dinner/steps`, каждый `{enabled, time "HH:MM"}`); `user_id`; `next_slot`.
- **Операции:** `/update` (сохранить, снять старый alarm, `schedule_next_alarm`); `/get`; `/test-alarm` (тест, alarm через `delay_s` default 90с); `alarm()` → `handle_alarm()`: если слот `enabled` — push через `send_push_to_user`, затем перепланирование с буфером `30_000` мс.
- **Инварианты:** alarm всегда на ближайший включённый слот (минимальный `fire_ms`); local→UTC через `utc_offset_minutes` + `rem_euclid(24*60)`; `fire_ms <= now + buffer` → +сутки (buffer против мгновенного повтора); нет включённых слотов → alarm и `next_slot` удаляются.

---

## ocr-queue

### QueueDO (`QUEUE_DO`, worker `ocr-queue`)
- **Назначение:** единая FIFO-очередь OCR-задач: метаданные задачи, чанкованный base64-образ, прогресс; отдаёт задачи поллеру и статус клиенту.
- **Адресация:** единый глобальный инстанс `id_from_name("global")`, запинен к региону `QUEUE_REGION` (`weur`) через locationHint. Epoch нет.
- **Хранилище:** KV (SQLite-backed namespace, но через storage KV-API): `queue` → `Vec<String>` id (позиция = порядок); `job:<id>` → `Job` (`id`, `status` queued/processing/done/error, `owner`, `custom_nutrients`, `chunks`, `created_at`, `started_at?`, `updated_at`, `phase?` thinking/answer, `thinking_tokens?`, `answer_tokens?`, `result?`, `error?`); `img:<id>:<n>` → n-й чанк base64 (`CHUNK = 700_000` символов).
- **Операции:** `POST /enqueue` (режет образ на чанки, `status="queued"`, id в хвост); `/claim` (снять с головы, пропуская не-`queued`; первый queued → processing); `/image?id=` (склейка чанков); `POST /progress` (phase/токены); `POST /complete` (error→`error`/иначе `done`+result, затем удалить чанки); `/status?id=` (статус + позиция 1-based); `/tail?id=&since=` (long-poll до 20 000 мс, интервал 250 мс). `alarm()` — нет.
- **Инварианты:** строго FIFO (push в хвост, `remove(0)` с головы); `/claim` идемпотентен (не-`queued` пропускаются). `CHUNK = 700_000` ниже per-value лимита DO и ОБЯЗАН совпадать с TS-константой (иначе повреждение образа). Образы удаляются только после `/complete`. Cross-script binding `SUBSCRIPTION_DO`→`SubscriptionDO` (script payment-worker) — чужой DO, для гейта.
