//! Ленивая запись еды: сначала записали, потом распознали.
//!
//! Человек фотографирует и пишет словами, запись ложится в дневник СРАЗУ и
//! нераспознанной. Разбор идёт фоном и требует сети; сети нет — запись просто
//! остаётся нераспознанной и продолжает так выглядеть, пока не распознается.
//!
//! Пока запись не разобрана, в итоги дня она не входит вовсе (`is_countable`): это
//! не еда с неизвестными нутриентами, а обещание разобраться. Показать её нулём
//! калорий значило бы соврать в другую сторону.
//!
//! Работает ТОЛЬКО под флагом `features::LAZY_FOOD`. Старый путь записи еды остаётся
//! рядом нетронутым, и переключение между ними — дело куратора, не человека.

use api_types::{DiaryEntry, DiaryEntryKind, DiaryFoodItem, Food};

use crate::services::{ai, db, food_search, i18n::t, images, local};

/// Записи, которые ждут разбора. Чистая функция: очередь — это не отдельное
/// хранилище, а те самые записи дневника, которые ещё не распознаны.
///
/// Удалённые пропускаются: человек мог стереть запись, пока она стояла в очереди, и
/// распознавать её незачем.
pub fn awaiting_recognition(entries: &[DiaryEntry], now: chrono::DateTime<chrono::Utc>) -> Vec<&DiaryEntry> {
    entries
        .iter()
        .filter(|e| !e.deleted && e.kind == DiaryEntryKind::Pending && may_try(e, now))
        .collect()
}

/// Сколько ждать после исчерпания попыток, прежде чем пробовать снова.
///
/// Сутки, и по той же причине, что у нутриентов (`food_probe::RETRY_AFTER_MS`): за
/// сутки меняется то, что может изменить исход — выходит новая сборка, чинится
/// воркер, кончается сбой у провайдера. Чаще бессмысленно, реже — запись висит
/// нераспознанной дольше, чем нужно.
pub const RETRY_AFTER_MS: i64 = 24 * 60 * 60 * 1000;

/// Можно ли брать запись в разбор сейчас.
///
/// Попытки не вышли — берём. Вышли — берём, если с последней прошли сутки, и это
/// касается ЛЮБОГО технического сбоя, а не только 5xx: 401 через сутки не изменится,
/// если протух ключ, но изменится, если за эти сутки починили воркер. Хоронить
/// запись насовсем из-за одной неудачи неправильно.
///
/// Записи, которые ждут ЧЕЛОВЕКА (переснять, дописать), суточный повтор не трогает:
/// за нас никто не переснимет, и через сутки ответ будет тот же. Их признак —
/// счётчик выведен за предел, а `recognized_at` пуст: см. `after_failure`.
pub fn may_try(entry: &DiaryEntry, now: chrono::DateTime<chrono::Utc>) -> bool {
    if entry.recognition_tries < TRIES_ON_5XX {
        return true;
    }
    if !entry.retry_after_wait {
        return false;
    }
    entry
        .updated_at
        .parse::<chrono::DateTime<chrono::FixedOffset>>()
        .is_ok_and(|at| (now - at.with_timezone(&chrono::Utc)).num_milliseconds() >= RETRY_AFTER_MS)
}

/// Сколько раз пробуем ПРИ 5xx, прежде чем оставить запись в покое.
///
/// Только при 5xx. Сервер прилёг — это пройдёт само, и вторая попытка имеет смысл.
/// Долбиться в 401 смысла нет никакого: ответ не изменится ни на второй раз, ни на
/// двадцатый, а запросы будут гореть.
///
/// Счётчик обнуляется, когда человек правит снимки или описание: это уже другой
/// вопрос к модели, и отвечать на него надо заново.
pub const TRIES_ON_5XX: u32 = 3;

/// Чем кончился неудавшийся разбор.
#[derive(Clone, PartialEq, Debug)]
pub enum Failure {
    /// Человек может поправить сам: нечего разбирать, ни один кадр не прочёлся, еды
    /// не нашлось. Строка уже готова к показу.
    Actionable(String),
    /// Сбой на нашей стороне. Внутри — сырая причина: она уходит в телеметрию, а
    /// человеку показывается фраза с кодом.
    Technical(String),
}

/// Код ответа из технической причины: `HTTP 401: …`, `stream HTTP 503`.
///
/// Живёт в `errors`, а не здесь: по нему же решается, повторять ли ЛЮБОЙ запрос к
/// модели, а не только разбор еды. Две копии этого правила однажды разошлись бы.
pub use crate::services::errors::http_status;

impl Failure {
    /// Стоит ли пробовать ещё раз. ТОЛЬКО 5xx.
    pub fn retryable(&self) -> bool {
        match self {
            Failure::Actionable(_) => false,
            Failure::Technical(cause) => {
                http_status(cause).is_some_and(|s| (500..600).contains(&s))
            }
        }
    }
}

/// Запись после неудачного разбора.
///
/// `retry_allowed` — можно ли пробовать ещё. Нельзя — счётчик выводится за предел, и
/// очередь эту запись больше не возьмёт. Это честнее отдельного флага: «попыток не
/// осталось» и «повторять незачем» для очереди одно и то же.
///
/// Чистая функция — чтобы правило проверялось тестом, а не тремя провалами подряд.
pub fn after_failure(
    entry: &DiaryEntry,
    message: &str,
    retry_allowed: bool,
    technical: bool,
    at: String,
) -> DiaryEntry {
    let tries = entry.recognition_tries.saturating_add(1);
    DiaryEntry {
        recognition_error: Some(message.to_string()),
        recognition_tries: if retry_allowed { tries } else { TRIES_ON_5XX },
        // Технический сбой вернётся через сутки; то, что ждёт человека, — нет.
        retry_after_wait: technical,
        updated_at: at,
        ..entry.clone()
    }
}

/// Завести нераспознанную запись из снимков и описания.
///
/// Картинки уходят в провайдер изображений и адресуются хэшем: одна фотография,
/// попавшая в две записи, хранится один раз, а запись несёт короткие строки, а не
/// мегабайты.
pub async fn create_pending(
    images_base64: &[String],
    description: &str,
    date: &str,
    meal_label: Option<String>,
) -> DiaryEntry {
    let mut hashes = Vec::with_capacity(images_base64.len());
    for img in images_base64 {
        hashes.push(images::put(img).await);
    }
    let entry = DiaryEntry {
        id: local::new_id(),
        date: date.to_string(),
        time: Some(local::time_now()),
        meal_label,
        kind: DiaryEntryKind::Pending,
        description: (!description.trim().is_empty()).then(|| description.trim().to_string()),
        images: hashes,
        created_at: local::now(),
        updated_at: local::now(),
        ..DiaryEntry::direct()
    };
    db::put("diary", &entry).await;
    entry
}

/// Что делать с одной позицией разобранного списка: взять готовую еду из базы или
/// завести новую.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Нашли ту же самую еду — берём её и ничего не заводим.
    Existing(String),
    /// В базе такой нет; заводим новую, а нутриенты подберём отдельным шагом.
    New,
}

/// Решить судьбу позиции БЕЗ обращения к модели, если это возможно.
///
/// Порядок здесь и есть суть: сперва арифметика, потом имя, и только потом модель.
/// Спека (§6.4) велит сравнивать по имени И по КБЖУ и на любое отличие заводить
/// новую копию. Это правило арифметическое, и модели его доверять нельзя: на замере
/// она соглашалась считать творог 110/17/3.0 тем же, что лежащий в базе 96/18/1.2,
/// потому что названия совпадают. Цена ложного согласия — чужое КБЖУ в дневнике,
/// цена ложного отказа — всего лишь вторая копия в базе.
///
/// `None` означает «кодом не решается, спросите модель».
pub fn resolve_locally(
    seen_name: &str,
    seen: &food_search::SeenNutrition,
    candidates: &[Food],
) -> Option<Resolution> {
    let survivors = food_search::survivors(seen, candidates);
    if survivors.is_empty() {
        // Арифметика отвергла всех: по прочитанным числам это НЕ они. Спрашивать
        // модель не о чем — на замере ровно так снимались все четыре ловушки
        // (десерт «Картошка» против картофеля, кока-кола против кефира), и модель
        // на них не звалась вовсе.
        return Some(Resolution::New);
    }
    food_search::decide_without_model(seen, seen_name, &survivors).map(Resolution::Existing)
}

/// Нутриенты, прочитанные для позиции разобранного списка.
pub fn seen_of(item: &ai::MergedItem) -> food_search::SeenNutrition {
    food_search::SeenNutrition {
        kcal: item.kcal_per_100g,
        protein: item.protein_per_100g,
        fat: item.fat_per_100g,
        carbs: item.carbs_per_100g,
    }
}

/// Какую копию вытесняет новая еда с таким названием.
///
/// Вытесняется только ОДНОИМЁННАЯ и только неархивированная: «Творог» вытесняет
/// «Творог», но десерт «Картошка» не вытесняет картофель — это разная еда, и
/// архивировать её было бы прямым вредом. Из нескольких одноимённых берём самую
/// свежую: она и есть та, которой человек пользовался.
///
/// Чистая функция — чтобы правило проверялось тестом, а не базой.
pub fn superseded<'a>(new_name: &str, candidates: &'a [Food]) -> Option<&'a Food> {
    candidates
        .iter()
        .filter(|f| !f.archived && !f.is_recipe && food_search::same_name(new_name, &f.name))
        .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
}

/// Перенести в новую копию то, что заполняется ФОНОМ.
///
/// Признаки (овощ/фрукт, гемовое железо, молочно-жировая глобула и прочие) и свои
/// нутриенты выясняются отдельными запросами к модели. У вытесненной копии они уже
/// выяснены, и та же еда с чуть иной этикеткой не должна выяснять их заново: это
/// лишние деньги и лишнее ожидание. Спецификация (§6.4) говорит прямо: «то, что
/// заполняется фоном, копируем из предыдущей еды».
///
/// КБЖУ не переносим НИКОГДА — ради них новая копия и заводится.
pub fn inherit_background(mut fresh: Food, prev: &Food) -> Food {
    fresh.is_veg_fruit = fresh.is_veg_fruit.or(prev.is_veg_fruit);
    fresh.is_heme = fresh.is_heme.or(prev.is_heme);
    fresh.is_milk_globule = fresh.is_milk_globule.or(prev.is_milk_globule);
    fresh.is_red_meat = fresh.is_red_meat.or(prev.is_red_meat);
    fresh.is_processed_meat = fresh.is_processed_meat.or(prev.is_processed_meat);
    fresh.is_egg = fresh.is_egg.or(prev.is_egg);
    // Свои нутриенты (кальций, железо…) — те, которых у новой ещё нет.
    for (key, value) in &prev.nutrients {
        fresh.nutrients.entry(key.clone()).or_insert(*value);
    }
    // Ключевые слова тоже разовая работа: у одноимённой копии они те же.
    if fresh.keywords.is_empty() {
        fresh.keywords = prev.keywords.clone();
    }
    fresh
}

/// Завести новую еду по разобранной позиции, вытеснив прежнюю одноимённую копию.
///
/// Ключевые слова размечаются ЗДЕСЬ, при заведении, а не при каждом поиске: это
/// разовая работа на продукт, и только благодаря ей потом находится «ракушки» по
/// слову «макароны». Разметка не удалась — заводим без неё: еда всё равно найдётся
/// по названию, а слова допишет следующая попытка.
///
/// Прежняя одноимённая копия АРХИВИРУЕТСЯ (§6.4): в поиске должна оставаться одна,
/// иначе у человека копятся «Творог», «Творог», «Творог». Из дневника она никуда не
/// девается — прошлые записи продолжают считаться по тем цифрам, что были тогда.
pub async fn create_food(item: &ai::MergedItem, candidates: &[Food]) -> Food {
    let prev = superseded(&item.name, candidates).cloned();
    let keywords = ai::keywords_for(&item.name, |_| {}).await.unwrap_or_default();
    let mut food = Food {
        id: local::new_id(),
        name: item.name.clone(),
        kcal: item.kcal_per_100g.unwrap_or(0.0),
        protein: item.protein_per_100g.unwrap_or(0.0),
        fat: item.fat_per_100g.unwrap_or(0.0),
        carbs: item.carbs_per_100g.unwrap_or(0.0),
        keywords,
        created_at: local::now(),
        updated_at: local::now(),
        ..Food::default()
    };
    if let Some(prev) = &prev {
        food = inherit_background(food, prev);
    }
    db::put("foods", &food).await;
    if let Some(prev) = prev {
        local::archive_food(&prev.id, true).await;
    }
    food
}

/// Сопоставить одну позицию с базой человека: отбор кандидатов, арифметика, при
/// необходимости — модель. Возвращает `id` еды, готовой лечь в запись.
pub async fn resolve_item(item: &ai::MergedItem, index: &food_search::Index, foods: &[Food]) -> String {
    let seen = seen_of(item);
    let ids = index.candidates(&item.name, &[]);
    let candidates: Vec<Food> = foods.iter().filter(|f| ids.contains(&f.id)).cloned().collect();

    match resolve_locally(&item.name, &seen, &candidates) {
        Some(Resolution::Existing(id)) => return id,
        Some(Resolution::New) => return create_food(item, &candidates).await.id,
        None => {}
    }
    let survivors = food_search::survivors(&seen, &candidates);
    match ai::pick_same_food(&item.name, &seen, &survivors, |_| {}).await {
        Ok(Some(id)) => id,
        // И отказ модели, и её сбой ведут в одно место: заводим новую копию. Это
        // хуже, чем найти существующую, но несравнимо лучше, чем приписать еде
        // чужие нутриенты.
        _ => create_food(item, &candidates).await.id,
    }
}

/// Разобрать одну нераспознанную запись и превратить её в агрегатор.
///
/// Кадры разбираются ПО ОДНОМУ — в этом весь смысл первого прохода, и сбой на одном
/// кадре не отменяет остальные: у человека может быть три снимка, из которых один
/// смазан. Ни одного разобранного кадра и пустое описание — разбирать нечего, и
/// запись остаётся нераспознанной до следующей попытки.
pub async fn recognize(entry: &DiaryEntry) -> Result<DiaryEntry, Failure> {
    let mut frames = Vec::new();
    let mut unread = 0usize;
    for hash in &entry.images {
        let Some(image) = images::get(hash).await else { continue };
        match ai::read_photo(&image, |_, _, _| {}).await {
            Ok(read) => frames.push((hash.clone(), read)),
            Err(e) => {
                // Не только в консоль: непрочитанный кадр это то, о чём человек
                // должен узнать (§6.6). Считаем их и скажем словами ниже.
                leptos::logging::warn!("кадр {hash} не разобран: {e}");
                unread += 1;
            }
        }
    }
    let description = entry.description.clone().unwrap_or_default();
    if frames.is_empty() && description.trim().is_empty() {
        return Err(Failure::Actionable(if unread > 0 {
            // Снимки были, но ни один не прочёлся, и слов человек не написал.
            // Подставить справочные значения нельзя — их не из чего брать.
            t("lazy_food.err.no_frames_read").to_string()
        } else {
            t("lazy_food.err.nothing_to_read").to_string()
        }));
    }

    // Сбой модели или сети НЕ уходит человеку как есть: в записи должно оказаться
    // объяснение словами, а не «LLM output error: ModelExecution("HTTP 401: …")».
    // Подробность нужна нам и живёт в консоли; человеку нужно, что делать дальше.
    let merged = match ai::merge_into_items(&frames, &description, |_| {}).await {
        Ok(m) => m,
        // Сырую причину НЕ показываем — она уходит в телеметрию, а человеку
        // достаётся фраза с кодом. Решение, повторять ли, принимается по ней же.
        Err(e) => return Err(Failure::Technical(e)),
    };
    if merged.items.is_empty() {
        return Err(Failure::Actionable(t("lazy_food.err.empty_list").to_string()));
    }

    // Сопоставляем ТОЛЬКО с неархивированными копиями (§6.4): архивированную не
    // воскрешаем — если совпадение нашлось бы с ней, заводим новую. Рецепты тоже
    // мимо: рецепт это не сырой продукт из справочника.
    let foods: Vec<Food> = local::list_foods()
        .await
        .into_iter()
        .filter(|f| !f.archived && !f.is_recipe)
        .collect();
    let index = food_search::Index::build(&foods);
    let mut items = Vec::with_capacity(merged.items.len());
    for it in &merged.items {
        items.push(DiaryFoodItem { food_id: resolve_item(it, &index, &foods).await, grams: it.grams });
    }

    let done = DiaryEntry {
        kind: DiaryEntryKind::Aggregate,
        items,
        label: Some(short_label(&merged.items)),
        recognized_at: Some(local::now()),
        // Разобралось — прежняя неудача больше не про эту запись.
        recognition_error: None,
        recognition_tries: 0,
        retry_after_wait: false,
        updated_at: local::now(),
        ..entry.clone()
    };
    db::put("diary", &done).await;
    Ok(done)
}

/// Короткий лейбл, которым распознанная запись показывается вместо фотографий.
///
/// Собирается из названий позиций, а не сочиняется отдельным запросом к модели:
/// лишний запрос стоит денег и времени, а человеку нужно узнать свою запись, а не
/// прочесть про неё сочинение.
fn short_label(items: &[ai::MergedItem]) -> String {
    let names: Vec<&str> = items.iter().map(|i| i.name.trim()).filter(|n| !n.is_empty()).collect();
    match names.len() {
        0 => "Еда".to_string(),
        1 => names[0].to_string(),
        2 => format!("{} и {}", names[0], names[1]),
        n => format!("{}, {} и ещё {}", names[0], names[1], n - 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(id: &str) -> DiaryEntry {
        DiaryEntry { id: id.into(), kind: DiaryEntryKind::Pending, ..DiaryEntry::direct() }
    }

    fn food(id: &str, name: &str, kcal: f64, p: f64, f: f64, c: f64) -> Food {
        Food { id: id.into(), name: name.into(), kcal, protein: p, fat: f, carbs: c, ..Food::default() }
    }

    fn item(name: &str, kcal: Option<f64>, p: Option<f64>, f: Option<f64>, c: Option<f64>) -> ai::MergedItem {
        ai::MergedItem {
            name: name.into(),
            from_frames: None,
            where_grams_came_from: "package_netto".into(),
            grams: 100.0,
            kcal_per_100g: kcal,
            protein_per_100g: p,
            fat_per_100g: f,
            carbs_per_100g: c,
        }
    }

    #[test]
    fn queue_is_the_undigested_entries_and_nothing_else() {
        let entries = vec![
            pending("a"),
            DiaryEntry { id: "b".into(), ..DiaryEntry::direct() },
            DiaryEntry { id: "c".into(), kind: DiaryEntryKind::Aggregate, ..DiaryEntry::direct() },
            DiaryEntry { id: "d".into(), deleted: true, ..pending("d") },
        ];
        let ids: Vec<&str> = awaiting_recognition(&entries, now()).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["a"], "стёртая запись в очередь не возвращается");
    }

    #[test]
    fn arithmetic_alone_rules_out_every_candidate() {
        // Ловушка из замера: десерт «Картошка» против картофеля отварного. Числа
        // расходятся, кандидатов не остаётся, и модель не зовётся вовсе.
        let potato = food("f28", "Картофель отварной", 82.0, 2.0, 0.4, 16.7);
        let seen = food_search::SeenNutrition {
            kcal: Some(377.0), protein: Some(5.9), fat: Some(16.5), carbs: Some(51.4),
        };
        assert_eq!(
            resolve_locally("Десерт «Картошка»", &seen, &[potato]),
            Some(Resolution::New)
        );
    }

    #[test]
    fn full_copy_by_numbers_is_taken_without_the_model() {
        let pack = food("f08", "Творог обезжиренный ВкусВилл", 96.0, 18.0, 1.2, 3.3);
        let seen = food_search::SeenNutrition {
            kcal: Some(96.0), protein: Some(18.0), fat: Some(1.2), carbs: Some(3.3),
        };
        assert_eq!(
            resolve_locally("Творог «Пластовой» обезжиренный", &seen, &[pack]),
            Some(Resolution::Existing("f08".into()))
        );
    }

    #[test]
    fn different_fat_is_a_new_copy_not_the_same_food() {
        // То, на чём модель ошибалась: имена совпадают, жирность нет.
        let lean = food("f08", "Творог обезжиренный", 96.0, 18.0, 1.2, 3.3);
        let seen = food_search::SeenNutrition {
            kcal: Some(110.0), protein: Some(17.0), fat: Some(3.0), carbs: Some(3.3),
        };
        assert_eq!(resolve_locally("Творог обезжиренный", &seen, &[lean]), Some(Resolution::New));
    }

    #[test]
    fn without_numbers_and_with_two_candidates_the_model_decides() {
        let a = food("f01", "Макароны", 337.0, 10.4, 1.1, 71.5);
        let b = food("f02", "Спагетти Barilla №5", 359.0, 12.0, 1.5, 71.2);
        let seen = food_search::SeenNutrition::default();
        assert_eq!(resolve_locally("Ракушки", &seen, &[a, b]), None);
    }

    #[test]
    fn seen_carries_only_what_was_read() {
        let partial = seen_of(&item("Творог", Some(96.0), Some(18.0), None, None));
        assert_eq!(partial.kcal, Some(96.0));
        assert_eq!(partial.fat, None);
        assert!(!partial.all_four_read());
    }

    #[test]
    fn label_names_the_food_and_counts_the_rest() {
        assert_eq!(short_label(&[]), "Еда");
        assert_eq!(short_label(&[item("Творог", None, None, None, None)]), "Творог");
        assert_eq!(
            short_label(&[item("Творог", None, None, None, None), item("Мёд", None, None, None, None)]),
            "Творог и Мёд"
        );
        assert_eq!(
            short_label(&[
                item("Печень", None, None, None, None),
                item("Капуста", None, None, None, None),
                item("Хлеб", None, None, None, None),
            ]),
            "Печень, Капуста и ещё 1"
        );
    }

    // ── §6.4: вытеснение прежней копии ──

    /// Продукт для проверок вытеснения: важны только имя и время правки.
    fn copy_of_food(id: &str, name: &str, updated: &str) -> Food {
        Food {
            id: id.into(),
            name: name.into(),
            updated_at: updated.into(),
            ..Food::default()
        }
    }

    #[test]
    fn vytesnyaetsya_odnoimyonnaya_i_samaya_svezhaya() {
        let cands = vec![
            copy_of_food("f1", "Творог обезжиренный", "2026-01-01T00:00:00Z"),
            copy_of_food("f2", "Творог обезжиренный", "2026-06-01T00:00:00Z"),
        ];
        assert_eq!(superseded("Творог обезжиренный", &cands).map(|f| f.id.as_str()), Some("f2"));
    }

    #[test]
    fn raznaya_eda_ne_vytesnyaetsya() {
        // Десерт «Картошка» не должен архивировать картофель: это не копии одного
        // продукта, а разная еда, и архивировать её было бы прямым вредом.
        let cands = vec![copy_of_food("f1", "Картофель отварной", "2026-01-01T00:00:00Z")];
        assert!(superseded("Десерт «Картошка»", &cands).is_none());
    }

    #[test]
    fn arhivirovannuyu_kopiyu_ne_voskreshaem() {
        let mut old = copy_of_food("f1", "Творог", "2026-01-01T00:00:00Z");
        old.archived = true;
        assert!(superseded("Творог", &[old]).is_none(), "архивная не вытесняется повторно");
    }

    #[test]
    fn recept_ne_vytesnyaetsya() {
        let mut r = copy_of_food("r1", "Творог", "2026-01-01T00:00:00Z");
        r.is_recipe = true;
        assert!(superseded("Творог", &[r]).is_none(), "рецепт это не копия продукта");
    }

    #[test]
    fn fonovye_priznaki_perehodyat_v_novuyu_kopiyu() {
        let mut prev = copy_of_food("f1", "Творог", "2026-01-01T00:00:00Z");
        prev.is_milk_globule = Some(true);
        prev.is_veg_fruit = Some(false);
        prev.keywords = vec!["творог".into(), "молочное".into()];
        prev.nutrients.insert("Calcium".into(), 120.0);

        let fresh = Food { id: "f2".into(), name: "Творог".into(), kcal: 150.0, ..Food::default() };
        let out = inherit_background(fresh, &prev);

        assert_eq!(out.is_milk_globule, Some(true), "выясненное фоном не выясняем заново");
        assert_eq!(out.is_veg_fruit, Some(false));
        assert_eq!(out.nutrients.get("Calcium"), Some(&120.0));
        assert_eq!(out.keywords, prev.keywords, "слова для поиска у одноимённой те же");
        assert_eq!(out.kcal, 150.0, "КБЖУ НЕ наследуются — ради них копия и заводится");
    }

    // ── §6.6: неудача не прячется ──

    /// «Сейчас» для проверок очереди. Записи в них свежие, и сутки ещё не прошли.
    fn now() -> chrono::DateTime<chrono::Utc> {
        "2026-09-05T10:00:00Z".parse().unwrap()
    }

    /// Запись, неудача которой случилась `hours` часов назад.
    fn failed_hours_ago(hours: i64, technical: bool) -> DiaryEntry {
        let at = now() - chrono::Duration::hours(hours);
        let e = DiaryEntry { id: "e1".into(), kind: DiaryEntryKind::Pending, ..DiaryEntry::direct() };
        after_failure(&e, "сбой", false, technical, at.to_rfc3339())
    }

    #[test]
    fn tehnicheskij_sboj_vozvrashchaetsya_cherez_sutki() {
        // Через двадцать три часа ещё рано — сутки не прошли.
        assert!(awaiting_recognition(&[failed_hours_ago(23, true)], now()).is_empty());
        // Через двадцать пять — пробуем снова: могла выйти новая сборка или
        // починиться воркер.
        assert_eq!(awaiting_recognition(&[failed_hours_ago(25, true)], now()).len(), 1);
    }

    #[test]
    fn ozhidanie_cheloveka_sutkami_ne_lechitsya() {
        // За нас никто не переснимет: и через сутки, и через месяц ответ тот же.
        assert!(awaiting_recognition(&[failed_hours_ago(25, false)], now()).is_empty());
        assert!(awaiting_recognition(&[failed_hours_ago(24 * 30, false)], now()).is_empty());
    }

    #[test]
    fn kod_otveta_vychityvaetsya_iz_prichiny() {
        assert_eq!(http_status("HTTP 401: {\"error\":\"Unauthorized\"}"), Some(401));
        assert_eq!(http_status("stream HTTP 503"), Some(503));
        assert_eq!(http_status("LLM output error: ModelExecution(\"HTTP 500: …\")"), Some(500));
        assert_eq!(http_status("сеть отвалилась"), None, "кода нет — и выдумывать нечего");
    }

    #[test]
    fn povtoryaem_tolko_pyatisotye() {
        // Сервер прилёг — пройдёт само, вторая попытка имеет смысл.
        assert!(Failure::Technical("HTTP 500: сервер".into()).retryable());
        assert!(Failure::Technical("stream HTTP 503".into()).retryable());
        // А это не изменится ни на второй раз, ни на двадцатый.
        assert!(!Failure::Technical("HTTP 401: Unauthorized".into()).retryable());
        assert!(!Failure::Technical("HTTP 403: Forbidden".into()).retryable());
        assert!(!Failure::Technical("HTTP 429: Too Many Requests".into()).retryable());
        // Без кода повторять тоже незачем: мы не знаем, что чинить.
        assert!(!Failure::Technical("ответ не разобрался".into()).retryable());
        // То, что человек правит сам, повтором не лечится вовсе.
        assert!(!Failure::Actionable("опишите словами".into()).retryable());
    }

    #[test]
    fn neudacha_lozhitsya_v_zapis() {
        let e = DiaryEntry { id: "e1".into(), kind: DiaryEntryKind::Pending, ..DiaryEntry::direct() };
        let once = after_failure(&e, "не прочёлся ни один снимок", true, true, now().to_rfc3339());
        assert_eq!(once.recognition_error.as_deref(), Some("не прочёлся ни один снимок"));
        assert_eq!(once.recognition_tries, 1);
    }

    #[test]
    fn bez_prava_na_povtor_zapis_srazu_vypadaet_iz_ocheredi() {
        // 401 не должен получить ни второй попытки, ни третьей: счётчик сразу
        // выводится за предел.
        let e = DiaryEntry { id: "e1".into(), kind: DiaryEntryKind::Pending, ..DiaryEntry::direct() };
        let out = after_failure(&e, "технический сбой", false, true, now().to_rfc3339());
        assert_eq!(out.recognition_tries, TRIES_ON_5XX);
        assert!(
            awaiting_recognition(&[out], now()).is_empty(),
            "сейчас очередь её не берёт — вернётся только через сутки"
        );
    }

    #[test]
    fn pyatisotaya_poluchaet_svoi_tri_popytki() {
        let mut e = DiaryEntry { id: "e1".into(), kind: DiaryEntryKind::Pending, ..DiaryEntry::direct() };
        for expected in 1..TRIES_ON_5XX {
            e = after_failure(&e, "сервис недоступен", true, true, now().to_rfc3339());
            assert_eq!(e.recognition_tries, expected);
            assert_eq!(awaiting_recognition(&[e.clone()], now()).len(), 1, "попытки ещё есть");
        }
        // Последняя: права на повтор уже нет, и запись выпадает.
        let last = after_failure(&e, "сервис недоступен", false, true, now().to_rfc3339());
        assert!(awaiting_recognition(&[last], now()).is_empty());
    }
}

// ── фоновая очередь ──────────────────────────────────────────────────────────
//
// Запись ложится в дневник сразу, а разбирается когда получится. Сети нет — запись
// остаётся нераспознанной и продолжает так выглядеть; это состояние законное, а не
// сбой, и никакой ошибки человеку показывать не нужно.

use std::cell::Cell;

thread_local! {
    /// Идёт ли разбор прямо сейчас. Без этого возврат в приложение, приход сети и
    /// новая запись запустили бы три прохода разом по одной и той же очереди, и
    /// одна запись разобралась бы трижды, заведя три копии каждой еды.
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

/// Разобрать всё, что стоит в очереди. Зовётся при запуске, при появлении сети и
/// после того, как человек записал новую еду.
///
/// Флаг возможности проверяется ЗДЕСЬ, а не у каждого вызывающего: выключили
/// возможность — фон замолкает сам, где бы его ни позвали.
pub async fn run_queue() {
    if !crate::services::features::is_on(crate::services::features::LAZY_FOOD) {
        return;
    }
    if !crate::services::net::online_now() {
        return;
    }
    if RUNNING.with(|r| r.replace(true)) {
        return;
    }
    let entries: Vec<DiaryEntry> = db::list_all::<DiaryEntry>("diary").await;
    let queue: Vec<DiaryEntry> =
        awaiting_recognition(&entries, chrono::Utc::now()).into_iter().cloned().collect();
    for entry in queue {
        // Сеть могла пропасть посреди очереди — тогда останавливаемся и оставляем
        // остальное на следующий раз, а не молотим впустую по одной ошибке.
        if !crate::services::net::online_now() {
            break;
        }
        match recognize(&entry).await {
            Ok(_) => crate::services::sync::push_background(),
            Err(failure) => {
                // Повторяем ТОЛЬКО 5xx и только пока не вышли попытки: 401 не
                // изменится ни на второй раз, ни на двадцатый.
                let retry = failure.retryable() && entry.recognition_tries + 1 < TRIES_ON_5XX;
                let message = match &failure {
                    Failure::Actionable(m) => {
                        leptos::logging::warn!("запись {} не разобрана: {m}", entry.id);
                        m.clone()
                    }
                    Failure::Technical(cause) => {
                        // Причина целиком уходит в телеметрию и в консоль; человеку
                        // достаётся фраза с кодом.
                        leptos::logging::warn!("запись {} — технический сбой: {cause}", entry.id);
                        crate::services::errors::recognition_failed(cause)
                    }
                };
                let technical = matches!(failure, Failure::Technical(_));
                let marked = after_failure(&entry, &message, retry, technical, local::now());
                db::put("diary", &marked).await;
                crate::services::sync::push_background();
            }
        }
    }
    RUNNING.with(|r| r.set(false));
}

/// Запустить разбор, не дожидаясь его окончания.
pub fn run_queue_background() {
    leptos::spawn_local(async {
        run_queue().await;
    });

}
