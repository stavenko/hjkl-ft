# Онбординг с лендинга с подпиской (счастливый путь)

Человек с лендинга оформляет подписку и заходит в PWA. Это базовая цепочка; все денежные
ветвления (прерванный онбординг, отмена, возврат, возобновление) отходят от её шагов.

**Акторы:** пользователь · лендинг (`renorma.app`) · Telegram + бот
(`@renorma_payment_helper_bot`) · Mini App (`tg.renorma.app`, telegram-worker) ·
payment-worker (`pay.renorma.app`) · lava.top · PWA (`fit.renorma.app`, frontend) ·
auth-worker (`auth.renorma.app`).

**Почему такая связка:** passkey/WebAuthn не работает во встроенном браузере Telegram,
поэтому оплата и онбординг открываются во внешнем Safari через `Telegram.WebApp.openLink`.

---

## Поток по шагам

### 1. Лендинг → Telegram
- Пользователь на `renorma.app` жмёт CTA → ссылка `t.me/renorma_payment_helper_bot/pay`.
- Открывается Telegram, бот запускает Mini App (`web_app` кнопка → `tg.renorma.app/`).
- **Данные:** нет. **Внешние:** нет.

### 2. Mini App: проверка «уже оплачено?»
- `serve_miniapp_page` отдаёт HTML. JS читает `Telegram.WebApp.initData`.
  - **Gate:** нет `initData` (открыто вне Telegram) → экран «только в Telegram», UI оплаты не показывается.
- `POST /miniapp/me` (initData HMAC-валидируется → `tg_user_id`):
  - `TgSessionDO.miniapp_claims` — claim'ы этого `tg_user_id` (пока нет).
  - → ответ `{status:"none"}` → **форма оплаты** (промокод + «Оплатить»).
- **Читает:** `TgSessionDO.miniapp_claims`. **Внешние:** нет.

### 3. Оплата → создание claim + счёт lava
- Пользователь вводит промокод (опц.), жмёт «Оплатить» → `POST /miniapp/checkout`.
- telegram-worker: валидирует initData → `tg_user_id` + `@username`; зовёт payment-worker
  `POST /internal/checkout` (гейт `INTERNAL_PUSH_KEY`, prod-only) → `do_guest_checkout`:
  1. Провайдер = lava; `offer_id = LAVA_OFFER_ID` (единственный оффер, каталога в конфиге нет).
  2. Генерирует `claim_id` и `secret` (256-бит), `secret_hash = sha256(secret)`.
  3. **lava `POST /api/v3/invoice`** `{email:"<claimId>@guest.renorma.app", offerId, currency:"RUB", promoCode}` → `paymentUrl` + `id` (это `contractId`).
  4. **`ClaimDO./create-pending`** — строка claim:
     `{claim_id, secret_hash, provider:"lava", plan_id:offer_id, contract_id:<order>, status:"pending", tg_user_id, tg_username, created_at}`.
  5. **`PaymentIndexDO`**: `claim-contract:<contractId>` → `claim_id` (чтобы webhook нашёл строку).
  6. Возвращает `{payUrl, claimId, secret}` (секрет — только доверенному telegram-worker).
- telegram-worker: **`TgSessionDO.miniapp_claims./put`** `{claim_id, tg_user_id, secret, created_at}`
  (владелец + секрет живут ТОЛЬКО здесь); возвращает Mini App `{claimId, payUrl}` — **без секрета**.
- Mini App: `openLink(payUrl)` → Safari → hosted-checkout lava; переходит в состояние «ожидаем оплату», поллит `/miniapp/status`.
- **Создаёт:** `ClaimDO`(pending), `PaymentIndexDO`(mapping), `TgSessionDO.miniapp_claims`.
  **Внешние:** lava invoice.
  **Секрет-безопасность:** `secret` уходит из payment-worker только в telegram-worker; из
  Mini App — никогда (позже вернётся к пользователю только через фрагмент `#claim=`).

### 4. Пользователь платит на lava
- Оплата картой на стороне lava (вне нашей системы). Возврата на наш сайт lava по дизайну не делает — канал уведомления = webhook.

### 5. Webhook: подтверждение оплаты
- **lava webhook → payment-worker `POST /webhook/lava`**:
  1. Проверка подписи: заголовок `X-Api-Key` == `lava-hook-api-key`.
  2. `parse_webhook`: `eventType:"payment.success"` → `Paid`; вытаскивает `contractId`, `amount`, `currency`, `buyer.email`, стабильный `eventKey`.
  3. Резолв: `PaymentIndexDO` `claim-contract:<contractId>` → `claim_id`.
  4. **`ClaimDO./mark-paid`** (только webhook): `status pending→paid`, проставляет `paid_at`, `period_end`, `email`, `amount`, `currency`, `paid_event_key`. Идемпотентно по `eventKey`; tombstone-guard (void не воскрешается).
  5. **Уведомление в бот:** `notify_telegram_paid(claimId)` → telegram-worker `POST /internal/paid`:
     `TgSessionDO.miniapp_claims` по `claimId` → `tg_user_id`; бот шлёт «Оплата прошла успешно! Откройте приложение…» + `web_app`-кнопка (переоткрыть Mini App). `mark_notified` (идемпотентно).
- **Пишет:** `ClaimDO`(paid), `TgSessionDO.miniapp_claims.notified_at`. **Внешние:** Telegram Bot API.

### 6. Возврат в Mini App → «Получить доступ»
- Пользователь открывает Mini App (или тапает кнопку в боте) → `POST /miniapp/me`:
  - `TgSessionDO.miniapp_claims` по `tg_user_id` → claim'ы (новые первее).
  - Для claim: payment-worker `GET /claim/status?claimId=` → `"paid"`.
  - `status == "paid"` (ещё не привязан) → кнопка **«Получить доступ к re:Norma»**;
    `onboardUrl = "<APP_ONBOARD_URL>#claim=<claimId>.<secret>"` (секрет — во **фрагменте**, на сервер не уходит, в логи не пишется).
- Тап → `openLink(onboardUrl)` → Safari → `fit.renorma.app/onboard#claim=…`.
- **Читает:** `TgSessionDO.miniapp_claims`, `ClaimDO.status`. Секрет отдаётся владельцу только при `paid`/`claimed`.

### 7. Онбординг: регистрация + привязка
- `OnboardPage` парсит `#claim=claimId.secret`. Шаг «регистрация»: имя + passkey.
  1. **`auth::register(name)`** → **auth-worker** (WebAuthn): создаёт аккаунт в `AuthDO`/`UserDO`, выдаёт `user_id` + JWT (сохраняются в localStorage).
  2. **`subscription::claim(claimId, secret)`** → payment-worker `POST /claim` (JWT, `user_id`):
     - `secret_hash = sha256(secret)`.
     - **`ClaimDO./claim`** — атомарный CAS (money-safety): `paid → claimed`, `claimed_by = user_id`, `claimed_at`. Возвращает `period_end`, `provider`, `contract_id`, `email`.
     - **`SubscriptionDO(user_id)./activate`** `{periodEnd, provider, contractId, email, activateKey}`:
       `status:"paid"`, `plan:"paid"`, `start:now`, `end:periodEnd`, `no_renew:false`, `contract_id`, `email`, `activate_key` (идемпотентность).
- **Создаёт:** аккаунт (`AuthDO`/`UserDO`), `SubscriptionDO`(активная). **Меняет:** `ClaimDO`→claimed.
  **Внешние:** WebAuthn. **Инвариант:** одна подписка = один аккаунт (CAS отвергает чужой `user_id`).

### 8. Вход в PWA
- `OnboardPage` видит активную подписку → `location = "/"` (приложение).
- Гейт `AppState`: `has_active_sub()` (кэш) / фоновая проверка `subscription::status()` (payment-worker `GET /subscription` → `SubscriptionDO.active = now < end`) → **Ready**.
- Предложение установить PWA на домашний экран.

---

## Итоговая модель данных (что создано)

| Хранилище | Ключ / строка | Создаётся на шаге | Источник значений |
|-----------|---------------|-------------------|-------------------|
| `TgSessionDO.miniapp_claims` | `claim_id` → `{tg_user_id, secret, notified_at}` | 3 (put), 5 (notified) | initData (tg_user_id), payment-worker (claim_id/secret) |
| `ClaimDO.claims` | `claim_id` → `{status, secret_hash, contract_id, tg_user_id, amount…}` | 3 (pending) → 5 (paid) → 7 (claimed) | do_guest_checkout + webhook lava + /claim |
| `PaymentIndexDO` | `claim-contract:<order>` → `claim_id` | 3 | lava `id` (contract) |
| `SubscriptionDO(user_id)` | `sub` → `{status, start, end, contract_id, no_renew…}` | 7 (activate) | ClaimDO period_end/contract/email |
| `AuthDO`/`UserDO` | аккаунт + credential | 7 | WebAuthn |

**Деньги — источник истины lava:** сумма/факт оплаты приходят из webhook; период (`end`)
якорится на `periodEnd` провайдера, не на wall-clock хендлера.

## Точки ветвления (отдельные сценарии)

- Оплатил, но не открыл онбординг / passkey не сработал / claim не прошёл / закрыл PWA →
  **[Прерванный онбординг](onboarding-interrupted.md)**.
- Активная подписка → **[Отмена](cancellation.md)** → **[Возврат](refund.md)**.
- После отмены/истечения → **[Возобновление](renewal.md)**.
