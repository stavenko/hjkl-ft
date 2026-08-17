//! Признаки продукта — конвейер `arti_pipes`: опознание, затем ПЯТЬ отдельных
//! вопросов.
//!
//! Раньше каждый признак спрашивался своим запросом и каждый начинал с того, что
//! опознавал продукт заново — внутри собственного вопроса. Из-за этого опознание
//! выходило В ТОН вопросу: «Голец» в вопросе про мясо оказывался кониной («horse
//! meat») и съедал недельную планку красного мяса, а в вопросе про гем тот же голец
//! объявлялся печенью рыбы. Просьба «не думай пока о категориях» не помогает:
//! категории лежат в том же промпте и уже в контексте, когда модель пишет опознание.
//!
//! Ровно то же было у железа и решено так же — см. [`super::iron_pipeline`], где в
//! шапке записано «голец объявлялся говядиной».
//!
//! # Почему 1 + 5, а не 1 + 1
//!
//! Сначала пять вердиктов спрашивались ОДНИМ запросом — так конвейер выходил вдвое
//! дешевле. Замер показал размен: незнакомые слова вылечились (перекрёстный набор из
//! редких рыб, овощей, фруктов и круп — 160 проверок, ни одного ложного «да»), но
//! границы внутри знакомого поплыли: солёная сельдь и копчёная скумбрия уехали в
//! переработанное мясо, говяжий язык — в красное, салями с хамоном потеряли гем.
//! Пять правил в одном ответе теснят друг друга.
//!
//! Поэтому опознание отдельным узлом, а каждый признак — своим запросом со своим
//! перечнем категорий, куда опознание приходит готовым фактом. Дороже на три запроса,
//! зато каждый шаг делает ровно одну работу.
//!
//! # Что это НЕ меняет
//!
//! Схему данных. Опознание живёт только в контексте конвейера, никуда не пишется;
//! наружу выходят те же пять признаков и ложатся в те же поля `Food`.
//!
//! # Что открывает
//!
//! Маршрутизацию: `select_next_node` видит контекст, поэтому «не спрашивать мясные
//! вопросы у того, что опознано как растение» — это условие в одном месте, а не
//! пятое правило в пяти промптах. Пока не сделано: сперва нужно, чтобы опознание
//! было надёжным (замер поймал «страусятина → strawberry»).

use arti_pipes::executor::PromptExecutor;
use arti_pipes::llm_executors::qwen::Qwen;
use arti_pipes::node::{Node, NodeEvent, NodeRunner, NodeWrapper};
use arti_pipes::pipeline::{run_pipeline, Pipeline};
use arti_pipes::prompt::{Prompt, PromptExecutionEvent};
use futures::stream::LocalBoxStream;
use futures::StreamExt;
use schemars::JsonSchema;
use serde::Deserialize;

use super::ai::{build_executor_think, strip_code_fences, veg_fruit_from_category};

/// Сколько раз повторить каждый шаг, прежде чем сдаться.
const MAX_TRIES: u32 = 3;

/// Редкие имена и то, ЧЕМ они являются.
///
/// Нужны ровно здесь — в шаге опознания. Рядом, в профиле жира, тот же «Голец»
/// определяется без осечек, потому что ПЕРЕЧИСЛЕН среди примеров строки
/// `fish_fatty_cold`: модели не нужно его знать, если ей сказать. Уговоры не
/// работали — ни рамка «имя взято из дневника питания», ни разрешение ответить
/// «unknown» (после него модель начала отвечать так и про сосиски с бастурмой).
///
/// В промпты признаков словарь НЕ идёт: там он вредит. Гем с ним просел с 22/22 до
/// 21/22 три прогона подряд, всегда на одном месте («chicken thigh → MEAT OF
/// MAMMALS») — два десятка рыбьих имён оттягивают внимание от границы, на которой
/// признак балансирует. Здесь же они безвредны: вердиктов рядом нет.
///
/// Пополняется по мере находок: сюда идёт всё, на чём признаки спотыкались.
const RARE_NAMES: &[(&str, &str)] = &[
    ("голец", "Arctic char, a fish"),
    ("пикша", "haddock, a fish"),
    ("сайда", "saithe (pollock), a fish"),
    ("зубатка", "wolffish, a fish"),
    ("муксун", "muksun, a whitefish"),
    ("омуль", "omul, a whitefish"),
    ("нельма", "nelma, a whitefish"),
    ("кижуч", "coho salmon, a fish"),
    ("кета", "chum salmon, a fish"),
    ("нерка", "sockeye salmon, a fish"),
    ("чавыча", "chinook salmon, a fish"),
    ("горбуша", "pink salmon, a fish"),
    ("сайра", "saury, a fish"),
    ("мойва", "capelin, a fish"),
    ("путассу", "blue whiting, a fish"),
    ("минтай", "alaska pollock, a fish"),
    ("навага", "navaga, a fish"),
    ("палтус", "halibut, a fish"),
    ("толстолобик", "silver carp, a fish"),
    ("пангасиус", "pangasius, a fish"),
    ("страусятина", "ostrich meat, the flesh of a bird"),
];

/// Словарь редких имён как текст промпта — из того же массива, что и сам словарь.
fn rare_names_block() -> String {
    if RARE_NAMES.is_empty() {
        return String::new();
    }
    let lines = RARE_NAMES
        .iter()
        .map(|(name, what)| format!("{name} — {what}"))
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "Some names people write in a food diary are rare and easy to mistake for something \
         else. Here is what they are: {lines}.\n\n"
    )
}

/// Пять признаков. `None` — этот признак так и не выяснили: он останется пустым и
/// будет переспрошен позже, а не запишется наугад.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    pub veg_fruit: Option<bool>,
    pub heme: Option<bool>,
    pub red_meat: Option<bool>,
    pub processed_meat: Option<bool>,
    pub milk_globule: Option<bool>,
}

/// Контекст, который течёт по конвейеру.
#[derive(Clone)]
pub struct FlagsCtx {
    pub food_name: String,
    /// Заполняется первым узлом: «Arctic char, a fish».
    pub identity: Option<String>,
    pub flags: Flags,
    /// Попытки по шагам: у каждого свои, чтобы упавший не тратил чужие.
    tries: [u32; 6],
    /// Обоснования по шагам — для лога и телеметрии.
    reasons: Vec<String>,
    pub last_error: Option<String>,
}

impl FlagsCtx {
    fn new(food_name: &str) -> Self {
        Self {
            food_name: food_name.to_string(),
            identity: None,
            flags: Flags::default(),
            tries: [0; 6],
            reasons: Vec::new(),
            last_error: None,
        }
    }

    /// Строка «это ЕСТЬ то-то» для промптов признаков. Пустая, если опознать не
    /// удалось: тогда признак работает по одному названию, как работал до конвейера.
    ///
    /// В промпте она стоит ПОСЛЕ перечня категорий, и это важно. Сначала опознание
    /// стояло первым — и перебивало правила: солёная сельдь уходила в переработанное
    /// мясо по слову «salted», молочные сосиски объявлялись непереработанными, а
    /// ливерная колбаса — красным мясом, всё по два раза из двух. Тот же урок уже был
    /// получен на оговорке про рыбу: поставленная до определения, она делала хуже
    /// (29/32), а среди правил дала 32/32. Что читается позже — весит больше.
    fn given(&self) -> String {
        match &self.identity {
            Some(w) => format!(
                "WHAT THIS FOOD IS has already been established: {w}. Take it as given — do not \
                 second-guess it and never re-guess the species yourself.\n\n"
            ),
            None => String::new(),
        }
    }
}

/// Общий для всех промптов прогон: schema-injected JSON mode, оба потока
/// раскручиваются, финальный текст уходит в `Completed`.
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

// Комментарии у структур ответа обычные, а НЕ доковые: док структуры уезжает в
// json_schema как "description" всего объекта, и модель возвращает его вместо
// данных — так однажды обвалился разбор овощей целиком. У ПОЛЕЙ доки, наоборот,
// нужны: они и есть инструкция.

#[derive(Debug, Deserialize, JsonSchema)]
struct IdentityAnswer {
    /// The creature or plant this food comes from, WHICH PART of it, and what was
    /// done to it. English, one short phrase, at most 12 words.
    what_this_food_is: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CategoryAnswer {
    /// The ONE word naming the category that fits, or NONE if none does.
    category_that_fits: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FlagAnswer {
    /// Which category fits, or that none does. One short sentence, at most 12 words.
    reason: String,
    /// True if the food belongs to one of the categories.
    verdict: bool,
}

// ── Шаг 0: что это за еда ────────────────────────────────────────────────────

struct IdentifyPrompt {
    food_name: String,
}

impl Prompt for IdentifyPrompt {
    type Output = String;
    type Context = FlagsCtx;

    fn name(&self) -> String {
        "flags.identify".to_string()
    }

    fn serialize(&self) -> String {
        format!(
            "Say what this food is. Nothing else is asked of you: no judgement, no category, \
             no numbers.\n\n\
             The name was typed by a person into their FOOD DIARY, so it always names something \
             eaten — never a material, a device or a term from another trade, however much the \
             word may look like one.\n\n\
             Name the creature or plant it comes from, WHICH PART of it — flesh, an organ, a \
             fruit, a berry, a seed, a leaf, a root, milk, an egg — and what was done to it if \
             the name says so: smoked, salted, boiled, dried, minced, in syrup. Answer in \
             ENGLISH, one short phrase: \"Arctic char, a fish\", \"chicken thigh, poultry \
             meat\", \"beef liver, an organ\", \"buckwheat, a grain\", \"chicken egg\", \
             \"cherry in syrup, a berry with added sugar\".\n\n\
             Never merely repeat the name back — a name copied out says nothing. Never answer \
             with a food that merely SOUNDS similar: a Russian word for meat is not a berry \
             because their first letters agree. If you truly do not recognise the word, name the \
             most likely KIND of food it is and say you are unsure; do not invent a species.\n\n\
             {rare}The food: {name}\n\n\
             Respond with ONLY a single minified JSON object and nothing else.",
            rare = rare_names_block(),
            name = self.food_name,
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries[0] += 1;
        match serde_json::from_str::<IdentityAnswer>(strip_code_fences(raw.trim())) {
            Ok(a) if !a.what_this_food_is.trim().is_empty() => {
                ctx.identity = Some(a.what_this_food_is.trim().to_string());
                ctx.last_error = None;
            }
            Ok(_) => ctx.last_error = Some("опознание пустое".to_string()),
            Err(e) => ctx.last_error = Some(format!("опознание не разобрано: {e}, ответ: {raw}")),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(
        &self,
        executor: E,
    ) -> LocalBoxStream<'static, PromptExecutionEvent> {
        run_structured::<E, IdentityAnswer>(executor, self.serialize(), self.name())
    }
}

struct IdentifyNode {
    executor: Qwen,
}

impl Node for IdentifyNode {
    type Prompt = IdentifyPrompt;
    type Executor = Qwen;
    type Error = String;
    type Context = FlagsCtx;

    fn prompt(&self, ctx: &Self::Context) -> Self::Prompt {
        IdentifyPrompt { food_name: ctx.food_name.clone() }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        run_node(self.prompt(&context), self.prompt_executor(), context, 0)
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        // Не опознали, но попытки есть — пробуем снова. Иначе идём к признакам:
        // работа по одному названию — это ровно прежнее поведение, и оно лучше, чем
        // не выяснить о продукте ничего.
        if ctx.identity.is_none() && ctx.tries[0] < MAX_TRIES {
            return Some(Box::new(NodeWrapper::new(IdentifyNode {
                executor: self.executor.clone(),
            })));
        }
        Some(Box::new(NodeWrapper::new(FlagNode {
            executor: self.executor.clone(),
            step: Step::VegFruit,
        })))
    }
}

/// Общий прогон узла: раскрутить оба потока, обновить контекст, отдать `Completed`.
/// Сбой сети или модели тратит попытку так же, как неразобранный ответ.
fn run_node<P>(
    prompt: P,
    executor: Qwen,
    context: FlagsCtx,
    slot: usize,
) -> LocalBoxStream<'static, NodeEvent<FlagsCtx>>
where
    P: Prompt<Output = String, Context = FlagsCtx> + 'static,
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
            c.tries[slot] += 1;
            c.last_error = Some(e);
            c
        } else {
            prompt.update_context(context, out)
        };
        yield NodeEvent::Completed(ctx);
    })
}

// ── Шаги 1–5: признаки, каждый своим запросом ────────────────────────────────

/// Какой признак спрашивает узел. Порядок тот же, что в `FLAGS` у `classify`.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    VegFruit,
    Heme,
    RedMeat,
    ProcessedMeat,
    MilkGlobule,
}

impl Step {
    /// Номер ячейки попыток: нулевая занята опознанием.
    fn slot(self) -> usize {
        match self {
            Step::VegFruit => 1,
            Step::Heme => 2,
            Step::RedMeat => 3,
            Step::ProcessedMeat => 4,
            Step::MilkGlobule => 5,
        }
    }

    fn next(self) -> Option<Step> {
        match self {
            Step::VegFruit => Some(Step::Heme),
            Step::Heme => Some(Step::RedMeat),
            Step::RedMeat => Some(Step::ProcessedMeat),
            Step::ProcessedMeat => Some(Step::MilkGlobule),
            Step::MilkGlobule => None,
        }
    }

    /// Ставить ли опознание ПЕРЕД правилами.
    ///
    /// Место опознания весит больше, чем кажется, и лучшее место у признаков РАЗНОЕ —
    /// это измерено, а не выбрано. Мясным лучше после правил: с опознанием впереди
    /// солёная сельдь уходила в переработанное мясо по слову «salted», а ливерная
    /// колбаса — в красное мясо, оба раза по два из двух. Гему наоборот: с опознанием
    /// после правил он проседал с 22/22 до 20–21/22, всегда на «бедре курином».
    fn identity_first(self) -> bool {
        match self {
            Step::VegFruit | Step::Heme => true,
            Step::RedMeat | Step::ProcessedMeat | Step::MilkGlobule => false,
        }
    }

    /// Спрашивать ли КАТЕГОРИЮ ОДНИМ СЛОВОМ вместо булева вердикта.
    ///
    /// У овощей с фруктами вердикт нельзя спрашивать: модель называет верную
    /// категорию и ставит противоположный ответ — «true — FRUITS» на вишню в сиропе,
    /// хотя правило про сахар велит не относить её никуда. Признак вылечили тем, что
    /// «да/нет» выводится из названного в коде ([`veg_fruit_from_category`]), и
    /// противоречить стало нечему. Остальным четырём это не нужно: у них имена
    /// категорий несут ограничения внутри себя («моллюски ТОЛЬКО этих видов»), и одно
    /// слово их стирает — на геме такая замена давала «Chicken thigh → ORGAN».
    fn as_category(self) -> bool {
        matches!(self, Step::VegFruit)
    }

    fn label(self) -> &'static str {
        match self {
            Step::VegFruit => "фрукты/овощи",
            Step::Heme => "гем",
            Step::RedMeat => "красное мясо",
            Step::ProcessedMeat => "переработанное мясо",
            Step::MilkGlobule => "молочная глобула",
        }
    }

    /// Устойчивый ключ телеметрии — не менять задним числом.
    fn kind(self) -> &'static str {
        match self {
            Step::VegFruit => "flag.veg_fruit",
            Step::Heme => "flag.heme",
            Step::RedMeat => "flag.red_meat",
            Step::ProcessedMeat => "flag.processed_meat",
            Step::MilkGlobule => "flag.milk_globule",
        }
    }

    fn get(self, f: &Flags) -> Option<bool> {
        match self {
            Step::VegFruit => f.veg_fruit,
            Step::Heme => f.heme,
            Step::RedMeat => f.red_meat,
            Step::ProcessedMeat => f.processed_meat,
            Step::MilkGlobule => f.milk_globule,
        }
    }

    fn set(self, f: &mut Flags, v: bool) {
        match self {
            Step::VegFruit => f.veg_fruit = Some(v),
            Step::Heme => f.heme = Some(v),
            Step::RedMeat => f.red_meat = Some(v),
            Step::ProcessedMeat => f.processed_meat = Some(v),
            Step::MilkGlobule => f.milk_globule = Some(v),
        }
    }

    /// Чем кончается промпт: просьбой назвать категорию словом или дать вердикт.
    fn tail(self) -> &'static str {
        if self.as_category() {
            "Write in \"category_that_fits\" the ONE word naming the category that fits — \
             VEGETABLES, FRUITS or DISH — or the word NONE if no category fits. Nothing else, \
             one word. Never list the categories that do not fit: running through them turns \
             into denying them all, the right one included."
        } else {
            "Fill the reason field FIRST — name the ONE category that fits, or say that none \
             does — and let the verdict follow from it. Never list the categories that do not \
             fit: running through them turns into denying them all, the right one included."
        }
    }

    /// Перечень категорий — дословно тот, что прошёл замеры в отдельных запросах.
    fn rules(self) -> &'static str {
        match self {
            Step::VegFruit => "\
                THE CATEGORIES:\n\
                — VEGETABLES: any plant part eaten as a vegetable — roots, tubers, leaves, \
                stalks, cabbages, squashes, onions, garlic, tomatoes, cucumbers, peppers, green \
                beans, peas, mushrooms. A GRAIN is not a vegetable, whatever it was cooked \
                into;\n\
                — FRUITS, and BERRIES COUNT AS FRUITS: apples, pears, citrus, bananas, grapes, \
                melons, and berries such as cherries, sour cherries, strawberries, raspberries, \
                blueberries, currants, cranberries;\n\
                — DISHES MADE MAINLY OF THEM: salads, stewed or roasted vegetables, vegetable \
                soups, fruit salads.\n\n\
                Sugar changes what a food is: jam, preserves, fruit in syrup, candied fruit and \
                juices belong to none of the categories, however much fruit went into them.",
            Step::Heme => "\
                THE CATEGORIES OF RICH HEME-IRON SOURCES:\n\
                — LIVER itself — of any animal, bird or fish — and food made mainly of liver: \
                pâté, liver sausage, liver cake;\n\
                — OTHER INNER ORGANS themselves — hearts, kidneys, tongues, gizzards, lungs, \
                blood sausage — of any animal, bird or fish. It is the ORGAN that belongs to \
                this category, never the animal's flesh: a fish fillet, a bird's breast or leg \
                is not an organ;\n\
                — MEAT OF MAMMALS: beef, veal, pork, lamb, mutton, goat, and the meat of other \
                farmed or wild mammals — venison, elk, boar, horse, rabbit — in any cut, ground \
                or whole, raw or cooked, and food made mainly of such meat: sausage, \
                frankfurters, ham, bacon, salami, mince, meatballs, cutlets, stew, canned meat. \
                A creature that is not a mammal has no meat in this category;\n\
                — MOLLUSCS OF THESE KINDS ONLY: mussels, oysters, clams and vongole, cockles, \
                octopus, whelks, winkles. The category is these kinds, not the zoological \
                class: a mollusc of another kind does not belong to it.\n\n\
                Do not reason about how many milligrams of iron a food holds — you do not know \
                those numbers, and the categories already account for them.",
            Step::RedMeat => "\
                THE CATEGORIES OF RED MEAT:\n\
                — THE FLESH of MAMMALS: beef, veal, pork, lamb, mutton, goat, horse, rabbit, \
                and the flesh of wild mammals — venison, elk, boar. Any cut, whole or ground, \
                raw or cooked;\n\
                — FOOD MADE MAINLY OF SUCH FLESH: sausages, frankfurters, ham, bacon, salami, \
                mince, meatballs, cutlets, stew, canned meat — whatever the flesh has been \
                through.\n\n\
                It is the FLESH — muscle — that belongs to these categories. INNER ORGANS do \
                not: liver, heart, kidney, tongue, lung, tripe and dishes made mainly of them \
                are outside, however red they look. Neither do BIRDS — chicken, turkey, duck, \
                goose, ostrich — nor fish, seafood, eggs, dairy or anything from a plant.",
            Step::ProcessedMeat => "\
                THE QUESTION: is this meat PRESERVED — cured, smoked, salted, dried or \
                fermented for keeping? Meat here is the flesh of a MAMMAL or a BIRD.\n\n\
                Meat is preserved when it has been through one of these: CURING with nitrite or \
                nitrate salt, SMOKING, prolonged SALTING, AIR-DRYING, or FERMENTATION. Cooking \
                is not preserving: boiling, frying, baking, stewing, grilling, mincing, \
                freezing and packaging leave meat unpreserved, however industrial the \
                process.\n\n\
                PRESERVED, therefore TRUE: sausages and frankfurters, wieners, salami and other \
                dry sausages — fuet, chorizo, sobrassada, soppressata, sujuk, kabanos, \
                landjäger, pepperoni, whatever else they are called abroad — ham, bacon, \
                gammon, pastrami, prosciutto and jamon, bresaola, basturma, smoked and cured \
                brisket, liver sausage and pâté made with cure, hot dogs, corned beef, canned \
                luncheon meat, jerky. Also a dish whose MAIN meat part is one of these — pizza \
                with salami, pasta carbonara, sausage in a bun, solyanka.\n\n\
                NOT PRESERVED, therefore FALSE: fresh and frozen meat of any kind, mince, \
                cutlets and meatballs, home-made or shop-bought, boiled or baked meat, roast, \
                stew, kebab, dumplings, canned meat that was merely boiled in the tin, poultry \
                that went through none of the treatments above. FISH, SEAFOOD AND ROE are false \
                ALWAYS, however they were treated: smoked mackerel, salted herring, lightly \
                salted salmon, dried cod, canned sprats, caviar — a fish that was smoked is a \
                smoked fish, never a preserved meat.",
            Step::MilkGlobule => "\
                THE QUESTION: is the fat of this food still enclosed in INTACT MILK FAT \
                GLOBULES?\n\
                Milk fat leaves the udder wrapped in a membrane. Only three things destroy that \
                membrane: CHURNING, RENDERING and MELTING WITH EMULSIFYING SALTS. Nothing else \
                does — not fermenting, not souring, not heating, not ageing, not salting, not \
                freezing, not drying, not concentrating.\n\
                Decide in THIS ORDER and STOP at the first step that matches.\n\
                STEP 1 — does this food contain MILK FAT at all? If it is not dairy — meat, \
                poultry, fish, eggs, vegetable oils, nuts, seeds, plants, or a plant «milk» \
                made of soy, oat, almond or coconut — then FALSE, there is no milk fat to ask \
                about.\n\
                STEP 2 — was that milk fat CHURNED out (butter, buttermilk-made spreads), \
                RENDERED (ghee, clarified butter, butter oil, anhydrous milk fat) or MELTED \
                WITH EMULSIFYING SALTS (processed cheese, cheese spread, cheese in a tub)? Then \
                FALSE. The same applies when a cook or a factory added the milk fat AS BUTTER \
                rather than as milk or cream: milk chocolate, buttercream, shortcrust pastry, \
                croissants, waffles, biscuits are FALSE.\n\
                STEP 3 — otherwise the fat is still in its native globules: TRUE. This is the \
                normal case for dairy, and it holds however the product was processed short of \
                churning — milk of any fat content, cream, sour cream, kefir, ryazhenka, ayran, \
                acidophilus, drinking and thick yogurt, cottage cheese of any fat content \
                including fat-free, curd mass, ricotta, natural cheeses (hard, semi-hard, soft, \
                brined), condensed and powdered milk, whey, ice cream, and dishes whose fat \
                comes mainly from these. Do NOT hedge here: a cheese or a cottage cheese that \
                was not churned HAS intact globules, even though the curd was pressed, salted, \
                aged or heated.",
        }
    }
}

struct FlagPrompt {
    food_name: String,
    given: String,
    step: Step,
}

impl Prompt for FlagPrompt {
    type Output = String;
    type Context = FlagsCtx;

    fn name(&self) -> String {
        format!("flags.{}", self.step.kind())
    }

    fn serialize(&self) -> String {
        format!(
            "Decide ONE question about one food, and nothing else.\n\n\
             The food: {name}\n\n\
             {before}{rules}\n\n\
             {after}\
             A composite dish belongs to a category only when such an ingredient is its MAIN \
             part. Words about preparation, storage, freezing, packaging, cut, grade or country \
             of origin never change what a food is.\n\n\
             {tail}\n\n\
             Respond with ONLY a single minified JSON object with the fields from the schema.",
            name = self.food_name,
            before = if self.step.identity_first() { self.given.clone() } else { String::new() },
            after = if self.step.identity_first() { String::new() } else { self.given.clone() },
            rules = self.step.rules(),
            tail = self.step.tail(),
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries[self.step.slot()] += 1;
        let parsed: Result<(bool, String), String> = if self.step.as_category() {
            // Вердикт выводим САМИ из названной категории: пока его ставила модель,
            // она называла «FRUITS» и отвечала «да» на вишню в сиропе.
            serde_json::from_str::<CategoryAnswer>(strip_code_fences(raw.trim()))
                .map_err(|e| format!("категория не разобрана: {e}, ответ: {raw}"))
                .and_then(|a| {
                    veg_fruit_from_category(&a.category_that_fits)
                        .map(|v| (v, a.category_that_fits))
                })
        } else {
            serde_json::from_str::<FlagAnswer>(strip_code_fences(raw.trim()))
                .map(|a| (a.verdict, a.reason))
                .map_err(|e| format!("{} не разобран: {e}, ответ: {raw}", self.step.label()))
        };
        match parsed.map(|(verdict, reason)| FlagAnswer { reason, verdict }) {
            Ok(a) => {
                self.step.set(&mut ctx.flags, a.verdict);
                ctx.reasons.push(format!("{}: {}", self.step.label(), a.reason));
                ctx.last_error = None;
                leptos::logging::log!(
                    "{} «{}»: {} — {}",
                    self.step.label(),
                    ctx.food_name,
                    a.verdict,
                    a.reason
                );
                crate::services::telemetry::report_detection(
                    self.step.kind(),
                    &ctx.food_name,
                    &a.verdict.to_string(),
                    &format!(
                        "{} → {}",
                        ctx.identity.clone().unwrap_or_else(|| "(не опознано)".to_string()),
                        a.reason
                    ),
                    &[],
                );
            }
            Err(e) => ctx.last_error = Some(e),
        }
        ctx
    }

    fn execute<E: PromptExecutor>(
        &self,
        executor: E,
    ) -> LocalBoxStream<'static, PromptExecutionEvent> {
        if self.step.as_category() {
            run_structured::<E, CategoryAnswer>(executor, self.serialize(), self.name())
        } else {
            run_structured::<E, FlagAnswer>(executor, self.serialize(), self.name())
        }
    }
}

struct FlagNode {
    executor: Qwen,
    step: Step,
}

impl Node for FlagNode {
    type Prompt = FlagPrompt;
    type Executor = Qwen;
    type Error = String;
    type Context = FlagsCtx;

    fn prompt(&self, ctx: &Self::Context) -> Self::Prompt {
        FlagPrompt {
            food_name: ctx.food_name.clone(),
            given: ctx.given(),
            step: self.step,
        }
    }

    fn prompt_executor(&self) -> Self::Executor {
        self.executor.clone()
    }

    fn run(&self, context: Self::Context) -> LocalBoxStream<'static, NodeEvent<Self::Context>> {
        run_node(self.prompt(&context), self.prompt_executor(), context, self.step.slot())
    }

    fn select_next_node(&self, ctx: &Self::Context) -> Option<Box<dyn NodeRunner<Self::Context>>> {
        // Свой признак не выяснен и попытки есть — повторяем этот же шаг.
        if self.step.get(&ctx.flags).is_none() && ctx.tries[self.step.slot()] < MAX_TRIES {
            return Some(Box::new(NodeWrapper::new(FlagNode {
                executor: self.executor.clone(),
                step: self.step,
            })));
        }
        // Иначе — дальше по цепочке. Упавший признак не мешает остальным: он
        // останется пустым и будет переспрошен в другой раз.
        self.step.next().map(|next| {
            Box::new(NodeWrapper::new(FlagNode {
                executor: self.executor.clone(),
                step: next,
            })) as Box<dyn NodeRunner<Self::Context>>
        })
    }
}

// ── Запуск ───────────────────────────────────────────────────────────────────

/// Все пять признаков продукта за один конвейер: опознание, затем пять вопросов.
///
/// Возвращает то, что удалось выяснить: неполученный признак остаётся `None` и будет
/// переспрошен позже. FAILS LOUDLY только если не выяснено НИЧЕГО — тогда есть о чём
/// сообщить в журнал ошибок.
pub async fn classify_all(food_name: &str) -> Result<Flags, String> {
    // Thinking OFF: с рассуждением qwen3 паркует короткий ответ в канал размышления
    // и возвращает пустой контент.
    let executor = build_executor_think(false)?;
    let pipeline = Pipeline::new(Box::new(NodeWrapper::new(IdentifyNode { executor })));

    let mut stream = run_pipeline(pipeline, FlagsCtx::new(food_name));
    let mut last: Option<FlagsCtx> = None;
    while let Some(ev) = stream.next().await {
        if let NodeEvent::Completed(ctx) = ev {
            last = Some(ctx);
        }
    }

    let ctx = last.ok_or_else(|| "конвейер признаков не дал результата".to_string())?;
    let f = ctx.flags;
    let got = [f.veg_fruit, f.heme, f.red_meat, f.processed_meat, f.milk_globule]
        .iter()
        .filter(|v| v.is_some())
        .count();
    if got == 0 {
        return Err(ctx
            .last_error
            .unwrap_or_else(|| "конвейер признаков не дал ни одного вердикта".to_string()));
    }
    leptos::logging::log!(
        "признаки «{food_name}»: {} — выяснено {got} из 5 ({})",
        ctx.identity.clone().unwrap_or_else(|| "(не опознано)".to_string()),
        ctx.reasons.join(" · ")
    );
    Ok(f)
}
