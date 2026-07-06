# Доставка «уведомление получено» из service worker в приложение (iOS PWA)

Как факт получения push-уведомления доводится до Leptos-приложения и завершает
story-задание (`/story/setup`, задача `notif`). Схема выстрадана на живом iOS —
здесь же зафиксированы баги платформы, из-за которых она именно такая.

## Задача

Пуш принимает **service worker** (`frontend/sw.js`) — он работает даже при
закрытом приложении. Галочку задания ставит **страница** (Leptos/WASM). Общей
памяти у них нет — только общие хранилища и `postMessage`. Задание должно
завершаться **по факту получения** уведомления (не по отправке и не по тапу),
и живьём — без перезапуска приложения.

## Кодирование уведомления

Уведомление «выполни задание» несёт в URL параметр
`ntf=<kind>.<section>.<task>.<rand>`, например `?ntf=tc.setup.notif.a4f2`:

- `tc` — вид (task complete);
- `setup` — секция истории;
- `notif` — id задания (маппится на story-флаг через `story::flag_for_task`);
- `<rand>` — 4 hex-символа, чтобы уведомления различались.

URL формирует кнопка «Проверить уведомления» (`frontend/src/pages/settings.rs`),
отправка — `push::send_test` → push-worker → Web Push.

## Баги iOS, определившие конструкцию

1. **Отвал Cache Storage у страницы.** После `pushManager.subscribe()` /
   получения пуша подключение *страницы* к Cache Storage отваливается: все
   чтения возвращают пусто до полного перезапуска приложения (наблюдалось на
   устройстве как `CACHE-LOG SHRANK 39 -> 0` через секунду после отправки
   пуша). Данные целы, у service worker подключение рабочее — сломана только
   видимость со стороны страницы. Поэтому Cache непригоден как живой канал
   SW → страница.
2. **postMessage из push-обработчика не доставляется.** `clients.matchAll` +
   `client.postMessage` из события `push` до открытой страницы на iOS не
   доходит (проверено на устройстве). При этом *ответ* на сообщение,
   инициированное страницей (`event.source.postMessage`), доходит исправно.
3. **WebKit bug 252544:** в notificationclick повторное использование
   существующего window-клиента (navigate/focus/postMessage) оставляет его
   «инертным» — работает только `clients.openWindow(url)`.
4. **`localStorage` недоступен в service worker** — мост в WASM возможен
   только через страницу.

## Рабочая схема — три канала с дублированием

Доставка идемпотентна (повторная установка того же story-флага безвредна),
поэтому каналы дублируют друг друга.

```
push-worker ──Web Push──▶ sw.js (push):
                            код ntf → IndexedDB 'rn-notif'/kv        (1: живой канал)
                            код ntf → Cache 'notif-deeplink'
                                      /__notif_received__             (2: фолбэк на следующий запуск)
                            код ntf → postMessage окнам               (3: на iOS живьём не доходит)

index.html (страница), раз в 1 с (diagTick):
    IndexedDB take-and-delete ────────┐
    Cache-маркер take-and-delete ─────┼──▶ localStorage['rn_notif_received']
    при boot/resume: query_notif ─────┘
      (SW читает Cache СВОИМ рабочим подключением и отвечает postMessage)

lib.rs (WASM), раз в 1 с (install_notif_receipt_poll):
    localStorage['rn_notif_received'] → flag_for_task(task) →
    story::set_flag(флаг) → bump db::version("story") → галочка реактивно
```

Какой канал срабатывает когда:

| Сценарий | Канал |
|---|---|
| Приложение открыто в момент пуша | IndexedDB (~1–3 с) |
| Приложение было закрыто, запуск позже | Cache-маркер на boot (страница подключается к Cache заново — свежее подключение читает нормально) |
| Приложение в фоне, возврат без перезапуска | `query_notif` при pageshow/focus/visibilitychange (Cache-подключение страницы к этому моменту может быть отвалившимся — SW читает за неё) |

## Участники (код)

- `frontend/sw.js` — push-обработчик: разбор `ntf`, запись в три канала
  (`idbPutNotif`, Cache-маркер, `postMessage`); обработчик `message`
  (`query_notif`, `ping`); `notificationclick` → только `openWindow`.
- `frontend/index.html` — `idbTakeNotif`, `bridgeNotifReceived` (Cache-мост),
  `queryNotif`, слушатель `message`; всё складывает в
  `localStorage['rn_notif_received']`.
- `frontend/src/lib.rs` — `install_notif_receipt_poll`: чтение localStorage,
  `story::flag_for_task`, `story::set_flag`.
- `frontend/src/services/story.rs` — таблица `TASK_FLAG` (id задания → флаг),
  `flag_for_task`.
- `frontend/src/pages/settings.rs` — кнопка, генерация `ntf`-кода, отправка.

## Диагностика (Настройки → «Разработка»)

Панель показывает: возраст «пульсов» таймеров (`hb js` — JS-интервал страницы,
`hb wasm` — WASM-поллер; в норме ≤2 с), зеркало SW-лога из Cache и журнал
страницы из localStorage (`rn_pj`) — он пишется мимо Cache и потому переживает
отвал. Кнопки «Обновить лог» и «Скопировать» (живое обновление сбрасывает
выделение текста — копировать только кнопкой). Маркеры в журнале:
`CACHE-LOG SHRANK N -> M` — зафиксирован отвал Cache-подключения страницы;
`SW pong ft-vNN` — какой билд service worker реально активен.

## Чего НЕ делать (проверено, не работает)

- Считать Cache Storage живым каналом SW → страница: отвал по п. 1.
- `postMessage` из события `push`: не доставляется (п. 2).
- Завершать задание по тапу на уведомление: iOS открывает start_url и теряет
  URL уведомления; к тому же семантика задания — «получено», не «тапнуто».
- Завершать задание по факту успешной отправки: пуш мог не дойти.
- `visibilitychange` как единственный триггер консьюма: в standalone-PWA на
  iOS событие ненадёжно — поэтому опрос интервалом (таймеры при этом живы,
  проверено пульсами: `hb js=0s wasm=1s` в момент бага).
