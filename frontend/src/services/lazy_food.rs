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

use crate::services::{ai, db, food_search, images, local};

/// Записи, которые ждут разбора. Чистая функция: очередь — это не отдельное
/// хранилище, а те самые записи дневника, которые ещё не распознаны.
///
/// Удалённые пропускаются: человек мог стереть запись, пока она стояла в очереди, и
/// распознавать её незачем.
pub fn awaiting_recognition(entries: &[DiaryEntry]) -> Vec<&DiaryEntry> {
    entries
        .iter()
        .filter(|e| !e.deleted && e.kind == DiaryEntryKind::Pending)
        .collect()
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

/// Завести новую еду по разобранной позиции.
///
/// Ключевые слова размечаются ЗДЕСЬ, при заведении, а не при каждом поиске: это
/// разовая работа на продукт, и только благодаря ей потом находится «ракушки» по
/// слову «макароны». Разметка не удалась — заводим без неё: еда всё равно найдётся
/// по названию, а слова допишет следующая попытка.
pub async fn create_food(item: &ai::MergedItem) -> Food {
    let keywords = ai::keywords_for(&item.name, |_| {}).await.unwrap_or_default();
    let food = Food {
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
    db::put("foods", &food).await;
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
        Some(Resolution::New) => return create_food(item).await.id,
        None => {}
    }
    let survivors = food_search::survivors(&seen, &candidates);
    match ai::pick_same_food(&item.name, &seen, &survivors, |_| {}).await {
        Ok(Some(id)) => id,
        // И отказ модели, и её сбой ведут в одно место: заводим новую копию. Это
        // хуже, чем найти существующую, но несравнимо лучше, чем приписать еде
        // чужие нутриенты.
        _ => create_food(item).await.id,
    }
}

/// Разобрать одну нераспознанную запись и превратить её в агрегатор.
///
/// Кадры разбираются ПО ОДНОМУ — в этом весь смысл первого прохода, и сбой на одном
/// кадре не отменяет остальные: у человека может быть три снимка, из которых один
/// смазан. Ни одного разобранного кадра и пустое описание — разбирать нечего, и
/// запись остаётся нераспознанной до следующей попытки.
pub async fn recognize(entry: &DiaryEntry) -> Result<DiaryEntry, String> {
    let mut frames = Vec::new();
    for hash in &entry.images {
        let Some(image) = images::get(hash).await else { continue };
        match ai::read_photo(&image, |_, _, _| {}).await {
            Ok(read) => frames.push((hash.clone(), read)),
            Err(e) => leptos::logging::warn!("кадр {hash} не разобран: {e}"),
        }
    }
    let description = entry.description.clone().unwrap_or_default();
    if frames.is_empty() && description.trim().is_empty() {
        return Err("нечего разбирать: ни одного кадра и нет описания".to_string());
    }

    let merged = ai::merge_into_items(&frames, &description, |_| {}).await?;
    if merged.items.is_empty() {
        return Err("список еды вышел пустым".to_string());
    }

    let foods = local::list_foods().await;
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
        let ids: Vec<&str> = awaiting_recognition(&entries).iter().map(|e| e.id.as_str()).collect();
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
    let queue: Vec<DiaryEntry> = awaiting_recognition(&entries).into_iter().cloned().collect();
    for entry in queue {
        // Сеть могла пропасть посреди очереди — тогда останавливаемся и оставляем
        // остальное на следующий раз, а не молотим впустую по одной ошибке.
        if !crate::services::net::online_now() {
            break;
        }
        match recognize(&entry).await {
            Ok(_) => crate::services::sync::push_background(),
            Err(e) => leptos::logging::warn!("запись {} не разобрана: {e}", entry.id),
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
