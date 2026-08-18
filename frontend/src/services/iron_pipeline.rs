//! Железо: два запроса одной формы — СКОЛЬКО и НАСКОЛЬКО УСВАИВАЕТСЯ.
//!
//! Оба устроены как кальций, и это не подражание, а вывод из замеров: строку таблицы
//! модель выбирает хорошо (27 из 30), а числа помнит плохо (18 из 30). Мидиям она
//! давала 1.5 мг при справочных 6.7, петрушке 1.1 при 6.2, кунжуту 6.1 при 14.6.
//! Поэтому числа наши, а её работа — узнать продукт и назвать запись.
//!
//! Форма у обоих запросов одна:
//!
//!   1. что человек записал в дневник и что об этом сказало ОПОЗНАНИЕ;
//!   2. СПРАВОЧНИК «продукт → число»: нашёлся — назови его ключ;
//!   3. ТАБЛИЦА КАТЕГОРИЙ — для всего, чего в справочнике нет;
//!   4. один ответ.
//!
//! УЗЛА КЛАССИФИКАЦИИ ЗДЕСЬ БОЛЬШЕ НЕТ. Раньше первым шагом стоял `CategoryNode`,
//! который заодно опознавал продукт своим полем `food_type` — без словаря, без версий
//! с уверенностью — и опознавал плохо: «Голец - это говядина». Теперь опознание
//! приходит готовым из общего узла (`services::flags_pipeline`), и делать его дважды,
//! разными словами, незачем.
//!
//! Доля усвоения устроена так же: свой справочник «продукт → доля» перед глазами,
//! таблица строк с долями следом, и число ВОЗВРАЩАЕТ МОДЕЛЬ — переписав его оттуда,
//! куда сама же отнесла продукт. Оно и идёт в счёт; в коде остаётся лишь зажим в
//! 0…1, потому что доля вне этого промежутка не значит ничего.
//!
//! ПОВТОРЫ идут через `select_next_node`: узел, чей ответ не разобрался, возвращает
//! себя следующим, пока не исчерпает попытки.

use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use arti_pipes::node::{Node, NodeEvent, NodeRunner, NodeWrapper};
use arti_pipes::pipeline::{run_pipeline, Pipeline};
use arti_pipes::prompt::{Prompt, PromptExecutionEvent};
use futures::stream::LocalBoxStream;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::Deserialize;

use super::ai::{
    build_executor_think, iron_reference_table, strip_code_fences, IRON_CATEGORIES, IRON_REFERENCE,
};

/// Сколько раз повторить каждый шаг, прежде чем сдаться.
const MAX_TRIES: u32 = 3;

/// Контекст конвейера.
#[derive(Clone)]
pub struct IronCtx {
    pub food_name: String,
    /// Готовое опознание: «Arctic char, a cold-water fish (0.90); …».
    pub identity: String,
    /// Заполняется первым узлом.
    pub amount_mg: Option<f64>,
    /// Заполняется вторым узлом.
    pub absorption: Option<f64>,
    tries_amount: u32,
    tries_absorption: u32,
    pub last_error: Option<String>,
    reason_amount: String,
    reason_absorption: String,
}

impl IronCtx {
    fn new(food_name: &str, identity: &str) -> Self {
        Self {
            food_name: food_name.to_string(),
            identity: identity.to_string(),
            amount_mg: None,
            absorption: None,
            tries_amount: 0,
            tries_absorption: 0,
            last_error: None,
            reason_amount: String::new(),
            reason_absorption: String::new(),
        }
    }

    /// Общая шапка обоих запросов: что человек записал и что об этом известно.
    fn head(&self) -> String {
        let known = if self.identity.trim().is_empty() {
            String::new()
        } else {
            format!(
                "Our automatic classifier says this product is: {}\n\n",
                self.identity
            )
        };
        format!(
            "A person wrote this into their food diary: {}\n\n{known}",
            self.food_name
        )
    }
}

/// Строки таблицы БЕЗ ЧИСЕЛ — для запроса количества. Числа там не нужны, а их
/// присутствие вредит: модель, увидев числа, начинает ими отвечать.
fn category_keys() -> String {
    IRON_CATEGORIES
        .iter()
        .map(|c| format!("  {key:<20} — {ex}", key = c.key, ex = c.examples))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Строки С ДОЛЯМИ — для запроса усвоения. Здесь доля и есть предмет вопроса,
/// поэтому она показывается: без таблицы модель даёт чечевице и творогу те же 0.20,
/// что и говядине.
fn absorption_keys() -> String {
    IRON_CATEGORIES
        .iter()
        .map(|c| {
            format!(
                "  {key:<20} {a:<5} — {ex}",
                key = c.key,
                a = c.absorption,
                ex = c.examples
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Справочник долей усвоения — из того же массива, что и справочник количества.
/// Доля ПРОДУКТА точнее доли его группы: у куриной печени 0.25, а строка
/// `meat_poultry`, куда её однажды отнесли, даёт 0.15.
fn absorption_reference_table() -> String {
    IRON_REFERENCE
        .iter()
        .map(|(name, _mg, absorption)| format!("  {name}: {absorption}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Общий прогон: schema-injected JSON mode, оба потока раскручиваются, финальный
/// текст уходит в `Completed`.
fn run_structured<E, S>(
    executor: E,
    text: String,
    name: String,
) -> LocalBoxStream<'static, PromptExecutionEvent>
where
    E: PromptExecutor + 'static,
    S: JsonSchema + 'static,
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

/// Общий прогон узла: раскрутить поток, обновить контекст, отдать `Completed`.
fn run_node<P>(
    prompt: P,
    executor: Qwen,
    context: IronCtx,
    amount_step: bool,
) -> LocalBoxStream<'static, NodeEvent<IronCtx>>
where
    P: Prompt<Output = String, Context = IronCtx> + 'static,
{
    let mut stream = prompt.execute(executor);
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
            if amount_step {
                c.tries_amount += 1;
            } else {
                c.tries_absorption += 1;
            }
            c.last_error = Some(e);
            c
        } else {
            prompt.update_context(context, out)
        };
        yield NodeEvent::Completed(ctx);
    })
}

// Комментарии у структур ответа обычные, а НЕ доковые: док структуры уезжает в
// json_schema как "description", и модель возвращает его вместо данных.

#[derive(Debug, Deserialize, JsonSchema)]
struct AmountAnswer {
    /// One short sentence: which reference entry or which category, and why.
    reason: String,
    /// The name of the REFERENCE entry this food matches, copied exactly, or NONE.
    reference_key: String,
    /// The category row key from the table, copied exactly.
    category: String,
    /// Iron per 100 g, in milligrams.
    iron_mg: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AbsorptionAnswer {
    /// One short sentence: which reference entry or which category, and why.
    reason: String,
    /// The name of the REFERENCE entry this food matches, copied exactly, or NONE.
    reference_key: String,
    /// The category row key from the table, copied exactly.
    category: String,
    /// The absorbed FRACTION of this food's iron, from 0.0 to 1.0.
    absorbed_fraction: f64,
}

/// Запись справочника по ключу, который назвала модель.
fn reference_hit(key: &str) -> Option<&'static (&'static str, f64, f64)> {
    let key = key.trim();
    if key.is_empty() || key.eq_ignore_ascii_case("none") {
        return None;
    }
    IRON_REFERENCE
        .iter()
        .find(|(name, _, _)| name.eq_ignore_ascii_case(key))
}

// ── Шаг 1: сколько железа ────────────────────────────────────────────────────

struct AmountPrompt {
    head: String,
}

impl Prompt for AmountPrompt {
    type Output = String;
    type Context = IronCtx;

    fn name(&self) -> String {
        "iron.amount".to_string()
    }

    fn serialize(&self) -> String {
        format!(
            "{head}\
             How much IRON does this food hold per 100 grams, in milligrams?\n\n\
             FIRST look for it in the REFERENCE below. If it is there — or is plainly the same \
             food under another name, in another grammatical case or with a cut or grade \
             attached — put that entry's name into \"reference_key\", copied exactly.\n\n\
             {reference}\n\n\
             The reference keys are written in RUSSIAN: copy the key letter for letter as it \
             stands there, never translated and never transliterated.\n\n\
             Whether or not you found it, ALSO place the food in one row of the table below and \
             answer with that row's key. If the reference had nothing, answer \"reference_key\" \
             with NONE and give a value that fits the row.\n\n\
             {rows}\n\n\
             For raw or dry as-sold products use the RAW value unless the name says cooked or \
             boiled.\n\n\
             Fill \"reason\" FIRST, then the two keys, then the milligrams.\n\n\
             Respond with ONLY a minified JSON object and nothing else.",
            head = self.head,
            reference = iron_reference_table(),
            rows = category_keys(),
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries_amount += 1;
        match serde_json::from_str::<AmountAnswer>(strip_code_fences(raw.trim())) {
            Ok(v) => {
                // Справочник — как есть: строка описывает ГРУППУ, справочник — этот
                // продукт, и расходятся они законно (варёная чечевица 3.3 мг против
                // строки `legumes` 5–8, которая про сухую).
                let (mg, source) = match reference_hit(&v.reference_key) {
                    Some((name, mg, _)) => (*mg, format!("справочник «{name}»")),
                    None => {
                        let row = v.category.trim().to_ascii_lowercase();
                        match IRON_CATEGORIES.iter().find(|c| c.key == row) {
                            // Вне справочника значение поджимается границами строки:
                            // отвергать и переспрашивать бессмысленно, когда подставить
                            // нечего.
                            Some(cat) => (
                                v.iron_mg.clamp(cat.mg_min, cat.mg_max),
                                format!("строка {}", cat.key),
                            ),
                            None => {
                                ctx.last_error =
                                    Some(format!("неизвестная строка железа «{}»", v.category));
                                return ctx;
                            }
                        }
                    }
                };
                ctx.amount_mg = Some(mg);
                ctx.reason_amount = format!("{} [{source}]", v.reason);
                ctx.last_error = None;
            }
            Err(e) => ctx.last_error = Some(format!("количество не разобрано: {e}, ответ: {raw}")),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(
        &self,
        executor: E,
    ) -> LocalBoxStream<'static, PromptExecutionEvent> {
        run_structured::<E, AmountAnswer>(executor, self.serialize(), self.name())
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
        AmountPrompt { head: ctx.head() }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        run_node(self.prompt(&context), self.prompt_executor(), context, true)
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        // Количество не вышло, но попытки есть — пробуем снова. Иначе идём за
        // усвоением: без него миллиграммы всё равно бесполезны.
        if ctx.amount_mg.is_none() && ctx.tries_amount < MAX_TRIES {
            return Some(Box::new(NodeWrapper::new(AmountNode {
                executor: self.executor.clone(),
            })));
        }
        Some(Box::new(NodeWrapper::new(AbsorptionNode {
            executor: self.executor.clone(),
        })))
    }
}

// ── Шаг 2: насколько усваивается ─────────────────────────────────────────────

struct AbsorptionPrompt {
    head: String,
}

impl Prompt for AbsorptionPrompt {
    type Output = String;
    type Context = IronCtx;

    fn name(&self) -> String {
        "iron.absorption".to_string()
    }

    fn serialize(&self) -> String {
        format!(
            "{head}\
             What FRACTION of this food's iron does the body actually take up? Heme iron — from \
             liver, offal, meat and shellfish — absorbs several times better than the non-heme \
             iron of plants, and tea, coffee and oxalates hold it back further.\n\n\
             FIRST look for the food in the REFERENCE below. If it is there — or is plainly the \
             same food under another name, in another grammatical case or with a cut or grade \
             attached — put that entry's name into \"reference_key\", copied exactly, and answer \
             \"absorbed_fraction\" with THAT ENTRY'S NUMBER, copied exactly. Do not round it, do \
             not adjust it, do not replace it with one you remember.\n\n\
             {reference}\n\n\
             The reference keys are written in RUSSIAN: copy the key letter for letter as it \
             stands there, never translated and never transliterated.\n\n\
             Whether or not you found it, ALSO place the food in one row of the table below and \
             answer with that row's key. The number after the key is that row's fraction; if the \
             reference had nothing, answer \"reference_key\" with NONE and give THAT ROW'S \
             number as the fraction.\n\n\
             {rows}\n\n\
             Fill \"reason\" FIRST, then the two keys, then the fraction.\n\n\
             Respond with ONLY a minified JSON object and nothing else.",
            head = self.head,
            reference = absorption_reference_table(),
            rows = absorption_keys(),
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries_absorption += 1;
        match serde_json::from_str::<AbsorptionAnswer>(strip_code_fences(raw.trim())) {
            Ok(v) => {
                // Долю ВОЗВРАЩАЕТ МОДЕЛЬ — она видит и справочник с процентами, и
                // таблицу строк, и её дело найти нужную запись и назвать её число.
                //
                // Единственная поправка — зажим в 0…1: доля вне этого промежутка не
                // означает ничего, и хранить её нельзя. Это не отказ и не повтор,
                // просто негодное значение не может попасть в счёт.
                let fraction = v.absorbed_fraction.clamp(0.0, 1.0);
                let source = match reference_hit(&v.reference_key) {
                    Some((name, _, _)) => format!("справочник «{name}»"),
                    None => format!("строка {}", v.category.trim()),
                };
                ctx.absorption = Some(fraction);
                ctx.reason_absorption = format!("{} [{source}]", v.reason);
                ctx.last_error = None;
            }
            Err(e) => ctx.last_error = Some(format!("усвоение не разобрано: {e}, ответ: {raw}")),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(
        &self,
        executor: E,
    ) -> LocalBoxStream<'static, PromptExecutionEvent> {
        run_structured::<E, AbsorptionAnswer>(executor, self.serialize(), self.name())
    }
}

struct AbsorptionNode {
    executor: Qwen,
}

impl Node for AbsorptionNode {
    type Prompt = AbsorptionPrompt;
    type Executor = Qwen;
    type Error = String;
    type Context = IronCtx;

    fn prompt(&self, ctx: &Self::Context) -> Self::Prompt {
        AbsorptionPrompt { head: ctx.head() }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        run_node(self.prompt(&context), self.prompt_executor(), context, false)
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        match ctx.absorption {
            Some(_) => None,
            None if ctx.tries_absorption < MAX_TRIES => {
                Some(Box::new(NodeWrapper::new(AbsorptionNode {
                    executor: self.executor.clone(),
                })))
            }
            None => None,
        }
    }
}

// ── Запуск ───────────────────────────────────────────────────────────────────

/// Железо продукта: `(мг на 100 г, доля усвоения)`.
///
/// `identity` — готовое опознание из общего узла. Пустая строка допустима: тогда оба
/// запроса работают по одному названию, как работали до конвейера признаков.
///
/// FAILS LOUDLY, если не дошли до обоих чисел: миллиграммы без доли усвоения
/// бессмысленны, их нельзя ни сложить, ни сравнить с планкой.
pub async fn lookup_iron(food_name: &str, identity: &str) -> Result<(f64, f64), String> {
    // Thinking OFF: с рассуждением qwen3 паркует короткий ответ в канал размышления и
    // возвращает пустой контент.
    let executor = build_executor_think(false)?;
    let pipeline = Pipeline::new(Box::new(NodeWrapper::new(AmountNode { executor })));

    let mut stream = run_pipeline(pipeline, IronCtx::new(food_name, identity));
    let mut last: Option<IronCtx> = None;
    while let Some(ev) = stream.next().await {
        if let NodeEvent::Completed(ctx) = ev {
            last = Some(ctx);
        }
    }

    let ctx = last.ok_or_else(|| "конвейер железа не дал результата".to_string())?;
    match (ctx.amount_mg, ctx.absorption) {
        (Some(mg), Some(absorption)) => {
            leptos::logging::log!(
                "iron «{food_name}»: {mg} мг · усвоение {absorption} — {} / {}",
                ctx.reason_amount,
                ctx.reason_absorption
            );
            Ok((mg, absorption))
        }
        _ => Err(ctx
            .last_error
            .unwrap_or_else(|| "конвейер железа не дошёл до значений".to_string())),
    }
}
