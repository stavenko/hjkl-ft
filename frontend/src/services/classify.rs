//! Background per-food category classifier.
//!
//! As soon as a food is logged into the diary, its id is enqueued here; a single
//! app-scoped worker drains the queue ONE food at a time, спрашивая КАЖДЫЙ признак
//! отдельным запросом, so foods are classified in order without a parallel
//! burst. Tags (`is_liquid_cal` / `is_veg_fruit` / `is_heme`) are cached on the
//! `Food` and synced. Idempotent: признак, который уже проставлен, не переспрашивается, so
//! re-logging a known food costs nothing.
//!
//! This replaces the old "tag snacks at daily-summary time" flow: classification
//! is now immediate and per-food, not deferred to the (now removed) daily report.

use std::cell::RefCell;
use std::collections::VecDeque;

use api_types::Food;
use leptos::spawn_local;

use super::{ai, db, errors, local};

/// Online per the global connectivity probe (AI worker reachable) — the real
/// signal for background classification, which itself calls the AI worker.
fn is_online() -> bool {
    super::net::online_now()
}

/// Block until the browser reports connectivity again (polls; capped ~30 min).
async fn wait_for_online() {
    for _ in 0..600 {
        if is_online() {
            return;
        }
        ai::sleep_ms(3000).await;
    }
}

/// Run one background AI op with up to 3 attempts. On failure:
/// - if the device is OFFLINE → wait for connectivity, then try 3 more times
///   (a lost connection must never burn a food's error slot);
/// - if ONLINE but still failing (bad JSON / model error / transient) → record the
///   error and give up (the next activation sweep retries it anyway).
/// Returns `Some(value)` on success, `None` if given up.
async fn with_retries<F, Fut, T>(mut op: F, aspect: errors::FoodAspect, food: &str) -> Option<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    loop {
        let mut last = String::from("unknown error");
        for attempt in 0..3 {
            match op().await {
                Ok(v) => return Some(v),
                Err(e) => {
                    last = e;
                    if attempt < 2 {
                        ai::sleep_ms(1500).await;
                    }
                }
            }
        }
        if !is_online() {
            wait_for_online().await;
            continue; // connection is a temporary problem — retry the whole batch
        }
        errors::record_food(aspect, food, &last);
        return None;
    }
}

struct Queue {
    pending: VecDeque<String>,
    running: bool,
}

thread_local! {
    static Q: RefCell<Queue> = const { RefCell::new(Queue { pending: VecDeque::new(), running: false }) };
}

/// Reset the queue. Called once from `main()` for symmetry with other services
/// (the thread_local is already initialised lazily).
pub fn init() {
    Q.with(|q| {
        let mut q = q.borrow_mut();
        q.pending.clear();
        q.running = false;
    });
}

/// Признаки, которые СПРАШИВАЮТСЯ у модели, и как проверить, что признака ещё нет.
///
/// Общего «нужна классификация» здесь нет намеренно: единого промпта больше нет, и
/// один пустой признак не имеет права тянуть перезапрос остальных. У каждого свой
/// гейт, свой запрос и своя запись.
///
/// Спрашивается ровно то, что кто-то ЧИТАЕТ: фрукты/овощи — дневная планка в
/// виджете, гем — недельные порции, красное и переработанное мясо — свою неделю.
/// Перекус, яйца и жидкие калории не спрашиваются: их потребители ушли вместе с
/// удалённой «Оценкой».
///
/// Мясные признаки собираются С ПЕРВОГО ДНЯ, не дожидаясь своей недели, — по той же
/// причине, что жиры и железо: иначе в день открытия вся съеденная за месяцы еда
/// разом уходит к модели, а человек смотрит на пустую шкалу.
const FLAGS: &[(
    &str,
    fn(&Food) -> bool,
    local::FoodFlag,
)] = &[
    ("фрукты/овощи", |f| f.is_veg_fruit.is_none(), local::FoodFlag::VegFruit),
    ("гем", |f| f.is_heme.is_none(), local::FoodFlag::Heme),
    ("молочная глобула", |f| f.is_milk_globule.is_none(), local::FoodFlag::MilkGlobule),
    ("красное мясо", |f| f.is_red_meat.is_none(), local::FoodFlag::RedMeat),
    ("переработанное мясо", |f| f.is_processed_meat.is_none(), local::FoodFlag::ProcessedMeat),
    ("яйцо", |f| f.is_egg.is_none(), local::FoodFlag::Egg),
];

/// True if `food` still misses at least one of the asked flags — то есть работа для
/// прохода вообще есть. Что именно спрашивать, решает [`FLAGS`], каждый сам за себя.
fn needs_classification(food: &Food) -> bool {
    FLAGS.iter().any(|(_, missing, _)| missing(food))
}

/// Enqueue a food id for background classification (no-op if already queued) and
/// start the worker if idle. Safe to call from anywhere (no UI context needed).
pub fn enqueue(food_id: String) {
    let start = Q.with(|q| {
        let mut q = q.borrow_mut();
        if q.pending.iter().any(|id| *id == food_id) {
            return false;
        }
        q.pending.push_back(food_id);
        if q.running {
            false
        } else {
            q.running = true;
            true
        }
    });
    if start {
        spawn_local(run_worker());
    }
}

/// Drain the queue one food at a time (classify + nutrient enrichment). Every
/// failure is recorded in the error log so it stays visible.
async fn run_worker() {
    loop {
        let next = Q.with(|q| q.borrow_mut().pending.pop_front());
        let Some(id) = next else {
            Q.with(|q| q.borrow_mut().running = false);
            return;
        };
        // Load the food; skip if gone. Один последовательный проход добирает и
        // признаки, и нутриенты, и железо — по запросу на каждую недостающую вещь,
        // строго по очереди, чтобы модель не получала их пачкой.
        let Some(mut food) = db::get::<Food>("foods", &id).await else { continue };
        // То же и здесь: в очередь блюдо могло попасть напрямую через `enqueue`.
        if food.is_recipe {
            continue;
        }

        // ПРИЗНАКИ СПРАШИВАЕТ КОНВЕЙЕР, и другого пути больше нет.
        //
        // Раньше рядом жил старый: пять отдельных запросов, по одному на признак,
        // каждый опознавал продукт заново и в тон своему вопросу — отсюда «Голец →
        // horse meat» в вопросе про мясо и печень рыбы в вопросе про гем. Его
        // оставляли, чтобы померить оба на одном наборе; конвейер выиграл, а ветка
        // осталась и молча договаривала за него всё, чего он не дал. На неопознанной
        // еде это выходило прямой ложью: «Кыстыбый» получал гем и красное мясо.
        //
        // Опознание живёт до конца прохода: его ждут и кальций, и железо, и жиры.
        let mut identity = String::new();
        if needs_classification(&food) {
            let name = food.name.clone();
            if let Some(r) = with_retries(
                move || {
                    let n = name.clone();
                    async move { super::flags_pipeline::classify_all(&n).await }
                },
                errors::FoodAspect::Kind,
                &food.name,
            )
            .await
            {
                // ПРОДУКТ НЕ ОПОЗНАН — проход по нему кончается здесь. Ни признаков,
                // ни нутриентов, ни железа, ни жиров: всё это было бы выдумано о еде,
                // которой мы не знаем. Человеку скажем словами, в Cloudflare уйдёт
                // событие «identity.unknown», и по нему стоит оповещение.
                if !r.recognised {
                    errors::record_food(
                        errors::FoodAspect::Kind,
                        &food.name,
                        "продукт не опознан: у модели нет уверенной версии",
                    );
                    continue;
                }
                let (f, fibre, ident) = (r.flags, r.fibre_g, r.identity);
                // Записывается только выясненное. Неполученный признак остаётся
                // пустым и будет переспрошен в другой раз — записать его наугад
                // значило бы соврать молча.
                for (flag, value) in [
                    (local::FoodFlag::VegFruit, f.veg_fruit),
                    (local::FoodFlag::Heme, f.heme),
                    (local::FoodFlag::MilkGlobule, f.milk_globule),
                    (local::FoodFlag::RedMeat, f.red_meat),
                    (local::FoodFlag::ProcessedMeat, f.processed_meat),
                    (local::FoodFlag::Egg, f.egg),
                ] {
                    if let Some(v) = value {
                        local::cache_food_flag(&id, flag, v).await;
                    }
                }
                identity = ident.unwrap_or_default();
                // Клетчатка приходит вместе с частями растения — своего запроса у неё
                // больше нет.
                if let Some(g) = fibre {
                    let mut one = std::collections::BTreeMap::new();
                    one.insert(super::indicators::N_FIBER.to_string(), g.max(0.0));
                    local::cache_food_nutrients(&id, one).await;
                }
                // Копия в памяти ОБЯЗАНА догнать базу: гейт ниже смотрит на `food`, и
                // без этого прежний путь переспрашивал всё заново и затирал ответы
                // конвейера своими — замер тогда мерит не то, что думает.
                food.is_veg_fruit = f.veg_fruit.or(food.is_veg_fruit);
                food.is_heme = f.heme.or(food.is_heme);
                food.is_milk_globule = f.milk_globule.or(food.is_milk_globule);
                food.is_red_meat = f.red_meat.or(food.is_red_meat);
                food.is_processed_meat = f.processed_meat.or(food.is_processed_meat);
                food.is_egg = f.egg.or(food.is_egg);
            }
        }

        // ОПОЗНАНИЕ НУЖНО И ТОГДА, когда признаки уже собраны. Так бывает после
        // миграции, стирающей кальций и железо, и после конфликта синхронизации:
        // конвейер выше не запускается, `identity` остаётся пустой, и кальций,
        // железо и жиры идут к модели с голым именем из дневника. Живой прогон
        // поймал это дословно: «Голец» получил 120 мг кальция как «жидкий молочный
        // продукт вроде кефира» — при том, что первым проходом ему верно дали 25.
        let needs_identity = super::enrich::needs_enrichment(&food)
            || food.fat_profile.is_none()
            || super::iron::needs_iron(&food);
        if identity.is_empty() && needs_identity {
            let name = food.name.clone();
            if let Some(ident) = with_retries(
                move || {
                    let n = name.clone();
                    async move { super::flags_pipeline::identify(&n).await }
                },
                errors::FoodAspect::Kind,
                &food.name,
            )
            .await
            {
                identity = ident.unwrap_or_default();
            }
        }

        // `food.nutrients` is unchanged by classification, so the loaded copy is
        // still current for the enrichment gate.
        if super::enrich::needs_enrichment(&food) {
            let f = food.clone();
            let ident_for_calcium = identity.clone();
            with_retries(
                move || {
                    let f = f.clone();
                    let id = ident_for_calcium.clone();
                    async move { super::enrich::enrich_food(&f, &id).await }
                },
                errors::FoodAspect::Nutrients,
                &food.name,
            ).await;
        }

        // Жиры — свой проход: спрашивается ПРОФИЛЬ (доли от жира), а не количество,
        // и одним запросом сразу все пять величин.
        //
        // Идёт С ПЕРВОГО ДНЯ, не дожидаясь открытия недели жиров. Показываем мы
        // постепенно, а СОБИРАТЬ данные надо сразу: иначе в день открытия вся
        // съеденная за месяцы еда разом уходит в очередь к модели — шквал запросов,
        // и человек смотрит на пустые шкалы, пока он разгребается.
        if food.fat_profile.is_none() {
            let name = food.name.clone();
            let ident_for_fat = identity.clone();
            if let Some(profile) = with_retries(
                move || {
                    let n = name.clone();
                    let id = ident_for_fat.clone();
                    async move { super::ai::lookup_fat_profile(&n, &id).await }
                },
                errors::FoodAspect::Fats,
                &food.name,
            )
            .await
            {
                local::cache_food_fat_profile(&id, profile).await;
            }
        }

        // Iron is its OWN pass: it needs the absorbed fraction alongside the
        // amount, so it can't ride the generic nutrient request (see `iron.rs`).
        // Тоже с первого дня — по той же причине, что и жиры.
        if super::iron::needs_iron(&food) {
            let f = food.clone();
            let ident_for_iron = identity.clone();
            with_retries(
                move || {
                    let f = f.clone();
                    let id = ident_for_iron.clone();
                    async move { super::iron::enrich_iron(&f, &id).await }
                },
                errors::FoodAspect::Iron,
                &food.name,
            ).await;
        }
    }
}

/// True if a food still needs any background AI processing (tags, nutrients, iron).
///
/// БЛЮДА СЮДА НЕ ПОПАДАЮТ. У рецепта состав известен: и калории, и нутриенты, и
/// железо считаются из ингредиентов (`local::recipe_nutrition`). Спрашивать про
/// него модель — значит в лучшем случае повторить посчитанное, а в худшем
/// подменить его догадкой по названию. Именно так у блюда из мидий оказывалось
/// мидийное железо: модель узнавала слово, а не состав.
fn needs_processing(food: &Food) -> bool {
    if food.is_recipe {
        return false;
    }
    needs_classification(food)
        || super::enrich::needs_enrichment(food)
        || super::iron::needs_iron(food)
        || food.fat_profile.is_none()
}

/// Enqueue EVERY product in the database that still lacks something — tags,
/// nutrients or iron. Called on app activation (launch + foreground).
///
/// This used to walk the last two weeks of the DIARY instead. That window was cut
/// for the daily indicators, whose verdict spans seven days; it is far too short for
/// a weekly one — the iron indicator judges eight completed weeks, so six of them
/// could never be filled and stayed empty forever. And when a NEW nutrient is added
/// later, every product ever created is missing it, not just the recently eaten
/// ones. So the sweep is over the whole `foods` store, and the answer to «is there
/// anything to do» comes from the product itself.
///
/// It cannot loop: a product is enqueued only while something is genuinely absent,
/// and every pass either fills a field or records an error. A value of ZERO counts
/// as filled — «this food has no iron» is an answer, not a gap.
pub async fn sweep_unprocessed() {
    for food in local::list_foods().await {
        if needs_processing(&food) {
            enqueue(food.id.clone());
        }
    }
}
