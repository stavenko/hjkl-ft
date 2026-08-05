//! Определение железа — конвейер `arti_pipes` из двух узлов.
//!
//! Совмещённый промпт делал две разные работы разом и путался: арбуз получал
//! усвоение 0.15 (долю мясной строки), а голец объявлялся говядиной — «Голец - это
//! говядина». Работы разделены по узлам:
//!
//!   1. [`CategoryNode`] — КЛАССИФИКАЦИЯ. Список строк даётся без диапазонов, поэтому
//!      списывать нечего; единственная работа — узнать продукт.
//!   2. [`AmountNode`] — ВЕЛИЧИНА. Категория уже выбрана и передана готовой вместе с
//!      её диапазоном; выбирать больше нечего, надо назвать одно количество.
//!
//! КТО НАЗЫВАЕТ ЧИСЛО — решает уверенность. Модель, которую жёстко инструктируют,
//! свободы не проявляет: она исполняет правило буквально (при запрете отвечать
//! границами брала середину, при правиле «не знаешь — бери низ» брала низ даже для
//! говядины). Но если СПРОСИТЬ её об уверенности, она отвечает честно: замер дал
//! незнакомому опаху 0.0 и прямое «не имею информации», клыкачу 0.3–0.6, а печени,
//! говядине, кунжуту и чечевице — 0.8–0.9, причём значения оказались ближе к
//! справочным, чем наши границы (кунжут 12.7 при справочных 14.6 против 3.5 у нижней
//! границы строки).
//!
//! Поэтому: содержание берётся у модели, когда она уверена, и из нашей строки, когда
//! нет. Доля усвоения — ВСЕГДА из строки: по ней уверенность модели ниже (0.4–0.8), а
//! ошибки грубее — чечевице она давала то 0.02, то 0.3 при верных ~0.05, и именно
//! доля мясной строки, приписанная арбузу, дала 9 мг усвоенного за день.
//!
//! ПОВТОРЫ идут через `select_next_node`: узел, чей ответ не разобрался или не прошёл
//! проверку, возвращает СЕБЯ следующим, пока не исчерпает попытки. Отдельного цикла
//! ретраев здесь нет — ветвление конвейера и есть механизм повтора.
//!
//! Доля усвоения не спрашивается ни на одном шаге: её даёт `IRON_CATEGORIES` по
//! выбранной строке. Модель её не знает — без таблицы она выдаёт чечевице и творогу
//! те же 0.20, что и говядине.

use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use arti_pipes::node::{Node, NodeEvent, NodeRunner, NodeWrapper};
use arti_pipes::pipeline::{run_pipeline, Pipeline};
use arti_pipes::prompt::{Prompt, PromptExecutionEvent};
use futures::stream::LocalBoxStream;
use futures::StreamExt;

use super::ai::{
    build_executor_think, strip_code_fences, IronAmount, IronCategory, IronCategoryPick,
    IRON_CATEGORIES,
};

/// Сколько раз повторить каждый шаг, прежде чем сдаться.
const MAX_TRIES: u32 = 3;

/// С какой уверенности значение модели принимается вместо нашего. Замер: знакомые
/// продукты она оценивает на 0.8–0.9 (печень, говядина, кунжут, чечевица),
/// незнакомые — на 0.0–0.6 (опах прямо писал «не имею информации»). Порог проходит
/// между этими кучами.
const CONFIDENT: f64 = 0.7;

/// Контекст, который течёт по конвейеру.
#[derive(Clone)]
pub struct IronCtx {
    pub food_name: String,
    pub lang: &'static str,
    /// Заполняется первым узлом.
    pub category: Option<&'static IronCategory>,
    pub food_type: String,
    /// Заполняется вторым узлом.
    pub amount_mg: Option<f64>,
    /// Сколько попыток потратил каждый шаг.
    pub tries_pick: u32,
    pub tries_amount: u32,
    /// Последняя причина отказа — попадает в текст ошибки, если конвейер не дошёл.
    pub last_error: Option<String>,
    pub reason_pick: String,
    pub reason_amount: String,
}

impl IronCtx {
    fn new(food_name: &str, lang: &'static str) -> Self {
        Self {
            food_name: food_name.to_string(),
            lang,
            category: None,
            food_type: String::new(),
            amount_mg: None,
            tries_pick: 0,
            tries_amount: 0,
            last_error: None,
            reason_pick: String::new(),
            reason_amount: String::new(),
        }
    }
}

/// Список категорий БЕЗ ЧИСЕЛ — для шага классификации. Диапазоны там не нужны, а их
/// присутствие вредит: модель, увидев числа, начинает ими отвечать.
fn category_keys() -> String {
    IRON_CATEGORIES
        .iter()
        .map(|c| format!("  {key:<20} — {ex}", key = c.key, ex = c.examples))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Общий для обоих промптов прогон: schema-injected JSON mode, оба потока
/// раскручиваются, финальный текст уходит в `Completed`.
fn run_structured<E, S>(executor: E, text: String, name: String) -> LocalBoxStream<'static, PromptExecutionEvent>
where
    E: PromptExecutor + 'static,
    S: schemars::JsonSchema + 'static,
{
    Box::pin(async_stream::stream! {
        yield PromptExecutionEvent::Scheduled(name);
        match executor.execute::<S>(text).await {
            Ok(result) => {
                let mut thinking = result.thinking_stream;
                while let Some(t) = thinking.next().await {
                    match t {
                        Ok(token) => yield PromptExecutionEvent::ThinkingToken(token),
                        Err(e) => yield PromptExecutionEvent::Error(e),
                    }
                }
                let mut content = result.content_stream;
                while let Some(t) = content.next().await {
                    match t {
                        Ok(token) => yield PromptExecutionEvent::ContentToken(token),
                        Err(e) => yield PromptExecutionEvent::Error(e),
                    }
                }
                match result.output.await {
                    Ok(out) => yield PromptExecutionEvent::Completed(out.result),
                    Err(e) => yield PromptExecutionEvent::Error(e),
                }
            }
            Err(e) => yield PromptExecutionEvent::Error(e),
        }
    })
}

// ── Шаг 1: категория ─────────────────────────────────────────────────────────

struct CategoryPrompt {
    food_name: String,
    lang: &'static str,
}

impl Prompt for CategoryPrompt {
    type Output = String;
    type Context = IronCtx;

    fn name(&self) -> String {
        "iron.category".to_string()
    }

    fn serialize(&self) -> String {
        format!(
            "You are a nutritional database. Classify the food item \"{name}\" into ONE of \
             these categories.\n\n\
             {keys}\n\n\
             Report:\n\
             - category: the key, copied EXACTLY as written above\n\
             - food_type: what this product is, in two or three words, in {lang}\n\
             - reason: ONE short sentence (under 15 words) in {lang} — why this category\n\n\
             Rules:\n\
             - ANY fish goes to `fish`, including names you don't recognise. A fish is never \
               `meat_red` — that row is for beef, pork, lamb and the like.\n\
             - Drinks go to `drinks`: water, juice, beer (with or without alcohol), wine, \
               kvass. Only milk, kefir and other MILK drinks go to `dairy`.\n\
             - Fresh fruit and berries → `fruit_fresh`; dried ones → `fruit_dried`.\n\
             - Vegetables → `vegetables`; leafy herbs eaten by the bunch (parsley, dill, \
               rocket) → `greens_herbs`.\n\
             - A cooked dish goes to `dish_with_meat` when it contains meat, otherwise to \
               `dish_meatless`.\n\
             - If several fit, pick the one that dominates the food's iron.\n\n\
             Base the answer only on the food name \"{name}\". Respond with ONLY a single \
             minified JSON object and nothing else — no markdown, no prose.",
            name = self.food_name,
            keys = category_keys(),
            lang = self.lang,
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries_pick += 1;
        match serde_json::from_str::<IronCategoryPick>(strip_code_fences(raw.trim())) {
            Ok(pick) => {
                let key = pick.category.trim().to_ascii_lowercase();
                match IRON_CATEGORIES.iter().find(|c| c.key == key) {
                    Some(cat) => {
                        ctx.category = Some(cat);
                        ctx.food_type = pick.food_type;
                        ctx.reason_pick = pick.reason;
                        ctx.last_error = None;
                    }
                    None => {
                        ctx.last_error =
                            Some(format!("неизвестная категория «{}»", pick.category));
                    }
                }
            }
            Err(e) => ctx.last_error = Some(format!("категория не разобрана: {e}, ответ: {raw}")),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(&self, executor: E) -> LocalBoxStream<'static, PromptExecutionEvent> {
        run_structured::<E, IronCategoryPick>(executor, self.serialize(), self.name())
    }
}

struct CategoryNode {
    executor: Qwen,
}

impl Node for CategoryNode {
    type Prompt = CategoryPrompt;
    type Executor = Qwen;
    type Error = String;
    type Context = IronCtx;

    fn prompt(&self, ctx: &Self::Context) -> Self::Prompt {
        CategoryPrompt { food_name: ctx.food_name.clone(), lang: ctx.lang }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        let prompt = self.prompt(&context);
        let mut stream = prompt.execute(self.prompt_executor());
        Box::pin(async_stream::stream! {
            let id = uuid::Uuid::new_v4();
            let mut out = String::new();
            let mut failed: Option<String> = None;
            while let Some(ev) = stream.next().await {
                match &ev {
                    PromptExecutionEvent::Completed(o) => out = o.clone(),
                    PromptExecutionEvent::Error(e) => failed = Some(format!("{e:?}")),
                    _ => {}
                }
                yield NodeEvent::Prompt(id, ev);
            }
            // Сбой ИСПОЛНЕНИЯ (оборванный fetch, недоступный воркер) — тоже повод
            // повторить, поэтому он ложится в тот же счётчик попыток.
            let ctx = if let Some(e) = failed {
                let mut c = context;
                c.tries_pick += 1;
                c.last_error = Some(e);
                c
            } else {
                prompt.update_context(context, out)
            };
            yield NodeEvent::Completed(ctx);
        })
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        match ctx.category {
            // Категория есть — дальше за величиной.
            Some(_) => Some(Box::new(NodeWrapper::new(AmountNode {
                executor: self.executor.clone(),
            }))),
            // Нет — ПОВТОР этого же узла, пока есть попытки. Это и есть механизм
            // ретраев: ветвление конвейера, а не цикл вокруг него.
            None if ctx.tries_pick < MAX_TRIES => Some(Box::new(NodeWrapper::new(CategoryNode {
                executor: self.executor.clone(),
            }))),
            None => None,
        }
    }
}

// ── Шаг 2: количество ────────────────────────────────────────────────────────

struct AmountPrompt {
    food_name: String,
    food_type: String,
    lang: &'static str,
    cat: &'static IronCategory,
}

impl Prompt for AmountPrompt {
    type Output = String;
    type Context = IronCtx;

    fn name(&self) -> String {
        "iron.amount".to_string()
    }

    fn serialize(&self) -> String {
        format!(
            "You are a nutritional database. For the food item \"{name}\" ({ftype}), report how \
             much IRON it contains per 100 grams, in MILLIGRAMS.\n\n\
             This food belongs to the category `{key}` ({ex}). Foods of this category carry \
             between {min} and {max} mg of iron per 100 g — your answer MUST be inside that \
             range.\n\n\
             For raw/dry as-sold products (grains, rice, pasta, flour, meat, fish, legumes) use \
             the RAW value unless the name says cooked/boiled/fried/steamed.\n\n\
             First bracket the plausible range, then commit to one value inside it:\n\
             - min_value_iron: lowest reasonable amount (number, mg)\n\
             - max_value_iron: highest reasonable amount (number, mg)\n\
             - recommended_iron: the amount for THIS food (number, mg)\n\
             - iron_confidence: how sure you are of `recommended_iron`, from 0.0 to 1.0\n\
             - reason: ONE short sentence (under 15 words) in {lang} — why this amount\n\n\
             All three amounts are MILLIGRAMS per 100 g, and min ≤ recommended ≤ max.\n\n\
             BE HONEST ABOUT `iron_confidence`. It is not a formality:\n\
             - 0.8–1.0 — you KNOW this specific food's iron content.\n\
             - 0.3–0.7 — you are reasoning from a similar food.\n\
             - 0.0–0.2 — you do not know this food at all and are guessing from the category.\n\
             A low number is not a bad answer: when you are unsure we substitute a safe \
             value ourselves. An overstated number is the harmful one — it tells the person \
             they have met an iron target they have not.\n\n\
             Respond with ONLY a single minified JSON object and nothing else — no markdown, \
             no prose.",
            name = self.food_name,
            ftype = self.food_type,
            key = self.cat.key,
            ex = self.cat.examples,
            min = self.cat.mg_min,
            max = self.cat.mg_max,
            lang = self.lang,
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries_amount += 1;
        match serde_json::from_str::<IronAmount>(strip_code_fences(raw.trim())) {
            Ok(v) => {
                let said = v.recommended_iron;
                // Бракет не украшение: модель, поставившая своё «наиболее вероятное»
                // вне собственных min…max, себе противоречит.
                if !(v.min_value_iron <= said && said <= v.max_value_iron && v.min_value_iron >= 0.0)
                {
                    ctx.last_error = Some(format!(
                        "железо противоречит себе: {}…{}…{} мг",
                        v.min_value_iron, said, v.max_value_iron
                    ));
                    return ctx;
                }
                // Замер показал: спрошенная напрямую, модель знает содержание лучше
                // нашей таблицы — кунжуту даёт 12.7 при справочных 14.6, чечевице 6.6
                // при 7.5, тогда как нижняя граница строки давала 3.5 и 5. И она
                // ЧЕСТНО отличает знание от догадки: незнакомый опах получал
                // уверенность 0.0 и прямое «не имею информации», клыкач 0.3–0.6.
                //
                // Поэтому уверенное значение берём у неё, неуверенное — своё. Даже
                // уверенное поджимается границами строки: категория выбрана отдельным
                // шагом и надёжна, так что грубый промах ей уже не пройти.
                let (mg, source) = if v.iron_confidence >= CONFIDENT {
                    (said.clamp(self.cat.mg_min, self.cat.mg_max), "модель")
                } else {
                    (self.cat.mg_min, "низ строки")
                };
                ctx.amount_mg = Some(mg);
                ctx.reason_amount =
                    format!("{} [{source}, уверенность {:.1}]", v.reason, v.iron_confidence);
                ctx.last_error = None;
            }
            Err(e) => ctx.last_error = Some(format!("количество не разобрано: {e}, ответ: {raw}")),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(&self, executor: E) -> LocalBoxStream<'static, PromptExecutionEvent> {
        run_structured::<E, IronAmount>(executor, self.serialize(), self.name())
    }
}

struct AmountNode {
    executor: Qwen,
}

impl Node for AmountNode {
    type Prompt = AmountPrompt;
    type Executor = Qwen;
    type Error = String;
    type Context = IronCtx;

    fn prompt(&self, ctx: &Self::Context) -> Self::Prompt {
        AmountPrompt {
            food_name: ctx.food_name.clone(),
            food_type: ctx.food_type.clone(),
            lang: ctx.lang,
            cat: ctx.category.expect("узел величины запускается только с выбранной категорией"),
        }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        let prompt = self.prompt(&context);
        let mut stream = prompt.execute(self.prompt_executor());
        Box::pin(async_stream::stream! {
            let id = uuid::Uuid::new_v4();
            let mut out = String::new();
            let mut failed: Option<String> = None;
            while let Some(ev) = stream.next().await {
                match &ev {
                    PromptExecutionEvent::Completed(o) => out = o.clone(),
                    PromptExecutionEvent::Error(e) => failed = Some(format!("{e:?}")),
                    _ => {}
                }
                yield NodeEvent::Prompt(id, ev);
            }
            let ctx = if let Some(e) = failed {
                let mut c = context;
                c.tries_amount += 1;
                c.last_error = Some(e);
                c
            } else {
                prompt.update_context(context, out)
            };
            yield NodeEvent::Completed(ctx);
        })
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        match ctx.amount_mg {
            Some(_) => None,
            None if ctx.tries_amount < MAX_TRIES => Some(Box::new(NodeWrapper::new(AmountNode {
                executor: self.executor.clone(),
            }))),
            None => None,
        }
    }
}

// ── Запуск ───────────────────────────────────────────────────────────────────

/// Железо продукта: `(мг на 100 г, доля усвоения)`. FAILS LOUDLY, если конвейер не
/// дошёл до величины за отведённые попытки.
pub async fn lookup_iron(food_name: &str) -> Result<(f64, f64), String> {
    let lang = match crate::services::i18n::get_lang() {
        crate::services::i18n::Lang::Ru => "Russian",
        crate::services::i18n::Lang::En => "English",
    };
    // Thinking OFF: с рассуждением qwen3 паркует короткий ответ в канал размышления и
    // возвращает пустой контент.
    let executor = build_executor_think(false)?;
    let pipeline = Pipeline::new(Box::new(NodeWrapper::new(CategoryNode { executor })));

    let mut stream = run_pipeline(pipeline, IronCtx::new(food_name, lang));
    let mut last: Option<IronCtx> = None;
    while let Some(ev) = stream.next().await {
        if let NodeEvent::Completed(ctx) = ev {
            last = Some(ctx);
        }
    }

    let ctx = last.ok_or_else(|| "конвейер железа не дал результата".to_string())?;
    match (ctx.category, ctx.amount_mg) {
        (Some(cat), Some(mg)) => {
            leptos::logging::log!(
                "iron «{food_name}»: {mg} мг · {} · усвоение {} — {} ({} / {})",
                cat.key,
                cat.absorption,
                ctx.food_type,
                ctx.reason_pick,
                ctx.reason_amount
            );
            Ok((mg, cat.absorption))
        }
        _ => Err(ctx
            .last_error
            .unwrap_or_else(|| "конвейер железа не дошёл до значения".to_string())),
    }
}
