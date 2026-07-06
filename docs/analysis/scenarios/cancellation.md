# Отмена подписки

Ветвление от [счастливого пути](onboarding.md): у пользователя активная подписка, он
отменяет её. Отмена = **стоп автопродления**, доступ сохраняется до конца оплаченного
периода (не путать с [возвратом](refund.md), где доступ обрывается сразу).

**Точка входа:** PWA → Настройки → Управление подпиской → «Отменить подписку»
(доступна только пока `status=paid` и `no_renew != true`).

---

## Поток по шагам

### 1. Подтверждение в приложении
- Диалог: «Отменить подписку? Доступ сохранится ещё **N дней**» — именно **количество
  дней** (`days_left` из `SubscriptionDO.end`), не дата.

### 2. `POST /cancel` (payment-worker, JWT `user_id`)
1. Читает `SubscriptionDO(user_id)` → `provider`, `contract_id`, `email`.
2. **lava `DELETE /api/v1/subscriptions?contractId=&email=`** — останавливает рекуррент.
   - Если lava вернула ошибку → **502**, и `no_renew` локально НЕ выставляется (иначе бы
     соврали: контракт остался бы активным и продолжал списывать). Money-safety: не
     сообщаем успех, если провайдер не подтвердил.
3. **`SubscriptionDO./cancel`**: `no_renew = true`, `status = "cancelled"`. `end` **не
   меняется** — доступ до конца периода. `active` по-прежнему `now < end`.
4. **Уведомление в бот** (best-effort): `notify_bot_cancelled(user_id, end)`:
   - `ClaimDO./tg-for-user {userId}` → `tg_user_id` (по `claimed_by` привязанного claim).
   - Если найден → telegram-worker `POST /internal/cancelled {tgUserId, daysLeft}` → бот
     шлёт «Подписка отменена. Доступ к re:Norma сохранится ещё N дней.».

### 3. Отражение статуса
- **PWA**: лейбл «Отменена», «Доступ ещё N дней», появляется кнопка «Запросить возврат»
  (см. [возврат](refund.md)); кнопка «Отменить» скрывается.
- **Mini App**: `/miniapp/me` → по привязанному claim (`claimed_by`) достаёт
  `SubscriptionDO` → `subStatus:"cancelled"` + `daysLeft` → показывает «Подписка отменена ·
  доступ ещё N дней».

### 4. Истечение
- Пока `now < end` — доступ есть (гейт `active`). После `end` подписка становится
  неактивной → гейт `Locked`; путь назад = [возобновление](renewal.md).

---

## Отмена со стороны lava (параллельный путь)

Если рекуррент отменён на стороне lava, приходит webhook `subscription.cancelled`:
`PaymentIndexDO contract:<id>` → `user_id` → `SubscriptionDO./cancel {periodEnd:willExpireAt}`
(идемпотентно; может сдвинуть `end` на дату из lava). Итог тот же: `no_renew`, доступ до `end`.

---

## Модель данных

| Хранилище | Что меняется | Источник |
|-----------|--------------|----------|
| `SubscriptionDO(user_id)` | `status→"cancelled"`, `no_renew→true`; `end` без изменений | — |
| `ClaimDO` | только читается (`tg-for-user`) | — |

**Внешние API:** lava `DELETE /api/v1/subscriptions` (обязательно, иначе 502); Telegram
Bot API (уведомление, best-effort). **Ничего не создаётся** — только правится
`SubscriptionDO` и шлётся сообщение.
