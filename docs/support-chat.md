# Human support chat — workers, UI & deploy runbook

The **human** support chat (user ↔ expert), separate from the AI `/chat` assistant.
A user toggles `/chat` to **Live** mode; messages go to a server thread. Experts
answer from a separate PWA (the admin console). This doc covers every moving part
and how to deploy/operate it.

> Companion memory: `[[project_support_human_chat]]` (running status/decisions).

---

## 1. Components

| Component | What | Where (dev) | Where (prod) |
|---|---|---|---|
| **support-worker** (Rust) | The chat backend: threads, queue, expert + admin-auth API | `support-worker.vg-stavenko.workers.dev` | `support.renorma.app` (`support-worker-prod`) |
| **auth-worker** (Rust) | Passkey auth for BOTH the user app and the admin console (origin-aware RP) | `auth-worker.vg-stavenko.workers.dev` | `auth.renorma.app` |
| **main-flow** (Rust) | Holds VAPID keys + push subscriptions; sends web-push | `main-flow.vg-stavenko.workers.dev` | `push.renorma.app` (`main-flow-prod`) |
| **frontend** (Leptos) | The user app; `/chat` AI/Live toggle + Live thread client | `hjkl-ft.pages.dev` | `fit.renorma.app` (`renorma-app`) |
| **admin** (Leptos) | Expert console PWA: queue → thread → reply | `renorma-admin.pages.dev` (`renorma-admin`) | `admin.renorma.app` |

---

## 1a. Куратор

Куратор — САМОСТОЯТЕЛЬНАЯ роль, а не разновидность эксперта.

| | эксперт (админка) | куратор |
|---|---|---|
| домен и RP паскея | `admin.renorma.app` | `curator.renorma.app` |
| как получают доступ | оператор одобряет код (`/admin/approve`) | регистрируются сами (`/curator/register`) |
| кого видят | все треды, адресованные админу | только своих клиентов |
| откуда берут данные | присланные снимки | присланные отчёты |

**Маршрутизация.** `/message`, `/messages`, `/read` смотрят привязку: есть куратор —
тред с ним, нет — тред с админом. Развилка живёт ТОЛЬКО в воркере
(`routing_of_user`); приложение худеющего про адресата не знает и знать не обязано.
У человека с куратором сообщения в очередь админа не попадают.

**Приглашение.** Куратор заводит слот с именем (это имя видит только он — у худеющего
имени в системе нет) и получает ссылку `{app_origin}/curator?c=<код>`. Код гаснет
СОГЛАСИЕМ, а не открытием: человек вправе подумать и вернуться. Приглашение — для тех,
кто уже пользуется приложением; открывшего без сессии экран отправляет на лендинг.

**Данные.** Сервер данных худеющего не хранит и здесь. Куратор просит отчёт
(`kind:"data_request"`, `payload={scope}`), человек отправляет его сам
(`kind:"data_share"`), и последний отчёт кэшируется в слоте — чтобы дашборд открывался
сразу, а не перелистывал переписку.

**Срок отчёта — выбор из двух, а не число дней.** `scope` = `new` (всё после прошлого
отчёта) или `all` (вся история). Числа дней нет ни у кого: ни куратор, ни худеющий не
знают, сколько дней накопилось, — они знают «это я уже видел» и «я не видел ничего».

Граница «нового» — `report.period.to` ПРОШЛОГО отчёта, прочитанный прямо из него:
отчёты лежат в том же треде и синкаются вместе с ним, так что отдельного поля под
границу не нужно. Нет прошлого отчёта — вариант «только новое» не показывается вовсе,
ни в одном из двух приложений: новое относительно чего?

**Сегодняшний день в отчёт не попадает никогда.** Последний день данных — вчера. День
ещё заполняется, и куратор, увидевший «съедено 400 ккал» в обед, стал бы лечить
недобор, которого нет.

**Планки: одно место, одно правило.** Действующая планка — это запись в
`planka_history`; нет записи — работает наше правило по умолчанию из общего крейта
`common/plankas`. Больше планка нигде не живёт: ни `goals`, ни `ProfileRow.steps_planka`
источником не являются. Фасад — `frontend/src/services/plankas.rs` (синхронный кэш по
образцу `profile`, гидратируется после смены базы, синка, миграций и каждой записи).

Отсюда три следствия, на которых всё и держится:

- **Привязка к куратору сама по себе выключает автопересчёт.** Замков на отдельные
  планки нет: пока человека ведёт куратор, приложение не двигает НИ ОДНУ — ни недельный
  цикл калорий и шагов, ни вывод белка из калорий. Недельные письма про новую планку при
  кураторе не приходят вовсе: пересчёта нет — сообщать не о чем.
- **Для девяти постоянных видов запись может появиться ТОЛЬКО от куратора** — приложение
  их никогда не пишет. Поэтому «стереть кураторское» — это `plankas::forget`, и различать
  авторство не нужно.
- **Куратор всегда отправляет ЧИСЛО.** Директивы «пересчитай» нет. Посчитать он может сам:
  кнопка «Рассчитать» в редакторе калорий и белка зовёт `plankas::suggest` по последнему
  отчёту — тот же код, которым живёт недельный цикл у худеющего, — подставляет результат в
  поле, и куратор отправляет его как есть или поправив.

**Директивы.** `set_planka_v2` с `{key, amount}` — одна на любой из двенадцати видов.
Несёт ЧИСЛО; текст плашки и письма собирает приложение худеющего на СВОЁМ языке
(`frontend/src/services/directives.rs`). Прежние `set_planka` и `open_week` переведены
на тот же порядок.

**Отчёт для расчёта.** Кроме индикаторов и веса он везёт `body` (пол, рост, ВОЗРАСТ —
посчитанный у человека: у куратора своя дата, а нормы железа идут ступеньками по
возрасту), действующие планки по всем двенадцати видам, их историю и `avg_kcal_7d` —
среднее съеденное за 7 завершённых дней. Последнее и есть вход `adherence`: без него
не отличить «вес стоит, потому что планка велика» от «вес стоит, потому что её не
держат», а недельный цикл решает на этом различии.

**Отвязка.** Куратором, самим человеком или переходом к другому куратору — во всех трёх
случаях приложение узнаёт её по одному признаку: сменился адресат в опросе. Тогда девять
ПОСТОЯННЫХ планок забываются (стирается запись, а не пишется наше число: клетчатка обязана
и дальше следовать за калориями, железо — за профилем), а калории, шаги и белок остаются
кураторскими до ближайшего недельного пересчёта — он их и заберёт обратно. Оба недельных
якоря сдвигаются на сегодня, и приходит письмо с ПЕРЕЧНЕМ действующих планок и кнопкой
«пересчитать сейчас». При переходе сразу к другому куратору письма нет: планки ведёт он.

**Проверка:** `node scripts/curator-e2e.mjs` — весь путь по ручкам живого dev-воркера.

---

## 2. support-worker

`cloudflare/support-worker/` — Rust (worker 0.8, SQLite Durable Objects).

### Durable Objects
- **`ConversationDO`** — тред на ПАРУ, а не на человека. Пара «худеющий ↔ админ»
  сохранила прежний ключ (`id_from_name(user_id)`), пара «худеющий ↔ куратор» —
  `id_from_name("{user_id}|curator:{curator_id}")`. Переписка личная: новый куратор
  не видит разговора с предыдущим. SQLite `messages(seq PK, client_id UNIQUE, sender,
  expert_id, text, created_at, kind, payload, sender_name)` + отметки прочтения. `seq`
  монотонен ВНУТРИ треда и он же курсор опроса — между тредами номера несравнимы.
- **`CuratorIndexDO`** — **global** (`id_from_name("curators")`). Кураторы и их слоты
  клиентов: `curators(curator_id PK, name, lang)` и `clients(id PK, curator_id, name,
  invite_code UNIQUE, user_id, …, last_report, request_scope)`. Частичный UNIQUE по
  `user_id` держит правило «один худеющий — один куратор».
- **`ConversationIndexDO`** — **global** (`id_from_name("index")`). The sortable queue
  (`conversations`, `pending_seq` monotonic counter) **plus** the admin allowlist:
  `admins(sub PK, approved_at)` and `admin_requests(code PK, sub UNIQUE, created_at)`.

### HTTP API (`src/lib.rs` router)
**User** (auth: JWT, `sub` = user_id):
- `POST /message` `{client_id,text}` → `{seq,created_at}` (idempotent by client_id)
- `GET  /messages?after_seq&limit` → `{messages,next_after_seq,has_more}` (cursor paging)
- `POST /read` `{seq}`

**Expert** (auth: `auth_expert` — JWT `sub` approved in the DO `admins` table):
- `GET  /conversations?status=pending|answered&after&limit` — pending = oldest-waiting first
- `GET  /conversations/:uid/messages?after_seq&limit`
- `POST /conversations/:uid/reply` `{client_id,text}` → `{seq}` (also fires the push nudge)
- `POST /conversations/:uid/read` `{seq}`

**Admin auth** (the no-redeploy expert-approval flow):
- `GET  /admin/me` (user JWT) → `{approved:bool, code:string|null}` — env-experts report `approved:true`
- `POST /admin/request` (user JWT) → `{code}` — short random code bound to THIS token's `sub`, idempotent
- `POST /admin/approve` (header `X-Admin-Secret: <ADMIN_APPROVE_SECRET>`, NO user JWT) `{code}` → adds the code's `sub` to `admins`

### Auth model
- `auth_user` — verifies the JWT (HS256, `JWT_SECRET`), returns `sub`.
- `auth_expert` (async) — passes iff the index DO's `admins` table has `sub` (the ONLY
  source of truth; no env allowlist). Fail-loud 500 on any DO error; 403 otherwise.
- **Expert approval flow** (adds an expert WITHOUT redeploy):
  1. Candidate signs in on the admin console (passkey → JWT with their `sub`).
  2. Admin UI calls `POST /admin/request` → shows a short code.
  3. Candidate forwards the code to the operator.
  4. Operator calls `POST /admin/approve` with `X-Admin-Secret` + `{code}`.
  5. `auth_expert` now lets that `sub` through (DO-backed; no redeploy).
  - The approved `sub` is resolved ONLY from the stored `code→sub` record, never from the
    approve caller. `admin_approve` **fails closed** (500) if `ADMIN_APPROVE_SECRET` is unset.

### Push nudge (best-effort)
After an expert reply, `nudge_user_push` calls main-flow `POST /push/notify` to nudge the
user to re-open Live. Reaches main-flow via the **`MAIN_FLOW` service binding** (a
Worker→Worker fetch over `*.workers.dev` returns 404 — the binding avoids that). Header
`X-Internal-Key: INTERNAL_PUSH_KEY`, body `{userId, body, url:"/chat?notif=1"}`. **Best-effort**:
a push failure is logged loudly (`console_error!`) but does NOT fail the reply (same policy
as payment-worker's `notifyPush`).

### Vars / secrets (`wrangler.toml`)
| Name | Dev (`[vars]`) | Prod | Notes |
|---|---|---|---|
| `JWT_SECRET` | `dev-secret-change-in-production` | **secret** | MUST equal auth-worker's |
| `INTERNAL_PUSH_KEY` | `dev-internal-push-key` | **secret** | MUST equal main-flow's |
| `ADMIN_APPROVE_SECRET` | `dev-admin-approve-secret` | **secret** | operator approve key; unset ⇒ approve fails closed |
| `MAIN_FLOW` (service binding) | → `main-flow` | → `main-flow-prod` | not a var |

CORS allowlist (`is_allowed_origin`): `renorma.app`, `*.renorma.app`, `hjkl-ft.pages.dev`,
`renorma-admin.pages.dev`, localhost.

---

## 3. auth-worker — origin-aware passkey RP

One auth-worker serves both apps. `passkey_config(origin)` picks the relying-party scope by
the request Origin (fixed env values only, never echoes the client origin; fails loud on empty):

| Origin | RP_ID | RP_ORIGIN |
|---|---|---|
| user app (dev / prod) | `hjkl-ft.pages.dev` / `renorma.app` | `https://hjkl-ft.pages.dev` / `https://fit.renorma.app` |
| admin (dev / prod) | `renorma-admin.pages.dev` / `admin.renorma.app` | `https://renorma-admin.pages.dev` / `https://admin.renorma.app` |

Prod admin `RP_ID` is `admin.renorma.app` (NOT `renorma.app`) so app and admin credentials
are isolated at the WebAuthn rp_id-hash level (else discoverable login crosses scopes).
Passkeys can't use a public-suffix domain as rp_id, so the admin must be served from its own
host (`renorma-admin.pages.dev` dev, `admin.renorma.app` prod) — never a bare `pages.dev`.

---

## 4. Admin console (`admin/`)

Leptos 0.6 CSR PWA. Light inline CSS, no IndexedDB.
- `auth.rs` — passkey register/authenticate against the auth-worker (origin-aware). Session =
  `user_id` + `auth_token` in localStorage.
- `api.rs` — expert client (`ApiError::Auth` on 401/403), `admin_me()`, `admin_request()`.
- `app.rs` — `View::{Login, RequestAccess, Queue, Thread}`. After login → `/admin/me`:
  approved → **Queue** (pending oldest-first + answered tab, 5s auto-poll) → **Thread** (reply,
  4s poll); not approved → **RequestAccess** ("Запросить доступ" → shows code; "Проверить доступ").
- Config: `config/frontend.toml` (`auth_base_url`, `support_base_url`); `config-prod/` for prod.

---

## 5. User-side Live chat (frontend)

- `/chat` has an **AI / Live** toggle (`components/mode_toggle.rs`). Live renders a separate
  subtree; the two threads never share state.
- `services/support_chat.rs` — Live client: cursor + optimistic outbox, IndexedDB stores
  `support_messages` / `support_outbox` / `support_meta` (DB v12). Polls every 4s in Live.
- **Deep link**: a push nudge opens `/chat?notif=1`; `chat.rs` reads `location.search` and
  forces Live mode (persisted).

---

## 6. Deploy runbook

### Dev (routine; `hjkl-ft.pages.dev` is the test env)
```bash
# workers
(cd cloudflare/support-worker && npx wrangler deploy)
(cd cloudflare/auth-worker    && npx wrangler deploy)
# user app
(cd frontend && trunk build --release && npx wrangler pages deploy dist --project-name=hjkl-ft --branch main --commit-dirty=true)
# admin
(cd admin    && trunk build --release && npx wrangler pages deploy dist --project-name=renorma-admin --branch main --commit-dirty=true)
```

### Prod cutover
**Prereqs:** custom domains pointed at the prod workers/Pages (`support.renorma.app`,
`auth.renorma.app`, `push.renorma.app`, `fit.renorma.app`, `admin.renorma.app`).

1. **Secrets** (interactive — run yourself via `! wrangler secret put …`):
   ```bash
   # support-worker-prod
   (cd cloudflare/support-worker && \
     wrangler secret put JWT_SECRET           --env production && \  # = auth-worker-prod's
     wrangler secret put INTERNAL_PUSH_KEY    --env production && \  # = main-flow-prod's
     wrangler secret put ADMIN_APPROVE_SECRET --env production)      # operator approve key
   ```
   Approve the first expert via the code flow (`POST /admin/request` → `POST /admin/approve`
   with `ADMIN_APPROVE_SECRET`); there is no env allowlist.
2. **Workers:**
   ```bash
   (cd cloudflare/auth-worker    && npx wrangler deploy --env production)
   (cd cloudflare/support-worker && npx wrangler deploy --env production)
   ```
3. **User app:** `frontend/scripts/deploy-prod.sh` (builds, swaps prod config + CSP → `*.renorma.app`,
   deploys to the `renorma-app` Pages project / `fit.renorma.app`).
4. **Admin:** `admin/scripts/deploy-prod.sh` (swaps `config-prod` + prod CSP `auth.renorma.app`/
   `support.renorma.app`, deploys). Create the Pages project + custom domain `admin.renorma.app` first.

### Verify
- `node scripts/admin-smoke.mjs` (local dist vs live worker) / with `ADMIN_URL=…` for a deployed URL.
- `node scripts/admin-passkey-check.mjs` (real passkey register/authenticate on the deployed admin).
- `cd e2e && npx playwright test support-chat.spec.ts` (user-side Live thread + `?notif=1`).
- Approval flow smoke (request → approve with the secret → expert access) — see
  `scripts/` ad-hoc node checks.

---

## 7. Known gaps / TODO
- **Push delivery** works via the service binding on support-worker; **payment-worker still
  uses the public-URL fetch** and likely 404s silently — same `MAIN_FLOW` binding fix applies.
- Prod cutover not done (needs the secrets + custom domains above).
- The admin console has no real-passkey-through-the-queue e2e yet (smoke injects an expert JWT
  for the queue path; passkey is checked separately by `admin-passkey-check.mjs`).
