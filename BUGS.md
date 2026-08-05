# Bugs

## Open

### `iron-mg-mirrors-absorption` — железо: миллиграммы повторяют коэффициент усвоения
- **Где:** `lookup_iron` (`frontend/src/services/ai.rs`), поля `*_iron` структуры `IronDetail`.
- **Симптом:** содержание железа приходит равным доле усвоения того же продукта. Замер на dev 2026-08-05, шесть продуктов подряд:

  | продукт | железо, мг | усвоение | должно быть, мг |
  |---|---|---|---|
  | Творог | 0.02 | 0.02 | ~0.4 |
  | Йогурт | 0.02 | 0.02 | ~0.1 |
  | Яблоко | 0.1 | 0.15 | ~0.1 |
  | Куриная печень | 0.25 | 0.2 | ~9 |
  | Говядина | 0.2 | 0.15 | ~2.6 |
  | Чечевица | 0.05 | 0.05 | ~7.5 |

  Все значения лежат в 0…0.25 — ровно диапазон таблицы усвоения.
- **Причина (гипотеза, не проверена):** таблица категорий в промпте — единственные числа, которые видит модель, и она подставляет их в оба блока полей. Проверка `min ≤ recommended ≤ max` такие ответы пропускает: внутри блока они согласованы.
- **Что НЕ сломано:** категория выбирается верно, включая `liver_offal` для куриной печени.
- **Последствие:** недельный gauge железа и недельный индикатор железа считаются по величинам, заниженным в десятки раз. Живёт на проде.

### `sse-answer-truncated` — ответ модели обрывается на середине
- **Где:** тракт ai-worker → arti-pipes → `generate_validated`.
- **Симптом:** сетевой ответ приходит оборванным посреди JSON. Замер на dev 2026-08-05: `{"min_value":100,"max_value":200,"recommended":15` (49 символов), `…"comment":"Г` (64 символа), `…"max_value_iron_absorption":` (97 символов). Разбор падает, попытка тратится впустую.
- **Не устранено** снятием `tee` в ai-worker: тот фикс убрал потерю числовых токенов, обрыв остался.
- **Последствие:** лишние запросы к модели; при исчерпании попыток продукт остаётся неразобранным.

### `model-runaway-to-max-tokens` — генерация уходит в мусор до потолка
- **Где:** тот же тракт.
- **Симптом:** вместо ответа приходит 6774 символа `!!!!!!!!…` (замер на dev 2026-08-05, запрос омега-3 для «Гречки»). Ранее наблюдалось 8000 токенов на ответ длиной в 20.
- **Последствие:** оплаченные токены за мусор.

### `nutrients-two-storage-shapes` — одно поле хранится в двух формах
- **Где:** `db::put_untracked` против `db::put_json_untracked` (`frontend/src/services/db.rs`).
- **Симптом:** `Food.nutrients` (`BTreeMap`) при локальной записи ложится в IndexedDB как JS `Map` (дефолтный `serde_wasm_bindgen`), а строка, приехавшая синком, — как обычный объект (`Serializer::json_compatible`). Обе формы читаются, но в одном сторе поле лежит в двух видах.
- **Последствие:** любой внешний читатель (тест, отладка из консоли) получает `Object.keys(...) === 0` на половине строк. На этом я уже ошибся: заключил, что нутриенты не пишутся вовсе.

## Fixed

### `schema-echoed-as-answer` — модель отвечала схемой вместо значения — FIXED 2026-08-05
- **Где:** `SingleNutrient` (`frontend/src/services/ai.rs`).
- **Симптом:** вместо значения приходил документ JSON Schema на 1099 символов, начинавшийся с `{"description":"A focused, FLAT single-nutrient answer. Deliberately not the nested \`Nutr…` — то есть с доккоммента к нашей же структуре.
- **Причина:** `schemars` превращает `///` в `description` схемы, а ai-worker (`lib.rs:381`) вклеивает схему в системное сообщение текстом со словами «The JSON MUST conform to this exact schema». Доккоммент был рассказом про наш код, и модель зачитывала его как задание. `response_format` при этом до Workers AI не доезжает — в `run_params` его нет, никакого structured output не существует, схема работает только как уговор в промпте.
- **Фикс:** развёрнутые пояснения переведены из `///` в `//` у `SingleNutrient`, `FoodVerdicts`, `MatchResult`. Заодно снят `unwrap_schema_envelope` — костыль, вычленявший ответ из схемы-эха.
- **Проверено:** прогон `check-food-sweep` на dev, 10 запросов, эха нет ни в одном ответе.

### Bottom nav visible on the PWA install onboarding screen — FIXED 2026-06-21
- **Where:** `AppState::PwaPrompt` overlay (`frontend/src/app.rs`).
- **Symptom:** the fixed bottom nav (z-index:40) showed through at the bottom of the "Food Tracker — install the app" onboarding screen, because `PwaPrompt` (a normal-flow `min-height:100vh` div) created no stacking context above it — unlike the `Auth`/`PushOnboarding` overlays.
- **Fix:** wrapped the `PwaPrompt` overlay in `position:fixed; inset:0; z-index:100; background:var(--bulma-scheme-main); overflow-y:auto`, matching the other overlays.
- **Verified (Playwright):** with the prompt showing, `document.elementFromPoint` at the nav's center returns the overlay `DIV`, not the `A[data-testid=nav-story]` link, and the nav is not visible in a screenshot.
