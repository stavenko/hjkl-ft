## Баг 1 — лишние кнопки при загрузке фото

### Где

`frontend/src/pages/progress.rs` (страница «Прогресс», фото поз спереди/сбоку/сзади).

### Воспроизведение (до)

**Шаги воспроизведения:**

1. Открыть приложение.
2. Перейти на страницу «Прогресс».
3. Каждая строка позы показывает ДВЕ кнопки: «Камера» и «Галерея».

**Почему это баг:**

На iOS системный лист выбора у пикера «Галерея» уже предлагает «Снять фото», поэтому отдельная кнопка «Камера» избыточна. То же самое на Android: chooser без атрибута `capture` уже включает камеру.

Выводы из исследования:

- **Android behavior:** "On Android Chrome, `<input type="file" accept="image/*">` WITHOUT the capture attribute triggers the system intent chooser, which lists the Camera app alongside Gallery/Photos/Files. Camera is reachable from that single dialog — same behavior as iOS in effect."
- **iOS behavior:** "On iOS Safari, the same no-capture `<input type="file" accept="image/*">` opens the system action sheet offering "Take Photo or Video / Photo Library / Choose File", so the camera is already reachable without a dedicated capture input."

**Скриншот «до»:**

![before](./img/bug1-before.png)

**Код ДО (два `<input>` + две `<button>` из `progress.rs`):**

```rust
let cam_id = format!("progress-cam-{pose}");
let gal_id = format!("progress-gal-{pose}");
let cam_btn = cam_id.clone();
let gal_btn = gal_id.clone();
let latest = move || photos.get().unwrap_or_default().into_iter().find(|p| p.pose == pose).map(|p| p.image);
view! {
    <div style=CARD>
        // Camera (forces capture) and gallery (plain picker) — two inputs.
        <input type="file" accept="image/*" capture="environment" id=cam_id style="display: none;"
            on:change=move |ev| handle_file(pose, ev) />
        <input type="file" accept="image/*" id=gal_id style="display: none;"
            on:change=move |ev| handle_file(pose, ev) />
        <div style="display: flex; align-items: center; gap: 8px; padding: 12px 16px;">
            <span class="is-size-6 has-text-weight-medium" style="flex: 1;">{move || t(label)}</span>
            <button class="button is-link is-small" on:click=move |_| click_input(&cam_btn)>
                {move || t("progress.take_photo")}
            </button>
            <button class="button is-small" on:click=move |_| click_input(&gal_btn)>
                {move || t("progress.from_gallery")}
            </button>
        </div>
```

### Исправление

**Решение:** Confirmed — сворачиваем до ОДНОЙ кнопки + ОДНОГО инпута (`accept="image/*"`, без `capture`) на ОБЕИХ платформах, без ветвления по платформе. И iOS Safari, и Android Chrome уже показывают камеру + галерею из единственного инпута без `capture`; инпут с `capture="environment"` наоборот ПРИНУДИТЕЛЬНО открывает только камеру и убирает галерею — это ровно та избыточная кнопка «Камера», на которую жаловался пользователь. Удаление capture-инпута и его кнопки убирает дублирование, при этом доступ к камере сохраняется везде. Поскольку обе платформы ведут себя здесь эквивалентно (единый системный лист/chooser предлагает и то и другое), отдельная ветка под Android не нужна. Переиспользуем существующий лейбл `progress.capture` вместо `take_photo`/`from_gallery`, а теперь неиспользуемые ключи `progress.take_photo` / `progress.from_gallery` можно удалить.

**Код ПОСЛЕ (один `<input>` + одна `<button>`, лейбл «Сделать фото»):**

```rust
let photo_id = format!("progress-photo-{pose}");
let photo_btn = photo_id.clone();
let latest = move || photos.get().unwrap_or_default().into_iter().find(|p| p.pose == pose).map(|p| p.image);
view! {
    <div style=CARD>
        // Single picker (no capture attr) — the native chooser already offers camera + gallery on iOS and Android.
        <input type="file" accept="image/*" id=photo_id style="display: none;"
            on:change=move |ev| handle_file(pose, ev) />
        <div style="display: flex; align-items: center; gap: 8px; padding: 12px 16px;">
            <span class="is-size-6 has-text-weight-medium" style="flex: 1;">{move || t(label)}</span>
            <button class="button is-link is-small" on:click=move |_| click_input(&photo_btn)>
                {move || t("progress.capture")}
            </button>
        </div>
```

Ветвление по платформе не требуется: и iOS, и Android показывают камеру + галерею из одного инпута без `capture`.

### Результат (после)

Та же самая последовательность действий (открыть страницу «Прогресс») теперь показывает ОДНУ кнопку «Сделать фото» на каждую позу; нажатие на неё открывает нативный chooser, где есть и камера, и галерея.

**Скриншот «после»:**

![after](./img/bug1-after.png)

### Проверка сборки

SUCCESS. Сборка завершилась с «✅ success» за 14.76s. Только предупреждения (16 pre-existing warnings: unreachable match arms, dead code) — ошибок компилятора нет. Правки Bug 1 в `progress.rs` / `i18n.rs` компилируются чисто.
