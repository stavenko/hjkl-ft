//! Запросы к модели: справочники нутриентов и их промпты.
//!
//! ╔══════════════════════════════════════════════════════════════════════════╗
//! ║  ВНИМАНИЕ: ПРАВИТЕ ПРОМПТ ИЛИ СПРАВОЧНИК — СНАЧАЛА                       ║
//! ║  `docs/food-identity.md`                                                 ║
//! ║                                                                          ║
//! ║  Там записано, почему числа наши, а не модели; почему справочник         ║
//! ║  отвечает КЛЮЧОМ записи, а не величиной; и общие грабли промптов —       ║
//! ║  схема уезжает модели текстом, `serde(default)` роняет поля под          ║
//! ║  грамматикой, имя поля сильнее оговорок в тексте.                        ║
//! ║                                                                          ║
//! ║  Правку промпта меряют ЖИВЫМ путём и обязательно вместе с выдумкой:      ║
//! ║  см. `scripts/measure-*.mjs` и `scripts/check-full-pass.mjs`.            ║
//! ╚══════════════════════════════════════════════════════════════════════════╝

use std::cell::RefCell;
use std::collections::BTreeMap;

use api_types::*;
use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{auth, bug_report, config, local, update};

#[derive(Debug, Deserialize, JsonSchema)]
struct NutritionResponse {
    /// A SHORT product/dish name the model derives from the input (which may be a
    /// plain name OR a free-form description). Used to fill the name field.
    #[serde(default)]
    product_name: String,
    kcal: NutrientDetail,
    protein: NutrientDetail,
    fat: NutrientDetail,
    carbs: NutrientDetail,
    custom_nutrients: BTreeMap<String, NutrientDetail>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct NutrientDetail {
    min_value: ValueUnit,
    max_value: ValueUnit,
    recommended: ValueUnit,
    /// Why this value is appropriate for this food
    comment: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ValueUnit {
    value: f64,
    /// One of: kcal, kg, g, mg, mkg
    unit: String,
}

impl NutrientDetail {
    fn into_api(self) -> AiNutrientDetail {
        AiNutrientDetail {
            min_value: AiValueWithUnit { value: self.min_value.value, unit: self.min_value.unit },
            max_value: AiValueWithUnit { value: self.max_value.value, unit: self.max_value.unit },
            recommended: AiValueWithUnit { value: self.recommended.value, unit: self.recommended.unit },
            comment: self.comment,
        }
    }
}

pub(crate) fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.trim_start_matches(|c: char| c.is_alphanumeric());
        let rest = rest.trim_start_matches('\n');
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        s
    }
}

/// Разобрать ОДИН объект ответа модели, каким бы он ни приехал.
///
/// Модель просят ответить единственным объектом, и Workers AI держит эту форму
/// грамматикой. Сторонний провайдер соблюдает схему как пожелание: замер на
/// qwen3.8-27b показал 3 массива `[{…}]` из 8 ответов — поля внутри те же, обёртка
/// лишняя. Ни `response_format: strict`, ни прямая просьба «one object, never an
/// array» этого не убрали, поэтому обёртку снимаем здесь: ответ модели — это её
/// содержание, а не скобки вокруг него.
///
/// Массив ИЗ НЕСКОЛЬКИХ элементов — не обёртка, а другой ответ, и он ошибка.
pub(crate) fn parse_one<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T, String> {
    let value: serde_json::Value = serde_json::from_str(strip_code_fences(raw.trim()))
        .map_err(|e| format!("{e}"))?;
    let value = match value {
        serde_json::Value::Array(mut items) if items.len() == 1 => items.remove(0),
        other => other,
    };
    serde_json::from_value(value).map_err(|e| format!("{e}"))
}

/// Which stream a token belongs to, reported to the caller as lookup progresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiPhase {
    Thinking,
    Answer,
}

fn build_executor() -> Result<Qwen, String> {
    build_executor_think(true)
}

/// Build the Qwen executor with reasoning on/off. Off (`think=false`) is used by the
/// background nutrient enrichment: with thinking ON qwen3 sometimes emits a whole
/// short answer into the reasoning channel and nothing into content (empty response);
/// OFF makes it answer in content reliably. The ai-worker maps this `think` flag to
/// `chat_template_kwargs.enable_thinking`.
pub(crate) fn build_executor_think(think: bool) -> Result<Qwen, String> {
    let cfg = config::get();
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let executor = Qwen::builder()
        .api_base(&cfg.ai_base_url)
        .api_key(token)
        .model("@cf/qwen/qwen3-30b-a3b-fp8")
        // Emit reasoning tokens to the thinking stream (for the "thinking" UI phase)
        // only when reasoning is enabled.
        .think(think)
        // Потолок ответа. С рассуждением он должен вмещать И рассуждение, И ответ —
        // 8000, иначе qwen3 съедает лимит размышлением и возвращает пустой контент.
        //
        // БЕЗ рассуждения столько не нужно никогда: самый длинный ответ здесь —
        // КБЖУ с пятью нутриентами и комментариями, около 2500 символов. Зато
        // модель изредка срывается в генерацию мусора и молотит до самого потолка:
        // наблюдалось 6774 символа «!!!!!!!!…» на запрос омега-3. Разобрать это всё
        // равно нельзя — попытка пропадает, — но платим мы за все выданные токены.
        // 2000 хватает любому здешнему ответу и вчетверо укорачивает срыв.
        .max_tokens(if think { 8000 } else { 2000 })
        .build();
    Ok(executor)
}

/// Исполнитель для узла ОПОЗНАНИЯ. Отличается от общего только моделью: если в
/// конфиге задан `identity_model`, вопрос уходит ей — ai-worker сам увезёт его к
/// стороннему провайдеру. Пусто — ровно тот же исполнитель, что у признаков.
///
/// Рассуждение здесь всегда выключено: рассуждая, модель уговаривает себя, и
/// незнание перестаёт быть незнанием (см. `IdentifyNode`).
pub(crate) fn build_identity_executor() -> Result<Qwen, String> {
    let model = config::get().identity_model.trim();
    if model.is_empty() {
        return build_executor_think(false);
    }
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    Ok(Qwen::builder()
        .api_base(&config::get().ai_base_url)
        .api_key(token)
        .model(model)
        .think(false)
        .max_tokens(2000)
        .build())
}

/// Shared skeleton for both nutrition-lookup prompts. The only thing that differs
/// between the two use cases is `form_clause` (raw/as-sold vs cooked/as-served),
/// so the identical structure (naming rules, per-100 g, JSON contract) lives here
/// and the two named builders below supply their clause.
fn build_nutrition_prompt(name: &str, custom: &str, lang: &str, form_clause: &str) -> String {
    format!(
        "You are a nutritional database. The user's input may be a plain food NAME (e.g. \
         «яйцо», «рис», «гречка») OR a free-form DESCRIPTION of a dish, possibly with added \
         ingredients (e.g. «жареная курица, добавил немного лука, чайную ложку масла»). \
         Input: \"{name}\".\n\n\
         First, set \"product_name\" to a SHORT dish/product name in {lang}. HARD LIMIT: at most \
         THREE words, ideally TWO; a third word ONLY when indispensable to identify the dish. \
         Do NOT list the added ingredients in the name — name only the core dish. Good examples \
         (2–3 words): «Жареная курица», «Куриная грудка», «Овсяная каша», «Гречка с грибами». \
         For a plain name, keep it (tidied); for a description, name the resulting core dish \
         concisely within this limit.\n\n\
         Then provide nutritional values per 100 GRAMS of the resulting food/dish (account for \
         the added ingredients — e.g. the oil raises fat and kcal).\n\n\
         {form}\n\n\
         For each nutrient (kcal, protein, fat, carbs{custom}), provide:\n\
         - min_value: lowest reasonable value for this food\n\
         - max_value: highest reasonable value for this food\n\
         - recommended: the most likely value to select\n\
         - comment: brief explanation why this value is appropriate, written in {lang}\n\n\
         Use these units: kcal for calories, g/mg/mkg/kg for weights.\n\
         All values are per 100g. Compute real values specifically for the input — do not \
         copy any sample numbers.\n\n\
         Respond with ONLY a single minified JSON object and nothing else — no markdown, no \
         prose before or after. Include \"product_name\" as a string. EVERY key and EVERY \
         string value MUST be wrapped in double quotes. EVERY `value` MUST be a real number \
         (e.g. 12.5), never empty or null. Custom nutrients go in the \"custom_nutrients\" \
         object (use {{}} if none).",
        name = name,
        custom = custom,
        lang = lang,
        form = form_clause,
    )
}

/// USE CASE 1 — «поиск по названию/описанию»: the user TYPED a food name or a
/// free-form description and enters/edits the weight themselves. People weigh food
/// AS BOUGHT, so values are for the RAW / as-sold product (dry pasta ~370, raw
/// meat) unless the text itself says cooked. Used by the manual food-entry lookup.
fn lookup_prompt_by_name(name: &str, custom: &str, lang: &str) -> String {
    build_nutrition_prompt(
        name,
        custom,
        lang,
        "Form of the product: for items bought and weighed raw/dry (grains, rice, pasta, \
         flour, meat, fish, legumes, etc.), give values for the RAW / as-sold product — \
         NOT cooked — unless the input says cooked, boiled, fried, steamed, ready-to-eat, \
         or clearly describes a prepared dish.",
    )
}

/// USE CASE 2 — «распознавание по фото тарелки»: the food name came from a PLATE
/// PHOTO and the estimated grams are the COOKED / as-served portion. Values must be
/// for the cooked / ready-to-eat food (boiled pasta ~130 kcal/100 g, not dry ~370),
/// else a cooked-portion weight × raw density over-counts calories ~2–3×. Used by
/// the food-photo detection lookup.
fn lookup_prompt_from_photo(name: &str, custom: &str, lang: &str) -> String {
    build_nutrition_prompt(
        name,
        custom,
        lang,
        "Form of the product: this food was photographed ON A PLATE, ready to eat, and \
         its weight is the COOKED / as-served portion. Give values for the food in its \
         COOKED / ready-to-eat state (e.g. boiled pasta ~130 kcal/100 g, boiled rice \
         ~120, NOT the dry product), even if the name alone sounds like a raw ingredient.",
    )
}

pub async fn lookup(
    input: &AiLookupInput,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<AiLookupOutput, String> {
    let custom_part = if input.custom_nutrients.is_empty() {
        String::new()
    } else {
        let keys: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("\"{}\"", s.key))
            .collect();
        let descriptions: Vec<String> = input
            .custom_nutrients
            .iter()
            .map(|s| format!("{} = {}", s.key, s.name))
            .collect();
        format!(
            "\n\nAlso provide values for these custom nutrients in custom_nutrients map. \
             Use ONLY these strings as keys: {}. \
             Reference: {}.",
            keys.join(", "),
            descriptions.join(", "),
        )
    };

    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    // Two distinct prompts for two distinct use cases (see each fn's doc). The
    // `as_served` flag on the input selects which — set by the calling feature.
    let prompt = if input.as_served {
        lookup_prompt_from_photo(&input.name, &custom_part, lang)
    } else {
        lookup_prompt_by_name(&input.name, &custom_part, lang)
    };

    let key_to_name: BTreeMap<String, String> = input
        .custom_nutrients
        .iter()
        .map(|s| (s.key.clone(), s.name.clone()))
        .collect();

    // The schema-injected JSON mode reliably keeps the model in JSON (without it it
    // often replies in Markdown). No numeric example in the prompt on purpose:
    // concrete sample values anchored the model (it kept returning cooked-rice
    // 155 kcal for «рис»); the schema carries the shape.
    let response: NutritionResponse = generate(prompt, on_token).await?;

    let nutrients: BTreeMap<String, AiNutrientDetail> = response
        .custom_nutrients
        .into_iter()
        .filter_map(|(ai_key, v)| {
            let display_name = key_to_name.get(&ai_key)?;
            Some((display_name.clone(), v.into_api()))
        })
        .collect();

    let product_name = response.product_name.trim();
    Ok(AiLookupOutput {
        // Fill the name from the model's summarised product name (a plain input
        // comes back tidied; a description is condensed to a dish name).
        name: (!product_name.is_empty()).then(|| product_name.to_string()),
        kcal: response.kcal.into_api(),
        protein: response.protein.into_api(),
        fat: response.fat.into_api(),
        carbs: response.carbs.into_api(),
        nutrients,
        package_weight: None,
    })
}

/// Ask the model for a single JSON object of type `T`. THE one path every
/// structured request takes — `lookup`, `lookup_calcium`, `lookup_iron`,
/// `classify_food`, `match_food`. The ai-worker turns `T`'s schema into a JSON-mode
/// instruction; `on_token` reports thinking/answer tokens so a caller can drive the
/// live "thinking/answer (N tok)" UI (pass `|_| {}` when there is no UI).
pub async fn generate<T>(
    prompt: String,
    on_token: impl Fn(AiPhase) + Clone + 'static,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + JsonSchema,
{
    generate_validated(prompt, on_token, 3, |_: &T| Ok(())).await
}

/// Same, plus a check the parsed answer must pass before the caller sees it. A
/// rejected answer costs one attempt like any other failure — the model gets
/// another go instead of the caller storing a number nobody stands behind.
///
/// EVERY failure mode is retryable here: a dropped fetch, an empty content stream,
/// malformed JSON, a failed check. This used to be three hand-copied loops that
/// disagreed with each other — in two of them an `execute` error escaped the loop
/// through `?` and failed the whole call on the first hiccup.
pub async fn generate_validated<T>(
    prompt: String,
    on_token: impl Fn(AiPhase) + Clone + 'static,
    attempts: usize,
    validate: impl Fn(&T) -> Result<(), String>,
) -> Result<T, String>
where
    T: serde::de::DeserializeOwned + JsonSchema,
{
    let mut last_err = String::from("no attempts were made");
    for attempt in 0..attempts {
        if attempt > 0 {
            // Мгновенный повтор часто приносит тот же мусор — даём модели передышку.
            leptos::logging::warn!("generate retry #{attempt}: {last_err}");
            sleep_ms(800).await;
        }
        // Thinking OFF: with it ON qwen3 parks a short answer in the reasoning
        // channel and returns empty content (observed ~⅔ of the time for some foods).
        let executor = build_executor_think(false)?;
        let result = match executor.execute::<T>(prompt.clone()).await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("LLM execute error: {e:?}");
                continue;
            }
        };

        let mut thinking_stream = result.thinking_stream;
        let on_think = on_token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = thinking_stream.next().await {
                if token.is_ok() {
                    on_think(AiPhase::Thinking);
                }
            }
        });

        let mut content_stream = result.content_stream;
        let on_answer = on_token.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = content_stream.next().await {
                if token.is_ok() {
                    on_answer(AiPhase::Answer);
                }
            }
        });

        let output = match result.output.await {
            Ok(o) => o,
            Err(e) => {
                last_err = format!("LLM output error: {e:?}");
                continue;
            }
        };
        let raw = output.result.trim();
        if raw.is_empty() {
            last_err = "model returned an empty response".to_string();
            continue;
        }
        let value: T = match serde_json::from_str(strip_code_fences(raw)) {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("parse error: {e}, raw: {raw}");
                continue;
            }
        };
        match validate(&value) {
            Ok(()) => return Ok(value),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}
// ── Сколько кальция в продукте, по категориям ─────────────────────────────────
//
// Измерено на 200 запросах (по десять на двадцать продуктов, `scripts/measure-
// calcium-consistency.sh`): модель кальций НЕ знает. Десять медиан из двадцати
// легли в узкий коридор 120–135 мг независимо от продукта — соевое молоко без
// добавок 135 при реальных 25, брокколи 127.5 при 47, миндаль 125 при 269. Семена
// занижаются кратно и устойчиво: кунжут 220 при 975, мак 22.5 при 1438. Слово
// «обогащённое кальцием» в названии игнорируется полностью.
//
// Поэтому числа — НАШИ, как и у железа. Работа модели — узнать продукт и поместить
// его в строку; величину задаёт строка. Таблица одна: промпт РЕНДЕРИТСЯ из этого
// массива, ответ ПРОВЕРЯЕТСЯ по нему же, разойтись они не могут.
//
// Продукты, не попавшие ни в одну строку, получают НОЛЬ (`other_none`). Мясо, рыба
// без костей, яйца, крупы, хлеб, макароны, овощи, фрукты, масла, сахар несут
// десятки миллиграммов, но для недельной нормы это шум — считаем, что кальция там
// нет, вместо того чтобы гадать.
struct CalciumCategory {
    /// Stable key the model must echo back in `category`.
    key: &'static str,
    mg_min: f64,
    mg_max: f64,
    /// Russian examples — the input names are Russian, so they match better.
    pub(crate) examples: &'static str,
}

const CALCIUM_CATEGORIES: &[CalciumCategory] = &[
    // ── Сыры: чем твёрже и суше, тем больше кальция ──
    CalciumCategory { key: "cheese_hard", mg_min: 600.0, mg_max: 1300.0,
        examples: "пармезан, грана падано, чеддер, гауда, эмменталь, российский, пекорино" },
    CalciumCategory { key: "cheese_semi", mg_min: 350.0, mg_max: 700.0,
        examples: "моцарелла, сулугуни, адыгейский, брынза, фета, эдам, косичка" },
    CalciumCategory { key: "cheese_soft", mg_min: 80.0, mg_max: 400.0,
        examples: "рикотта, камамбер, бри, маскарпоне, творожный сыр, филадельфия" },
    CalciumCategory { key: "cheese_processed", mg_min: 200.0, mg_max: 900.0,
        examples: "плавленый сыр, сырок плавленый, сыр в ванночке, хохланд, виола" },
    // ── Остальная молочка ──
    CalciumCategory { key: "milk_liquid", mg_min: 90.0, mg_max: 150.0,
        examples: "молоко, кефир, ряженка, айран, простокваша, питьевой йогурт" },
    CalciumCategory { key: "yogurt_sour_cream", mg_min: 80.0, mg_max: 180.0,
        examples: "йогурт, греческий йогурт, скир, сметана, простокваша густая, \
                   сливочный сыр без соли" },
    CalciumCategory { key: "cottage_cheese", mg_min: 80.0, mg_max: 200.0,
        examples: "творог, зернёный творог, творожная масса, сырники" },
    CalciumCategory { key: "milk_concentrated", mg_min: 250.0, mg_max: 1300.0,
        examples: "сухое молоко, сгущённое молоко, молочная сыворотка сухая" },
    CalciumCategory { key: "cream_whey", mg_min: 50.0, mg_max: 150.0,
        examples: "сливки, сыворотка, мороженое пломбир, мороженое сливочное, эскимо, \
                   молочный коктейль" },
    // ── Немолочные источники, где кальция действительно много ──
    CalciumCategory { key: "seeds_high", mg_min: 500.0, mg_max: 1500.0,
        examples: "мак, кунжут, семена чиа" },
    CalciumCategory { key: "sesame_paste", mg_min: 250.0, mg_max: 550.0,
        examples: "тахини, халва тахинная, кунжутная паста, козинак кунжутный" },
    // Строка ТОЛЬКО про консервы, которые едят вместе с костями: кальций там из
    // костей, а не из рыбы. Без соседней строки `fish_plain` модель складывала сюда
    // ЛЮБУЮ рыбу и выдавала середину диапазона — замер: кижуч и треска получали 285
    // мг при настоящих 36 и 16. Слово «лосось» в примерах эту ошибку и кормило,
    // поэтому названий рыб здесь больше нет, есть признак «с костями».
    CalciumCategory { key: "fish_with_bones", mg_min: 120.0, mg_max: 450.0,
        examples: "рыбные консервы, которые едят ВМЕСТЕ С КОСТЯМИ: сардины, шпроты, килька, \
                   консервы «с костями» на этикетке" },
    // Обычная рыба и морепродукты — кальция в них десятки миллиграммов, для
    // недельной нормы это шум. Строка нужна отдельно от `other_none`: модель ищет
    // САМУЮ ПОХОЖУЮ строку, и без рыбной среди них любая рыба уезжала в консервы.
    CalciumCategory { key: "fish_plain", mg_min: 10.0, mg_max: 60.0,
        examples: "рыба и морепродукты без костей: лосось, кижуч, горбуша, форель, треска, \
                   минтай, скумбрия, сельдь, тунец, филе любой рыбы, креветки, кальмар" },
    CalciumCategory { key: "tofu", mg_min: 100.0, mg_max: 700.0,
        examples: "тофу, соевый творог" },
    // Строки узкие НАМЕРЕННО: модель берёт середину диапазона, поэтому широкая
    // строка даёт систематический промах. Замер: при `nuts` 50–300 миндаль получал
    // 150 вместо 269, при `greens_low_oxalate` 40–250 брокколи — 150 вместо 47.
    CalciumCategory { key: "nuts_high_calcium", mg_min: 200.0, mg_max: 300.0,
        examples: "миндаль, бразильский орех" },
    CalciumCategory { key: "nuts_other", mg_min: 40.0, mg_max: 150.0,
        examples: "фундук, фисташки, кешью, грецкий орех, арахис, кедровый орех" },
    CalciumCategory { key: "legumes_dry", mg_min: 60.0, mg_max: 300.0,
        examples: "фасоль сухая, нут сухой, соевые бобы, маш" },
    CalciumCategory { key: "greens_leafy", mg_min: 130.0, mg_max: 260.0,
        examples: "петрушка, укроп, кале, руккола, базилик, кинза" },
    // ФРУКТЫ И ЯГОДЫ ЖИВУТ ЗДЕСЬ ЖЕ, и не по родству, а по величине: у них те же
    // единицы и десятки миллиграммов, что у овощей. Своей строки у них не было
    // вовсе, и живой прогон это поймал — «мороженая вишня» проваливалась в
    // `other_none` с нулём, а до того брала 130 мг по созвучному «мороженому».
    // Нижняя граница снижена с 15 до 5: у яблока кальция около шести.
    CalciumCategory { key: "vegetables_other", mg_min: 5.0, mg_max: 90.0,
        examples: "брокколи, цветная капуста, белокочанная капуста, пекинская капуста, \
                   стручковая фасоль, репа, картофель, морковь, свёкла, кабачок, помидор, \
                   огурец, тыква, а также ЛЮБЫЕ фрукты и ягоды: яблоко, груша, банан, \
                   апельсин, вишня, клубника, черника, виноград — свежие и замороженные" },
    CalciumCategory { key: "greens_oxalate", mg_min: 40.0, mg_max: 120.0,
        examples: "шпинат, щавель, ревень, свекольная ботва" },
    CalciumCategory { key: "dried_fruit", mg_min: 30.0, mg_max: 180.0,
        examples: "инжир сушёный, курага, урюк, финики" },
    // ── Обогащение задаёт значение, и узнаётся оно ТОЛЬКО по названию ──
    // Обогащают почти всегда «до уровня молока» — отсюда узкий диапазон.
    CalciumCategory { key: "fortified", mg_min: 100.0, mg_max: 180.0,
        examples: "обогащённое кальцием растительное молоко, сок с кальцием, хлопья с кальцием" },
    CalciumCategory { key: "plant_milk_plain", mg_min: 5.0, mg_max: 40.0,
        examples: "соевое молоко без добавок, овсяное молоко, миндальное молоко, кокосовое молоко" },
    // ── Всё прочее — ноль ──
    CalciumCategory { key: "other_none", mg_min: 0.0, mg_max: 0.0,
        examples: "мясо, птица, яйца, крупы, макароны, хлеб, растительное масло, сахар, \
                   конфеты, вода, чай, кофе, алкоголь" },
];

/// СПРАВОЧНИК КЛЕТЧАТКИ: продукт → граммы пищевых волокон на 100 г.
///
/// Клетчатку спрашивает растительный узел вместе с частями растения, и числа он
/// называл по памяти — та же болезнь, что была у кальция и железа. Замер: 22 из 29,
/// варёная чечевица 3.8 г против справочных 7.9, соя 2.6 против 9.3.
///
/// ФОРМА — ЧАСТЬ ЗАПИСИ, как и в справочнике КБЖУ: у сухой гречки волокон 10 г, у
/// варёной 2.7, и запись без формы врала бы вчетверо.
pub(crate) const FIBRE_REFERENCE: &[(&str, f64)] = &[
    // ── Крупы, хлеб, макароны: сухие ──
    ("гречка сухая", 10.0),
    ("овсянка сухая", 10.0),
    ("перловка сухая", 15.6),
    ("пшено сухое", 8.5),
    ("булгур сухой", 12.5),
    ("киноа сухая", 7.0),
    ("рис белый сухой", 1.3),
    ("рис бурый сухой", 3.5),
    ("макароны сухие", 3.0),
    ("манка сухая", 3.6),
    ("отруби пшеничные", 43.0),
    ("отруби овсяные", 15.4),
    ("попкорн", 14.5),
    // ── Крупы и бобовые: варёные ──
    ("гречка варёная", 2.7),
    ("овсянка на воде", 1.7),
    ("рис белый варёный", 0.4),
    ("макароны варёные", 1.8),
    ("чечевица варёная", 7.9),
    ("фасоль варёная", 6.4),
    ("нут варёный", 7.6),
    // ── Хлеб ──
    ("хлеб пшеничный", 2.7),
    ("хлеб ржаной", 5.8),
    ("хлеб бородинский", 7.9),
    ("хлеб цельнозерновой", 7.0),
    // ── Бобовые сухие ──
    ("чечевица сухая", 10.7),
    ("фасоль сухая", 15.0),
    ("нут сухой", 12.2),
    ("горох сухой", 11.0),
    ("маш", 16.0),
    ("соя", 9.3),
    ("тофу", 0.4),
    // ── Орехи и семена ──
    ("миндаль", 12.5),
    ("грецкий орех", 6.7),
    ("фундук", 9.7),
    ("кешью", 3.3),
    ("фисташки", 10.0),
    ("кедровый орех", 3.7),
    ("арахис", 8.5),
    ("семена подсолнечника", 8.6),
    ("семена тыквы", 6.0),
    ("кунжут", 11.8),
    ("семена льна", 27.0),
    ("семена чиа", 34.0),
    // ── Овощи ──
    ("картофель", 2.2),
    ("морковь", 2.8),
    ("капуста белокочанная", 2.5),
    ("брокколи", 2.6),
    ("цветная капуста", 2.0),
    ("помидор", 1.2),
    ("огурец", 0.7),
    ("лук репчатый", 1.7),
    ("болгарский перец", 1.7),
    ("кабачок", 1.0),
    ("свёкла", 2.8),
    ("тыква", 0.5),
    ("топинамбур", 1.6),
    ("стручковая фасоль", 3.4),
    ("шампиньоны", 1.0),
    ("сельдерей стебель", 1.6),
    ("сельдерей корень", 1.8),
    ("спаржа", 2.1),
    ("ревень", 1.8),
    // ── Зелень ──
    ("петрушка", 3.3),
    ("укроп", 2.1),
    ("шпинат", 2.2),
    ("руккола", 1.6),
    ("салат листовой", 1.3),
    // ── Фрукты, ягоды, сухофрукты ──
    ("яблоко", 2.4),
    ("груша", 3.1),
    ("банан", 2.6),
    ("апельсин", 2.4),
    ("виноград", 0.9),
    ("клубника", 2.0),
    ("черника", 2.4),
    ("вишня", 1.6),
    ("малина", 6.5),
    ("смородина", 4.8),
    ("авокадо", 6.7),
    ("курага", 7.3),
    ("изюм", 3.7),
    ("чернослив", 7.0),
    ("финики", 8.0),
];

/// Справочник клетчатки в том виде, в каком его видит модель.
pub(crate) fn fibre_reference_table() -> String {
    FIBRE_REFERENCE
        .iter()
        .map(|(name, g)| format!("  {name}: {g}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Сравнение ключей справочника — БЕЗ УЧЁТА РЕГИСТРА И ДЛЯ КИРИЛЛИЦЫ ТОЖЕ.
///
/// `eq_ignore_ascii_case` приводит регистр только у латиницы: «Соя» и «соя» она
/// считает РАЗНЫМИ строками, потому что кириллические байты не трогает. Ключи у нас
/// русские, и любая заглавная буква в ответе модели молча роняла поиск записи —
/// значение уходило на строку таблицы, как это было с соей.
pub(crate) fn key_eq(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Запись справочника клетчатки по ключу, который назвала модель.
pub(crate) fn fibre_reference_hit(key: &str) -> Option<&'static (&'static str, f64)> {
    let key = key.trim();
    if key.is_empty() || key_eq(key, "none") {
        return None;
    }
    FIBRE_REFERENCE.iter().find(|(name, _)| key_eq(name, key))
}

/// Таблица кальция как текст промпта — из того же массива, что и проверка.
/// СПРАВОЧНИК КАЛЬЦИЯ: продукт → миллиграммы на 100 г.
///
/// Категории с диапазонами ([`CALCIUM_CATEGORIES`]) отвечают на вопрос «какого рода
/// этот продукт», а число внутри диапазона до сих пор называла модель по памяти — и
/// называла плохо: замер по группам источников дал верную строку в 26 случаях из 30
/// и верную величину лишь в 13. Сыру российскому она давала 130 мг при справочных
/// 880, моцарелле 150 при 500, сливочному мороженому 12–22 мг три попытки подряд.
/// Строку она выбирает хорошо, цифры помнит плохо — поэтому цифры теперь наши.
///
/// Здесь только то, что люди едят часто, и прежде всего молочное: на него приходится
/// большая часть кальция в рационе, и там же были самые грубые промахи. Всё
/// остальное по-прежнему считается по строке и её диапазону.
///
/// Значения типовые, на 100 г съедобной части. Пополняется по мере находок.
pub(crate) const CALCIUM_REFERENCE: &[(&str, f64)] = &[
    // Жидкое молочное.
    ("молоко", 120.0),
    ("кефир", 120.0),
    ("ряженка", 124.0),
    ("простокваша", 118.0),
    ("айран", 118.0),
    ("питьевой йогурт", 110.0),
    // Кисломолочное густое.
    ("йогурт натуральный", 120.0),
    ("йогурт греческий", 110.0),
    ("скир", 150.0),
    ("сметана", 90.0),
    // Творог.
    ("творог", 150.0),
    ("творог обезжиренный", 120.0),
    ("творожная масса", 135.0),
    ("творожок детский", 110.0),
    // Сыры.
    ("пармезан", 1180.0),
    ("грана падано", 1160.0),
    ("эмменталь", 1000.0),
    ("маасдам", 800.0),
    ("сыр российский", 880.0),
    ("чеддер", 720.0),
    ("гауда", 700.0),
    ("сулугуни", 650.0),
    ("брынза", 530.0),
    ("моцарелла", 500.0),
    ("адыгейский сыр", 520.0),
    ("фета", 360.0),
    ("камамбер", 390.0),
    ("рикотта", 210.0),
    ("бри", 185.0),
    ("творожный сыр", 100.0),
    ("плавленый сыр", 700.0),
    // Прочее молочное.
    ("молоко сухое", 1000.0),
    ("молоко сгущённое", 120.0),
    ("сливки", 86.0),
    ("сыворотка молочная", 60.0),
    ("мороженое пломбир", 130.0),
    ("мороженое сливочное", 120.0),
    // Немолочные источники, где кальция много.
    ("мак", 1460.0),
    ("кунжут", 975.0),
    ("семена чиа", 630.0),
    ("тахини", 420.0),
    // Халвы две, и кальций у них разный втрое: тахинная делается из кунжута с его
    // 975 мг, подсолнечная — из семечек, где кальция вчетверо меньше.
    ("халва подсолнечная", 211.0),
    ("халва тахинная", 420.0),
    ("семена подсолнечника", 78.0),
    // Козинак — те же семечки, но лишь около половины массы: остальное карамель,
    // кальция в ней нет. Без своей строки он брал запись халвы и получал её 420.
    ("козинак подсолнечный", 45.0),
    ("тофу", 350.0),
    ("соя", 240.0),
    ("миндаль", 260.0),
    ("сардины консервированные", 380.0),
    ("шпроты", 300.0),
    ("лосось консервированный", 180.0),
    // Растительное.
    ("петрушка", 245.0),
    ("укроп", 223.0),
    ("руккола", 160.0),
    ("кале", 150.0),
    ("шпинат", 99.0),
    ("брокколи", 47.0),
    ("капуста белокочанная", 40.0),
    ("капуста квашеная", 48.0),
    ("цветная капуста", 22.0),
    ("гречка сухая", 20.0),
    ("гречка варёная", 7.0),
    ("рис белый варёный", 3.0),
    ("овсянка на воде", 8.0),
    ("макароны варёные", 7.0),
    ("чечевица варёная", 19.0),
    ("фасоль варёная", 35.0),
    ("нут варёный", 49.0),
    ("овсянка сухая", 52.0),
    ("перловка сухая", 29.0),
    ("пшено сухое", 27.0),
    ("булгур сухой", 35.0),
    ("киноа сухая", 47.0),
    ("полба сухая", 25.0),
    ("рис белый сухой", 10.0),
    ("макароны сухие", 21.0),
    ("маш", 132.0),
    ("чечевица сухая", 56.0),
    ("горох сухой", 115.0),
    ("фасоль сухая", 143.0),
    ("нут сухой", 105.0),
    ("курага", 160.0),
    ("инжир сушёный", 162.0),
    ("апельсин", 40.0),
    // Овощи, фрукты и ягоды поимённо. Строка без чисел даёт им потолок коридора:
    // замер поймал вишне 90 мг при настоящих шестнадцати, отварному картофелю 40 при
    // двенадцати. Величины мелкие, но они складываются за день.
    ("картофель", 12.0),
    ("морковь", 33.0),
    ("помидор", 10.0),
    ("огурец", 16.0),
    ("кабачок", 15.0),
    ("тыква", 21.0),
    ("яблоко", 6.0),
    ("груша", 9.0),
    ("банан", 5.0),
    ("виноград", 10.0),
    ("вишня", 16.0),
    ("клубника", 16.0),
    ("черника", 6.0),
    // Сливочное масло — почти чистый жир, кальция в нём практически нет. Без своей
    // записи модель брала «сливки» с их 86 мг.
    ("сливочное масло", 24.0),
    // Частое, где кальция мало, — чтобы не уходило в ноль по недоразумению.
    ("яйцо куриное", 55.0),
    ("рыба, филе", 25.0),
    ("хлеб", 25.0),
    ("говядина", 12.0),
];

/// Справочник как текст промпта — из того же массива, что и сам справочник.
fn calcium_reference_table() -> String {
    CALCIUM_REFERENCE
        .iter()
        .map(|(name, mg)| format!("  {name}: {mg:.0}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Строки таблицы БЕЗ ЧИСЕЛ. Диапазоны нужны нам, чтобы поджать ответ, но показывать
/// их модели вредно: увидев числа, она начинает ими отвечать вместо величины
/// продукта — так же, как это было у железа.
fn calcium_category_keys() -> String {
    CALCIUM_CATEGORIES
        .iter()
        .map(|c| format!("  {key:<20} — {ex}", key = c.key, ex = c.examples))
        .collect::<Vec<_>>()
        .join("\n")
}

// Комментарии у полей — доковые: они уезжают в json_schema и работают инструкцией.
// Пояснения для читателя кода держим в `//`, иначе док СТРУКТУРЫ уедет в схему как
// `description` объекта и модель вернёт его вместо данных.
#[derive(Debug, Deserialize, JsonSchema)]
struct CalciumAnswer {
    /// One short sentence: which reference entry or which category, and why.
    reason: String,
    /// The name of the REFERENCE entry this food matches, copied exactly, or NONE.
    reference_key: String,
    /// The category row key from the table, copied exactly.
    category: String,
    /// Calcium per 100 g, in milligrams.
    calcium_mg: f64,
}

/// Запись справочника по ключу, который назвала модель.
fn calcium_reference_hit(key: &str) -> Option<&'static (&'static str, f64)> {
    let key = key.trim();
    if key.is_empty() || key_eq(key, "none") {
        return None;
    }
    CALCIUM_REFERENCE.iter().find(|(name, _)| key_eq(name, key))
}

/// Текст запроса о кальции. Отдельной функцией, чтобы замер брал ЕГО, а не копию:
/// рукописные копии расходились с кодом и мерили не то, что работает.
///
/// ПРАВИТЕ ФОРМУЛИРОВКИ — сперва `docs/food-identity.md`: там записано, почему
/// таблица показывается без чисел, а справочник отвечает ключом записи.
fn calcium_prompt(food_name: &str, known: &str) -> String {
    format!(
        "A person wrote this into their food diary: {food_name}\n\n{known}\
         How much CALCIUM does this food hold per 100 grams, in milligrams?\n\n\
         FIRST look for it in the REFERENCE below. If it is there — or is plainly the same food \
         under another name, in another grammatical case or with a fat percentage attached — \
         put that entry's name into \"reference_key\", copied exactly. An entry fits ONLY when \
         it is THE SAME FOOD: «мороженая вишня» is a frozen berry and not «мороженое», however \
         alike the words. When no entry is the same food, answer NONE — that is a good answer, \
         the table below then decides.\n\n\
         {reference}\n\n\
         The reference keys are written in RUSSIAN: copy the key letter for letter as it \
         stands there, never translated and never transliterated.\n\n\
         Whether or not you found it, ALSO place the food in one row of the table below and \
         answer with that row's key. If the reference had nothing, answer \"reference_key\" \
         with NONE and give a value that fits the row.\n\n\
         {table}\n\n\
         For raw or dry as-sold products (grains, seeds, legumes, flour) use the RAW value \
         unless the name says cooked, boiled or soaked. THIS DECIDES THE ENTRY TOO: for a name \
         that says nothing about cooking, «Овсянка» or «Гречка», the dry entry is the right \
         one — a boiled entry may be picked only when the name itself says boiled.\n\n\
         Rules for choosing the row:\n\
         - Cheese goes by HARDNESS: hard and dry → cheese_hard, semi-hard and brined → \
           cheese_semi, soft and fresh → cheese_soft, melted/processed → cheese_processed.\n\
         - A plant drink counts as fortified ONLY if the name says so («обогащённое», «с \
           кальцием», «fortified»). Otherwise it is plant_milk_plain, which has very little.\n\
         - Canned fish counts as fish_with_bones only when the bones are eaten (sardines, \
           sprats, canned salmon). Fillet without bones is other_none.\n\
         - The examples in a row are EXAMPLES, not the whole row. A food that is not named \
           there still belongs to the row it is closest to: скир goes with yogurts, мороженое \
           with cream, айран with milk, любой сыр — to the cheese row that matches its \
           hardness. Answer other_none ONLY when the food genuinely carries no calcium worth \
           counting — meat, eggs, cereals, bread, oil, sugar, water, tea, coffee. Never answer \
           other_none merely because the name is missing from the examples.\n\
         - Dairy is NEVER other_none: every milk product belongs to one of the dairy rows. \
           Neither is a VEGETABLE, and vegetables_other holds EVERY vegetable, root and tuber \
           there is — raw, boiled, fried or frozen, named in the examples or not. A boiled \
           potato belongs there just as a raw one does.\n\n\
         Fill \"reason\" FIRST, then the two keys, then the milligrams.\n\n\
         Respond with ONLY a minified JSON object and nothing else.",
        reference = calcium_reference_table(),
        table = calcium_category_keys(),
    )
}

/// Сколько кальция в продукте, мг на 100 г.
///
/// Форма запроса — общая с железом, и это не подражание, а вывод из замеров: строку
/// таблицы модель выбирает хорошо, а числа помнит плохо (13 из 30 до справочника,
/// 30 из 30 после). Порядок частей:
///
///   1. что человек записал в дневник и что об этом сказало ОПОЗНАНИЕ;
///   2. СПРАВОЧНИК «продукт → миллиграммы»: нашёлся — назови его ключ;
///   3. ТАБЛИЦА СТРОК — для всего, чего в справочнике нет;
///   4. правила выбора строки — они ловят промахи, которые видны в замерах;
///   5. один ответ.
///
/// `identity` — готовое опознание из конвейера признаков; пустая строка допустима
/// (продукт, у которого все признаки уже собраны, конвейер не гоняет).
pub async fn lookup_calcium(food_name: &str, identity: &str) -> Result<f64, String> {
    let known = if identity.trim().is_empty() {
        String::new()
    } else {
        format!("Our automatic classifier says this product is: {identity}\n\n")
    };
    let prompt = calcium_prompt(food_name, &known);

    // Единственное, что осталось проверкой: строка обязана существовать. Она хранится
    // и решает, куда продукт отнесён, — выдуманный ключ хранить нельзя.
    //
    // Всё остальное больше НЕ отвергается. Раньше выход за границы строки стоил
    // попытки: модель по три раза подряд предлагала мороженому 12–22 мг при коридоре
    // 50–150, и все три уходили впустую. Отвергать бессмысленно, когда подставить
    // нечего; теперь есть что — справочник.
    let v: CalciumAnswer = generate_validated(prompt, |_| {}, 3, |v: &CalciumAnswer| {
        if calcium_reference_hit(&v.reference_key).is_some() {
            return Ok(());
        }
        let key = v.category.trim().to_ascii_lowercase();
        if !CALCIUM_CATEGORIES.iter().any(|c| c.key == key) {
            return Err(format!("unknown calcium category «{}» for «{food_name}»", v.category));
        }
        Ok(())
    })
    .await?;
    // Величина из СПРАВОЧНИКА берётся как есть: строка описывает ГРУППУ, справочник —
    // этот продукт, и расходятся они законно (мороженое против сливок). Прочее
    // поджимается границами строки — грубому промаху так не пройти.
    let (mg, source) = match calcium_reference_hit(&v.reference_key) {
        Some((name, mg)) => (*mg, format!("справочник «{name}»")),
        None => {
            let key = v.category.trim().to_ascii_lowercase();
            let cat = CALCIUM_CATEGORIES
                .iter()
                .find(|c| c.key == key)
                .expect("строка проверена выше");
            (v.calcium_mg.clamp(cat.mg_min, cat.mg_max), format!("строка {}", cat.key))
        }
    };
    leptos::logging::log!("calcium «{food_name}»: {mg} мг [{source}] ({})", v.reason);
    Ok(mg)
}

// ── How well iron is absorbed, by food category ──────────────────────────────
//
// The model does NOT know these numbers. Measured directly: strip the figures from
// the prompt and it hands lentils and cottage cheese 0.20 — the same as beef, four
// to six times too high — because it loses the heme/non-heme distinction the whole
// mechanism rests on. So the table is OURS; the model's job is to place the food in
// a row of it and say the amount.
//
// One source of truth: the prompt table below is RENDERED from this array, and the
// model's answer is CHECKED against the same array. Ranges follow the standard
// picture — heme iron 15–35 %, non-heme 2–20 %, pushed down by phytates (legumes,
// whole grains, seeds), by calcium (dairy) and by tannins (tea, coffee), pushed up
// by vitamin C and by meat eaten alongside.
pub(crate) struct IronCategory {
    /// Stable key the model must echo back in `category`.
    pub(crate) key: &'static str,
    /// Доля усвоения — НАША величина, модель её не называет. Раньше спрашивали и её,
    /// но числа усвоения были единственными числами в промпте, и модель списывала их
    /// в поле миллиграммов: печень выходила 0.25 мг вместо ~9, чечевица 0.05 вместо
    /// ~7.5. Модель выбирает строку, доля берётся отсюда.
    pub(crate) absorption: f64,
    /// Правдоподобное СОДЕРЖАНИЕ железа, мг на 100 г. Границы намеренно широкие —
    /// внутри строки лежат очень разные продукты (в dairy и молоко 0.03, и сыр 0.7),
    /// поэтому диапазон отсекает грубую ошибку, а не уточняет значение.
    pub(crate) mg_min: f64,
    pub(crate) mg_max: f64,
    /// Russian examples — the input names are Russian, so they match better.
    pub(crate) examples: &'static str,
}

// ГРАНИЦЫ УЗКИЕ, И НИЖНЯЯ — РАБОЧЕЕ ЗНАЧЕНИЕ.
//
// Модель, которую жёстко инструктируют, свободы не проявляет: она исполняет правило
// буквально. Пока промпт запрещал отвечать границами, она брала середину; когда
// правилом стало «не знаешь — бери низ», низ пошёл и на знакомые продукты — куриная
// печень получила 3 мг вместо 9, говядина 0.8 вместо 2.6. Значит число задаём МЫ,
// а строка должна быть такой, чтобы её нижняя граница была настоящим значением
// типичного представителя, а не самого бедного продукта в куче.
//
// Границы — справочные значения на 100 г съедобной части (USDA / стандартные
// таблицы), сырой продукт: низ = самый бедный из ТИПИЧНЫХ представителей строки,
// верх = самый богатый.
/// СПРАВОЧНИК ЖЕЛЕЗА: продукт → миллиграммы на 100 г.
///
/// Заведён по той же причине, что и [`CALCIUM_REFERENCE`], и по тем же измерениям:
/// строку таблицы модель выбирает верно в 27 случаях из 30, а величину — лишь в 18.
/// Мидиям она давала 1.5 мг при справочных 6.7, петрушке 1.1 при 6.2, кунжуту 6.1
/// при 14.6; творогу и яблоку — втрое больше должного. Причём трижды подряд
/// предлагала значения ВНЕ диапазона собственной строки, и приложение их отвергало:
/// проверка коридором работает, но подставить ей нечего.
///
/// Третье число — ДОЛЯ УСВОЕНИЯ этого продукта. Модели она не показывается вовсе:
/// без таблицы та даёт чечевице и творогу те же 0.20, что и говядине, вчетверо
/// завышая усвоенное. Модель называет КЛЮЧ записи, а оба числа берём мы.
///
/// Для продуктов вне справочника доля по-прежнему берётся из строки
/// [`IRON_CATEGORIES`], которую выбирает отдельный шаг.
///
/// Значения типовые, на 100 г съедобной части; для круп и бобовых — в сухом виде,
/// если в названии не сказано «варёная». Пополняется по мере находок.
pub(crate) const IRON_REFERENCE: &[(&str, f64, f64)] = &[
    ("печень свиная", 18.0, 0.25),
    ("печень куриная", 9.0, 0.25),
    ("печень говяжья", 6.9, 0.25),
    ("печень индейки", 7.5, 0.25),
    ("печень трески", 1.9, 0.25),
    ("сердечки куриные", 5.9, 0.25),
    ("сердце говяжье", 4.3, 0.25),
    ("почки говяжьи", 5.9, 0.25),
    ("желудки куриные", 3.2, 0.25),
    ("язык говяжий", 4.1, 0.25),
    ("говядина", 2.6, 0.2),
    ("баранина", 1.9, 0.2),
    ("крольчатина", 1.3, 0.2),
    ("свинина", 0.9, 0.2),
    ("телятина", 1.1, 0.2),
    ("оленина", 3.4, 0.2),
    ("куриная грудка", 1.0, 0.15),
    ("куриное бедро", 1.3, 0.15),
    ("индейка филе", 1.4, 0.15),
    ("колбаса варёная", 1.7, 0.15),
    ("сосиски", 1.8, 0.15),
    ("салями", 1.5, 0.15),
    ("ветчина", 1.3, 0.15),
    ("устрицы", 9.2, 0.25),
    ("мидии", 6.7, 0.25),
    ("креветки", 1.8, 0.25),
    ("гребешки", 0.6, 0.25),
    ("кальмар", 0.7, 0.25),
    ("скумбрия", 1.6, 0.15),
    ("сельдь", 1.1, 0.15),
    ("лосось", 0.8, 0.15),
    ("тунец", 1.0, 0.15),
    ("треска", 0.4, 0.15),
    ("голец", 0.5, 0.15),
    ("икра красная", 1.8, 0.2),
    ("оливковое масло", 0.1, 0.08),
    ("подсолнечное масло", 0.1, 0.08),
    ("сливочное масло", 0.1, 0.02),
    ("сало свиное", 0.4, 0.15),
    ("сахар", 0.0, 0.08),
    ("мёд", 0.4, 0.08),
    ("кунжут", 14.6, 0.04),
    ("семена тыквы", 8.8, 0.04),
    ("чечевица сухая", 7.5, 0.05),
    ("фасоль сухая", 6.7, 0.05),
    ("нут сухой", 6.2, 0.05),
    ("соя", 15.7, 0.03),
    ("семена подсолнечника", 5.3, 0.04),
    ("кешью", 6.7, 0.04),
    ("фисташки", 4.2, 0.04),
    ("миндаль", 3.7, 0.04),
    ("грецкий орех", 2.9, 0.04),
    ("арахис", 4.6, 0.04),
    ("чечевица варёная", 3.3, 0.05),
    ("фасоль варёная", 2.9, 0.05),
    ("тофу", 5.4, 0.03),
    ("гречка сухая", 6.7, 0.04),
    ("перловка сухая", 2.5, 0.04),
    ("пшено сухое", 2.7, 0.04),
    ("булгур сухой", 2.5, 0.04),
    ("киноа сухая", 4.6, 0.04),
    ("полба сухая", 4.4, 0.04),
    ("манка сухая", 1.0, 0.08),
    ("рис белый сухой", 0.8, 0.08),
    ("макароны сухие", 1.3, 0.08),
    ("гречка варёная", 1.5, 0.04),
    ("овсянка сухая", 4.3, 0.04),
    ("овсянка на воде", 1.7, 0.04),
    ("рис белый варёный", 0.5, 0.08),
    ("хлеб ржаной", 3.9, 0.04),
    ("хлеб пшеничный", 1.5, 0.08),
    ("макароны варёные", 1.3, 0.08),
    ("петрушка", 6.2, 0.08),
    ("укроп", 6.6, 0.08),
    ("шпинат", 2.7, 0.02),
    ("руккола", 1.5, 0.08),
    ("брокколи", 0.7, 0.08),
    ("капуста белокочанная", 0.6, 0.08),
    ("картофель", 0.9, 0.08),
    ("морковь", 0.7, 0.08),
    ("яблоко", 0.1, 0.1),
    ("банан", 0.3, 0.1),
    ("курага", 3.2, 0.1),
    ("изюм", 1.9, 0.1),
    ("чернослив", 0.9, 0.1),
    ("яйцо куриное", 1.7, 0.04),
    ("творог", 0.4, 0.02),
    ("молоко", 0.1, 0.02),
    ("сыр твёрдый", 0.7, 0.02),
    ("шоколад тёмный", 11.9, 0.02),
    ("какао порошок", 13.9, 0.02),
];

/// Справочник железа как текст промпта — из того же массива, что и сам справочник.
pub(crate) fn iron_reference_table() -> String {
    IRON_REFERENCE
        .iter()
        .map(|(name, mg, _absorption)| format!("  {name}: {mg}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) const IRON_CATEGORIES: &[IronCategory] = &[
    // ── Гем: усваивается лучше всего ──
    // Печень говяжья 6.5 · куриная 9 · свиная 18; сердце 4.3, почки 4.6.
    IronCategory { key: "liver_offal", absorption: 0.25, mg_min: 5.0, mg_max: 18.0,
        examples: "печень куриная, печень говяжья, печень свиная, сердце, почки" },
    // Кальмар 0.7 · креветки 1.8 · мидии 6.7 · устрицы до 28 (крайность отброшена).
    IronCategory { key: "shellfish", absorption: 0.25, mg_min: 1.5, mg_max: 8.0,
        examples: "мидии, устрицы, гребешки, кальмар, креветки" },
    // Икра красная 1.8 · чёрная 2.4.
    IronCategory { key: "roe", absorption: 0.20, mg_min: 1.5, mg_max: 3.0,
        examples: "икра красная, икра чёрная" },
    // Свинина 0.9 · кролик 1.3 · баранина 1.9 · говядина 2.6 · телятина 1.1.
    IronCategory { key: "meat_red", absorption: 0.20, mg_min: 1.2, mg_max: 3.0,
        examples: "говядина, телятина, баранина, свинина, кролик" },
    // Куриная грудка 0.7 · бедро 1.3 · индейка 1.4 · утка 2.7.
    IronCategory { key: "meat_poultry", absorption: 0.15, mg_min: 0.7, mg_max: 2.5,
        examples: "курица, индейка, утка, куриная грудка" },
    // Треска 0.4 · лосось 0.8 · тунец 1.0 · сельдь 1.1 · скумбрия 1.6.
    // Примеров нарочно много и разных: «голец» модель отнесла к говядине с
    // обоснованием «Голец — это говядина», и рыба получила усвоение красного мяса.
    IronCategory { key: "fish", absorption: 0.15, mg_min: 0.4, mg_max: 1.8,
        examples: "лосось, треска, тунец, скумбрия, сельдь, голец, форель, горбуша, кета, \
                   минтай, судак, щука, палтус, камбала — ЛЮБАЯ рыба" },
    // Колбаса/ветчина/сосиски ~1.0 · бекон 1.4 · паштет ближе к печени.
    IronCategory { key: "meat_processed", absorption: 0.15, mg_min: 0.9, mg_max: 2.5,
        examples: "колбаса, сосиски, ветчина, бекон, паштет" },
    IronCategory { key: "dish_with_meat", absorption: 0.12, mg_min: 0.8, mg_max: 2.5,
        examples: "плов с мясом, борщ с говядиной, паста болоньезе, пельмени" },
    // ── Негем: усваивается хуже, тем сильнее чем больше фитатов/кальция/танинов ──
    // Апельсин 0.1 · яблоко 0.12 · арбуз 0.24 · банан 0.26 · черешня 0.36 · клубника 0.4.
    IronCategory { key: "fruit_fresh", absorption: 0.10, mg_min: 0.1, mg_max: 0.5,
        examples: "яблоко, апельсин, клубника, киви, черешня, арбуз, банан, виноград" },
    // Чернослив 0.9 · финики 1.0 · изюм 1.9 · инжир 2.0 · курага 2.7.
    IronCategory { key: "fruit_dried", absorption: 0.10, mg_min: 0.9, mg_max: 2.8,
        examples: "курага, изюм, чернослив, инжир сушёный, финики" },
    // Лук 0.2 · помидор/огурец/морковь 0.3 · перец 0.4 · капуста 0.5 · брокколи 0.7 ·
    // картофель 0.8.
    IronCategory { key: "vegetables", absorption: 0.08, mg_min: 0.2, mg_max: 0.9,
        examples: "болгарский перец, помидор, огурец, брокколи, капуста, морковь, кабачок, \
                   лук, картофель" },
    // Зелёный лук 1.0 · руккола 1.5 · кинза 1.8 · базилик 3.2 · петрушка 6.2 · укроп 6.6.
    IronCategory { key: "greens_herbs", absorption: 0.08, mg_min: 1.0, mg_max: 6.5,
        examples: "петрушка, укроп, руккола, базилик, кинза, зелёный лук" },
    // Белый рис 0.8 · манка 1.0 · макароны 1.3 · белый хлеб до 2.
    IronCategory { key: "grains_refined", absorption: 0.08, mg_min: 0.8, mg_max: 2.0,
        examples: "белый хлеб, белый рис, макароны, манка" },
    IronCategory { key: "fortified", absorption: 0.05, mg_min: 3.0, mg_max: 20.0,
        examples: "хлопья с добавленным железом, каши быстрого приготовления, детские смеси" },
    IronCategory { key: "dish_meatless", absorption: 0.05, mg_min: 0.3, mg_max: 1.5,
        examples: "овощное рагу, вегетарианский суп, каша на воде" },
    // СУХИЕ: фасоль 5.9 · нут 6.2 · маш 6.7 · горох 6.8 · чечевица 7.5.
    IronCategory { key: "legumes", absorption: 0.05, mg_min: 5.0, mg_max: 8.0,
        examples: "фасоль, нут, чечевица, горох, маш (в сухом виде)" },
    // Бурый рис 1.5 · гречка 2.2 · булгур 2.5 · овсянка 4.7.
    IronCategory { key: "grains_whole", absorption: 0.04, mg_min: 1.5, mg_max: 5.0,
        examples: "гречка, овсянка, бурый рис, цельнозерновой хлеб, булгур" },
    // Миндаль 3.7 · фундук 4.7 · кешью 6.7 · тыквенные семечки 8.8 · кунжут 14.6.
    IronCategory { key: "nuts_seeds", absorption: 0.04, mg_min: 3.5, mg_max: 15.0,
        examples: "кунжут, кешью, миндаль, тыквенные семечки, фундук" },
    // Яйцо целиком 1.75.
    IronCategory { key: "eggs", absorption: 0.04, mg_min: 1.2, mg_max: 2.0,
        examples: "яйцо куриное, омлет, яичница" },
    // Соевое молоко 0.4 · тофу 2.7 · соевое мясо ~5.
    IronCategory { key: "soy", absorption: 0.03, mg_min: 0.4, mg_max: 5.0,
        examples: "тофу, соевое молоко, соевое мясо, эдамаме" },
    // Ревень 0.2 · свёкла 0.8 · щавель 2.4 · шпинат 2.7.
    IronCategory { key: "spinach_oxalate", absorption: 0.02, mg_min: 0.8, mg_max: 3.0,
        examples: "шпинат, щавель, свёкла, ревень" },
    // Молоко 0.03 · кефир/йогурт 0.1 · сыр 0.2–0.7 · творог 0.4.
    IronCategory { key: "dairy", absorption: 0.02, mg_min: 0.05, mg_max: 0.7,
        examples: "молоко, творог, сыр, йогурт, кефир, ряженка, ацидофилин, сливки" },
    // Отдельная строка для напитков: «пиво безалкогольное» уходило в dairy — просто
    // потому, что подходящей строки не было, а выбрать модель обязана.
    IronCategory { key: "drinks", absorption: 0.02, mg_min: 0.0, mg_max: 0.1,
        examples: "вода, сок, морс, лимонад, пиво, безалкогольное пиво, вино, компот, квас" },
    // Заваренный чай/кофе ~0.02 · шоколад 2.4 · какао-порошок 13.9 (сухой, редко ест
    // 100 г — верх намеренно ниже).
    IronCategory { key: "tea_coffee_cocoa", absorption: 0.02, mg_min: 0.05, mg_max: 3.0,
        examples: "чай, кофе, какао, шоколад" },
];

// ── Профиль жира ─────────────────────────────────────────────────────────────
//
// Спрашиваются ДОЛИ от жира продукта, а не миллиграммы на 100 г: `Food::fat` у нас
// уже есть, граммы кислот считаются умножением, и профиль не плывёт от разбавления
// и уварки (см. `api_types::FatProfile`).
//
// Строка таблицы — один источник правды: из этого массива И рендерится промпт, И
// берутся границы, в которые зажимается ответ. Замер (scripts/measure-fatty-acids.sh)
// показал, что БЕЗ диапазонов промпт негоден — словесные подсказки модель применяет
// непоследовательно: грецкий орех уходил в мононенасыщенные, чечевица давала разброс
// ПНЖК от 17 до 60. С диапазонами: 72 базовых продукта, ни одного промаха по строке.

pub(crate) struct FatRow {
    /// Ключ, который модель обязана вернуть дословно.
    pub(crate) key: &'static str,
    /// Границы долей: НЖК, МНЖК, ПНЖК, АЛК, EPA+DHA — в процентах от общего жира.
    pub(crate) sfa: (f64, f64),
    pub(crate) mufa: (f64, f64),
    pub(crate) pufa: (f64, f64),
    pub(crate) epa_dha: (f64, f64),
    /// Примеры — по ним модель узнаёт продукт; они важнее диапазона.
    pub(crate) examples: &'static str,
}

/// Строки — по ТИПУ ЖИРА, а не по типу блюда: профиль задаёт источник жира.
pub(crate) const FAT_ROWS: &[FatRow] = &[
    FatRow { key: "coconut_palm", sfa: (82.0, 92.0), mufa: (5.0, 12.0), pufa: (1.0, 3.0),
        epa_dha: (0.0, 0.0),
        examples: "кокосовое масло, пальмовое масло, кокосовая стружка, кокосовое молоко, \
                   пальмоядровый жир" },
    FatRow { key: "dairy_fat", sfa: (60.0, 70.0), mufa: (22.0, 30.0), pufa: (2.0, 6.0),
        epa_dha: (0.0, 0.0),
        examples: "сливочное масло, топлёное масло, сливки, сметана, сыр, молоко, творог, \
                   мороженое, кефир" },
    FatRow { key: "beef_lamb_fat", sfa: (38.0, 48.0), mufa: (40.0, 50.0), pufa: (2.0, 6.0),
        epa_dha: (0.0, 0.0),
        examples: "говядина, телятина, баранина, курдючное сало, говяжий жир, \
                   субпродукты жвачных" },
    FatRow { key: "pork_fat", sfa: (33.0, 40.0), mufa: (42.0, 50.0), pufa: (8.0, 14.0),
        epa_dha: (0.0, 0.0),
        examples: "свинина, сало, шпик, бекон, ветчина, колбаса, сосиски, свиные субпродукты" },
    FatRow { key: "poultry_fat", sfa: (26.0, 32.0), mufa: (40.0, 48.0), pufa: (18.0, 26.0),
        epa_dha: (0.0, 1.0),
        examples: "курица, индейка, утка, гусь, куриная кожа, куриный жир, куриная печень" },
    FatRow { key: "egg_fat", sfa: (28.0, 34.0), mufa: (40.0, 48.0), pufa: (12.0, 18.0),
        epa_dha: (0.0, 2.0),
        examples: "яйцо, яичный желток, омлет, яичница" },
    FatRow { key: "olive_avocado", sfa: (12.0, 18.0), mufa: (65.0, 75.0), pufa: (10.0, 16.0),
        epa_dha: (0.0, 0.0),
        examples: "оливковое масло, оливки, маслины, авокадо, масло авокадо" },
    FatRow { key: "rapeseed_mustard", sfa: (6.0, 9.0), mufa: (58.0, 66.0), pufa: (26.0, 32.0),
        epa_dha: (0.0, 0.0),
        examples: "рапсовое масло, каноловое масло, горчичное масло, семена горчицы" },
    FatRow { key: "peanut", sfa: (15.0, 20.0), mufa: (45.0, 55.0), pufa: (25.0, 35.0),
        epa_dha: (0.0, 0.0),
        examples: "арахис, арахисовое масло, арахисовая паста" },
    FatRow { key: "seed_oils_linoleic", sfa: (10.0, 15.0), mufa: (18.0, 28.0), pufa: (55.0, 68.0),
        epa_dha: (0.0, 0.0),
        examples: "подсолнечное масло, кукурузное масло, соевое масло, масло виноградных \
                   семечек, семечки подсолнуха, тыквенные семечки, кунжут, тахини, майонез, \
                   маргарин" },
    FatRow { key: "high_oleic_oils", sfa: (7.0, 12.0), mufa: (75.0, 85.0), pufa: (5.0, 12.0),
        epa_dha: (0.0, 0.0),
        examples: "высокоолеиновое подсолнечное, высокоолеиновое сафлоровое, \
                   олеиновый подсолнечник" },
    FatRow { key: "flax_chia", sfa: (8.0, 11.0), mufa: (15.0, 22.0), pufa: (66.0, 75.0),
        epa_dha: (0.0, 0.0),
        examples: "льняное масло, семена льна, семена чиа, рыжиковое масло" },
    FatRow { key: "walnut_hemp", sfa: (9.0, 12.0), mufa: (13.0, 22.0), pufa: (65.0, 75.0),
        epa_dha: (0.0, 0.0),
        examples: "грецкий орех, конопляное семя, масло грецкого ореха" },
    // Диапазоны сверены со справочными долями трёх рыб этой строки (доли от
    // собственного жира): кижуч дикий 21/36/34/18, сельдь атлантическая 23/41/24/17,
    // лосось атлантический фермерский 23/28/29/15. Прежние ПНЖК 20–30 обрезали
    // верх: модель брала середину 25 и занижала кижуча на треть (настоящие 34).
    // Строки узкие намеренно — широкая даёт систематический промах в середину.
    FatRow { key: "fish_fatty_cold", sfa: (20.0, 25.0), mufa: (28.0, 42.0), pufa: (24.0, 34.0),
        epa_dha: (13.0, 20.0),
        examples: "скумбрия, сельдь, лосось, форель, сардина, шпроты, анчоус, икра, рыбий жир, \
                   печень трески, голец, кета, чавыча, нерка, горбуша, сайра, мойва, палтус" },
    FatRow { key: "fish_lean_seafood", sfa: (18.0, 25.0), mufa: (12.0, 20.0), pufa: (30.0, 45.0),
        epa_dha: (25.0, 40.0),
        examples: "треска, минтай, навага, судак, тунец, креветки, мидии, кальмар, гребешок, \
                   крабовые палочки" },
    FatRow { key: "fish_warm_low_n3", sfa: (25.0, 35.0), mufa: (25.0, 40.0), pufa: (20.0, 30.0),
        epa_dha: (3.0, 8.0),
        examples: "тилапия, пангасиус, баса, дорадо, сибас, карп, толстолобик, сом — \
                   тепловодная и прудовая рыба" },
    FatRow { key: "nuts_tree", sfa: (7.0, 14.0), mufa: (55.0, 70.0), pufa: (15.0, 30.0),
        epa_dha: (0.0, 0.0),
        examples: "миндаль, фундук, кешью, фисташки, кедровый орех, бразильский орех, макадамия" },
    FatRow { key: "cocoa_confectionery", sfa: (55.0, 65.0), mufa: (28.0, 35.0), pufa: (2.0, 5.0),
        epa_dha: (0.0, 0.0),
        examples: "шоколад, какао, какао-масло, конфеты, вафли, печенье, торт, кондитерский жир" },
    FatRow { key: "grain_legume", sfa: (15.0, 22.0), mufa: (12.0, 25.0), pufa: (45.0, 60.0),
        epa_dha: (0.0, 0.0),
        examples: "крупы, хлеб, макароны, рис, овсянка, бобовые, чечевица, нут, соя, тофу" },
    FatRow { key: "no_fat", sfa: (0.0, 0.0), mufa: (0.0, 0.0), pufa: (0.0, 0.0),
        epa_dha: (0.0, 0.0),
        examples: "овощи, фрукты, зелень, сахар, мёд, вода, чай, кофе, алкоголь, желатин, \
                   специи — всё, где жира практически нет" },
];

/// СПРАВОЧНИК профилей жира: продукт → доли от его собственного жира, в процентах.
/// Пятое число — EPA+DHA, часть ПНЖК.
///
/// Строка таблицы описывает ГРУППУ, и внутри группы разброс велик: у `nuts_tree`
/// коридор МНЖК 55–70 при макадамии 79 и кедровом орехе 28, а строка `coconut_palm`
/// требует 82–92 % насыщенных — верно для кокосового масла, но не для пальмового,
/// где их около половины. Строка такому продукту не поможет, справочник поможет.
///
/// Числа — типовые справочные доли; три рыбы (кижуч, сельдь, лосось) сверены при
/// настройке строки `fish_fatty_cold`.
pub(crate) const FAT_REFERENCE: &[(&str, f64, f64, f64, f64)] = &[
    // ── Масла и выделенные жиры ──
    ("оливковое масло", 14.0, 73.0, 11.0, 0.0),
    ("подсолнечное масло", 11.0, 20.0, 66.0, 0.0),
    ("высокоолеиновое подсолнечное масло", 9.0, 83.0, 4.0, 0.0),
    ("кукурузное масло", 13.0, 28.0, 55.0, 0.0),
    ("соевое масло", 15.0, 23.0, 58.0, 0.0),
    ("рапсовое масло", 7.0, 63.0, 28.0, 0.0),
    ("льняное масло", 9.0, 18.0, 68.0, 0.0),
    ("кунжутное масло", 14.0, 40.0, 42.0, 0.0),
    ("арахисовое масло", 17.0, 46.0, 32.0, 0.0),
    ("кокосовое масло", 87.0, 6.0, 2.0, 0.0),
    ("пальмовое масло", 49.0, 37.0, 9.0, 0.0),
    ("сливочное масло", 63.0, 26.0, 4.0, 0.0),
    ("топлёное масло", 65.0, 25.0, 4.0, 0.0),
    ("сало свиное", 39.0, 45.0, 11.0, 0.0),
    ("говяжий жир", 42.0, 44.0, 4.0, 0.0),
    ("куриный жир", 30.0, 45.0, 21.0, 0.0),
    // ── Орехи, семена, жирные плоды ──
    ("грецкий орех", 9.0, 14.0, 72.0, 0.0),
    ("миндаль", 8.0, 65.0, 25.0, 0.0),
    ("фундук", 8.0, 78.0, 10.0, 0.0),
    ("кешью", 20.0, 59.0, 17.0, 0.0),
    ("фисташки", 13.0, 54.0, 31.0, 0.0),
    ("кедровый орех", 7.0, 28.0, 62.0, 0.0),
    ("бразильский орех", 24.0, 37.0, 36.0, 0.0),
    ("макадамия", 16.0, 79.0, 2.0, 0.0),
    ("арахис", 17.0, 49.0, 31.0, 0.0),
    ("семена подсолнечника", 11.0, 19.0, 66.0, 0.0),
    ("семена тыквы", 19.0, 34.0, 45.0, 0.0),
    ("кунжут", 15.0, 40.0, 42.0, 0.0),
    ("семена чиа", 11.0, 8.0, 78.0, 0.0),
    ("семена льна", 9.0, 19.0, 68.0, 0.0),
    ("авокадо", 15.0, 68.0, 13.0, 0.0),
    ("оливки", 13.0, 74.0, 9.0, 0.0),
    // ── Мясо, птица, яйцо ──
    ("говядина", 40.0, 45.0, 4.0, 0.0),
    ("баранина", 45.0, 42.0, 6.0, 0.0),
    ("свинина", 37.0, 45.0, 12.0, 0.0),
    ("курица", 29.0, 44.0, 21.0, 0.0),
    ("индейка", 30.0, 40.0, 25.0, 0.0),
    ("утка", 33.0, 49.0, 13.0, 0.0),
    ("яйцо куриное", 31.0, 44.0, 15.0, 1.0),
    // ── Рыба и морепродукты ──
    ("кижуч", 21.0, 36.0, 34.0, 18.0),
    ("сельдь", 23.0, 41.0, 24.0, 17.0),
    ("лосось", 23.0, 28.0, 29.0, 15.0),
    ("скумбрия", 24.0, 40.0, 24.0, 18.0),
    ("печень трески", 18.0, 50.0, 25.0, 20.0),
    ("треска", 21.0, 15.0, 47.0, 40.0),
    ("тунец", 26.0, 26.0, 30.0, 25.0),
    ("креветки", 25.0, 17.0, 40.0, 35.0),
    ("мидии", 22.0, 22.0, 35.0, 28.0),
    ("тилапия", 35.0, 33.0, 24.0, 5.0),
    ("пангасиус", 38.0, 40.0, 17.0, 2.0),
    // ── Молочное и кондитерское ──
    ("молоко", 63.0, 26.0, 4.0, 0.0),
    ("творог", 63.0, 26.0, 4.0, 0.0),
    ("сыр твёрдый", 63.0, 26.0, 4.0, 0.0),
    ("сметана", 63.0, 26.0, 4.0, 0.0),
    ("шоколад тёмный", 60.0, 33.0, 3.0, 0.0),
    ("какао порошок", 60.0, 33.0, 3.0, 0.0),
];

/// Справочник в том виде, в каком его видит модель.
pub(crate) fn fat_reference_table() -> String {
    FAT_REFERENCE
        .iter()
        .map(|(name, s, m, p, e)| {
            format!("  {name}: SFA {s:.0}, MUFA {m:.0}, PUFA {p:.0}, EPA+DHA {e:.0}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Запись справочника по ключу, который назвала модель.
fn fat_reference_hit(key: &str) -> Option<&'static (&'static str, f64, f64, f64, f64)> {
    let key = key.trim();
    if key.is_empty() || key_eq(key, "none") {
        return None;
    }
    FAT_REFERENCE.iter().find(|(name, ..)| key_eq(name, key))
}

fn fat_row_table() -> String {
    FAT_ROWS
        .iter()
        .map(|r| {
            format!(
                "  {key:<20} SFA {s0}–{s1}, MUFA {m0}–{m1}, PUFA {p0}–{p1}, \
                 EPA+DHA {e0}–{e1} — {ex}",
                key = r.key,
                s0 = r.sfa.0, s1 = r.sfa.1, m0 = r.mufa.0, m1 = r.mufa.1,
                p0 = r.pufa.0, p1 = r.pufa.1,
                e0 = r.epa_dha.0, e1 = r.epa_dha.1, ex = r.examples,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// NB: `///` на полях уезжает В ПРОМПТ (схема вставляется текстом) — писать их как
// инструкцию модели. Пояснения для людей — `//`, как это.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FatProfileAnswer {
    /// The name of the REFERENCE entry this food matches, copied exactly, or NONE.
    pub(crate) reference_key: String,
    /// The category key from the table, copied EXACTLY as written there.
    pub(crate) category: String,
    /// What this product actually is, in two or three words.
    pub(crate) food_type: String,
    /// SATURATED fatty acids as a PERCENT OF THIS FOOD'S TOTAL FAT, by weight. Not
    /// per 100 g of food.
    pub(crate) sfa_pct: f64,
    /// MONOUNSATURATED fatty acids as a PERCENT OF THIS FOOD'S TOTAL FAT, by weight.
    pub(crate) mufa_pct: f64,
    /// POLYUNSATURATED fatty acids as a PERCENT OF THIS FOOD'S TOTAL FAT, by weight.
    pub(crate) pufa_pct: f64,
    /// EPA plus DHA together as a PERCENT OF THIS FOOD'S TOTAL FAT. Part of
    /// `pufa_pct`, so it can never exceed it. Zero for everything except fish,
    /// seafood and fish oil.
    pub(crate) epa_dha_pct: f64,
    /// One short sentence: why this row and this profile. Keep it under 20 words.
    pub(crate) reason: String,
}

// ── Развилка: базовый продукт или составное блюдо ────────────────────────────
//
// Таблица строк разложена по ИСТОЧНИКУ жира и годится только там, где источник
// один. У составного блюда его нет: в фаршированном баклажане жир из фарша, из
// масла и из самого овоща, и ни одна строка ему не подходит. Модель это честно
// показала — замер (scripts/measure-fat-dishes.mjs) дал 36 из 36 попыток с
// выдуманным ключом «vegetable» на фаршированных овощах, а голубцам, котлетам и
// картофельной запеканке она с той же уверенностью назначала свинину, потому что
// выбрать было больше не из чего.
//
// Поэтому сперва спрашивается ОДНО: базовый это продукт или блюдо. Базовый идёт в
// таблицу, блюдо — в свой запрос без таблицы, где выдумать ключ попросту негде.

/// Обоснование первым — оно и есть размышление модели (см. раздел про признаки).
#[derive(Debug, Deserialize, JsonSchema)]
struct CompositeAnswer {
    /// Which ingredients this food is made of, and where its fat comes from. One
    /// short sentence.
    reason_where_the_fat_of_this_food_comes_from: String,
    /// True when the fat comes from SEVERAL different sources mixed together.
    verdict_is_a_composite_dish: bool,
}

/// Шапка всех трёх запросов о жире: что человек записал и что об этом известно.
fn fat_head(food_name: &str, identity: &str) -> String {
    let known = if identity.trim().is_empty() {
        String::new()
    } else {
        format!("Our automatic classifier says this product is: {identity}\n\n")
    };
    format!("A person wrote this into their food diary: {food_name}\n\n{known}")
}

/// Текст развилки «продукт или блюдо» — отдельной функцией ради замеров.
fn composite_prompt(food_name: &str, head: &str) -> String {
    format!(
        "{head}\
         Is this a DISH COOKED FROM SEVERAL DIFFERENT FOODS, or is it a SINGLE food?\n\n\
         A SINGLE FOOD is one food, however processed. These count as one:\n\
         — the flesh, organs, fat or skin of one animal, bird or fish, raw or cooked;\n\
         — one plant and what is pressed or ground from it: an oil, a nut, a seed, a grain, a \
         vegetable, a fruit;\n\
         — milk and everything made from milk alone: butter, cream, cheese, curd, yoghurt;\n\
         — eggs;\n\
         — a factory product built around one of these: sausage, ham, tinned fish, nut paste.\n\n\
         A DISH is cooked from several foods at once: stuffed vegetables, cabbage rolls, \
         cutlets and meatballs, casseroles and bakes, pies and pastry with a filling, pilaf, \
         stews and soups, salads with dressing, sandwiches, waffles, ready meals.\n\n\
         A dish stays a dish even when one of its foods plainly outweighs the rest: борщ with \
         beef is cooked from meat, vegetables and the oil they were fried in, and that makes \
         it a dish. Do not ask which part is the largest — ask whether more than one food went \
         into it.\n\n\
         Fill the reason FIRST — name what this food is made of — and let the verdict follow \
         from it.\n\n\
         Respond with ONLY a single minified JSON object.",
        head = head,
    )
}

/// Составное ли это блюдо (жир из нескольких источников), а не базовый продукт.
pub async fn is_composite_dish(food_name: &str, identity: &str) -> Result<bool, String> {
    // ВОПРОС — О ПРОДУКТЕ, А НЕ О ЖИРЕ, и это вывод из замера. Спрошенная «does the
    // FAT come from several sources», модель трижды из трёх отвечала про борщ «нет,
    // жир от говядины» — она понимала вопрос как «какой источник преобладает». Про
    // сам продукт — «сварено ли это из нескольких продуктов» — ответ однозначен.
    let prompt = composite_prompt(food_name, &fat_head(food_name, identity));
    let v: CompositeAnswer = generate(prompt, |_| {}).await?;
    leptos::logging::log!(
        "составное «{food_name}»: {} — {}",
        v.verdict_is_a_composite_dish,
        v.reason_where_the_fat_of_this_food_comes_from
    );
    crate::services::telemetry::report_detection(
        "dish.composite",
        food_name,
        &v.verdict_is_a_composite_dish.to_string(),
        &v.reason_where_the_fat_of_this_food_comes_from,
        &[],
    );
    Ok(v.verdict_is_a_composite_dish)
}

/// Профиль жира продукта: развилка, а дальше — своя ветка для каждого случая.
///
/// `identity` — готовое опознание из конвейера признаков; пустая строка допустима.
pub async fn lookup_fat_profile(
    food_name: &str,
    identity: &str,
) -> Result<api_types::FatProfile, String> {
    if is_composite_dish(food_name, identity).await? {
        return lookup_dish_fat_profile(food_name, identity).await;
    }
    lookup_basic_fat_profile(food_name, identity).await
}

/// Текст запроса о профиле жира базового продукта — отдельной функцией ради замеров.
fn basic_fat_prompt(food_name: &str, head: &str, lang: &str) -> String {
    let _ = food_name;
    format!(
        "{head}\
         Report the FATTY-ACID PROFILE OF THIS FOOD'S FAT.\n\n\
         Every number you report is a PERCENT OF THIS FOOD'S TOTAL FAT, by weight — NOT grams \
         per 100 g of food. A food's total fat is already known to us; you only describe how \
         that fat is composed. This means the profile does NOT depend on how much fat the food \
         has: pure sunflower oil and a sauce made from it share the same profile.\n\n\
         FIRST look for the food in the REFERENCE below. If it is there — or is plainly the \
         same food under another name, in another grammatical case or with a cut, grade or fat \
         percentage attached — put that entry's name into \"reference_key\", copied exactly.\n\n\
         {reference}\n\n\
         The reference keys are written in RUSSIAN: copy the key letter for letter as it \
         stands there, never translated and never transliterated.\n\n\
         Whether or not you found it, ALSO place the food in one row of the table below — rows \
         are by the TYPE OF FAT, not by the type of dish — and answer with that row's key. If \
         the reference had nothing, the values must fit that row's ranges.\n\n\
         {table}\n\n\
         Report:\n\
         - reference_key: the reference entry's name, copied exactly, or NONE\n\
         - category: the row key, copied EXACTLY as written above\n\
         - food_type: what this product is, in two or three words, in {lang}\n\
         - sfa_pct / mufa_pct / pufa_pct: percent of total fat, each INSIDE its range in the \
           chosen row. The remaining weight is glycerol, so the three need not add up to 100 — \
           never inflate a fraction to make them.\n\
         - epa_dha_pct: EPA plus DHA together, percent of total fat, inside the row's range. \
           Also part of pufa_pct. Zero for everything that is not fish, seafood or fish oil — \
           plants contain NO EPA or DHA, however rich in omega-3 they are.\n\
         - reason: ONE short sentence (under 20 words) in {lang} — why this row and this \
           profile\n\n\
         Rules for choosing the row:\n\
         - Go by WHERE THE FAT COMES FROM. A dish fried in sunflower oil whose own fat is mostly \
           that oil belongs to the oil's row, not to the row of the vegetable.\n\
         - A mixed dish goes to the row of its DOMINANT fat source.\n\
         - Cooking, freezing, canning and packaging do not change the profile. The row does not \
           depend on them.\n\
         - A plain name goes to the ORDINARY variety of that oil. Only an explicit \
           «высокоолеиновое» / «high-oleic» in the name moves sunflower or safflower oil to \
           high_oleic_oils.\n\
         - Fish splits by WHERE IT LIVES, not by how fatty it is: cold-water sea fish carry EPA \
           and DHA, while warm-water and farmed pond fish carry mostly linoleic acid and very \
           little n-3. A fish you do not recognise as a cold-water sea species goes to \
           fish_warm_low_n3.\n\
         - Anything with practically no fat goes to no_fat; its profile is irrelevant, report \
           the row and zeros.\n\n\
         Fill \"reason\" FIRST, then the two keys, then the numbers.\n\n\
         Respond with ONLY a single minified JSON object and nothing else — no markdown, no \
         prose.",
        head = head,
        reference = fat_reference_table(),
        table = fat_row_table(),
        lang = lang,
    )
}

/// Профиль жира БАЗОВОГО продукта: сперва справочник, потом строка таблицы.
pub async fn lookup_basic_fat_profile(
    food_name: &str,
    identity: &str,
) -> Result<api_types::FatProfile, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = basic_fat_prompt(food_name, &fat_head(food_name, identity), lang);

    let v: FatProfileAnswer = generate_validated(prompt, |_| {}, 4, |v: &FatProfileAnswer| {
        // Запись справочника снимает вопрос о строке: числа всё равно возьмутся оттуда.
        if fat_reference_hit(&v.reference_key).is_some() {
            return Ok(());
        }
        let key = v.category.trim().to_ascii_lowercase();
        if !FAT_ROWS.iter().any(|r| r.key == key) {
            return Err(format!("unknown fat row «{}» for «{food_name}»", v.category));
        }
        // Больше ста процентов жира не бывает. Нижнего порога у суммы НЕТ: у постной
        // рыбы справочники дают около семидесяти (остальное — фосфолипиды), а у
        // безжирного продукта честный ответ — нули.
        let sum = v.sfa_pct + v.mufa_pct + v.pufa_pct;
        if sum > 100.5 {
            return Err(format!("fat fractions sum to {sum:.1}% for «{food_name}»"));
        }
        Ok(())
    })
    .await?;

    // Профиль из СПРАВОЧНИКА берётся как есть: строка описывает группу, справочник —
    // этот продукт, и расходятся они законно. Пальмовое масло стоит в строке кокоса,
    // где насыщенных 82–92, а у него их около половины — зажим строкой испортил бы
    // верное число.
    let (profile, source) = match fat_reference_hit(&v.reference_key) {
        Some((name, s, m, p, e)) => (
            api_types::FatProfile {
                sfa_pct: *s,
                mufa_pct: *m,
                pufa_pct: *p,
                epa_dha_pct: e.min(*p),
            },
            format!("справочник «{name}»"),
        ),
        None => {
            let key = v.category.trim().to_ascii_lowercase();
            let row = FAT_ROWS
                .iter()
                .find(|r| r.key == key)
                .ok_or_else(|| format!("unknown fat row «{}»", v.category))?;
            let clamp = |x: f64, (lo, hi): (f64, f64)| x.clamp(lo, hi);
            let pufa = clamp(v.pufa_pct, row.pufa);
            // EPA+DHA — ЧАСТЬ ПНЖК, поэтому зажимается ещё и под неё: замер поймал
            // ответы, где часть превышала целое.
            let epa_dha = clamp(v.epa_dha_pct, row.epa_dha).min(pufa);
            (
                api_types::FatProfile {
                    sfa_pct: clamp(v.sfa_pct, row.sfa),
                    mufa_pct: clamp(v.mufa_pct, row.mufa),
                    pufa_pct: pufa,
                    epa_dha_pct: epa_dha,
                },
                format!("строка {}", row.key),
            )
        }
    };
    leptos::logging::log!(
        "жиры «{food_name}»: {source} · {} — НЖК {:.0} МНЖК {:.0} ПНЖК {:.0} EPA+DHA {:.0} ({})",
        v.food_type, profile.sfa_pct, profile.mufa_pct, profile.pufa_pct,
        profile.epa_dha_pct, v.reason
    );
    crate::services::telemetry::report_detection(
        "fat.row",
        food_name,
        &source,
        &format!("{} — {}", v.food_type, v.reason),
        &[profile.sfa_pct, profile.mufa_pct, profile.pufa_pct, profile.epa_dha_pct],
    );
    Ok(profile)
}

// ── Профиль жира СОСТАВНОГО БЛЮДА ────────────────────────────────────────────
//
// Таблицы здесь нет намеренно: у блюда нет одного источника жира, и любой выбор
// строки был бы догадкой — той самой, из-за которой голубцы и картофельная
// запеканка получали профиль свинины. Модель отвечает свободно, а неизбежная
// неточность трактуется НЕ В ПОЛЬЗУ человека: блюдом, состав которого мы знаем
// только по названию, нельзя закрыть недельную планку.

/// Обоснование первым — оно и есть размышление модели.
#[derive(Debug, Deserialize, JsonSchema)]
struct DishFatAnswer {
    /// Which ingredients give this dish its fat, and in what proportion. One short
    /// sentence.
    reason_where_the_fat_comes_from: String,
    /// SATURATED fatty acids as a PERCENT OF THIS DISH'S TOTAL FAT, by weight.
    sfa_pct: f64,
    /// MONOUNSATURATED fatty acids as a PERCENT OF THIS DISH'S TOTAL FAT, by weight.
    mufa_pct: f64,
    /// POLYUNSATURATED fatty acids as a PERCENT OF THIS DISH'S TOTAL FAT, by weight.
    pufa_pct: f64,
}

/// Насколько ниже нормы прижимается отношение (МНЖК+ПНЖК)/НЖК у блюда.
///
/// Ровно нормой обойтись нельзя: индикатор засчитывает неделю при `значение >=
/// нормы` (`fats::Fat::met`), и рацион из одних блюд выходил бы на планку впритык.
const DISH_RATIO_OF_TARGET: f64 = 0.9;

/// Свести профиль блюда к «не лучше, чем неизвестно».
///
/// Два действия, каждое — против конкретного индикатора (`fats.rs`):
///   * EPA+DHA обнуляются. Длинные морские омега-3 из названия блюда не выводятся,
///     а ошибка в плюс закрывает недельную норму рыбой, которой человек не ел.
///   * Отношение (МНЖК+ПНЖК)/НЖК прижимается ниже нормы, если модель дала выше.
///     Ответ ХУЖЕ нормы остаётся как есть: пессимизм не значит «портить всё».
///
/// Общая сумма кислот сохраняется — она задаёт, сколько жира вообще учтено; сдвиг
/// идёт долями внутри неё.
fn pessimise_dish_fat(sfa: f64, mufa: f64, pufa: f64) -> api_types::FatProfile {
    let (sfa, mufa, pufa) = (sfa.max(0.0), mufa.max(0.0), pufa.max(0.0));
    let total = sfa + mufa + pufa;
    let cap = crate::services::fats::UNSAT_TO_SAT_MIN * DISH_RATIO_OF_TARGET;
    let unsat = mufa + pufa;
    // Без насыщенных отношение не определено, и делить не на что.
    let worsen = sfa > 0.0 && unsat > sfa * cap;
    if !worsen || total <= 0.0 {
        return api_types::FatProfile { sfa_pct: sfa, mufa_pct: mufa, pufa_pct: pufa, epa_dha_pct: 0.0 };
    }
    // Искомое: (total - sfa') / sfa' = cap  ⇒  sfa' = total / (1 + cap).
    let new_sfa = total / (1.0 + cap);
    let k = (total - new_sfa) / unsat; // < 1: ненасыщенные ужимаются пропорционально
    api_types::FatProfile {
        sfa_pct: new_sfa,
        mufa_pct: mufa * k,
        pufa_pct: pufa * k,
        epa_dha_pct: 0.0,
    }
}

/// Профиль жира составного блюда: свободный ответ модели, прижатый к худшему.
async fn lookup_dish_fat_profile(
    food_name: &str,
    identity: &str,
) -> Result<api_types::FatProfile, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = format!(
        "{head}\
         This is a COMPOSITE DISH — its fat comes from several ingredients at once. Report the \
         FATTY-ACID PROFILE OF ITS FAT.\n\n\
         Every number you report is a PERCENT OF THIS DISH'S TOTAL FAT, by weight — NOT grams \
         per 100 g. How much fat the dish has is already known to us; you only describe how \
         that fat is composed.\n\n\
         THINK IT THROUGH FIRST in the reason field, in {lang}: name the ingredients this dish \
         is usually made of, say which of them carry the fat, and which of those dominates. \
         Then let the numbers follow from it.\n\n\
         Where the ingredient is not fixed by the name — the kind of mince, the fat used for \
         frying — assume the ORDINARY, most common version of the dish in home cooking.\n\n\
         Report sfa_pct, mufa_pct and pufa_pct as percents of total fat. The rest of the weight \
         is glycerol, so the three need not add up to 100 — never inflate a fraction to make \
         them.\n\n\
         Respond with ONLY a single minified JSON object and nothing else — no markdown, no \
         prose.",
        head = fat_head(food_name, identity),
    );

    let v: DishFatAnswer = generate_validated(prompt, |_| {}, 4, |v: &DishFatAnswer| {
        let sum = v.sfa_pct + v.mufa_pct + v.pufa_pct;
        if sum > 100.5 {
            return Err(format!("fat fractions sum to {sum:.1}% for «{food_name}»"));
        }
        Ok(())
    })
    .await?;

    let profile = pessimise_dish_fat(v.sfa_pct, v.mufa_pct, v.pufa_pct);
    leptos::logging::log!(
        "жиры блюда «{food_name}»: модель {:.0}/{:.0}/{:.0} → НЖК {:.0} МНЖК {:.0} ПНЖК {:.0} \
         EPA+DHA 0 ({})",
        v.sfa_pct, v.mufa_pct, v.pufa_pct,
        profile.sfa_pct, profile.mufa_pct, profile.pufa_pct,
        v.reason_where_the_fat_comes_from
    );
    // Числа — ОТВЕТ МОДЕЛИ, до пессимизации: замер должен видеть, что она сказала,
    // а не то, во что мы это превратили. Наш зажим восстановим сами, он известен.
    crate::services::telemetry::report_detection(
        "fat.dish",
        food_name,
        "dish",
        &v.reason_where_the_fat_comes_from,
        &[v.sfa_pct, v.mufa_pct, v.pufa_pct, 0.0],
    );
    Ok(profile)
}

// ── Признаки спрашиваются ПО ОДНОМУ ─────────────────────────────────────────
//
// Раньше все ехали одним запросом — дешевле по числу вызовов, но признаки в нём
// тянут друг друга: определения соседей стоят в том же тексте, и продукт,
// разобранный по одному пункту, притягивается к формулировкам другого. Гем на
// этом и сыпался: мидии уходили к «shrimp and crab» из пункта про ракообразных.
//
// Теперь у каждого признака свой промпт и свой запрос. Вызовов стало по одному на
// признак — это осознанная плата за то, что признаки перестали влиять друг на
// друга и каждый можно замерять и править отдельно.
//
// Спрашиваются ТРИ: фрукты/овощи, гем, молочная глобула. Перекус, яйца, красное
// мясо и жидкие калории у модели больше не спрашиваются.

// Три признака — ТРИ ОТДЕЛЬНЫХ ПРОМПТА, целиком, без общей обвязки.
//
// Раньше здесь был один шаблон, в который подставлялось «определение» признака, и
// одна общая схема ответа. Выглядело экономно, а на деле связало признаки друг с
// другом: правка ради гема (порядок полей в схеме) молча испортила глобулу — 20/20
// превратились в 17/20, хотя её текст никто не трогал. Общими были и заголовок, и
// требования к ответу, и порядок полей — то есть всё, кроме одного абзаца.
//
// Поэтому промпты разведены. Общей осталась только МЕХАНИКА, в которой нет ни
// одного слова, уходящего к модели: нумерация списка и сверка длин массивов.
// Дублирование строк — осознанная плата: признак теперь можно править и замерять,
// не оглядываясь на соседей.
//
// ОБОСНОВАНИЕ ВСЕГДА ПЕРВОЕ, вердикт вторым — во всех трёх схемах. Порядок полей
// здесь и есть порядок генерации: схема уходит в запрос дословно, и модель
// заполняет поля сверху вниз. Обоснование, поставленное впереди, заменяет ей
// размышление: она обязана сперва назвать, ЧЕМ считает продукт, и только потом
// вынести вердикт. С вердиктом впереди она решает первым же токеном, а объяснение
// подгоняет под уже сказанное — гем на этом терял свинину и куриные сердечки
// (19/22 против 22/22).
//
// Обратная сторона: рассуждение вслух вытаскивает наружу сомнения промпта. Глобула
// с прежним текстом падала до 17/20 — модель начинала рассуждать «в твороге
// глобулы не обязательно целые». Лечится это ТЕКСТОМ, а не порядком: промпт
// глобулы переписан процедурой, где рассуждению задано русло.
//
// FAIL LOUDLY: ошибка модели, разбора или несовпадение длины массива — это `Err`
// (вызывающий его показывает; ничего не размечается молча наугад).



// ── Овощ или фрукт ───────────────────────────────────────────────────────────

// ВНИМАНИЕ: у структур ответа комментарии обычные, а НЕ доковые (///).
//
// schemars кладёт док-комментарий в json_schema как "description", модель читает
// его наравне с полями и начинает возвращать в ответе. Длинное русское пояснение
// над этой структурой ровно так и уронило разбор: модель отвечала
// {"description":"Имя поля обоснования ДЛИННОЕ намеренно…"} вместо данных, и
// признак оставался пустым. Пояснения — в док функции, схема остаётся голой.
//
// Опознание — своим полем и ПЕРВЫМ: порядок полей есть порядок генерации, и «что
// это за еда» не должно писаться в графу, чьё имя уже утверждает вывод.
#[derive(Debug, Deserialize, JsonSchema)]
struct VegFruitAnswer {
    what_the_person_most_likely_ate: Vec<String>,
    category_that_fits: Vec<String>,
}

/// Категория из ответа → «да/нет».
///
/// Вердикта модель больше НЕ ставит, и это главная поправка. Пока он был отдельным
/// булевым массивом, он мог противоречить только что названной категории — замер
/// ловил «Мороженая вишня → FRUITS» с вердиктом `false` в двух прогонах из трёх.
/// Теперь «да» выводится из названного, и рассогласоваться нечему.
///
/// Неизвестное слово — ОШИБКА, а не тихое «нет»: [`generate_validated`] потратит на
/// это попытку и переспросит. Молча ронять непонятый ответ в `false` значит снова
/// разрешить признаку врать.
pub(crate) fn veg_fruit_from_category(category: &str) -> Result<bool, String> {
    let c = category.trim().to_ascii_uppercase();
    if c.starts_with("NONE") {
        return Ok(false);
    }
    if c.contains("VEGETABLE") || c.contains("FRUIT") || c.contains("BERR") || c.contains("DISH") {
        return Ok(true);
    }
    Err(format!("фрукты/овощи: неизвестная категория «{category}»"))
}


// ── Support-chat: tools + agentic tool-use loop ──
//
// The chat is a real tool-use loop. The model can either call a registered
// `arti_pipes` tool or end the turn with a final answer. Because the pinned
// Qwen executor is a plain text-completion executor (its `PromptExecutor` only
// exposes `execute_raw`/`execute`, the request carries no `tools` field, and it
// never surfaces `PromptExecutionEvent::ToolCallsRequested`), there is no native
// OpenAI `tool_calls` transport. We bridge it with an explicit text protocol the
// model is instructed to follow, parsed into real `arti_pipes::tool::ToolCall`s
// dispatched through a `ToolRegistry`:
//
//   * To CALL a tool, output on its OWN line, nothing after it:
//         [[tool]] <name> <json-arguments>
//     e.g. `[[tool]] read_progress {"days": 30}`
//   * To FINISH, output the marker then the user-facing answer:
//         [[final]] <answer text…>
//
// `[[final]]` is the explicit, unambiguous end-of-loop signal (see `chat_agent`).
// If a future executor gains native tool-calling, the same registry transfers
// unchanged.

/// Prefix the model emits (line-leading) to request a tool call.
pub const TOOL_PREFIX: &str = "[[tool]]";
/// Prefix the model emits to mark its final answer — the loop's end signal.
pub const FINAL_PREFIX: &str = "[[final]]";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EscalateInput {
    /// Short human-readable reason the conversation needs a live operator.
    pub reason: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EscalateOutput {
    pub ack: String,
}

pub struct EscalateToHuman;

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for EscalateToHuman {
    type Input = EscalateInput;
    type Output = EscalateOutput;

    fn name(&self) -> &str {
        "escalate_to_human"
    }

    fn description(&self) -> &str {
        "Escalate the conversation to a real human support operator."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        // STUB. TODO: call the real support worker here (enqueue the conversation
        // for a live operator). For now just acknowledge so the LLM/UI can react.
        Ok(EscalateOutput { ack: format!("Escalation requested: {}", input.reason) })
    }
}

// ── read_progress tool: weight / steps / goal-fulfilment over N days ──

fn default_days() -> u32 {
    30
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadProgressInput {
    /// How many past days to summarise (capped to the stored window).
    #[serde(default = "default_days")]
    pub days: u32,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WeightPoint {
    pub date: String,
    pub kg: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct StepPoint {
    pub date: String,
    pub steps: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GoalProgress {
    pub nutrient: String,
    pub direction: String,
    pub target: f64,
    pub unit: String,
    pub days_met: u32,
    pub days_logged: u32,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadProgressOutput {
    pub period_days: u32,
    pub weight_change_kg: Option<f64>,
    pub weight: Vec<WeightPoint>,
    pub steps_avg: Option<u32>,
    pub steps: Vec<StepPoint>,
    pub goals: Vec<GoalProgress>,
}

/// One logged day's totals, pre-computed in the snapshot so the tool's `call`
/// is pure (no IndexedDB access — its future must stay `Send`).
#[derive(Clone)]
struct DayTotals {
    date: String,
    values: BTreeMap<String, f64>, // "Calories"/"Protein"/"Fat"/"Carbs"/custom-name
}

#[derive(Clone)]
struct GoalSpec {
    nutrient: String,
    direction: GoalDirection,
    amount: f64,
    unit: String,
}

/// Plain, `Send` data snapshot the `read_progress` tool reads from. Built once
/// (off the IndexedDB) BEFORE the chat loop, so the tool itself never awaits a
/// non-`Send` future inside `Tool::call`.
#[derive(Clone)]
pub struct ProgressSnapshot {
    weight: Vec<WeightPoint>,    // ascending by date
    steps: Vec<StepPoint>,       // ascending by date
    days: Vec<DayTotals>,        // ascending by date, only days with entries
    goals: Vec<GoalSpec>,        // current daily goals
}

/// Maximum window we pre-load; the tool slices to the requested `days`.
const SNAPSHOT_WINDOW_DAYS: i64 = 120;

/// Build the progress snapshot off local IndexedDB. Call this in a wasm-local
/// context (its future is NOT `Send`); the resulting `ProgressSnapshot` is.
pub async fn build_progress_snapshot() -> ProgressSnapshot {
    let mut weight: Vec<WeightPoint> = local::list_weight_entries()
        .await
        .into_iter()
        .map(|e| WeightPoint { date: e.date, kg: e.weight_kg })
        .collect();
    weight.sort_by(|a, b| a.date.cmp(&b.date));

    let mut steps: Vec<StepPoint> = local::list_step_entries()
        .await
        .into_iter()
        .map(|e| StepPoint { date: e.date, steps: e.steps })
        .collect();
    steps.sort_by(|a, b| a.date.cmp(&b.date));

    let goals: Vec<GoalSpec> = local::list_goals()
        .await
        .into_iter()
        .filter(|g| g.period == GoalPeriod::Day && g.amount > 0.0)
        .map(|g| GoalSpec {
            nutrient: g.nutrient,
            direction: g.direction,
            amount: g.amount,
            unit: match g.unit {
                GoalUnit::Kcal => "kcal",
                GoalUnit::G => "g",
                GoalUnit::Mg => "mg",
                GoalUnit::Mcg => "mcg",
                GoalUnit::Steps => "steps",
            }
            .to_string(),
        })
        .collect();

    // Per-day nutrient totals over the window (only days that have diary entries).
    let today = chrono::Local::now().date_naive();
    let dates: Vec<String> = (0..SNAPSHOT_WINDOW_DAYS)
        .map(|i| (today - chrono::Duration::days(i)).format("%Y-%m-%d").to_string())
        .collect();
    let foods = local::list_foods().await;
    let fmap: BTreeMap<String, Food> = foods.into_iter().map(|f| (f.id.clone(), f)).collect();
    let entries = local::list_diary_range(&dates).await;

    let mut per_day: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for e in &entries {
        if let Some(food) = fmap.get(&e.food_id) {
            let factor = (e.grams - e.waste_grams).max(0.0) / 100.0;
            let day = per_day.entry(e.date.clone()).or_default();
            *day.entry("Calories".into()).or_default() += food.effective_kcal() * factor;
            *day.entry("Protein".into()).or_default() += food.protein * factor;
            *day.entry("Fat".into()).or_default() += food.fat * factor;
            *day.entry("Carbs".into()).or_default() += food.carbs * factor;
            for (k, v) in &food.nutrients {
                *day.entry(k.clone()).or_default() += v * factor;
            }
        }
    }
    let mut days: Vec<DayTotals> = per_day
        .into_iter()
        .map(|(date, values)| DayTotals { date, values })
        .collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));

    ProgressSnapshot { weight, steps, days, goals }
}

pub struct ReadProgress {
    snapshot: ProgressSnapshot,
}

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for ReadProgress {
    type Input = ReadProgressInput;
    type Output = ReadProgressOutput;

    fn name(&self) -> &str {
        "read_progress"
    }

    fn description(&self) -> &str {
        "Read the user's weight trend, daily steps, and how well their daily \
         nutrition goals were met over the last N days. Use it before giving \
         progress feedback so you cite real numbers instead of guessing."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        // Pure: slice the pre-fetched snapshot to the requested window. No await
        // of a non-Send future here.
        let days = input.days.max(1);
        let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(days as i64 - 1);
        let cutoff = cutoff.format("%Y-%m-%d").to_string();

        let weight: Vec<WeightPoint> = self
            .snapshot
            .weight
            .iter()
            .filter(|p| p.date >= cutoff)
            .map(|p| WeightPoint { date: p.date.clone(), kg: p.kg })
            .collect();
        let weight_change_kg = match (weight.first(), weight.last()) {
            (Some(a), Some(b)) if a.date != b.date => Some(b.kg - a.kg),
            _ => None,
        };

        let steps: Vec<StepPoint> = self
            .snapshot
            .steps
            .iter()
            .filter(|p| p.date >= cutoff)
            .map(|p| StepPoint { date: p.date.clone(), steps: p.steps })
            .collect();
        let steps_avg = if steps.is_empty() {
            None
        } else {
            Some((steps.iter().map(|s| s.steps as u64).sum::<u64>() / steps.len() as u64) as u32)
        };

        let in_window: Vec<&DayTotals> =
            self.snapshot.days.iter().filter(|d| d.date >= cutoff).collect();
        let goals: Vec<GoalProgress> = self
            .snapshot
            .goals
            .iter()
            .map(|g| {
                let mut days_met = 0u32;
                let mut days_logged = 0u32;
                for d in &in_window {
                    let val = d.values.get(&g.nutrient).copied().unwrap_or(0.0);
                    // A day "counts" toward a goal only if anything was logged.
                    if d.values.values().any(|v| *v > 0.0) {
                        days_logged += 1;
                        let met = match g.direction {
                            GoalDirection::AtLeast => val >= g.amount,
                            GoalDirection::AtMost => val <= g.amount,
                        };
                        if met {
                            days_met += 1;
                        }
                    }
                }
                GoalProgress {
                    nutrient: g.nutrient.clone(),
                    direction: match g.direction {
                        GoalDirection::AtLeast => "at_least".into(),
                        GoalDirection::AtMost => "at_most".into(),
                    },
                    target: g.amount,
                    unit: g.unit.clone(),
                    days_met,
                    days_logged,
                }
            })
            .collect();

        Ok(ReadProgressOutput {
            period_days: days,
            weight_change_kg,
            weight,
            steps_avg,
            steps,
            goals,
        })
    }
}

// ── read_documentation tool: short in-app help docs for usage advice ──
//
// A tiny built-in documentation index. The model is told the available doc ids
// (see `doc_index`, embedded in the system prompt) and fetches the one it needs
// before explaining how a feature works, so its advice matches the real app.

const DIARY_DOC: &str = "Дневник питания (вкладка «Дневник»).\n\
- Вверху — выбранный день и сводка КБЖУ за день (калории, белки, жиры, углеводы) относительно целей; день переключается выбором даты сверху.\n\
- Дневник разбит на три панели приёмов: Завтрак, Обед, Ужин.\n\
- Добавить съеденное: нажми «+» на нужном приёме (или на его названии) → страница добавления → найди продукт по названию в поиске → выбери его → укажи вес в граммах → сохрани. Запись попадёт в этот приём за текущий день.\n\
- Нет нужного продукта — его можно создать, указав КБЖУ (ИИ может подсказать значения по названию).\n\
- Долгое нажатие на строку записи за сегодня открывает меню: Дублировать, Изменить (вес/название/КБЖУ), Удалить.\n\
- Несъедобные части (кости, косточки) указываются как «отход» — их вес вычитается из съеденного.";

const WEIGHT_DOC: &str = "Дневник веса (вкладка «Вес»).\n\
- Записать измерение: введи вес в кг за день и сохрани. На одну дату — одно значение (повторная запись перезаписывает).\n\
- График показывает динамику веса; в карточке есть тренд (снижается/растёт) с оценкой уверенности примерно за последние 2 недели.\n\
- Текущий вес на виджете подсвечивается цветом: зелёный — снижение/дефицит, розовый — рост/профицит, чёрный — удержание.\n\
- Полезно взвешиваться регулярно, утром натощак; 7 дней подряд открывают дополнительные задания в «Истории».\n\
- Для женщин приложение умеет выделять колебания из-за менструального цикла и показывать «очищенный» от цикла вес.";

const STEPS_DOC: &str = "Дневник активности — шаги (вкладка «Шаги»).\n\
- Записывай число шагов за день вручную; на одну дату — одно значение (повторная запись перезаписывает).\n\
- Шаги учитываются в оценке активности и в ежедневной сводке.\n\
- Регулярная запись (серия дней подряд) открывает задания в «Истории».";

/// The documentation index: (doc_id, short human title). Embedded in the system
/// prompt so the model knows which docs it can fetch via `read_documentation`.
pub const DOC_INDEX: [(&str, &str); 3] = [
    ("diary", "Дневник питания — как добавлять и редактировать еду"),
    ("weight", "Дневник веса — как записывать вес и читать тренд"),
    ("steps", "Дневник активности — как записывать шаги"),
];

fn doc_content(id: &str) -> Option<&'static str> {
    match id {
        "diary" => Some(DIARY_DOC),
        "weight" => Some(WEIGHT_DOC),
        "steps" => Some(STEPS_DOC),
        _ => None,
    }
}

/// The documentation index rendered for the system prompt.
pub fn doc_index() -> String {
    DOC_INDEX
        .iter()
        .map(|(id, title)| format!("- {id}: {title}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadDocInput {
    /// Documentation page id to fetch. One of: "diary", "weight", "steps".
    pub doc_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadDocOutput {
    pub doc_id: String,
    pub found: bool,
    /// The document text (empty when `found` is false).
    pub content: String,
    /// Valid doc ids, returned when the requested one is unknown.
    pub available: Vec<String>,
}

pub struct ReadDocumentation;

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for ReadDocumentation {
    type Input = ReadDocInput;
    type Output = ReadDocOutput;

    fn name(&self) -> &str {
        "read_documentation"
    }

    fn description(&self) -> &str {
        "Fetch a short in-app help document so you can give accurate usage advice. \
         Valid doc_id values: diary (food diary), weight (weight diary), steps \
         (activity/steps). Call it before explaining how a feature works."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        match doc_content(&input.doc_id) {
            Some(c) => Ok(ReadDocOutput {
                doc_id: input.doc_id,
                found: true,
                content: c.to_string(),
                available: vec![],
            }),
            None => Ok(ReadDocOutput {
                doc_id: input.doc_id,
                found: false,
                content: String::new(),
                available: DOC_INDEX.iter().map(|(id, _)| id.to_string()).collect(),
            }),
        }
    }
}

// ── file_bug_report tool: compose + submit a bug report to the developers ──
//
// `Tool::call` futures must be `Send`, so this tool can't POST itself (web_sys
// fetch isn't Send). It captures the validated report into a thread-local slot;
// `chat_agent` (a non-Send wasm-local future) drains it and submits via
// `bug_report::submit`, feeding the real outcome back to the model.

thread_local! {
    static CAPTURED_BUG_REPORT: RefCell<Option<bug_report::BugReport>> = const { RefCell::new(None) };
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BugReportInput {
    /// Short one-line title summarising the bug.
    pub title: String,
    /// Which part of the app: "diary" | "weight" | "steps" | "chat" | "settings" | "story" | "other".
    pub area: String,
    /// Step-by-step description of what the user did that led to the problem.
    pub steps_to_reproduce: String,
    /// What the user expected to happen.
    pub expected: String,
    /// What actually happened instead.
    pub actual: String,
    /// How badly it blocks the user: "low" | "medium" | "high". Optional.
    #[serde(default)]
    pub severity: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct BugReportAck {
    pub status: String,
}

pub struct FileBugReport;

#[async_trait::async_trait]
impl arti_pipes::tool::Tool for FileBugReport {
    type Input = BugReportInput;
    type Output = BugReportAck;

    fn name(&self) -> &str {
        "file_bug_report"
    }

    fn description(&self) -> &str {
        "File a bug report to the developers. First gather ALL of these from the \
         user: a short title; the area (diary/weight/steps/chat/settings/story/other); \
         step-by-step reproduction; what they expected; and what actually happened \
         (severity low/medium/high is optional). Only call once you have title, area, \
         steps_to_reproduce, expected and actual."
    }

    async fn call(
        &self,
        input: Self::Input,
    ) -> Result<Self::Output, arti_pipes::error::ExecutionError> {
        // Capture the report; chat_agent submits it (this future must stay Send).
        CAPTURED_BUG_REPORT.with(|c| {
            *c.borrow_mut() = Some(bug_report::BugReport {
                title: input.title,
                area: input.area,
                steps_to_reproduce: input.steps_to_reproduce,
                expected: input.expected,
                actual: input.actual,
                severity: input.severity.unwrap_or_else(|| "medium".to_string()),
                app_version: String::new(), // filled in by chat_agent before POST
            });
        });
        Ok(BugReportAck { status: "captured".to_string() })
    }
}

/// The tool registry exposed to support chat: `escalate_to_human` (stub),
/// `read_progress` (reads the pre-built snapshot), `read_documentation` (built-in
/// help docs), and `file_bug_report` (captures a report; `chat_agent` submits).
/// Descriptors come from these real registrations, and calls are dispatched via
/// `ToolRegistry::execute`.
pub fn chat_registry(snapshot: ProgressSnapshot) -> arti_pipes::tool_registry::ToolRegistry {
    arti_pipes::tool_registry::ToolRegistry::new()
        .register(EscalateToHuman)
        .register(ReadProgress { snapshot })
        .register(ReadDocumentation)
        .register(FileBugReport)
}

/// A one-line, model-facing description of the registered tools, derived from
/// the actual registry descriptors (name + description + input schema). The
/// prompt embeds this so the advertised tool is the registered one, not a
/// hand-written copy that could drift.
pub fn tool_descriptions(registry: &arti_pipes::tool_registry::ToolRegistry) -> String {
    registry
        .descriptors()
        .into_iter()
        .map(|d| {
            let schema = serde_json::to_string(&d.input_schema)
                .unwrap_or_else(|_| "{}".to_string());
            format!("- {} — {} (input JSON schema: {})", d.name, d.description, schema)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Roles in the running chat transcript fed back into each turn's prompt.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
}

/// One rendered conversation turn. `Tool` turns carry a tool's JSON result so
/// the next LLM turn can read what the tool returned.
#[derive(Clone)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub text: String,
}

/// Live events emitted during the agentic loop, for the UI.
pub enum ChatEvent<'a> {
    /// A new LLM turn started (waiting on the first token).
    Requesting,
    /// A reasoning token arrived (count it).
    Thinking,
    /// A visible answer chunk (already stripped of control markers).
    Answer(&'a str),
    /// A tool is about to run — `(name, compact-json-params)`.
    ToolCall(&'a str, &'a str),
    /// A tool finished.
    ToolDone(&'a str),
}

/// One tool call made during a chat turn: the tool name, the compact-JSON params
/// it was called with, and the compact-JSON result it returned. Surfaced in the
/// UI ("Assistant requested tool: …") and kept as the model's bounded tool
/// context (the last [`MAX_CONTEXT_TOOLS`]), instead of bloating the transcript.
#[derive(Clone)]
pub struct ToolInvocation {
    pub name: String,
    pub params: String,
    pub result: String,
}

/// Outcome of the chat loop: the final user-facing answer, whether the
/// `escalate_to_human` tool fired, and the tool calls made this turn (for the UI).
pub struct ChatOutcome {
    pub answer: String,
    pub escalated: bool,
    pub tools: Vec<ToolInvocation>,
}

/// Hard cap on LLM↔tool round-trips, so a misbehaving model cannot loop forever.
const MAX_TOOL_ITERS: usize = 6;
/// Only the most recent N conversation turns (user + assistant) are sent.
const CONTEXT_WINDOW: usize = 20;
/// Most recent tool calls (with results) kept in the model's "Context" section.
const MAX_CONTEXT_TOOLS: usize = 7;

/// Run the support-chat agentic loop. Each iteration renders the last
/// `CONTEXT_WINDOW` turns into a prompt, streams the model's output (filtered of
/// control markers), then either:
///   * dispatches a `[[tool]]` call through the registry and feeds the JSON
///     result back as a `Tool` turn (loop continues), or
///   * stops on the explicit `[[final]]` marker — the end-of-loop signal — or on
///     a turn with no tool call, treated as a final answer.
/// `MAX_TOOL_ITERS` is the safety backstop against infinite looping.
///
/// `system` is the fixed preamble (assistant role, language, tool protocol).
/// FAIL LOUDLY: executor and tool-dispatch errors propagate as `Err`.
///
/// TODO: support chat inherits the ai-worker subscription 402 gate (surfaced as
/// `Err("HTTP 402: …")`); it may later need an ungated path.
pub async fn chat_agent(
    system: String,
    transcript: Vec<ChatTurn>,
    snapshot: ProgressSnapshot,
    on_event: impl Fn(ChatEvent) + Clone + 'static,
) -> Result<ChatOutcome, String> {
    let registry = chat_registry(snapshot);
    let mut escalated = false;
    // Tool calls made this turn. They do NOT enter the conversation transcript;
    // the last MAX_CONTEXT_TOOLS are rendered into a bounded "Context" section.
    let mut tools: Vec<ToolInvocation> = Vec::new();

    for _ in 0..MAX_TOOL_ITERS {
        on_event(ChatEvent::Requesting);

        let prompt = render_prompt(&system, &transcript, &tools);
        let executor = build_executor()?;
        let result = executor
            .execute_raw(prompt)
            .await
            .map_err(|e| format!("LLM execute error: {e:?}"))?;

        // Thinking stream: count only.
        let mut thinking_stream = result.thinking_stream;
        let on_think = on_event.clone();
        wasm_bindgen_futures::spawn_local(async move {
            while let Some(token) = thinking_stream.next().await {
                if token.is_ok() {
                    on_think(ChatEvent::Thinking);
                }
            }
        });

        // Answer stream: filter control markers so only visible answer text
        // reaches the UI. Authoritative parsing happens on the final `raw` below.
        let mut content_stream = result.content_stream;
        let on_answer = on_event.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut filter = ControlFilter::default();
            while let Some(token) = content_stream.next().await {
                if let Ok(t) = token {
                    let visible = filter.push(&t.content);
                    if !visible.is_empty() {
                        on_answer(ChatEvent::Answer(&visible));
                    }
                }
            }
        });

        let output = result.output.await.map_err(|e| format!("LLM output error: {e:?}"))?;
        let raw = output.result.trim().to_string();

        // 1) Explicit tool call? Dispatch through the registry and loop.
        if let Some(call) = parse_tool_call(&raw) {
            let name = call.name.clone();
            let params = serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".to_string());
            on_event(ChatEvent::ToolCall(&name, &params));
            if name == "escalate_to_human" {
                escalated = true;
            }
            let res = registry
                .execute(&call)
                .await
                .map_err(|e| format!("tool {name} failed: {e:?}"))?;
            let mut result_json =
                serde_json::to_string(&res.output).unwrap_or_else(|_| "{}".to_string());

            // file_bug_report only CAPTURED the report (its call must be Send and
            // can't fetch). Submit it here and feed the real outcome back so the
            // model confirms success (with the ticket id) or relays the error.
            if name == "file_bug_report" {
                if let Some(mut report) = CAPTURED_BUG_REPORT.with(|c| c.borrow_mut().take()) {
                    report.app_version = update::current_version();
                    result_json = match bug_report::submit(&report).await {
                        Ok(id) => serde_json::json!({ "status": "submitted", "report_id": id }).to_string(),
                        Err(e) => serde_json::json!({ "status": "error", "error": e }).to_string(),
                    };
                }
            }
            // Record the call + result as bounded tool context (NOT in the
            // conversation transcript). render_prompt feeds the model the last
            // MAX_CONTEXT_TOOLS of these.
            tools.push(ToolInvocation { name: name.clone(), params, result: result_json });
            on_event(ChatEvent::ToolDone(&name));
            continue;
        }

        // 2) Explicit `[[final]]` marker (clean end signal) or no marker at all
        //    (graceful fallback) → this is the answer. End the loop.
        let answer = strip_final_marker(&raw);
        return Ok(ChatOutcome { answer, escalated, tools });
    }

    // Hit the iteration cap with the model still asking for tools.
    Err(format!("chat loop did not finish within {MAX_TOOL_ITERS} tool steps"))
}

/// Render the system preamble, the last `CONTEXT_WINDOW` conversation turns, and
/// a bounded "Context" section with the last `MAX_CONTEXT_TOOLS` tool calls and
/// their results, into a single prompt ending with `Assistant:`.
fn render_prompt(system: &str, transcript: &[ChatTurn], tools: &[ToolInvocation]) -> String {
    let start = transcript.len().saturating_sub(CONTEXT_WINDOW);
    let mut p = String::from(system);
    p.push_str("\n\nConversation:\n");
    for turn in &transcript[start..] {
        let speaker = match turn.role {
            ChatRole::User => "User",
            ChatRole::Assistant => "Assistant",
            ChatRole::Tool => "Tool result",
        };
        p.push_str(&format!("{speaker}: {}\n", turn.text));
    }
    if !tools.is_empty() {
        let from = tools.len().saturating_sub(MAX_CONTEXT_TOOLS);
        p.push_str(
            "\nContext — recent tool calls and their results (newest last). \
             Use these results; do not repeat a call you already made:\n",
        );
        for inv in &tools[from..] {
            p.push_str(&format!("{} {} -> {}\n", inv.name, inv.params, inv.result));
        }
    }
    p.push_str("Assistant:");
    p
}

/// Parse a `[[tool]] <name> <json>` directive anywhere in the model output into
/// a real `ToolCall`. Returns None if there is no tool directive.
fn parse_tool_call(raw: &str) -> Option<arti_pipes::tool::ToolCall> {
    let idx = raw.find(TOOL_PREFIX)?;
    let after = raw[idx + TOOL_PREFIX.len()..].trim_start();
    let line_end = after.find('\n').unwrap_or(after.len());
    let line = after[..line_end].trim();
    // `<name> <json>` — name is the first whitespace-delimited token.
    let (name, args_str) = match line.split_once(char::is_whitespace) {
        Some((n, rest)) => (n.trim(), rest.trim()),
        None => (line, ""),
    };
    if name.is_empty() {
        return None;
    }
    let arguments = if args_str.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}))
    };
    Some(arti_pipes::tool::ToolCall {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        arguments,
    })
}

/// Strip a leading `[[final]]` marker (and following whitespace) if present;
/// otherwise return the text as-is.
fn strip_final_marker(raw: &str) -> String {
    match raw.find(FINAL_PREFIX) {
        Some(idx) => {
            let before = raw[..idx].trim_end();
            let after = raw[idx + FINAL_PREFIX.len()..].trim_start();
            if before.is_empty() {
                after.to_string()
            } else {
                format!("{before}\n{after}")
            }
        }
        None => raw.to_string(),
    }
}

/// Incremental display filter for the streamed answer. It hides control
/// directives so the user never sees the protocol mid-stream:
///   * a turn whose (trimmed) start is `[[tool]]` is suppressed whole;
///   * a leading `[[final]]` marker is stripped, the rest shown live;
///   * any other turn is shown live (plain final answer).
/// Until it can classify (the markers share the `[[` prefix), it buffers.
#[derive(Default)]
struct ControlFilter {
    buffer: String,
    decided: Option<FilterMode>,
}

#[derive(Clone, Copy)]
enum FilterMode {
    /// Show everything from here on.
    Pass,
    /// Suppress everything (a tool-call turn).
    Suppress,
}

impl ControlFilter {
    fn push(&mut self, chunk: &str) -> String {
        match self.decided {
            Some(FilterMode::Pass) => chunk.to_string(),
            Some(FilterMode::Suppress) => String::new(),
            None => {
                self.buffer.push_str(chunk);
                self.classify()
            }
        }
    }

    /// Try to classify the turn from the buffer so far, emitting any text that is
    /// safe to show once decided.
    fn classify(&mut self) -> String {
        let trimmed = self.buffer.trim_start();

        if trimmed.starts_with(TOOL_PREFIX) {
            self.decided = Some(FilterMode::Suppress);
            self.buffer.clear();
            return String::new();
        }
        if let Some(rest) = trimmed.strip_prefix(FINAL_PREFIX) {
            self.decided = Some(FilterMode::Pass);
            let out = rest.trim_start().to_string();
            self.buffer.clear();
            return out;
        }
        // Still possibly the start of a marker? Keep buffering.
        if TOOL_PREFIX.starts_with(trimmed) || FINAL_PREFIX.starts_with(trimmed) {
            return String::new();
        }
        // Definitely neither marker — plain answer. Flush the buffer.
        self.decided = Some(FilterMode::Pass);
        std::mem::take(&mut self.buffer)
    }
}

fn exact_range(value: f64, unit: &str) -> AiNutrientDetail {
    let v = AiValueWithUnit { value, unit: unit.to_string() };
    AiNutrientDetail {
        min_value: v.clone(),
        max_value: v.clone(),
        recommended: v,
        comment: crate::services::i18n::t("ai.extracted_from_label").to_string(),
    }
}

/// One job's status as returned by the ocr-queue worker.
#[derive(Deserialize)]
struct QueueJob {
    status: String,
    #[serde(default)]
    position: u32,
    #[serde(default)]
    created_at: f64,
    #[serde(default)]
    started_at: Option<f64>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    thinking_tokens: u32,
    #[serde(default)]
    answer_tokens: u32,
    /// The RAW model answer text (business logic — prompt + parsing — now lives on
    /// the frontend; the poller is a dumb executor that relays this string).
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Queue state from the status poller. `since_ms` is the epoch-ms start of the
/// current phase (queued-at / processing-at) for the seconds counter.
pub enum QueuePhase {
    Queued { position: u32, since_ms: f64 },
    Processing { since_ms: f64 },
    Done(AiLookupOutput),
    Error(String),
}

/// The per-100g recognition result the on-prem poller posts back.
#[derive(Deserialize)]
struct QueueResult {
    #[serde(default)]
    product_name: Option<String>,
    #[serde(default)]
    energy_kcal: Option<f64>,
    #[serde(default)]
    protein_g: Option<f64>,
    #[serde(default)]
    fat_g: Option<f64>,
    #[serde(default)]
    carbs_g: Option<f64>,
    #[serde(default)]
    package_weight_g: Option<f64>,
    #[serde(default)]
    custom_nutrients: BTreeMap<String, serde_json::Value>,
}

/// Await `ms` milliseconds via setTimeout (no extra crate needed).
pub async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let _ = web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

/// Authenticated request to the ocr-queue worker; returns (status, body text).
async fn queue_request(
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<(u16, String), String> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}{path}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    if let Some(b) = body {
        let s = serde_json::to_string(b).map_err(|e| e.to_string())?;
        opts.set_body(&JsValue::from_str(&s));
    }
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    // Fetch rejection = OCR queue worker unreachable — drop its flag now.
    let resp_val = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => {
            super::net::note_failure(super::net::Worker::Ocr);
            return Err(format!("{e:?}"));
        }
    };
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    Ok((resp.status(), text))
}

/// Short non-streaming text completion via ai-worker (qwen3, thinking off).
/// Used for daily/weekly summaries. Subscription-gated by the worker (402 → Err).
pub async fn summarize(prompt: &str) -> Result<String, String> {
    let base = &config::get().ai_base_url;
    let url = format!("{base}/chat/completions");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;
    let body = serde_json::json!({
        "model": "@cf/qwen/qwen3-30b-a3b-fp8",
        "stream": false,
        "chat_template_kwargs": { "enable_thinking": false },
        "messages": [{ "role": "user", "content": prompt }],
    });

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&serde_json::to_string(&body).map_err(|e| e.to_string())?));
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    // Fetch rejection = AI worker unreachable → drop the global online flag now.
    let resp_val = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => {
            super::net::note_failure(super::net::Worker::Ai);
            return Err(format!("{e:?}"));
        }
    };
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?).await.map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;
    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    // The worker passes the raw Workers AI response through untouched, so parse
    // it here. Shape varies: OpenAI `choices[].message.content` (or `reasoning`),
    // or Workers AI's native top-level `response`.
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("parse: {e}"))?;
    let msg = v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("message"));
    let out = msg
        .and_then(|m| m.get("content").and_then(|s| s.as_str()))
        .or_else(|| msg.and_then(|m| m.get("reasoning").and_then(|s| s.as_str())))
        .or_else(|| v.get("response").and_then(|r| r.as_str()))
        .unwrap_or("");
    Ok(out.trim().to_string())
}

// ── Vision prompt BUILDERS (business logic moved off the poller) ──
//
// The on-prem poller is now a dumb executor: it receives `{images, prompt}`, runs
// the prompt, and relays the RAW model text back. All the prompt engineering and
// the result parsing live here, on the frontend.

/// The label-OCR prompt. `product_name` follows the LABEL's own language, so this
/// prompt has no {lang} slot — only a {custom} slot listing requested nutrient keys.
/// Built with `.replace` (NOT `format!`) so the JSON braces in the literal aren't
/// treated as format placeholders.
fn label_prompt(custom_nutrients: &[NutrientSpec]) -> String {
    let keys = if custom_nutrients.is_empty() {
        "(none)".to_string()
    } else {
        custom_nutrients
            .iter()
            .map(|c| format!("\"{}\"", c.key))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let template = "You are reading a photo of a food package. Nutrition info may be a TABLE or a sentence in fine print, e.g. «Пищевая ценность (среднее значение) в 100 г: белки – 9,0 г; жиры – 2,1 г; углеводы – 3,5 г; 68,9 ккал / 290 кДж». Read ALL text on the package, then extract per-100g nutrition.\n\
        RU mapping: белки->protein, жиры->fat, углеводы->carbs, энергетическая ценность/калорийность->energy.\n\
        energy: kcal only (number before «ккал»), ignore kJ. Per-portion -> convert to per 100g. Do NOT use front-of-pack marketing («11 г белка в упаковке»). If info is absent/illegible, use null.\n\
        Also fill custom_nutrients for any of these requested keys found on the label: {custom}.\n\
        product_name: a SHORT name, MAXIMUM 3-4 words. Keep ONLY brand + core product (and a defining number like fat %). DROP process/marketing/descriptor words such as ультрапастеризованные, пастеризованные, стерилизованные, питьевые, отборное, «с массовой долей жира», «среднее значение». NEVER output more than 4 words. Example: «ВкусВилл Сливки питьевые ультрапастеризованные с массовой долей жира 10%» -> «ВкусВилл Сливки 10%».\n\
        Return ONLY JSON: {\"source_text\":\"<exact nutrition sentence>\",\"product_name\":\"...\",\"energy_kcal\":0,\"protein_g\":0,\"fat_g\":0,\"carbs_g\":0,\"package_weight_g\":0,\"custom_nutrients\":{}}";
    template.replace("{custom}", &keys)
}

/// The food-items (dish photo → list of foods) prompt. LANGUAGE-AWARE: the output
/// language must not waver, so we hand the model the whole prompt in its own
/// language (Russian for `Lang::Ru`, an English translation for `Lang::En`).
fn food_items_prompt() -> String {
    match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Ты — nutrition vision assistant. На фото — блюдо/тарелка/приём пищи. Выведи ТОЛЬКО ту еду, которую реально ВИДНО. Не выдумывай вероятные гарниры.\n\
            Рассуждай про себя, затем выдай СТРОГИЙ JSON.\n\
            Шаги:\n\
            1. Перечисли КАЖДЫЙ различимый компонент ОТДЕЛЬНОЙ позицией — в том числе сыр/моцареллу, мясо/сосиски/курицу, яйцо, орехи, семечки, зелень, видимый соус. Для салата/капрезе/боула дай компоненты ПО ОТДЕЛЬНОСТИ (помидоры, моцарелла, базилик...).\n\
               ЗАПРЕЩЕНО: добавлять обобщённую позицию («салат», «микс», «ассорти», «капрезе», «блюдо»), если её компоненты уже перечислены отдельно; считать одну и ту же еду дважды; дробить одну еду на две.\n\
            2. Порция: найди эталон масштаба (край обеденной тарелки ~26 см, десертной ~20 см, вилка ~19 см, рука, монета). Оцени граммы из покрытия тарелки и типичной плотности. Нет эталона — прикинь стандартную порцию. grams — ОДНО число, не диапазон. Крупные порции на больших тарелках часто НЕДООцениваются — не занижай их.\n\
            3. confidence 0-1 на позицию.\n\
            4. Скрытые жиры (масло/майонез не видно напрямую) добавь ОТДЕЛЬНЫМИ позициями с inferred=true:\n\
               - салат без видимой заправки -> «подсолнечное масло», ~10% массы зелени/овощей;\n\
               - салат/блюдо с белой заправкой (майонез/сметана/йогурт) -> «майонез», ~20% массы;\n\
               - жареные/тушёные овощи, масло не видно -> «подсолнечное масло», ~8-10% массы овощей.\n\
               Округляй граммы масла/майонеза до целых. Не добавляй скрытый жир к сырым продуктам, фруктам, напиткам, хлебу и явно обезжиренным блюдам.\n\
            Правила:\n\
            - Название — на РУССКОМ, ОДНО каноническое название (1-3 слова), БЕЗ СКОБОК и пояснений. Пиши конкретное слово: «омлет» (НЕ «яйца (омлет)»); «укроп» (НЕ «зелень (укроп)»).\n\
            - Сомневаешься, есть ли предмет — ПРОПУСТИ.\n\
            - Верни ТОЛЬКО JSON, без прозы: {\"items\":[{\"name\":\"огурец\",\"grams\":200,\"confidence\":0.7,\"inferred\":false},{\"name\":\"подсолнечное масло\",\"grams\":20,\"confidence\":0.4,\"inferred\":true}]}".to_string(),
        crate::services::i18n::Lang::En => "You are a nutrition vision assistant. The photo shows a dish/plate/meal. Output ONLY food that is actually VISIBLE. Do not invent likely side dishes.\n\
            Reason to yourself, then emit STRICT JSON.\n\
            Steps:\n\
            1. List EACH distinguishable component as a SEPARATE item — including cheese/mozzarella, meat/sausages/chicken, egg, nuts, seeds, greens, visible sauce. For a salad/caprese/bowl give the components SEPARATELY (tomatoes, mozzarella, basil...).\n\
               FORBIDDEN: adding a generic item («salad», «mix», «assorted», «caprese», «dish») when its components are already listed separately; counting the same food twice; splitting one food into two.\n\
            2. Portion: find a scale reference (dinner plate rim ~26 cm, dessert plate ~20 cm, fork ~19 cm, hand, coin). Estimate grams from plate coverage and typical density. No reference — estimate a standard portion. grams is ONE number, not a range. Large portions on big plates are often UNDERestimated — do not lowball them.\n\
            3. confidence 0-1 per item.\n\
            4. Hidden fats (oil/mayo not directly visible) add as SEPARATE items with inferred=true:\n\
               - salad with no visible dressing -> «sunflower oil», ~10% of the mass of the greens/vegetables;\n\
               - salad/dish with a white dressing (mayo/sour cream/yogurt) -> «mayonnaise», ~20% of the mass;\n\
               - fried/stewed vegetables, oil not visible -> «sunflower oil», ~8-10% of the mass of the vegetables.\n\
               Round oil/mayo grams to whole numbers. Do NOT add hidden fat to raw products, fruit, drinks, bread, or clearly fat-free dishes.\n\
            Rules:\n\
            - The name is in ENGLISH, ONE canonical name (1-3 words), WITHOUT PARENTHESES or explanations. Write the concrete word: «omelette» (NOT «eggs (omelette)»); «dill» (NOT «greens (dill)»).\n\
            - If you are unsure whether an item is there — SKIP it.\n\
            - Return ONLY JSON, no prose: {\"items\":[{\"name\":\"cucumber\",\"grams\":200,\"confidence\":0.7,\"inferred\":false},{\"name\":\"sunflower oil\",\"grams\":20,\"confidence\":0.4,\"inferred\":true}]}".to_string(),
    }
}

// ── Client-side parse helpers for the raw model text ──

/// Trim + strip code fences, then take the substring from the first `{` to the last
/// `}` and parse it as JSON. On failure return an Err including a truncated snippet.
fn extract_json_value(raw: &str) -> Result<serde_json::Value, String> {
    let cleaned = strip_code_fences(raw.trim());
    // Take the OUTERMOST JSON container: whichever of `{` / `[` appears first,
    // matched to the last corresponding `}` / `]`. The model sometimes returns a
    // bare array instead of the documented object — assuming `{` there would slice
    // `{a},{b},{c}` out of `[{a},{b},{c}]` and fail with "trailing characters".
    let obj_start = cleaned.find('{');
    let arr_start = cleaned.find('[');
    let (close, start) = match (obj_start, arr_start) {
        (Some(o), Some(a)) => {
            if a < o {
                (']', a)
            } else {
                ('}', o)
            }
        }
        (Some(o), None) => ('}', o),
        (None, Some(a)) => (']', a),
        (None, None) => return Err(format!("no JSON in model output: {}", snippet(cleaned))),
    };
    let slice = match cleaned.rfind(close) {
        Some(e) if e >= start => &cleaned[start..=e],
        _ => return Err(format!("unterminated JSON in model output: {}", snippet(cleaned))),
    };
    serde_json::from_str(slice)
        .map_err(|e| format!("JSON parse error: {e}, raw: {}", snippet(cleaned)))
}

/// A short, safe (char-boundary) prefix of `s` for error messages.
fn snippet(s: &str) -> String {
    const MAX: usize = 400;
    if s.len() <= MAX {
        s.to_string()
    } else {
        let mut end = MAX;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// Remove any `(...)` parenthetical segments from a name and normalise whitespace.
/// Manual depth-scan (no regex crate).
fn strip_parens(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
            }
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse the RAW label-OCR model text into a `QueueResult`.
///
/// Обёртку-массив вокруг единственного объекта снимаем так же, как в [`parse_one`]:
/// сторонний провайдер иногда отдаёт этикетку как `[{…}]`.
fn parse_label_result(raw: &str) -> Result<QueueResult, String> {
    let value = match extract_json_value(raw)? {
        serde_json::Value::Array(mut items) if items.len() == 1 => items.remove(0),
        other => other,
    };
    serde_json::from_value(value).map_err(|e| format!("label result parse error: {e}"))
}

/// Parse the RAW food-items model text into a `DetectedFood` list. Requires an
/// `items` array (a missing one is a real error, not an empty plate). Per item:
/// require `name` (str) + `grams` (f64 ≥ 0); strip parentheticals from the name;
/// clamp confidence to 0..1 (default 0.0); `inferred` default false.
fn parse_food_items(raw: &str) -> Result<Vec<DetectedFood>, String> {
    let value = extract_json_value(raw)?;
    // Accept both the documented `{"items":[…]}` and a bare `[…]` the model
    // sometimes returns instead.
    let items = match value.as_array() {
        Some(arr) => arr,
        None => value
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("no `items` array in model output: {}", snippet(raw)))?,
    };
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        let Some(name) = it.get("name").and_then(|v| v.as_str()) else { continue };
        let Some(grams) = it.get("grams").and_then(|v| v.as_f64()) else { continue };
        if grams < 0.0 {
            continue;
        }
        let name = strip_parens(name);
        if name.is_empty() {
            continue;
        }
        let confidence = it.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0).clamp(0.0, 1.0);
        let inferred = it.get("inferred").and_then(|v| v.as_bool()).unwrap_or(false);
        out.push(DetectedFood { name, grams, confidence, inferred });
    }
    Ok(out)
}

/// Submit label image(s) to the ocr-queue; returns the job id immediately.
/// The job is then processed asynchronously on-prem — poll it via `poll_vision`.
pub async fn submit_vision(input: &AiVisionInput) -> Result<String, String> {
    let submit_body = serde_json::json!({
        "images": input.images,
        "prompt": label_prompt(&input.custom_nutrients),
    });
    let (status, text) = queue_request("POST", "/submit", Some(&submit_body)).await?;
    if status == 402 {
        return Err("HTTP 402: subscription_required".to_string());
    }
    if status != 200 {
        return Err(format!("submit HTTP {status}: {text}"));
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("job_id").and_then(|j| j.as_str()).map(String::from))
        .ok_or_else(|| format!("no job_id in response: {text}"))
}

/// THE POLLER. One queue-status poll. Used while the job is `queued` (to show
/// the position) and to detect the transition to `processing` / `done`.
/// Transient network/5xx errors report as still-`Queued` so we keep waiting.
pub async fn poll_queue(job_id: &str, input: &AiVisionInput) -> Result<QueuePhase, String> {
    let (st, body) = queue_request("GET", &format!("/job/{job_id}"), None).await?;
    if st != 200 {
        return Ok(QueuePhase::Queued { position: 0, since_ms: 0.0 });
    }
    let job: QueueJob = serde_json::from_str(&body).map_err(|e| format!("job parse: {e}, raw: {body}"))?;
    Ok(match job.status.as_str() {
        "done" => QueuePhase::Done(map_result(
            input,
            parse_label_result(&job.result.ok_or_else(|| "job done but no result".to_string())?)?,
        )),
        "error" => QueuePhase::Error(job.error.unwrap_or_else(|| "recognition failed".to_string())),
        "processing" => QueuePhase::Processing { since_ms: job.started_at.unwrap_or(job.created_at) },
        _ => QueuePhase::Queued { position: job.position, since_ms: job.created_at },
    })
}

/// THE STREAMING. Used while the job is `processing`: opens the worker's SSE
/// stream and invokes `on_progress(llm_phase, thinking_tokens, answer_tokens)`
/// live (phase: 0=none, 1=thinking, 2=answer). Returns the final result.
pub async fn stream_vision(
    job_id: &str,
    input: &AiVisionInput,
    on_progress: impl Fn(u8, u32, u32),
) -> Result<AiLookupOutput, String> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}/stream/{job_id}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    if !resp.ok() {
        return Err(format!("stream HTTP {}", resp.status()));
    }
    let body = resp.body().ok_or_else(|| "no stream body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader =
        body.get_reader().dyn_into().map_err(|_| "no stream reader".to_string())?;

    let mut buf = String::new();
    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(|e| format!("stream read: {e:?}"))?;
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok().and_then(|d| d.as_bool()).unwrap_or(false);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|e| format!("{e:?}"))?;
        let bytes = js_sys::Uint8Array::new(&value).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(idx) = buf.find("\n\n") {
            let event = buf[..idx].to_string();
            buf.replace_range(..idx + 2, "");
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data: ") else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("progress") => {
                        let phase = match v.get("phase").and_then(|p| p.as_str()) {
                            Some("thinking") => 1u8,
                            Some("answer") => 2u8,
                            _ => 0u8,
                        };
                        let tt = v.get("thinking_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        let at = v.get("answer_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        on_progress(phase, tt, at);
                    }
                    Some("done") => {
                        // The relayed `result` is now the RAW model text (a JSON string).
                        let raw = v.get("result").and_then(|r| r.as_str())
                            .ok_or_else(|| "done event missing string result".to_string())?;
                        let r = parse_label_result(raw)?;
                        return Ok(map_result(input, r));
                    }
                    Some("error") => {
                        return Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("recognition failed").to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    Err("stream ended without result".to_string())
}

fn map_result(input: &AiVisionInput, r: QueueResult) -> AiLookupOutput {
    // Custom nutrients come back keyed by the requested key; remap to the
    // display name and wrap each scalar as an exact range.
    let mut nutrients = BTreeMap::new();
    for spec in &input.custom_nutrients {
        if let Some(val) = r.custom_nutrients.get(&spec.key).and_then(|v| v.as_f64()) {
            nutrients.insert(spec.name.clone(), exact_range(val, &spec.unit_label));
        }
    }
    AiLookupOutput {
        name: r.product_name,
        kcal: exact_range(r.energy_kcal.unwrap_or(0.0), "kcal"),
        protein: exact_range(r.protein_g.unwrap_or(0.0), "g"),
        fat: exact_range(r.fat_g.unwrap_or(0.0), "g"),
        carbs: exact_range(r.carbs_g.unwrap_or(0.0), "g"),
        nutrients,
        package_weight: r.package_weight_g,
    }
}

// --- Картинки НАПРЯМУЮ через ai-worker (сторонний провайдер) ------------------
//
// Второй путь для фотографий, помимо очереди ocr-queue: изображение уходит в наш
// ai-worker обычным `/chat/completions`, а он по имени модели маршрутизирует
// запрос стороннему провайдеру. Очереди и опроса статуса здесь нет — запрос
// живёт ровно столько, сколько отвечает модель, а прогресс идёт потоком SSE.
//
// Путь включается ИМЕНЕМ МОДЕЛИ в конфиге (`vision_model`): пусто — работает
// прежняя очередь, задано — картинки идут сюда.

/// Модель для картинок, если прямой путь включён конфигом.
pub fn direct_vision_model() -> Option<&'static str> {
    let m = config::get().vision_model.trim();
    (!m.is_empty()).then_some(m)
}

/// Каким путём отправлять картинку.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisionRoute {
    /// Очередь ocr-queue и свой сервер — путь по умолчанию, пока он жив.
    Queue,
    /// Прямо в ai-worker к стороннему провайдеру — за деньги, поэтому только
    /// когда свой сервер молчит.
    Direct,
}

/// Спросить очередь, давно ли приходил поллер, и выбрать путь.
///
/// Свой сервер бесплатный, поэтому он всегда в приоритете: на сторону уходим
/// ТОЛЬКО когда поллер молчит дольше своего окна (решает сама очередь) или когда
/// её воркер недоступен. Если прямой путь не настроен моделью, выбора нет —
/// работаем очередью и получаем её обычную ошибку.
pub async fn pick_vision_route() -> VisionRoute {
    if direct_vision_model().is_none() {
        return VisionRoute::Queue;
    }
    match poller_alive().await {
        Some(true) => VisionRoute::Queue,
        // Молчит или до очереди не достучаться — картинке нужен другой путь.
        _ => VisionRoute::Direct,
    }
}

/// `Some(true|false)` — очередь ответила; `None` — до неё не достучаться.
async fn poller_alive() -> Option<bool> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}/poller-status");
    let window = web_sys::window()?;
    let resp_val = JsFuture::from(window.fetch_with_str(&url)).await.ok()?;
    let resp: web_sys::Response = resp_val.dyn_into().ok()?;
    if !resp.ok() {
        return None;
    }
    let text = JsFuture::from(resp.text().ok()?).await.ok()?.as_string()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("alive")
        .and_then(|v| v.as_bool())
}

/// Потолок ответа для картиночных запросов: этикетка — короткий JSON, список еды
/// с тарелки — десяток строк. Столько хватает обоим с запасом.
const VISION_MAX_TOKENS: u32 = 2000;

/// Один запрос «промпт + картинки» в ai-worker, потоком. Возвращает СЫРОЙ текст
/// ответа модели — разбор остаётся на месте вызова, как и в очереди.
///
/// `on_progress(phase, thinking_tokens, answer_tokens)`: 1 = рассуждение,
/// 2 = ответ — те же фазы, что рисует кнопка распознавания.
async fn vision_chat(
    prompt: &str,
    images: &[String],
    on_progress: impl Fn(u8, u32, u32),
) -> Result<String, String> {
    let model = direct_vision_model().ok_or_else(|| "vision_model not configured".to_string())?;
    let base = &config::get().ai_base_url;
    let url = format!("{base}/chat/completions");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    // Картинки лежат как голый base64 (см. `file_to_jpeg_base64`), а провайдер ждёт
    // data-URL.
    let mut parts = vec![serde_json::json!({ "type": "text", "text": prompt })];
    for img in images {
        parts.push(serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:image/jpeg;base64,{img}") },
        }));
    }
    let body = serde_json::json!({
        "model": model,
        "stream": true,
        "max_tokens": VISION_MAX_TOKENS,
        "messages": [{ "role": "user", "content": parts }],
    });

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body.to_string()));
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(v) => v,
        Err(e) => {
            super::net::note_failure(super::net::Worker::Ai);
            return Err(format!("{e:?}"));
        }
    };
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    if !resp.ok() {
        let status = resp.status();
        let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
            .await
            .ok()
            .and_then(|t| t.as_string())
            .unwrap_or_default();
        return Err(format!("HTTP {status}: {text}"));
    }

    collect_sse(resp, on_progress).await
}

/// Собрать ответ из потока SSE (кадры OpenAI): `delta.content` склеивается в ответ,
/// `reasoning_content` только считается — в ответ рассуждение не попадает.
///
/// `on_progress(фаза, токены рассуждения, токены ответа)`: 1 — рассуждение, 2 — ответ.
/// Передавайте `|_, _, _| {}`, если показывать прогресс некому.
///
/// ВСЕ запросы к моделям — потоковые, поэтому разбор ответа живёт здесь один на всех.
async fn collect_sse(
    resp: web_sys::Response,
    on_progress: impl Fn(u8, u32, u32),
) -> Result<String, String> {
    let stream = resp.body().ok_or_else(|| "no stream body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader =
        stream.get_reader().dyn_into().map_err(|_| "no stream reader".to_string())?;

    let mut buf = String::new();
    let mut answer = String::new();
    let (mut think_tokens, mut answer_tokens) = (0u32, 0u32);
    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(|e| format!("stream read: {e:?}"))?;
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|d| d.as_bool())
            .unwrap_or(false);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|e| format!("{e:?}"))?;
        let bytes = js_sys::Uint8Array::new(&value).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));

        // Событие SSE кончается пустой строкой; неполный хвост ждёт следующего чанка.
        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].trim().to_string();
            buf.replace_range(..idx + 1, "");
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
            // Ошибку провайдера воркер кладёт в поток тем же кадром.
            if let Some(err) = v.get("error") {
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("stream error");
                return Err(msg.to_string());
            }
            let Some(delta) = v.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")) else {
                continue;
            };
            // Рассуждение приезжает в своём поле — у разных провайдеров под разными
            // именами; в ответ оно не попадает.
            if let Some(r) = delta
                .get("reasoning_content")
                .or_else(|| delta.get("reasoning"))
                .and_then(|r| r.as_str())
            {
                if !r.is_empty() {
                    think_tokens += 1;
                    on_progress(1, think_tokens, answer_tokens);
                }
            }
            if let Some(c) = delta.get("content").and_then(|c| c.as_str()) {
                if !c.is_empty() {
                    answer.push_str(c);
                    answer_tokens += 1;
                    on_progress(2, think_tokens, answer_tokens);
                }
            }
        }
    }

    if answer.trim().is_empty() {
        return Err("empty response from vision model".to_string());
    }
    Ok(answer)
}

/// Этикетка НАПРЯМУЮ: тот же промпт и тот же разбор, что у очереди, но без неё.
pub async fn vision_direct(
    input: &AiVisionInput,
    on_progress: impl Fn(u8, u32, u32),
) -> Result<AiLookupOutput, String> {
    let raw = vision_chat(&label_prompt(&input.custom_nutrients), &input.images, on_progress).await?;
    Ok(map_result(input, parse_label_result(&raw)?))
}

/// Фото блюда НАПРЯМУЮ: список распознанной еды, тот же промпт и разбор.
pub async fn food_items_direct(
    images: &[String],
    on_progress: impl Fn(u8, u32, u32),
) -> Result<Vec<DetectedFood>, String> {
    let raw = vision_chat(&food_items_prompt(), images, on_progress).await?;
    parse_food_items(&raw)
}

// --- Food-items (dish photo → list of foods) ---------------------------------
//
// Same ocr-queue plumbing as label OCR (`submit_vision`/`poll_queue`/`stream_vision`)
// but with the "food_items" job kind: the on-prem poller runs a different prompt
// and returns a LIST of detected foods (name + estimated grams + confidence +
// inferred-fat flag) instead of one per-100g nutrition label. The DO relays the
// result JSON verbatim; only this parser differs. See docs/food-photo-recognition.md.

/// One food-items status poll. Mirrors `QueueJob`; `result` is the RAW model text,
/// parsed client-side by `parse_food_items`.
#[derive(Deserialize)]
struct FoodItemsQueueJob {
    status: String,
    #[serde(default)]
    position: u32,
    #[serde(default)]
    created_at: f64,
    #[serde(default)]
    started_at: Option<f64>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Queue state for the food-items job. Parallel to `QueuePhase`, but `Done`
/// carries the detected food list.
pub enum FoodItemsPhase {
    Queued { position: u32, since_ms: f64 },
    Processing { since_ms: f64 },
    Done(Vec<DetectedFood>),
    Error(String),
}

/// Submit dish photo(s) to the ocr-queue with `kind = "food_items"`; returns the
/// job id immediately. Same as `submit_vision` but carries NO custom_nutrients
/// (the food-items pass returns names+grams only; КБЖУ comes downstream).
pub async fn submit_food_items(input: &AiFoodItemsInput) -> Result<String, String> {
    let submit_body = serde_json::json!({
        "images": input.images,
        "prompt": food_items_prompt(),
    });
    let (status, text) = queue_request("POST", "/submit", Some(&submit_body)).await?;
    if status == 402 {
        return Err("HTTP 402: subscription_required".to_string());
    }
    if status != 200 {
        return Err(format!("submit HTTP {status}: {text}"));
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("job_id").and_then(|j| j.as_str()).map(String::from))
        .ok_or_else(|| format!("no job_id in response: {text}"))
}

/// One queue-status poll for a food-items job. Copy of `poll_queue` but returns
/// `FoodItemsPhase::Done(items)` (no custom-nutrient remap). Transient errors
/// report as still-`Queued` so we keep waiting.
pub async fn poll_food_items(job_id: &str) -> Result<FoodItemsPhase, String> {
    let (st, body) = queue_request("GET", &format!("/job/{job_id}"), None).await?;
    if st != 200 {
        return Ok(FoodItemsPhase::Queued { position: 0, since_ms: 0.0 });
    }
    let job: FoodItemsQueueJob =
        serde_json::from_str(&body).map_err(|e| format!("job parse: {e}, raw: {body}"))?;
    Ok(match job.status.as_str() {
        "done" => FoodItemsPhase::Done(parse_food_items(
            &job.result.ok_or_else(|| "job done but no result".to_string())?,
        )?),
        "error" => FoodItemsPhase::Error(job.error.unwrap_or_else(|| "recognition failed".to_string())),
        "processing" => FoodItemsPhase::Processing { since_ms: job.started_at.unwrap_or(job.created_at) },
        _ => FoodItemsPhase::Queued { position: job.position, since_ms: job.created_at },
    })
}

/// Stream a food-items job while it's `processing`. Copy of `stream_vision`:
/// opens the worker's SSE stream, reports live `on_progress(phase, thinking,
/// answer)`, and on the terminal `"done"` event parses the result as a food list.
pub async fn stream_food_items(
    job_id: &str,
    on_progress: impl Fn(u8, u32, u32),
) -> Result<Vec<DetectedFood>, String> {
    let base = &config::get().ocr_queue_base_url;
    let url = format!("{base}/stream/{job_id}");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");
    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);
    let request = web_sys::Request::new_with_str_and_init(&url, &opts).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request)).await.map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;
    if !resp.ok() {
        return Err(format!("stream HTTP {}", resp.status()));
    }
    let body = resp.body().ok_or_else(|| "no stream body".to_string())?;
    let reader: web_sys::ReadableStreamDefaultReader =
        body.get_reader().dyn_into().map_err(|_| "no stream reader".to_string())?;

    let mut buf = String::new();
    loop {
        let chunk = JsFuture::from(reader.read()).await.map_err(|e| format!("stream read: {e:?}"))?;
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok().and_then(|d| d.as_bool()).unwrap_or(false);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).map_err(|e| format!("{e:?}"))?;
        let bytes = js_sys::Uint8Array::new(&value).to_vec();
        buf.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(idx) = buf.find("\n\n") {
            let event = buf[..idx].to_string();
            buf.replace_range(..idx + 2, "");
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data: ") else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else { continue };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("progress") => {
                        let phase = match v.get("phase").and_then(|p| p.as_str()) {
                            Some("thinking") => 1u8,
                            Some("answer") => 2u8,
                            _ => 0u8,
                        };
                        let tt = v.get("thinking_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        let at = v.get("answer_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        on_progress(phase, tt, at);
                    }
                    Some("done") => {
                        // The relayed `result` is now the RAW model text (a JSON string).
                        let raw = v.get("result").and_then(|r| r.as_str())
                            .ok_or_else(|| "done event missing string result".to_string())?;
                        return parse_food_items(raw);
                    }
                    Some("error") => {
                        return Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("recognition failed").to_string());
                    }
                    _ => {}
                }
            }
        }
    }
    Err("stream ended without result".to_string())
}

/// The best-matching candidate id (or null) plus a 0..1 confidence and a short reason.
// Used by `match_food` through the schema-injected JSON mode of `execute`.
#[derive(Debug, Deserialize, JsonSchema)]
struct MatchResult {
    /// The `id` of the best-matching candidate, or null if none is a real match.
    #[serde(default)]
    match_id: Option<String>,
    /// Confidence 0..1 that `match_id` is the same food as the detected name.
    confidence: f64,
    /// Short reason for the verdict (unused by the caller; keeps the model honest).
    #[serde(default)]
    #[allow(dead_code)]
    reason: String,
}

/// Match a detected food NAME against the user's existing foods. Given the
/// detected name and a list of candidate `(id, name)` foods, returns the best
/// match `id` and a 0..1 confidence, or `None` if the model finds no real match.
/// Uses the SAME qwen3 execute stack as `lookup`. Thresholds are applied by the
/// caller (≥0.8 auto, 0.5–0.8 suggested, <0.5/None → lookup).
pub async fn match_food(
    detected: &str,
    candidates: &[(String, String)],
) -> Result<(Option<String>, f64), String> {
    // No candidates to match against — skip the LLM call entirely.
    if candidates.is_empty() {
        return Ok((None, 0.0));
    }
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let list = candidates
        .iter()
        .map(|(id, name)| format!("- id={id}: {name}"))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "You match a detected food to an existing food catalog. The detected food name \
         (in {lang}) is: \"{detected}\".\n\n\
         Candidate foods (choose from these ids ONLY):\n{list}\n\n\
         Pick the candidate that is the SAME food as the detected one (same product/dish, \
         allowing spelling/word-order/synonym differences). If NONE of the candidates is \
         genuinely the same food, set match_id to null. Do NOT force a weak match.\n\n\
         Set confidence to your certainty (0..1) that match_id is the same food: near 1.0 \
         for an obvious match, ~0.6 for a plausible-but-uncertain one, near 0 when nothing \
         fits. Respond with ONLY a single minified JSON object.",
        lang = lang,
        detected = detected,
        list = list,
    );

    let result: MatchResult = generate(prompt, |_| {}).await?;
    let confidence = result.confidence.clamp(0.0, 1.0);
    // A confident-but-unknown id is meaningless — drop match_ids not in the list.
    let match_id = result
        .match_id
        .filter(|id| candidates.iter().any(|(cid, _)| cid == id));
    Ok((match_id, confidence))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn print_schema() {
        let schema = schemars::schema_for!(NutritionResponse);
        println!("{}", serde_json::to_string_pretty(&schema).unwrap());
    }

    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Otvet {
        verdict: bool,
        reason: String,
    }

    #[test]
    fn otvet_obyektom_i_v_obyortke_massiva_odno_i_to_zhe() {
        let obj = r#"{"verdict":true,"reason":"мясо"}"#;
        let arr = r#"[{"verdict":true,"reason":"мясо"}]"#;
        let fenced = "```json\n[{\"verdict\":true,\"reason\":\"мясо\"}]\n```";
        let want = Otvet { verdict: true, reason: "мясо".into() };
        assert_eq!(parse_one::<Otvet>(obj).unwrap(), want);
        assert_eq!(parse_one::<Otvet>(arr).unwrap(), want);
        assert_eq!(parse_one::<Otvet>(fenced).unwrap(), want);
    }

    #[test]
    fn massiv_iz_dvuh_otvetov_eto_oshibka_a_ne_obyortka() {
        let two = r#"[{"verdict":true,"reason":"а"},{"verdict":false,"reason":"б"}]"#;
        assert!(parse_one::<Otvet>(two).is_err());
    }

    #[test]
    fn parses_tool_call() {
        let c = parse_tool_call("[[tool]] read_progress {\"days\": 30}").unwrap();
        assert_eq!(c.name, "read_progress");
        assert_eq!(c.arguments, serde_json::json!({ "days": 30 }));
    }

    #[test]
    fn tool_call_without_args_defaults_empty_object() {
        let c = parse_tool_call("[[tool]] escalate_to_human").unwrap();
        assert_eq!(c.name, "escalate_to_human");
        assert_eq!(c.arguments, serde_json::json!({}));
    }

    #[test]
    fn no_tool_call_in_plain_answer() {
        assert!(parse_tool_call("[[final]] You are doing great.").is_none());
        assert!(parse_tool_call("Just a plain answer.").is_none());
    }

    #[test]
    fn strips_final_marker() {
        assert_eq!(strip_final_marker("[[final]] hello"), "hello");
        assert_eq!(strip_final_marker("plain, no marker"), "plain, no marker");
    }

    /// The vision model sometimes returns a BARE array instead of the documented
    /// `{"items":[…]}`. This is the exact shape that produced the user's
    /// "trailing characters at line 6 column 6" (first-`{`..last-`}` slicing).
    #[test]
    fn parses_bare_array_food_items() {
        let raw = r#"[
            { "name": "спиральчатый спагетти", "grams": 250, "confidence": 0.9, "inferred": false },
            { "name": "курица жареная", "grams": 150, "confidence": 0.8, "inferred": false },
            { "name": "огурец", "grams": 80, "confidence": 0.9, "inferred": false }
        ]"#;
        let items = parse_food_items(raw).expect("bare array must parse");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].name, "спиральчатый спагетти");
        assert_eq!(items[0].grams, 250.0);
        assert_eq!(items[1].name, "курица жареная");
        assert_eq!(items[2].grams, 80.0);
    }

    /// The documented object shape must keep working (regression guard).
    #[test]
    fn parses_items_object_food_items() {
        let raw = r#"{"items":[{"name":"рис","grams":200,"confidence":0.7}]}"#;
        let items = parse_food_items(raw).expect("items object must parse");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "рис");
        assert!(!items[0].inferred);
    }

    /// Feed a marker split across arbitrary chunk boundaries; the marker itself
    /// must never leak, only the answer after it.
    #[test]
    fn control_filter_hides_final_marker_across_chunks() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["[", "[fin", "al]] He", "llo!"] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "Hello!");
    }

    /// A whole tool-call turn must be fully suppressed from the visible stream.
    #[test]
    fn control_filter_suppresses_tool_turn() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["[[tool]] read_pro", "gress {\"days\": 7}"] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "");
    }

    /// A plain answer (no markers) streams through unchanged.
    #[test]
    fn control_filter_passes_plain_answer() {
        let mut f = ControlFilter::default();
        let mut out = String::new();
        for chunk in ["Your weight ", "is trending down."] {
            out.push_str(&f.push(chunk));
        }
        assert_eq!(out, "Your weight is trending down.");
    }

    // ── Пессимизация профиля блюда ──────────────────────────────────────────

    use super::pessimise_dish_fat;

    fn ratio(p: &api_types::FatProfile) -> f64 {
        (p.mufa_pct + p.pufa_pct) / p.sfa_pct
    }

    /// Морские омега-3 блюду не засчитываются никогда: их источник в названии не
    /// виден, а ошибка в плюс закрывает недельную норму рыбой, которой не было.
    #[test]
    fn dish_never_carries_epa_dha() {
        let p = pessimise_dish_fat(20.0, 40.0, 30.0);
        assert_eq!(p.epa_dha_pct, 0.0);
    }

    /// Слишком хороший ответ прижимается НИЖЕ нормы — планку блюдом не закрыть.
    #[test]
    fn dish_ratio_is_pushed_below_the_target() {
        let p = pessimise_dish_fat(10.0, 50.0, 30.0); // отношение 8.0
        assert!(ratio(&p) < crate::services::fats::UNSAT_TO_SAT_MIN,
            "получилось {}", ratio(&p));
    }

    /// Ответ ХУЖЕ нормы остаётся нетронутым: пессимизм не значит «портить всё».
    #[test]
    fn dish_worse_than_target_is_left_alone() {
        let p = pessimise_dish_fat(40.0, 20.0, 10.0); // отношение 0.75
        assert_eq!((p.sfa_pct, p.mufa_pct, p.pufa_pct), (40.0, 20.0, 10.0));
    }

    /// Сумма кислот сохраняется: сдвигаются доли внутри неё, а не объём жира.
    #[test]
    fn dish_pessimism_keeps_the_total() {
        let p = pessimise_dish_fat(10.0, 50.0, 30.0);
        let total = p.sfa_pct + p.mufa_pct + p.pufa_pct;
        assert!((total - 90.0).abs() < 1e-9, "сумма стала {total}");
    }

    /// Без насыщенных отношение не определено — делить не на что, ответ как есть.
    #[test]
    fn dish_without_saturated_is_left_alone() {
        let p = pessimise_dish_fat(0.0, 60.0, 30.0);
        assert_eq!((p.sfa_pct, p.mufa_pct, p.pufa_pct), (0.0, 60.0, 30.0));
    }
}


// ── Выгрузка промптов о нутриентах для замеров ───────────────────────────────
//
// Та же причина, что и у конвейера признаков (см. `flags_pipeline::prompt_dump`):
// замер обязан гонять текст и схему, которые реально уходят в модель, а не их
// рукописную копию.
#[cfg(test)]
pub(crate) fn prompt_dump() -> serde_json::Value {
    const FOOD: &str = "{{FOOD}}";
    const IDENT: &str = "{{IDENTITY}}";
    let known = format!("Our automatic classifier says this product is: {IDENT}\n\n");

    serde_json::json!({
        "calcium": {
            "prompt": calcium_prompt(FOOD, &known),
            "schema": schemars::schema_for!(CalciumAnswer),
        },
        "fat_composite": {
            "prompt": composite_prompt(FOOD, &known),
            "schema": schemars::schema_for!(CompositeAnswer),
        },
        "fat_profile": {
            "prompt": basic_fat_prompt(FOOD, &known, "Russian"),
            "schema": schemars::schema_for!(FatProfileAnswer),
        },
    })
}
