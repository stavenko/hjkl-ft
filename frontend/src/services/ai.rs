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
/// structured request takes — `lookup`, `lookup_nutrient`, `lookup_iron`,
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

// A focused, FLAT single-nutrient answer. Deliberately not the nested
// `NutrientDetail` (value+unit) shape: the batched `lookup` asks for kcal plus a
// `custom_nutrients` MAP of nested value/unit objects, and qwen3 reliably corrupts
// that deep structure (observed live: `,{"zhelezo"` instead of `,"zhelezo":`). One
// nutrient, three plain numbers + a comment keeps the model focused and the JSON
// trivially well-formed.
//
// NB: `///` here becomes the schema's `description`, and the ai-worker pastes the
// schema into the prompt verbatim — so `///` text is READ BY THE MODEL. This block
// is developer rationale, hence `//`. Written as `///` it made the model answer with
// the schema document itself instead of a value, and the whole nutrient pass wrote
// nothing to the database.
#[derive(Debug, Deserialize, JsonSchema)]
struct SingleNutrient {
    /// Lowest reasonable amount per 100 g (a plain number in the requested unit).
    min_value: f64,
    /// Highest reasonable amount per 100 g (a plain number in the requested unit).
    max_value: f64,
    /// Most likely amount per 100 g (a plain number in the requested unit).
    recommended: f64,
    /// Short explanation of the value, in the requested language.
    comment: String,
}

/// Look up ONE nutrient for a food, relying ONLY on the food NAME. Simplified,
/// focused counterpart to `lookup`: a flat prompt + flat schema so the model isn't
/// juggling five nutrients and a nested structure at once (which broke it). Same
/// streaming `execute` path as the working `lookup`. Returns the recommended amount
/// per 100 g in `unit`. Retries on a transient empty / unparseable response. FAIL
/// LOUDLY after the retries.
pub async fn lookup_nutrient(food_name: &str, nutrient: &str, unit: &str) -> Result<f64, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = format!(
        "You are a nutritional database. For the food item \"{food_name}\", give the amount of \
         {nutrient} per 100 grams, as a plain number in {unit}.\n\n\
         For raw/dry as-sold products (grains, rice, pasta, flour, meat, fish, legumes) use the \
         RAW value unless the name says cooked/boiled/fried/steamed.\n\n\
         Provide:\n\
         - min_value: lowest reasonable amount (number, in {unit})\n\
         - max_value: highest reasonable amount (number, in {unit})\n\
         - recommended: the most likely amount (number, in {unit})\n\
         - comment: brief explanation in {lang}\n\n\
         Base the answer only on the food name \"{food_name}\". All values are per 100 g in {unit} \
         — bare numbers, no unit text. Respond with ONLY a single minified JSON object and nothing \
         else — no markdown, no prose."
    );

    // Тот же бракет, что у железа: сначала диапазон, потом значение внутри него.
    // Модель, поставившая своё «наиболее вероятное» вне собственных min…max, себе
    // противоречит — такой ответ выбрасывается, а не записывается в базу.
    let v: SingleNutrient = generate_validated(prompt, |_| {}, 3, |v: &SingleNutrient| {
        if v.min_value <= v.recommended && v.recommended <= v.max_value && v.min_value >= 0.0 {
            Ok(())
        } else {
            Err(format!(
                "{nutrient} contradicts itself for «{food_name}»: {}…{}…{} {unit}",
                v.min_value, v.recommended, v.max_value
            ))
        }
    })
    .await?;
    Ok(v.recommended)
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
        examples: "йогурт, греческий йогурт, сметана, сливочный сыр без соли" },
    CalciumCategory { key: "cottage_cheese", mg_min: 80.0, mg_max: 200.0,
        examples: "творог, зернёный творог, творожная масса, сырники" },
    CalciumCategory { key: "milk_concentrated", mg_min: 250.0, mg_max: 1300.0,
        examples: "сухое молоко, сгущённое молоко, молочная сыворотка сухая" },
    CalciumCategory { key: "cream_whey", mg_min: 50.0, mg_max: 110.0,
        examples: "сливки, сыворотка, мороженое пломбир" },
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
    CalciumCategory { key: "fish_plain", mg_min: 0.0, mg_max: 0.0,
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
    CalciumCategory { key: "vegetables_other", mg_min: 25.0, mg_max: 90.0,
        examples: "брокколи, цветная капуста, белокочанная капуста, пекинская капуста, \
                   стручковая фасоль, репа" },
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
        examples: "мясо, птица, рыба без костей, яйца, крупы, макароны, хлеб, овощи, фрукты, \
                   масло, сахар, сладости, вода, чай, кофе, алкоголь — всё, чего нет выше" },
];

/// Таблица кальция как текст промпта — из того же массива, что и проверка.
fn calcium_category_table() -> String {
    CALCIUM_CATEGORIES
        .iter()
        .map(|c| {
            format!(
                "  {key:<20} {min}–{max} мг — {ex}",
                key = c.key, min = c.mg_min, max = c.mg_max, ex = c.examples
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Модель называет строку и величину внутри неё. `///` здесь читается моделью —
// developer-пояснения держим в `//`, как и у железа.
#[derive(Debug, Deserialize, JsonSchema)]
struct CalciumDetail {
    /// The category key from the table, copied EXACTLY as written there.
    category: String,
    /// What this product actually is, in two or three words.
    food_type: String,
    /// Lowest reasonable CALCIUM CONTENT per 100 g, in milligrams.
    min_value: f64,
    /// Highest reasonable CALCIUM CONTENT per 100 g, in milligrams.
    max_value: f64,
    /// Most likely CALCIUM CONTENT per 100 g, in milligrams.
    recommended: f64,
    /// One short sentence: why this category and this amount. Keep it under 20 words.
    reason: String,
}

/// Сколько кальция в продукте, мг на 100 г. Строку выбирает модель, величину
/// ограничивает наша таблица; продукт, не попавший ни в одну строку, даёт 0.
pub async fn lookup_calcium(food_name: &str) -> Result<f64, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = format!(
        "You are a nutritional database. For the food item \"{food_name}\", report how much \
         CALCIUM it contains per 100 grams, in MILLIGRAMS.\n\n\
         For raw/dry as-sold products (grains, seeds, legumes, flour) use the RAW value unless \
         the name says cooked/boiled/soaked.\n\n\
         First place the food in ONE row of this table, then give a value INSIDE that row's \
         range:\n\n\
         {table}\n\n\
         Report:\n\
         - category: the row key, copied EXACTLY as written above\n\
         - food_type: what this product is, in two or three words, in {lang}\n\
         - min_value / max_value / recommended: milligrams per 100 g, min ≤ recommended ≤ max\n\
         - reason: ONE short sentence (under 20 words) in {lang} — why this row and this amount\n\n\
         Rules for choosing the row:\n\
         - Cheese goes by HARDNESS: hard and dry → cheese_hard, semi-hard and brined → \
           cheese_semi, soft and fresh → cheese_soft, melted/processed → cheese_processed.\n\
         - A plant drink counts as fortified ONLY if the name says so («обогащённое», «с \
           кальцием», «fortified»). Otherwise it is plant_milk_plain, which has very little.\n\
         - Canned fish counts as fish_with_bones only when the bones are eaten (sardines, \
           sprats, canned salmon). Fillet without bones is other_none.\n\
         - Anything not covered by a row above is other_none, and its value is 0. Meat, fish \
           fillet, eggs, cereals, bread, pasta, vegetables, fruit, oils, sweets and drinks all \
           go there — they carry some calcium, but too little to count.\n\n\
         Base the answer only on the food name \"{food_name}\". Respond with ONLY a single \
         minified JSON object and nothing else — no markdown, no prose.",
        table = calcium_category_table(),
        lang = lang,
    );

    let v: CalciumDetail = generate_validated(prompt, |_| {}, 4, |v: &CalciumDetail| {
        if !(v.min_value <= v.recommended && v.recommended <= v.max_value && v.min_value >= 0.0) {
            return Err(format!(
                "calcium contradicts itself: {}…{}…{} мг",
                v.min_value, v.recommended, v.max_value
            ));
        }
        let key = v.category.trim().to_ascii_lowercase();
        let Some(cat) = CALCIUM_CATEGORIES.iter().find(|c| c.key == key) else {
            return Err(format!("unknown calcium category «{}» for «{food_name}»", v.category));
        };
        if !(cat.mg_min - 1e-9..=cat.mg_max + 1e-9).contains(&v.recommended) {
            return Err(format!(
                "calcium {} мг outside «{}» ({}–{} мг) for «{food_name}»",
                v.recommended, cat.key, cat.mg_min, cat.mg_max
            ));
        }
        Ok(())
    })
    .await?;
    leptos::logging::log!(
        "calcium «{food_name}»: {} мг · {} — {} ({})",
        v.recommended, v.category, v.food_type, v.reason
    );
    Ok(v.recommended)
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

// Модель отвечает ТОЛЬКО про содержание железа: min / max / most-likely — тот же
// бракет, что у любого другого нутриента. Доля усвоения у неё не спрашивается: она
// её не знает (без таблицы даёт чечевице и творогу те же 0.20, что и говядине), а с
// таблицей в промпте начинала списывать её числа в поле миллиграммов. Долю даёт
// `IRON_CATEGORIES` по выбранной строке.
//
// NB: `///` doc comments on this struct and its fields are turned into the JSON
// schema's `description` strings by `schemars`, and the ai-worker pastes that schema
// into the prompt verbatim. Everything written with `///` here is therefore READ BY
// THE MODEL — keep it addressed to the model, and put developer rationale in `//`
// comments like this one.
// ШАГ 1 — только КЛАССИФИКАЦИЯ. Ни одного числа: этому промпту таблица показывается
// без диапазонов, поэтому списать оттуда нечего, и вся его работа — узнать продукт.
// Совмещённый промпт (категория + величина разом) путался: арбуз получал усвоение
// 0.15 — долю мясной строки, — а голец объявлялся говядиной.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IronCategoryPick {
    /// The category key from the list, copied EXACTLY as written there.
    pub(crate) category: String,
    /// What this product actually is, in two or three words.
    pub(crate) food_type: String,
    /// One short sentence: why this category. Keep it under 15 words.
    pub(crate) reason: String,
}

// ШАГ 2 — только ВЕЛИЧИНА. Категория уже выбрана и передана готовой; здесь называется
// одно количество внутри её диапазона, и выбирать больше нечего.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IronAmount {
    /// Lowest reasonable IRON CONTENT per 100 g, in milligrams.
    pub(crate) min_value_iron: f64,
    /// Highest reasonable IRON CONTENT per 100 g, in milligrams.
    pub(crate) max_value_iron: f64,
    /// Most likely IRON CONTENT per 100 g, in milligrams.
    pub(crate) recommended_iron: f64,
    /// How sure you are of `recommended_iron`, from 0.0 to 1.0. Be honest: 1.0 means you
    /// know THIS food's iron content; 0.0 means you are guessing from the category alone.
    pub(crate) iron_confidence: f64,
    /// One short sentence: why this amount. Keep it under 15 words.
    pub(crate) reason: String,
}


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

/// Составное ли это блюдо (жир из нескольких источников), а не базовый продукт.
pub async fn is_composite_dish(food_name: &str) -> Result<bool, String> {
    let prompt = format!(
        "Answer ONE question about the food \"{food_name}\", and nothing else. Food names may \
         be in ANY language — judge by meaning, not by wording.\n\n\
         QUESTION: does the FAT of this food come from SEVERAL DIFFERENT SOURCES mixed \
         together?\n\n\
         A food whose fat comes from ONE source is NOT a composite dish, however processed it \
         is. These are of one source:\n\
         — the flesh, organs, fat or skin of one animal, bird or fish, raw or cooked;\n\
         — one plant and what is pressed or ground from it: an oil, a nut, a seed, a grain, a \
         vegetable, a fruit;\n\
         — milk and everything made from milk alone: butter, cream, cheese, curd, yoghurt;\n\
         — eggs;\n\
         — a factory product built around one of these: sausage, ham, tinned fish, nut paste.\n\n\
         A food whose fat comes from SEVERAL sources IS a composite dish. Typically these are \
         cooked from a recipe: stuffed vegetables, cabbage rolls, cutlets and meatballs, \
         casseroles and bakes, pies and pastry with a filling, pilaf, stews and soups, salads \
         with dressing, sandwiches, ready meals.\n\n\
         THINK IT THROUGH FIRST in the reason field — name what this food is made of and where \
         its fat comes from — and let the verdict follow from it.\n\n\
         Respond with ONLY a single minified JSON object.",
    );
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
pub async fn lookup_fat_profile(food_name: &str) -> Result<api_types::FatProfile, String> {
    if is_composite_dish(food_name).await? {
        return lookup_dish_fat_profile(food_name).await;
    }
    lookup_basic_fat_profile(food_name).await
}

/// Профиль жира БАЗОВОГО продукта. Промпт — дословно тот, что прошёл замер.
pub async fn lookup_basic_fat_profile(food_name: &str) -> Result<api_types::FatProfile, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = format!(
        "You are a nutritional database. For the food item \"{food_name}\", report the \
         FATTY-ACID PROFILE OF ITS FAT.\n\n\
         Every number you report is a PERCENT OF THIS FOOD'S TOTAL FAT, by weight — NOT grams \
         per 100 g of food. A food's total fat is already known to us; you only describe how \
         that fat is composed. This means the profile does NOT depend on how much fat the food \
         has: pure sunflower oil and a sauce made from it share the same profile.\n\n\
         First place the food in ONE row of this table — rows are by the TYPE OF FAT, not by \
         the type of dish — then give values INSIDE that row's ranges:\n\n\
         {table}\n\n\
         Report:\n\
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
         Base the answer only on the food name \"{food_name}\". Respond with ONLY a single \
         minified JSON object and nothing else — no markdown, no prose.",
        table = fat_row_table(),
        lang = lang,
    );

    let v: FatProfileAnswer = generate_validated(prompt, |_| {}, 4, |v: &FatProfileAnswer| {
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

    let key = v.category.trim().to_ascii_lowercase();
    let row = FAT_ROWS
        .iter()
        .find(|r| r.key == key)
        .ok_or_else(|| format!("unknown fat row «{}»", v.category))?;
    let clamp = |x: f64, (lo, hi): (f64, f64)| x.clamp(lo, hi);
    let pufa = clamp(v.pufa_pct, row.pufa);
    // EPA+DHA — ЧАСТЬ ПНЖК, поэтому зажимается ещё и под неё: замер поймал ответы,
    // где часть превышала целое.
    let epa_dha = clamp(v.epa_dha_pct, row.epa_dha).min(pufa);
    let profile = api_types::FatProfile {
        sfa_pct: clamp(v.sfa_pct, row.sfa),
        mufa_pct: clamp(v.mufa_pct, row.mufa),
        pufa_pct: pufa,
        epa_dha_pct: epa_dha,
    };
    leptos::logging::log!(
        "жиры «{food_name}»: {} · {} — НЖК {:.0} МНЖК {:.0} ПНЖК {:.0} EPA+DHA {:.0} ({})",
        row.key, v.food_type, profile.sfa_pct, profile.mufa_pct, profile.pufa_pct,
        profile.epa_dha_pct, v.reason
    );
    crate::services::telemetry::report_detection(
        "fat.row",
        food_name,
        row.key,
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
async fn lookup_dish_fat_profile(food_name: &str) -> Result<api_types::FatProfile, String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    let prompt = format!(
        "You are a nutritional database. \"{food_name}\" is a COMPOSITE DISH — its fat comes \
         from several ingredients at once. Report the FATTY-ACID PROFILE OF ITS FAT.\n\n\
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
         Base the answer only on the dish name \"{food_name}\". Respond with ONLY a single \
         minified JSON object and nothing else — no markdown, no prose.",
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

/// Список продуктов для промпта: «0. имя». Механика, не текст.
fn numbered(names: &[String]) -> String {
    names
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{i}. {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Сверить длины, записать вердикты в лог и отправить их в телеметрию
/// определений. Тоже механика: к модели отсюда не уходит ни одного слова.
///
/// `label` — для лога, по-русски; `kind` — устойчивый ключ, по которому вердикты
/// группируются в аналитике и который нельзя менять задним числом. ПУСТОЙ `kind`
/// значит «не отправлять»: у гема отсюда идут отдельные голоса, а в аналитику
/// должен попасть их итог, иначе один продукт даст три записи с разными ответами.
fn take_flags(
    label: &str,
    kind: &str,
    names: &[String],
    verdict: Vec<bool>,
    reason: Vec<String>,
) -> Result<Vec<bool>, String> {
    let n = names.len();
    if verdict.len() != n || reason.len() != n {
        return Err(format!(
            "{label}: length mismatch for {n} foods — verdict={}, reason={}",
            verdict.len(),
            reason.len()
        ));
    }
    for (i, name) in names.iter().enumerate() {
        leptos::logging::log!("{label} «{name}»: {} — {}", verdict[i], reason[i]);
        if !kind.is_empty() {
            crate::services::telemetry::report_detection(
                kind, name, &verdict[i].to_string(), &reason[i], &[],
            );
        }
    }
    Ok(verdict)
}

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
fn veg_fruit_from_category(category: &str) -> Result<bool, String> {
    let c = category.trim().to_ascii_uppercase();
    if c.starts_with("NONE") {
        return Ok(false);
    }
    if c.contains("VEGETABLE") || c.contains("FRUIT") || c.contains("BERR") || c.contains("DISH") {
        return Ok(true);
    }
    Err(format!("фрукты/овощи: неизвестная категория «{category}»"))
}

/// Овощ или фрукт — свежий, приготовленный или очевидное овощное/фруктовое блюдо.
///
/// Прежний промпт был вопросом «да/нет» с отрицательным перечнем в хвосте, и он
/// НЕ РАБОТАЛ: модель писала верную причину и ставила противоположный вердикт —
/// «Яблоко → false, apple is a fruit», «Огурец → false, it is a cucumber». Замер
/// живым путём давал 9/22, причём мимо шли самые обыкновенные яблоко с огурцом, а
/// не спорные случаи. Виновата тощая форма: причина ограничивалась десятью словами
/// и вырождалась в метку («apple»), вердикт рождался отдельным массивом, и следовать
/// ему было не за чем.
///
/// Теперь та же форма, что у гема, где она даёт 22/22: опознание отдельным полем,
/// категории заданы ПОЛОЖИТЕЛЬНО, причина называет подошедшую категорию.
///
/// Ягоды названы прямо. Именно на них сломался исходный случай — «Мороженая вишня»:
/// модель опознавала её верно («frozen cherry»), но в вопросе «vegetable or fruit»
/// ягоде места не находилось. Слова о заморозке и готовке сути продукта не меняют —
/// сказано отдельно, как и в геме.
pub async fn classify_veg_fruit(names: &[String]) -> Result<Vec<bool>, String> {
    let prompt = format!(
        "For each food below decide whether it belongs to one of the categories listed. Food \
         names may be in ANY language — judge by meaning, not by wording.\n\n\
         THE CATEGORIES:\n\
         — VEGETABLES: any plant part eaten as a vegetable — roots, tubers, leaves, stalks, \
         cabbages, squashes, onions, garlic, tomatoes, cucumbers, peppers, green beans, peas, \
         mushrooms. A GRAIN is not a vegetable, whatever it was cooked into;\n\
         — FRUITS, and BERRIES COUNT AS FRUITS: apples, pears, citrus, bananas, grapes, melons, \
         and berries such as cherries, sour cherries, strawberries, raspberries, blueberries, \
         currants, cranberries;\n\
         — DISHES MADE MAINLY OF THEM: salads, stewed or roasted vegetables, vegetable soups, \
         fruit salads.\n\n\
         For EVERY food, answer in THREE fields, in this order.\n\
         First, in \"what_the_person_most_likely_ate\": say what this food MOST LIKELY IS, \
         WITHOUT yet thinking about the categories at all. Every name here was typed by a person \
         into their FOOD DIARY, so it always names something eaten — never a material, a device \
         or a term from another trade, however much the word may look like one. Say it in \
         ENGLISH and say WHAT KIND of thing it is — \"cherry, a berry\", \"buckwheat, a grain\", \
         \"cod, a fish\". Never merely repeat the name back: a name copied out is not an answer. \
         And name it WHOLE — whatever the name says was added to it or made of it, syrup, sugar, \
         batter, juice, belongs in your answer too, not just the main word.\n\
         Second, in \"category_that_fits\": write the ONE word naming the category that fits \
         what you have just named — VEGETABLES, FRUITS or DISH — or the word NONE if no category \
         fits. Nothing else, one word. Never list the categories that do not fit: running \
         through them turns into denying them all, the right one included.\n\
         Each array holds exactly one item per food, never more.\n\n\
         Sugar changes what a food is: jam, preserves, fruit in syrup, candied fruit and juices \
         belong to none of the categories, however much fruit went into them. A composite dish \
         belongs to a category only when vegetables or fruit are its MAIN part. Words about \
         preparation, storage, freezing, packaging, cut, grade or country of origin never \
         change what a food is — frozen, dried and cooked produce stays produce.\n\n\
         Foods (index. name):\n{list}\n\n\
         Respond with ONLY a single minified JSON object: the two arrays described above — \
         one item per food in each — both in the SAME order as the foods above.",
        list = numbered(names),
    );
    let n = names.len();
    // Проверка ПЕРЕД разбором ответа: кривые длины и незнакомая категория стоят
    // попытки и переспрашиваются, а не превращаются в тихое «нет».
    let v: VegFruitAnswer = generate_validated(prompt, |_| {}, 3, move |a: &VegFruitAnswer| {
        if a.what_the_person_most_likely_ate.len() != n || a.category_that_fits.len() != n {
            return Err(format!(
                "фрукты/овощи: length mismatch for {n} foods — what_ate={}, category={}",
                a.what_the_person_most_likely_ate.len(),
                a.category_that_fits.len()
            ));
        }
        for c in &a.category_that_fits {
            veg_fruit_from_category(c)?;
        }
        Ok(())
    })
    .await?;
    let verdict = v
        .category_that_fits
        .iter()
        .map(|c| veg_fruit_from_category(c))
        .collect::<Result<Vec<bool>, String>>()?;
    // Опознание идёт в лог и телеметрию вместе с категорией: именно оно объясняет
    // промахи — по одной категории не видно, за что модель приняла продукт.
    let reason = v
        .what_the_person_most_likely_ate
        .iter()
        .zip(v.category_that_fits.iter())
        .map(|(ate, cat)| format!("{ate} → {cat}"))
        .collect();
    take_flags("фрукты/овощи", "flag.veg_fruit", names, verdict, reason)
}

// ── Источник гемового железа ─────────────────────────────────────────────────

// Комментарии обычные, а НЕ доковые: см. предупреждение над `VegFruitAnswer` —
// док-комментарий уезжает в json_schema как "description" и возвращается моделью
// вместо данных.
//
// Обоснование стоит ПЕРВЫМ — см. замер в шапке раздела. Опознание вынесено в СВОЁ
// поле: раньше оно писалось в графу «почему продукт отнесён к источникам гема», а
// имя поля модель видит в схеме, то есть оно работает как часть промпта — и
// заставляло записывать «что это за еда» в графу, само название которой уже
// утверждает вывод, причём в одну сторону.
#[derive(Debug, Deserialize, JsonSchema)]
struct HemeAnswer {
    what_the_person_most_likely_ate: Vec<String>,
    reason_why_this_product_classified_as_gem_source: Vec<String>,
    verdict: Vec<bool>,
}

/// Богатый источник ГЕМОВОГО железа.
///
/// Промпт — ПЕРЕЧЕНЬ КАТЕГОРИЙ и один вопрос: относится ли продукт к одной из них.
/// Ни шагов, ни списка исключений: и то и другое ставило модель перед ложным
/// выбором между двумя знакомыми словами — «печень или курица», — и на границе
/// («бедро куриное печёное») она выбирала слово, а не признак. Категории заданы
/// положительно; всё, что в них не попало, не попало — перечислять это незачем.
///
/// Про рыбу в промпте нет НИ ОДНОГО названия: категории лишь говорят, куда рыбу
/// отнести, если модель её узнает (печень — да, филе — нет). Опознание целиком на
/// модели, и на редких промысловых названиях она его проваливала: «Голец» она
/// сочла органом рыбы, отсюда и гем. Лечится не правилом, а рамкой — оговоркой,
/// что слово взято из ДНЕВНИКА ПИТАНИЯ и потому означает съеденное, а не материал,
/// прибор или термин чужого ремесла. Замер живым путём: набор редких рыб 1/10 → 10/10,
/// основной набор не пострадал (старая формулировка 21/20/20, новая 21/20/19 из 22 —
/// разница меньше разброса между прогонами одной и той же версии).
///
/// Категория названа МЯСОМ МЛЕКОПИТАЮЩИХ, а не красным мясом, хотя означает ровно
/// его. С прежним названием «бедро куриное печёное» уходило в неё единогласно —
/// дословно «Chicken thigh → RED MEAT», 3:0, то есть голосование тут не спасает.
/// Оговорка про MAMMALS в теле правила стояла и тогда, но проигрывала внешнему
/// знанию, где тёмное мясо птицы сплошь и рядом зовут красным. С названием
/// категории это знание спорит, с определением внутри неё — нет, поэтому заменено
/// именно название: под «мясо млекопитающих» курица не подходит никак.
///
/// Колбаса, сосиски и ветчина названы в категории прямо. Гем в них ЕСТЬ — по ГОСТ
/// мясо занимает почти весь батон, — и признак говорит об этом честно: съевшему
/// колбасы незачем добирать гем отдельно. Индикаторы существуют не затем, чтобы
/// быть зелёными. То, что это ПРИ ЭТОМ мясо глубокой переработки, говорит свой
/// индикатор — [`super::processed_meat`]; это разные разговоры и разные признаки.
pub async fn classify_heme(names: &[String]) -> Result<Vec<bool>, String> {
    let prompt = format!(
        "For each food below decide whether it belongs to one of the categories listed. Food \
         names may be in ANY language — judge by meaning, not by wording.\n\n\
         THE CATEGORIES OF RICH HEME-IRON SOURCES:\n\
         — LIVER itself — of any animal, bird or fish — and food made mainly of liver: pâté, \
         liver sausage, liver cake;\n\
         — OTHER INNER ORGANS themselves — hearts, kidneys, tongues, gizzards, lungs, blood \
         sausage — of any animal, bird or fish. It is the ORGAN that belongs to this category, \
         never the animal's flesh: a fish fillet, a bird's breast or leg is not an organ;\n\
         — MEAT OF MAMMALS: beef, veal, pork, lamb, mutton, goat, and the meat of other farmed \
         or wild mammals — venison, elk, boar, horse, rabbit — in any cut, ground or whole, raw \
         or cooked, and food made mainly of such meat: sausage, frankfurters, ham, bacon, \
         salami, mince, meatballs, cutlets, stew, canned meat. A creature that is not a mammal \
         has no meat in this category;\n\
         — MOLLUSCS OF THESE KINDS ONLY: mussels, oysters, clams and vongole, cockles, octopus, \
         whelks, winkles. The category is these kinds, not the zoological class: a mollusc of \
         another kind does not belong to it.\n\n\
         For EVERY food, answer in THREE fields, in this order.\n\
         First, in \"what_the_person_most_likely_ate\": say what this food MOST LIKELY IS, \
         WITHOUT yet thinking about heme iron or the categories at all. Every name here was \
         typed by a person into their FOOD DIARY, so it always names something eaten — never \
         a material, a device or a term from another trade, however much the word may look \
         like one. Name which animal, bird, fish or plant it comes from, and whether it is \
         that creature's FLESH, an ORGAN, or something else entirely.\n\
         Second, in the reason field: name the ONE category that fits what you have just \
         named, or say that none fits. Name only the category that FITS — never list the ones \
         that do not: running through them turns into denying them all, the right one \
         included.\n\
         Third, the verdict: true if it belongs to a category, false if it does not. Each \
         array holds exactly one item per food, never more.\n\n\
         Do not reason about how many milligrams of iron a food holds — you do not know those \
         numbers, and the categories already account for them. A composite dish belongs to a \
         category only when such an ingredient is its MAIN part. Words about preparation, \
         storage, packaging, cut, grade or country of origin never change what a food is.\n\n\
         Foods (index. name):\n{list}\n\n\
         Respond with ONLY a single minified JSON object: the three arrays described above — \
         one item per food in each — all in the SAME order as the foods above.",
        list = numbered(names),
    );
    // Сколько раз спросить и взять большинство.
    //
    // Голосование заводилось, когда качалось «бедро куриное печёное»: в одном
    // прогоне «нет», в другом «да». Потом выяснилось, что от той ошибки оно не
    // защищало вовсе — бедро уходило в красное мясо единогласно, 3:0. Причина была
    // в названии категории, и её починили названием; после этого замер дал 22/22
    // три прогона подряд.
    //
    // Отсюда и проверка: нужно ли голосование теперь, когда промпт перестал
    // ошибаться систематически. Цена ошибки прежняя — у бедра 73 г белка, и ложное
    // «да» превращает его в 2.9 порции гема, то есть закрывает недельную планку
    // одним блюдом, — но платится ОДИН раз на продукт за всю его жизнь.
    const ROUNDS: usize = 1;
    let mut votes: Vec<[u8; 2]> = vec![[0, 0]; names.len()];
    let mut last_reason: Vec<String> = vec![String::new(); names.len()];
    for _ in 0..ROUNDS {
        let v: HemeAnswer = generate(prompt.clone(), |_| {}).await?;
        if v.what_the_person_most_likely_ate.len() != names.len() {
            return Err(format!(
                "гем: length mismatch for {} foods — what_ate={}",
                names.len(),
                v.what_the_person_most_likely_ate.len()
            ));
        }
        let verdict = take_flags(
            "гем",
            "", // в аналитику уйдёт ИТОГ голосования, а не каждый голос
            names,
            v.verdict,
            v.reason_why_this_product_classified_as_gem_source.clone(),
        )?;
        for (i, yes) in verdict.into_iter().enumerate() {
            votes[i][usize::from(yes)] += 1;
            // Опознание хранится вместе с обоснованием: именно оно объясняет
            // промахи вроде «Голец → печень рыбы», а по одной категории этого
            // не видно.
            last_reason[i] = format!(
                "{} → {}",
                v.what_the_person_most_likely_ate[i],
                v.reason_why_this_product_classified_as_gem_source[i]
            );
        }
    }
    let verdict: Vec<bool> = votes.iter().map(|v| v[1] > v[0]).collect();
    for (i, name) in names.iter().enumerate() {
        leptos::logging::log!(
            "гем «{name}»: {} голосами {}:{} — {}",
            verdict[i], votes[i][1], votes[i][0], last_reason[i]
        );
        // Голоса уходят вместе с вердиктом: единогласное «да» и «да» со счётом
        // 2:1 — разные вещи, и вторая как раз и есть повод для своего замера.
        crate::services::telemetry::report_detection(
            "flag.heme",
            name,
            &verdict[i].to_string(),
            &last_reason[i],
            &[votes[i][1].into(), votes[i][0].into()],
        );
    }
    Ok(verdict)
}

// ── Молочно-жировая глобула ──────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct MilkGlobuleAnswer {
    reason_why_this_product_classified_as_milk_fat_globule: Vec<String>,
    verdict: Vec<bool>,
}

/// Жир продукта заключён в целую молочно-жировую глобулу.
///
/// Граница ровно одна и она механическая: мембрану рвёт СБИВАНИЕ и вытапливание.
/// Всё, что ими получено (масло сливочное, топлёное, безводный молочный жир,
/// молочный жир внутри кондитерки), глобулы не имеет; всё остальное молочное —
/// молоко, сливки, сыр, творог, йогурт, кефир — имеет.
pub async fn classify_milk_globule(names: &[String]) -> Result<Vec<bool>, String> {
    let prompt = format!(
        "Answer ONE yes/no question about each food below, and nothing else. Food names may be \
         in ANY language — judge by meaning, not by wording.\n\n\
         QUESTION: is the fat of this food still enclosed in INTACT MILK FAT GLOBULES?\n\
         Milk fat leaves the udder wrapped in a membrane. Only three things destroy that \
         membrane: CHURNING, RENDERING and MELTING WITH EMULSIFYING SALTS. Nothing else does — \
         not fermenting, not souring, not heating, not ageing, not salting, not freezing, not \
         drying, not concentrating.\n\
         Decide in THIS ORDER and STOP at the first step that matches.\n\
         STEP 1 — does this food contain MILK FAT at all? If it is not dairy — meat, poultry, \
         fish, eggs, vegetable oils, nuts, seeds, plants, or a plant «milk» made of soy, oat, \
         almond or coconut — then FALSE, there is no milk fat to ask about.\n\
         STEP 2 — was that milk fat CHURNED out (butter, buttermilk-made spreads), RENDERED \
         (ghee, clarified butter, butter oil, anhydrous milk fat) or MELTED WITH EMULSIFYING \
         SALTS (processed cheese, cheese spread, cheese in a tub)? Then FALSE. The same applies \
         when a cook or a factory added the milk fat AS BUTTER rather than as milk or cream: \
         milk chocolate, buttercream, shortcrust pastry, croissants, waffles, biscuits are \
         FALSE.\n\
         STEP 3 — otherwise the fat is still in its native globules: TRUE. This is the normal \
         case for dairy, and it holds however the product was processed short of churning — \
         milk of any fat content, cream, sour cream, kefir, ryazhenka, ayran, acidophilus, \
         drinking and thick yogurt, cottage cheese of any fat content including fat-free, curd \
         mass, ricotta, natural cheeses (hard, semi-hard, soft, brined), condensed and powdered \
         milk, whey, ice cream, and dishes whose fat comes mainly from these (cheesecake made \
         of cottage cheese, creamy sauce, milk porridge). Do NOT hedge here: a cheese or a \
         cottage cheese that was not churned HAS intact globules, even though the curd was \
         pressed, salted, aged or heated.\n\
         The answer depends ONLY on WHAT THE FOOD IS and whether its milk fat was churned, \
         rendered or salt-melted out; fat percentage, brand, packaging and country never change \
         it.\n\n\
         For EVERY food fill the reason field FIRST — name the step that matched, in one short \
         sentence, at most 10 words — and let the verdict follow from it.\n\n\
         Foods (index. name):\n{list}\n\n\
         Respond with ONLY a single minified JSON object: the reason array — one string per \
         food — and \"verdict\" — one boolean per food, both in the SAME order as the foods \
         above.",
        list = numbered(names),
    );
    let v: MilkGlobuleAnswer = generate(prompt, |_| {}).await?;
    take_flags(
        "молочная глобула",
        "flag.milk_globule",
        names,
        v.verdict,
        v.reason_why_this_product_classified_as_milk_fat_globule,
    )
}

// ── Красное мясо ─────────────────────────────────────────────────────────────

/// Обоснование первым — как у остальных признаков.
#[derive(Debug, Deserialize, JsonSchema)]
struct RedMeatAnswer {
    reason_why_this_product_classified_as_red_meat: Vec<String>,
    verdict: Vec<bool>,
}

/// Красное мясо — мышечная ткань млекопитающих и страуса.
///
/// Промпт устроен как гемовый: перечень категорий и один вопрос о принадлежности.
/// Отличие от гема — здесь считается ИМЕННО МЯСО, а не органы: печень и сердце в
/// гем входят, а в недельные граммы красного мяса нет, и путать их нельзя.
pub async fn classify_red_meat(names: &[String]) -> Result<Vec<bool>, String> {
    let prompt = format!(
        "For each food below decide whether it belongs to one of the categories listed. Food \
         names may be in ANY language — judge by meaning, not by wording.\n\n\
         THE CATEGORIES OF RED MEAT:\n\
         — THE FLESH of MAMMALS: beef, veal, pork, lamb, mutton, goat, horse, rabbit, and the \
         flesh of wild mammals — venison, elk, boar. Any cut, whole or ground, raw or cooked;\n\
         — THE FLESH of the OSTRICH and other ratites;\n\
         — FOOD MADE MAINLY OF SUCH FLESH: sausages, frankfurters, ham, bacon, salami, mince, \
         meatballs, cutlets, stew, canned meat — whatever the flesh has been through.\n\n\
         It is the FLESH — muscle — that belongs to these categories. INNER ORGANS do not: \
         liver, heart, kidney, tongue, lung, tripe and dishes made mainly of them are outside, \
         however red they look. Neither do BIRDS other than ratites — chicken, turkey, duck, \
         goose — nor fish, seafood, eggs, dairy or anything from a plant.\n\n\
         For EVERY food, first THINK IT THROUGH in the reason field: whose flesh is this, and \
         is it flesh at all? Name the category, or say that none of them fits. One short \
         sentence, at most 10 words. Then give the verdict: true if it belongs to a category, \
         false if it does not.\n\n\
         A composite dish belongs to a category only when such flesh is its MAIN part. Words \
         about preparation, storage, packaging, cut, grade or country of origin never change \
         what a food is.\n\n\
         Foods (index. name):\n{list}\n\n\
         Respond with ONLY a single minified JSON object: the reason array — one string per \
         food — and \"verdict\" — one boolean per food, both in the SAME order as the foods \
         above.",
        list = numbered(names),
    );
    let v: RedMeatAnswer = generate(prompt, |_| {}).await?;
    take_flags(
        "красное мясо",
        "flag.red_meat",
        names,
        v.verdict,
        v.reason_why_this_product_classified_as_red_meat,
    )
}

// ── Мясо глубокой переработки ────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct ProcessedMeatAnswer {
    reason_why_this_product_classified_as_processed_meat: Vec<String>,
    verdict: Vec<bool>,
}

/// Мясо глубокой переработки — консервированное ради хранения, а не просто
/// приготовленное.
///
/// Граница механическая, как у молочной глобулы: важно не «полуфабрикат ли это» и
/// не «вредно ли», а прошло ли мясо КОНСЕРВАЦИЮ — засол с нитритом, копчение,
/// вяление, ферментацию. Домашняя котлета из фарша её не проходила, магазинная
/// сосиска проходила, и различие между ними именно в этом, а не в том, где куплено.
pub async fn classify_processed_meat(names: &[String]) -> Result<Vec<bool>, String> {
    let prompt = format!(
        "Answer ONE yes/no question about each food below, and nothing else. Food names may be \
         in ANY language — judge by meaning, not by wording.\n\n\
         QUESTION: is this meat PRESERVED — cured, smoked, salted, dried or fermented for \
         keeping?\n\n\
         Meat is preserved when it has been through one of these: CURING with nitrite or \
         nitrate salt, SMOKING, prolonged SALTING, AIR-DRYING, or FERMENTATION. Cooking is not \
         preserving: boiling, frying, baking, stewing, grilling, mincing, freezing and \
         packaging leave meat unpreserved, however industrial the process.\n\n\
         PRESERVED, therefore TRUE: sausages and frankfurters, wieners, salami and other dry \
         sausages, ham, bacon, gammon, pastrami, prosciutto and jamon, bresaola, basturma, \
         smoked and cured brisket, liver sausage and pâté made with cure, hot dogs, corned \
         beef, canned luncheon meat, jerky. Also a dish whose MAIN meat part is one of these — \
         pizza with salami, pasta carbonara, sausage in a bun, solyanka.\n\n\
         NOT PRESERVED, therefore FALSE: fresh and frozen meat of any kind, mince, cutlets and \
         meatballs, home-made or shop-bought, boiled or baked meat, roast, stew, kebab, \
         dumplings, canned meat that was merely boiled in the tin, poultry and fish that went \
         through none of the treatments above.\n\n\
         For EVERY food, first THINK IT THROUGH in the reason field: is there meat here, and \
         was it preserved — by what treatment? One short sentence, at most 10 words. Then give \
         the verdict.\n\n\
         Foods (index. name):\n{list}\n\n\
         Respond with ONLY a single minified JSON object: the reason array — one string per \
         food — and \"verdict\" — one boolean per food, both in the SAME order as the foods \
         above.",
        list = numbered(names),
    );
    let v: ProcessedMeatAnswer = generate(prompt, |_| {}).await?;
    take_flags(
        "переработанное мясо",
        "flag.processed_meat",
        names,
        v.verdict,
        v.reason_why_this_product_classified_as_processed_meat,
    )
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
fn parse_label_result(raw: &str) -> Result<QueueResult, String> {
    serde_json::from_value(extract_json_value(raw)?)
        .map_err(|e| format!("label result parse error: {e}"))
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

