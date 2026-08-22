//! Признаки продукта — конвейер `arti_pipes`: опознание, затем ПЯТЬ отдельных
//! вопросов.
//!
//! ╔══════════════════════════════════════════════════════════════════════════╗
//! ║  ВНИМАНИЕ: ПРАВИТЕ ПРОМПТ — СНАЧАЛА `docs/food-identity.md`              ║
//! ║                                                                          ║
//! ║  Там собраны отказы, которые мы уже разобрали (рифма «огурец» → «голец», ║
//! ║  марки, открывшие ворота выдумке, схема, уезжающая модели текстом), и    ║
//! ║  порядок замера. Правка, проверенная только на том имени, ради которого  ║
//! ║  делалась, почти всегда ломает соседнее.                                 ║
//! ║                                                                          ║
//! ║  Документ лежит в `docs/`, а НЕ здесь, намеренно: всё, что попадает в    ║
//! ║  этот файл СТРОКАМИ, уезжает модели в промпт.                            ║
//! ╚══════════════════════════════════════════════════════════════════════════╝
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

use super::ai::{
    build_executor_think, build_identity_executor, build_identity_fallback_executor,
    veg_fruit_from_category,
};

/// НИЖЕ ЭТОЙ УВЕРЕННОСТИ ОПОЗНАНИЕ СЧИТАЕТСЯ НЕСОСТОЯВШИМСЯ.
///
/// Модель честно отвечает «не знаю» — «I do not know the word „Маш“», — и ставит
/// себе 0.10. Мы это до сих пор не читали: брали лучший вариант, какой бы плохой он
/// ни был, и дальше он шёл во все признаки как факт. Так «Кыстыбый» — татарская
/// лепёшка с картофелем — получал гем и красное мясо и съедал недельную планку.
///
/// Порог взят из замеров: знакомое опознаётся на 0.9, незнакомое — на 0.1, между
/// ними пусто. Продукт ниже порога остаётся БЕЗ признаков: пустое поле честнее
/// выдуманного, и его переспросят при следующем проходе.
const IDENTITY_MIN_CONFIDENCE: f64 = 0.6;

/// Во сколько раз обесценивается уверенность, когда модель сама призналась, что слова
/// не знает, а словарь его не содержит.
///
/// Числа она в этом случае ставит не еде, а СВОЕЙ ФОРМУЛИРОВКЕ: «Бубурек копчёный — a
/// type of smoked bread» набирал 0.80 при честном признании.
///
/// Множитель откалиброван по замеру, а не выбран на глаз. С признанием на руках
/// уверенность делится надвое: выдумка получает 0.70–0.80 («Зюзюбряк», «Квазилапша»,
/// «Мормышка»), настоящая еда со странным словом в имени — 0.90–0.95 («Спиральчатый
/// спагетти», «Экомилк Творожный сыр» из боевого дневника). Граница проходит между
/// ними: 0.6 / 0.7 ≈ 0.857, то есть 0.90 проходит, 0.80 — нет. Прежние 0.6 ставили
/// границу на 1.0 и рубили живые продукты вместе с выдумкой.
const IDENTITY_UNKNOWN_PENALTY: f64 = 0.7;

/// Имя аспекта опознания в следе попыток (`services::food_probe`). Устойчивое:
/// по нему отбираются продукты, которые не удалось опознать вовсе.
pub const ASPECT_IDENTITY: &str = "identity";

/// Сколько раз повторить каждый шаг, прежде чем сдаться.
const MAX_TRIES: u32 = 3;

/// Редкие имена: что это за еда и как называется по-английски.
///
/// Каждая запись — СВОЯ СТРОКА вида «слово: определение, could be translated as
/// [варианты]». Сначала словарь склеивался в одну строку через точку с запятой, и на
/// «бастурму» модель выдала `ostrich meat` — дословно значение СОСЕДНЕЙ записи,
/// стоявшей перед ней. Разделить записи строками стоит ничего, а спутать их так
/// труднее.
///
/// Нужен ровно здесь — в шаге опознания. Рядом, в профиле жира, тот же «Голец»
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
const RARE_NAMES: &[(&str, &str, &[&str])] = &[
    // Рыбы. Промысловых имён сотни, и почти все незнакомы модели.
    ("голец", "a cold-water fish of the salmon family", &["Arctic char", "char"]),
    ("пикша", "a fish of the cod family", &["haddock"]),
    ("сайда", "a fish of the cod family", &["saithe", "pollock", "coalfish"]),
    ("зубатка", "a large northern sea fish", &["wolffish", "sea wolf"]),
    ("муксун", "a northern whitefish", &["muksun"]),
    ("омуль", "a northern whitefish", &["omul"]),
    ("нельма", "a northern whitefish", &["nelma", "Siberian white salmon"]),
    ("кижуч", "a Pacific salmon", &["coho salmon"]),
    ("кета", "a Pacific salmon", &["chum salmon"]),
    ("нерка", "a Pacific salmon", &["sockeye salmon"]),
    ("чавыча", "a Pacific salmon", &["chinook salmon", "king salmon"]),
    ("горбуша", "a Pacific salmon", &["pink salmon"]),
    ("сайра", "a small oily sea fish", &["saury"]),
    ("мойва", "a small oily northern fish", &["capelin"]),
    ("путассу", "a fish of the cod family", &["blue whiting"]),
    ("минтай", "a fish of the cod family", &["Alaska pollock", "walleye pollock"]),
    ("навага", "a northern fish of the cod family", &["navaga", "wachna cod"]),
    ("палтус", "a large flatfish", &["halibut"]),
    ("толстолобик", "a freshwater carp", &["silver carp"]),
    ("пангасиус", "a farmed freshwater catfish", &["pangasius", "basa"]),
    // Мясные названия, на которых модель ошибалась.
    ("страусятина", "the flesh of an ostrich, that is of a BIRD", &["ostrich meat"]),
    ("бастурма", "air-dried cured BEEF, a whole muscle", &["basturma", "pastirma"]),
    ("суджук", "a dry cured sausage of beef or lamb", &["sujuk", "sucuk"]),
    ("буженина", "a whole piece of PORK, baked and not cured", &["buzhenina", "baked pork"]),
    // «Fuet truffle» модель звала грибом, цепляясь за слово truffle: это колбаса С
    // трюфелем, а не трюфель.
    ("fuet", "a Catalan dry cured PORK sausage", &["fuet", "fuet truffle"]),
    ("чоризо", "a Spanish dry cured pork sausage", &["chorizo"]),
    // ── Крупы, бобовые, зелень, ягоды: русские имена, которых модель не знает ──
    // Все они ловились живым путём: без строки в словаре «Полба» опознавалась как
    // «тип рыбы» с уверенностью 0.90, а «Маш» — как «имя или сокращение».
    ("полба", "spelt, an ancient wheat GRAIN", &["spelt", "farro", "emmer"]),
    ("булгур", "cracked parboiled wheat, a GRAIN", &["bulgur"]),
    ("кускус", "small steamed balls of wheat semolina, a GRAIN", &["couscous"]),
    ("маш", "mung bean, a small green LEGUME", &["mung bean", "moong"]),
    ("нут", "chickpea, a LEGUME", &["chickpea", "garbanzo"]),
    ("чечевица", "lentil, a LEGUME", &["lentil"]),
    ("топинамбур", "jerusalem artichoke, an edible TUBER", &["jerusalem artichoke", "sunchoke"]),
    ("руккола", "a peppery salad LEAF", &["rocket", "arugula"]),
    ("кинза", "the fresh LEAVES of coriander", &["cilantro", "coriander leaves"]),
    ("щавель", "a sour salad LEAF", &["sorrel"]),
    ("ирга", "saskatoon, a dark sweet BERRY on a shrub", &["saskatoon berry", "juneberry"]),
    ("жимолость", "honeyberry, an edible blue BERRY", &["honeyberry", "haskap"]),
    ("облепиха", "sea buckthorn, an orange BERRY", &["sea buckthorn"]),
    ("морошка", "cloudberry, an amber BERRY", &["cloudberry"]),
    ("фейхоа", "feijoa, a green FRUIT", &["feijoa", "pineapple guava"]),
    // ── Кухни, а не языки ──
    // Национальные блюда приходят в дневник как есть, и по имени модель достраивает
    // их наугад: «Кыстыбый» она звала лососем, «Эчпочмак» — русским пельменем. Здесь
    // важен СОСТАВ: по нему решаются мясные признаки и овощная планка.
    ("кыстыбый", "a Tatar flatbread folded over MASHED POTATO, usually without meat",
        &["kystybyi"]),
    ("эчпочмак", "a Tatar triangular PASTRY filled with BEEF or LAMB, potato and onion",
        &["echpochmak", "uchpuchmak"]),
    ("бешбармак", "boiled LAMB, BEEF or horse meat with flat noodles", &["beshbarmak"]),
    ("куурдак", "fried MEAT of lamb or beef with potato and onion", &["kuurdak", "kavardak"]),
    ("манты", "steamed DUMPLINGS filled with minced LAMB or BEEF and onion", &["manti"]),
    ("лагман", "hand-pulled NOODLES with MEAT and vegetables", &["lagman"]),
    ("шурпа", "a rich SOUP of LAMB or BEEF with vegetables", &["shurpa", "shorpa"]),
    ("самса", "a baked PASTRY filled with minced MEAT and onion", &["samsa", "samosa"]),
    ("чак-чак", "fried dough pieces in honey — a SWEET, no meat", &["chak-chak"]),
    ("хачапури", "a Georgian bread filled with CHEESE, sometimes an egg", &["khachapuri"]),
    ("хинкали", "large Georgian DUMPLINGS filled with minced MEAT", &["khinkali"]),
    ("чахохбили", "a Georgian stew of CHICKEN with tomatoes", &["chakhokhbili"]),
    ("лобио", "a Georgian dish of BEANS", &["lobio"]),
    ("аджапсандали", "a Georgian stew of VEGETABLES: aubergine, pepper, tomato",
        &["ajapsandali"]),
    ("долма", "vine leaves stuffed with minced MEAT and rice", &["dolma"]),
    ("люля-кебаб", "grilled minced LAMB or BEEF on a skewer", &["lula kebab"]),
    ("шаурма", "flatbread rolled around roasted MEAT with vegetables and sauce",
        &["shawarma", "doner"]),
    ("плов", "rice cooked with MEAT, carrot and onion", &["pilaf", "plov"]),
    ("пахлава", "layered pastry with NUTS and syrup — a SWEET", &["baklava"]),
    ("кутабы", "a thin flatbread folded over GREENS, cheese or minced meat", &["qutab"]),
    ("бозбаш", "a SOUP of LAMB with chickpeas", &["bozbash"]),
    ("хаш", "a broth of beef or lamb TRIPE and shanks", &["khash"]),
    ("сациви", "CHICKEN or turkey in a walnut sauce", &["satsivi"]),
    // ── Сладости и закуски, приходящие в дневник голым именем ──
    ("козинак", "SEEDS or nuts — most often sunflower — set in hard caramel, a SWEET",
        &["kozinaki", "seed brittle", "nut brittle"]),
    ("халва", "a dense SWEET of ground sunflower seeds or sesame with sugar", &["halva"]),
    ("унаги", "a Japanese sweet-soy SAUCE served with eel, and the grilled eel itself",
        &["unagi sauce", "kabayaki sauce"]),
    // ── Марки с этикеток ──
    // Марка — не еда, но она стоит первым словом, и без строки в словаре модель
    // отвечает «не знаю такого слова» и роняет опознание целиком. Живым путём так
    // потерялись «Calve Майонез» и «Экомилк Творожный сыр»: майонез и творожный сыр
    // модель знает прекрасно, споткнулась она о марку.
    ("calve", "a BRAND of mayonnaise and sauces — the brand only, not a food",
        &["Calve"]),
    ("экомилк", "a BRAND of dairy — the brand only, not a food", &["Ekomilk"]),
    ("даня", "a BRAND of children's quark and yoghurt desserts, not a name of a person",
        &["Danya", "Danone kids"]),
    ("жигули", "a BRAND of beer; «барное 0.0» is its alcohol-free lager", &["Zhiguli"]),
    ("балтика", "a BRAND of beer; «0.0» is its alcohol-free lager", &["Baltika"]),
];

/// Русские сокращения с этикеток. Их модель не знает и достраивает наугад: «Черника
/// с/м» была опознана как «blueberry in syrup» — «с/м» прочлось сиропом, после чего
/// верное правило про сахар отправило ягоду в NONE.
///
/// Живут здесь, а не в правилах признаков: это часть ОПОЗНАНИЯ, а не оценки.
const SHORTHANDS: &[(&str, &str)] = &[
    ("с/м", "fresh-frozen, nothing added"),
    ("св/м", "fresh-frozen, nothing added"),
    ("х/к", "cold-smoked"),
    ("г/к", "hot-smoked"),
    ("с/с", "lightly salted"),
    ("б/г", "headless, said of fish"),
    ("б/к", "skinless"),
    ("в/с", "top grade — a grade and nothing more"),
    ("п/ф", "a semi-prepared product"),
    ("ц/з", "wholegrain"),
];

/// Словарь как текст промпта — целиком, по строке на запись.
///
/// Целиком, а не «только подходящие строки». Была попытка отбирать записи НАШИМ
/// кодом по вхождению подстроки — и это оказалось прямо против задумки: сопоставлять
/// слово с записью модель умеет лучше нас. Она знает падежи («филе гольца»),
/// латиницу, опечатки и слово внутри фразы, а `contains` не знает ничего: ключ
/// «голец» не найдётся в «гольца», то есть словарь молчал бы ровно там, где нужен.
///
/// По строке на запись — потому что слитный список через «;» модель путает: на
/// «бастурму» она выдала `ostrich meat`, значение соседней записи.
fn dictionary_block() -> String {
    if RARE_NAMES.is_empty() && SHORTHANDS.is_empty() {
        return String::new();
    }
    let names = RARE_NAMES
        .iter()
        .map(|(word, what, tr)| {
            format!("  {word}: {what}, could be translated as [{}]", tr.join(", "))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let short = SHORTHANDS
        .iter()
        .map(|(word, what)| format!("  {word}: {what}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "A DICTIONARY of names that are rare or easy to mistake. Each line stands on its own \
         and has nothing to do with its neighbours. Match the name you were given against it \
         YOURSELF — the word may stand in another grammatical case, in the middle of a phrase, \
         in Latin letters or misspelled, and it still counts as the same word.\n\
         {names}\n\n\
         ABBREVIATIONS from Russian labels. Every one of them is about storage, cut or grade — \
         none adds an ingredient.\n\
         {short}\n\n"
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
    pub egg: Option<bool>,
}

/// Контекст, который течёт по конвейеру.
#[derive(Clone)]
pub struct FlagsCtx {
    pub food_name: String,
    /// Заполняется первым узлом: «Arctic char, a fish».
    pub identity: Option<String>,
    /// Уверенность лучшего варианта опознания, 0…1.
    pub identity_confidence: f64,
    /// Модель ответила про своё знание словом UNKNOWN.
    pub identity_no_food_named: bool,
    /// В словаре редких имён ничего не нашлось (NONE).
    pub identity_dictionary_empty: bool,
    /// В самом имени нашлось слово, которое модель знает как еду.
    pub identity_food_word: bool,
    pub flags: Flags,
    /// Клетчатка, г на 100 г. Её отдаёт растительный узел вместе с частью растения:
    /// продукт там уже опознан, и отдельный запрос за ней был лишними деньгами.
    pub fibre_g: Option<f64>,
    /// Какие шаги спрашивать. Пустой список значит «ни одного»: признаки на месте
    /// или их спрашивали меньше суток назад и получили отказ.
    wanted: Vec<Step>,
    /// Остановиться сразу после опознания. Нужно, когда признаки у продукта уже
    /// есть, а опознание всё равно требуется — его ждут кальций, железо и жиры.
    identify_only: bool,
    /// Попытки по шагам: у каждого свои, чтобы упавший не тратил чужие.
    tries: [u32; 7],
    /// Обоснования по шагам — для лога и телеметрии.
    reasons: Vec<String>,
    pub last_error: Option<String>,
}

impl FlagsCtx {
    fn new(food_name: &str) -> Self {
        Self {
            food_name: food_name.to_string(),
            identity: None,
            identity_confidence: 0.0,
            identity_no_food_named: false,
            identity_dictionary_empty: false,
            identity_food_word: false,
            flags: Flags::default(),
            fibre_g: None,
            wanted: Vec::new(),
            identify_only: false,
            tries: [0; 7],
            reasons: Vec::new(),
            last_error: None,
        }
    }

    /// Опознан ли продукт.
    ///
    /// РЕШАЕТ ПРИЗНАНИЕ, А НЕ ЧИСЛО. На выдуманное слово модель отвечает «I do not know
    /// this word» и тут же ставит версиям 0.80, 0.70, 0.60: числа выражают уверенность
    /// в ФОРМУЛИРОВКЕ, а не в том, что такая еда есть. «Бубурек копчёный — a type of
    /// smoked bread» набирал 0.80 три раза из трёх. Признание спрашивается отдельным
    /// булевым полем — разбирать его из текста значило бы гадать по словам.
    ///
    /// Поэтому опознание считается несостоявшимся, когда модель не вспомнила ничего
    /// САМА и словарь тоже промолчал. Одного из двух хватает: голец опознан словарём,
    /// творог — памятью.
    ///
    /// Порог по уверенности остаётся вторым ситом — он ловит случаи, где модель
    /// что-то припомнила, но сама себе не верит.
    fn recognised(&self) -> bool {
        self.identity_weight() >= IDENTITY_MIN_CONFIDENCE
    }

    /// Уверенность, ПОНИЖЕННАЯ ПРИЗНАНИЕМ: модель сказала, что слова не знает, и
    /// словарь тоже промолчал — значит версия построена ни на чём, и её число значит
    /// меньше. Множитель `IDENTITY_UNKNOWN_PENALTY` оставляет дорогу только тем
    /// ответам, где модель уверена полностью.
    ///
    /// Понижаем ТОЛЬКО когда молчат ВСЕ ТРИ: гольца модель своей памятью не знает,
    /// его опознаёт словарь, и штрафовать за это было бы наказанием за честность.
    ///
    /// Третье — слово-еда в самом имени. «SPAR Чипсы нори» и «ВкусВилл Десерт
    /// картошка» модель разбирала верно, но признавалась, что слов не знает, и
    /// ставила себе 0.80 — ровно столько же, сколько выдумке вроде «Зюзюбряк
    /// ассорти». По числам они неразличимы; различает их то, что в живом имени еда
    /// названа своим словом, а в выдуманном — нет.
    fn identity_weight(&self) -> f64 {
        let guessing = self.identity_no_food_named
            && self.identity_dictionary_empty
            && !self.identity_food_word;
        let penalty = if guessing { IDENTITY_UNKNOWN_PENALTY } else { 1.0 };
        self.identity_confidence * penalty
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
                "WHAT THIS FOOD MOST LIKELY IS, with the confidence of each guess: {w}. These \
                 are given to you — judge by them and do not re-guess the species yourself.\n\n"
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
struct IdentityOption {
    /// What this food most likely is. English, a sentence of five or six words.
    definition: String,
    /// How sure you are of this one, from 0.0 to 1.0.
    confidence: f64,
}

// ДВА ПОЛЯ ПЕРЕД ВАРИАНТАМИ — не украшение, а то, что делает уверенность честной.
//
// Без них модель ставит 0.90 чему угодно: «Кыстыбый — a type of fish», «Полба — тип
// рыбы». С ними она сперва отвечает, что помнит сама и что нашла в словаре, и уже
// после этого числа перестают врать: незнакомое получает 0.10, а голец — 0.90 через
// словарь. Замер это показал ещё до конвейера, но в код тогда перенесли только
// варианты, и порог отсекать было нечем.
// ПОЛЯ ЗДЕСЬ БЕЗ ДОКОВ, и это не небрежность.
//
// Доки полей уезжают в json_schema, а ai-worker вклеивает схему ТЕКСТОМ в промпт —
// модель читает их наравне с инструкцией. У этого узла инструкция уже говорит про
// каждое поле, и повтор оказался вреден: описание «если не знаешь — так и скажи»
// перевешивало словарь. Замер на слове «Маш», которое В СЛОВАРЕ ЕСТЬ: с описаниями
// оно не находилось ни разу из трёх, без них — три раза из трёх.
//
// У остальных узлов доки полей ОСТАЮТСЯ: там они несут смысл, которого в тексте нет
// (части растения, доли жира), и без них замеры проседают — растение падало с 32 из
// 34 до 28.
#[derive(Debug, Deserialize, JsonSchema)]
struct IdentityAnswer {
    // ПОРЯДОК ПОЛЕЙ — ЭТО ПОРЯДОК РАССУЖДЕНИЯ. Каждый флаг идёт СРАЗУ ЗА своим
    // текстом: сперва модель пишет, что вспомнила, и тут же отвечает, знает ли слово
    // вообще; потом — что нашла в словаре, и тут же, нашла ли. Два флага подряд
    // модель заполняет согласованно, и признание в одном тянет за собой другое.
    from_own_knowledge: String,
    /// За именем не стоит еды, которую модель могла бы назвать. Это и есть главный
    /// сигнал: на выдуманное имя она отвечает честно, но версиям всё равно ставит
    /// 0.80 — числа выражают уверенность в формулировке, а не в существовании еды.
    ///
    /// Вопрос именно о ЕДЕ, а не о слове. Прежнее имя поля —
    /// `i_dont_know_this_word` — спрашивало про слово, и модель отвечала буквально
    /// верно: слова «Помбир» (опечатка в «пломбир») или «Спиральчатый» она правда
    /// не знает. Признание тянуло штраф, и живые продукты отсекались — «ВкусВилл
    /// Помбир Сэндвич» и «Спиральчатый спагетти» пришли из боевого дневника.
    i_cannot_name_the_food_behind_this_name: bool,
    from_dictionary: String,
    /// Нашлась ли строка словаря ПРО ЭТУ еду. Булевым полем, а не разбором «NONE» из
    /// текста: строку она пишет по-разному — «NONE», «none», «NONE — nothing fits», —
    /// и любое отклонение молча означало бы «словарь нашёл», сняв штраф с выдумки.
    i_see_this_food_in_the_dictionary: bool,
    /// Слово ИЗ САМОГО ИМЕНИ, которое модель знает как еду, или NONE.
    ///
    /// Последняя опора, когда всё остальное молчит: слова незнакомы, словарь пуст,
    /// уверенность средняя. У живого имени такое слово почти всегда есть — «SPAR
    /// Чипсы нори» это чипсы, «ВкусВилл Десерт картошка» это десерт, — а у
    /// выдуманного нет: в «Зюзюбряк ассорти» и «Бубурек копчёный» еды не названо
    /// ни одним словом. Числа в этих случаях одинаковы (0.80 у всех), и различить
    /// их можно только так.
    known_food_word_in_the_name: String,
    options: Vec<IdentityOption>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CategoryAnswer {
    /// The ONE word naming the category that fits, or NONE if none does.
    category_that_fits: String,
}

// Растительный ответ: части растения и клетчатка.
//
// Прежде спрашивался наш собственный термин «овощ или фрукт», и модель угадывала,
// что мы под ним понимаем: картофель получал то овощ, то корнеплод, чипсы гуляли
// между прогонами. Части растения — факты, их угадывать не надо, а признак
// «овощи и фрукты» выводится из них НАШИМ кодом: растение, но не корень, не боб,
// не зерно и не семя. Замер: 29/29.
#[derive(Debug, Deserialize, JsonSchema)]
struct PlantAnswer {
    /// Is this food a part of some plant at all?
    is_product_a_part_of_some_plant: bool,
    /// A part that grew UNDERGROUND: a root, a tuber or a bulb — potato, carrot,
    /// beetroot, radish, celeriac, onion.
    is_root: bool,
    /// A part that grew ABOVE THE GROUND on the stem: a leaf or a stalk — cabbage,
    /// lettuce, dill, celery stalk, rhubarb, asparagus.
    is_leaf: bool,
    /// A fruit or a berry: apple, cherry, cucumber, tomato.
    is_fruit: bool,
    /// A seed or a nut: walnut, pistachio, sunflower seed, sesame.
    is_seed: bool,
    /// The fruit of a legume plant: lentils, beans, peas, soy, chickpeas.
    is_legume: bool,
    /// A grain: wheat, rye, spelt, buckwheat, rice, oats, and bread made of them.
    is_grain: bool,
    /// The name of the FIBRE REFERENCE entry this food matches, copied exactly, or NONE.
    fibre_reference_key: String,
    /// Dietary fibre in grams per 100 g. Zero is a valid answer.
    fibre_g_per_100g: f64,
}

// Ответ узла ЯЙЦА. Отдельная структура ради ИМЕНИ ПОЛЯ: «verdict» ничего не говорит
// модели о том, что решается, а имя-вопрос — говорит. Это тот же приём, что у
// растительного узла, и он там дал 29/29.
#[derive(Debug, Deserialize, JsonSchema)]
struct EggAnswer {
    /// Which category fits, or that none does. One short sentence, at most 12 words.
    reason: String,
    /// Does this product belong to bird eggs, or is it made of bird eggs?
    is_this_product_of_bird_eggs: bool,
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

    // ЗДЕСЬ СОБИРАЕТСЯ ПРОМПТ ОПОЗНАНИЯ — самый дорогой в приложении: на нём стоят
    // все признаки и все нутриенты. Прежде чем менять хоть слово, откройте
    // `docs/food-identity.md`: там перечислены отказы, которые эти формулировки уже
    // лечат, и порядок замера. Каждое предложение ниже — след поломки.
    fn serialize(&self) -> String {
        format!(
            "A person wrote this into their food diary: {name}\n\n\
             Answer five things about it.\n\
             1. \"from_own_knowledge\": the closest definition you can recall YOURSELF, without \
             the dictionary.\n\
             2. \"i_cannot_name_the_food_behind_this_name\": this asks about the FOOD, not \
             about the words. Diary names are full of words nobody can be expected to know — \
             makers («Экомилк», «Calve», «ВкусВилл»), misspellings («Помбир» for пломбир), \
             percentages, cuts, packaging, dialect. None of that matters while a food you DO \
             know still stands behind them: «Спиральчатый спагетти» is spaghetti, «ВкусВилл \
             Помбир Сэндвич» is an ice cream sandwich, «Маасдам 45%» is maasdam cheese, \
             «Жигули барное 0.0» is alcohol-free beer. For all of those the answer is FALSE — \
             you can name the food. It is FALSE for plain foods too: творог, гречка, скумбрия.\n\
             THIS ANSWER MUST AGREE WITH THE LINE YOU HAVE JUST WRITTEN. If \
             \"from_own_knowledge\" names a food — «spaghetti», «пломбир», «cream cheese» — \
             then you HAVE named the food, and this field is FALSE, however odd the wording \
             of the name was.\n\
             Answer TRUE only when nothing edible is left to name once the words you do \
             understand are set aside. «Копчёный», «сладкая», «ассорти», «столовые», «45%» \
             name no food: in «Бубурек копчёный» smoked is a way of cooking and бубурек is \
             nothing you know — TRUE. And an unknown word GLUED to a food word is not a maker \
             but an unknown food: «Квазилапша» is not lapsha by some «Квази» — TRUE.\n\
             3. \"from_dictionary\": the closest definition from the dictionary below, or NONE \
             if nothing in it fits this name.\n\
             THE DICTIONARY ANSWERS ABOUT THE SAME WORD, NEVER ABOUT A SIMILAR-SOUNDING ONE. \
             Russian words rhyme constantly, and a shared ending is not a match: «огурец» is a \
             cucumber and has nothing to do with «голец» the fish, «морковь» is not «морошка», \
             «крахмал» is not «кальмар». A line fits only when it is about the very same food \
             — the same word in another grammatical case, in Latin letters or misspelled. If \
             the closest line is merely a rhyme, answer NONE: you already know what a cucumber \
             is, and the dictionary is there for words you do NOT know.\n\
             4. \"i_see_this_food_in_the_dictionary\": true when a line of the dictionary is \
             about THIS food. False when the dictionary holds nothing about it — including the \
             case where the nearest line only sounds alike.\n\
             5. \"known_food_word_in_the_name\": one word COPIED FROM THE NAME that you know \
             as a food or a dish, or NONE if the name holds no such word. Not the maker, not \
             the way it was cooked, not the packaging: in «SPAR Чипсы нори» it is «Чипсы», in \
             «ВкусВилл Десерт картошка» it is «Десерт», in «Бубурек копчёный» it is NONE — \
             smoked names no food, and бубурек is nothing you know.\n\
             6. \"options\": the three most likely definitions of the food, the surest first, \
             each a sentence of five or six words, each with your confidence from 0.0 to \
             1.0.\n\n\
             {rare}Respond with ONLY a minified JSON object and nothing else.",
            rare = dictionary_block(),
            name = self.food_name,
        )
    }

    fn update_context(&self, mut ctx: Self::Context, raw: Self::Output) -> Self::Context {
        ctx.tries[0] += 1;
        match crate::services::ai::parse_one::<IdentityAnswer>(&raw) {
            Ok(a) => {
                // Признакам уходят ВСЕ варианты с их достоверностью, а не только
                // первый: пусть видно, что уверенности нет, — «голец» модель звала и
                // рыбой, и кониной, и разница между 0.9 и 0.4 здесь и есть ответ.
                ctx.identity_no_food_named = a.i_cannot_name_the_food_behind_this_name;
                ctx.identity_dictionary_empty = !a.i_see_this_food_in_the_dictionary;
                // NONE в любом написании значит «слова-еды в имени нет».
                let word = a.known_food_word_in_the_name.trim();
                ctx.identity_food_word =
                    !word.is_empty() && !word.to_lowercase().starts_with("none");
                leptos::logging::log!(
                    "опознание «{}»: память — {} (еду не назвать: {}) · словарь — {} (нашёл: {})",
                    ctx.food_name,
                    a.from_own_knowledge.trim(),
                    a.i_cannot_name_the_food_behind_this_name,
                    a.from_dictionary.trim(),
                    a.i_see_this_food_in_the_dictionary
                );
                let opts: Vec<String> = a
                    .options
                    .iter()
                    .filter(|o| !o.definition.trim().is_empty())
                    .take(3)
                    .map(|o| format!("{} ({:.2})", o.definition.trim(), o.confidence))
                    .collect();
                if opts.is_empty() {
                    ctx.last_error = Some("опознание пустое".to_string());
                } else {
                    ctx.identity_confidence = a
                        .options
                        .iter()
                        .filter(|o| !o.definition.trim().is_empty())
                        .map(|o| o.confidence)
                        .fold(0.0_f64, f64::max);
                    ctx.identity = Some(opts.join("; "));
                    ctx.last_error = None;
                    // УСПЕШНОЕ опознание тоже уходит в телеметрию — с уверенностью и
                    // с весом после штрафа. Раньше мы видели только отказы, а
                    // опасны и те, что прошли ЕДВА: продукт разобран, признаки
                    // проставлены, и ошибка в них не видна ниоткуда. Теперь по
                    // числам можно спросить, кто прошёл впритык к порогу.
                    crate::services::telemetry::report_detection(
                        "identity.ok",
                        &ctx.food_name,
                        opts.first().map(String::as_str).unwrap_or(""),
                        &format!(
                            "память: {} · словарь: {} · слово-еда: {}",
                            a.from_own_knowledge.trim(),
                            a.from_dictionary.trim(),
                            a.known_food_word_in_the_name.trim(),
                        ),
                        &[ctx.identity_confidence, ctx.identity_weight()],
                    );
                }
            }
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
    /// Исполнитель БЕЗ рассуждения — им спрашивается само опознание.
    ///
    /// Рассуждая, модель уговаривает саму себя: на «Кыстыбый» она в памяти пишет «I
    /// don't recognize this term», а вариантам проставляет 0.6–0.7 и выдаёт лосося.
    /// Без рассуждения тот же вопрос пять раз из пяти даёт честные 0.10. Незнание
    /// должно оставаться незнанием.
    executor: Qwen,
    /// ЗАПАСНОЙ исполнитель — та же модель у другого поставщика.
    ///
    /// Основная модель опознания живёт у стороннего провайдера, и отказать она может
    /// не из-за продукта: кончилась квота, не прошёл платёж, лежит сам провайдер.
    /// Повторять вопрос тому же поставщику в этом случае — терять попытки впустую,
    /// поэтому ПЕРВАЯ ЖЕ неудача уводит следующий заход сюда.
    ///
    /// `None` — запасного нет (или он уже используется), повтор идёт к основному.
    fallback: Option<Qwen>,
    /// Исполнитель С рассуждением — он уходит дальше, в узлы признаков: там
    /// противоречия настоящие («язык это мышца, но орган»), и их надо продумать.
    flags_executor: Qwen,
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
            // Запасной вступает с первого же повтора и дальше остаётся один: если
            // не ответил и он, дело в вопросе, а не в поставщике.
            let (executor, fallback) = match self.fallback.clone() {
                Some(spare) => {
                    leptos::logging::log!(
                        "опознание «{}»: основная модель не ответила ({}), пробуем запасную",
                        ctx.food_name,
                        ctx.last_error.clone().unwrap_or_default()
                    );
                    (spare, None)
                }
                None => (self.executor.clone(), None),
            };
            return Some(Box::new(NodeWrapper::new(IdentifyNode {
                executor,
                fallback,
                flags_executor: self.flags_executor.clone(),
            })));
        }
        // Ниже порога признаки не спрашиваются вовсе: продукт неизвестен, и любой
        // вердикт о нём был бы выдуман.
        if ctx.identify_only || !ctx.recognised() {
            return None;
        }
        // Первый ЗАПРОШЕННЫЙ шаг, а не первый по порядку: спрашивать признак, который
        // уже стоит, незачем — и это не экономия, а точность. Продукт с одним пустым
        // признаком гонял через модель все шесть вопросов, и пять готовых вердиктов
        // каждый раз переписывались новыми, которые могли и разойтись с прежними.
        ctx.wanted.first().map(|first| {
            Box::new(NodeWrapper::new(FlagNode {
                executor: self.flags_executor.clone(),
                step: *first,
            })) as Box<dyn NodeRunner<Self::Context>>
        })
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
    Egg,
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
            Step::Egg => 6,
        }
    }

    fn next(self) -> Option<Step> {
        match self {
            Step::VegFruit => Some(Step::Heme),
            Step::Heme => Some(Step::RedMeat),
            Step::RedMeat => Some(Step::ProcessedMeat),
            Step::ProcessedMeat => Some(Step::MilkGlobule),
            Step::MilkGlobule => Some(Step::Egg),
            Step::Egg => None,
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
            // Яйцу опознание впереди нужнее всего: без него «Афоня» или «Киндер» —
            // просто слова, а с ним видно, что это яйцо шоколадное, а не птичье.
            Step::Egg => true,
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
            Step::Egg => "яйцо",
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
            Step::Egg => "flag.egg",
        }
    }

    fn get(self, f: &Flags) -> Option<bool> {
        match self {
            Step::VegFruit => f.veg_fruit,
            Step::Heme => f.heme,
            Step::RedMeat => f.red_meat,
            Step::ProcessedMeat => f.processed_meat,
            Step::MilkGlobule => f.milk_globule,
            Step::Egg => f.egg,
        }
    }

    fn set(self, f: &mut Flags, v: bool) {
        match self {
            Step::VegFruit => f.veg_fruit = Some(v),
            Step::Heme => f.heme = Some(v),
            Step::RedMeat => f.red_meat = Some(v),
            Step::ProcessedMeat => f.processed_meat = Some(v),
            Step::MilkGlobule => f.milk_globule = Some(v),
            Step::Egg => f.egg = Some(v),
        }
    }

    /// Растительный шаг спрашивает части растения и клетчатку, а не наш термин.
    fn as_plant(self) -> bool {
        matches!(self, Step::VegFruit)
    }

    /// Яичный шаг спрашивает СВОИМ вопросом, а не через общий перечень категорий:
    /// вопрос здесь один и короткий, и перечня ему не нужно.
    fn as_egg(self) -> bool {
        matches!(self, Step::Egg)
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
                juices belong to none of the categories, however much fruit went into them. \
                FREEZING, DRYING AND COOKING change nothing: frozen berries are berries, boiled \
                beetroot is a vegetable, and a note about freezing in the name — «с/м», \
                «мороженая», «frozen» — is about storage, not about what the food is.",
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
                It is MUSCLE that belongs to these categories. A TONGUE and a HEART are \
                muscle, so beef tongue, pork heart and dishes made mainly of them DO belong. \
                Organs that are NOT muscle stay outside — liver, kidneys, lungs, tripe, brain — \
                however red they look. BIRDS are outside whatever the part: chicken, turkey, \
                duck, goose, ostrich, and a chicken heart with them, because a bird is not a \
                mammal. Fish, seafood, eggs, dairy and anything from a plant are outside too.",
            Step::ProcessedMeat => "\
                THE QUESTION: is this meat PRESERVED — cured, smoked, salted, dried or \
                fermented for keeping? Meat here is the flesh of a MAMMAL or a BIRD.\n\n\
                Meat is preserved when it has been through one of these: CURING with nitrite or \
                nitrate salt, SMOKING, prolonged SALTING, AIR-DRYING, or FERMENTATION. Cooking \
                is not preserving: boiling, frying, baking, stewing, grilling, mincing, \
                freezing and packaging leave meat unpreserved, however industrial the \
                process.\n\n\
                A word like «молочные», «сливочные», «milk» or «cream» in a sausage's name \
                describes its recipe, not its kind: milk sausages are sausages, cured with \
                nitrite like any other, and the answer for them is TRUE.\n\n\
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
            // Вопрос яйца задаётся своим промптом целиком (см. `serialize`), перечня
            // категорий у него нет.
            Step::Egg => "",
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
        if self.step.as_plant() {
            return format!(
                "A person wrote this into their food diary: {name}\n\n\
                 {given}\
                 First answer whether this food is a part of some plant at all. If it is, \
                 answer WHICH PART A PERSON EATS HERE — not where the plant keeps it. The \
                 parts: one that grew underground (root, tuber, bulb), one that grew above the \
                 ground on the stem (leaf or stalk), a fruit or berry, a seed or nut, a legume, \
                 a grain. Celery has a root, but its stalk is what is eaten here, and the stalk \
                 grew above the ground. If the food is not from a plant, every part field is \
                 false.\n\n\
                 A plant part is the PLANT MATTER ITSELF — whole or cut, raw or cooked, fresh, \
                 frozen or dried. What was PRESSED OR REFINED OUT of a plant is not a part of \
                 it: an oil, a sugar, a syrup, a starch. Potato starch is not a tuber — of the \
                 tuber nothing is left in it but the starch, and one substance taken out of a \
                 plant is not a part of that plant. For all of those every part field is false \
                 as well.\n\n\
                 Then give the dietary FIBRE of this food, in grams per 100 g. FIRST look for \
                 the food in the REFERENCE below and put its entry name into \
                 \"fibre_reference_key\", copied exactly — we take the number ourselves. THE \
                 FORM IS PART OF THE ENTRY: dry grains and boiled ones are separate, and \
                 picking the wrong one is worse than picking none. Answer NONE when no entry \
                 is the same food, and give the grams as you know them. Zero is a valid \
                 answer for food that has none.\n\n\
                 {fibre_reference}\n\n\
                 The reference keys are written in RUSSIAN: copy the key letter for letter \
                 as it stands there, never translated and never transliterated.\n\n\
                 Respond with ONLY a minified JSON object and nothing else.",
                name = self.food_name,
                given = self.given,
                fibre_reference = crate::services::ai::fibre_reference_table(),
            );
        }
        if self.step.as_egg() {
            return format!(
                "A person wrote this into their food diary: {name}\n\n\
                 {given}\
                 Decide whether this product belongs to BIRD EGGS, or is made of bird eggs — \
                 of any bird, farmed or wild.\n\n\
                 WE COUNT GRAMS. Say yes only when the grams of this product ARE grams of egg: \
                 the product is egg through and through, or egg with no more than a spoon of \
                 butter, milk or oil in it. Cooking, curing and drying change nothing — raw, \
                 boiled, fried, poached, smoked, salted, dried into powder, whole or beaten, \
                 yolk or white alone.\n\n\
                 Say no when eggs are ONE INGREDIENT AMONG SEVERAL and their share cannot be \
                 told from the name: mayonnaise, sauces, salads, pancakes, batter, pasta, \
                 cakes, biscuits, cutlets, casseroles. There is no way to count their grams of \
                 egg, so they are not egg.\n\n\
                 Judge by WEIGHT, not by importance. A yolk gives mayonnaise its name and its \
                 texture, but its grams are oil, so mayonnaise is NO. A SALAD is never egg, \
                 whatever it is called: «яичный салат» is eggs cut up with dressing and other \
                 foods, and nobody can say how much of it was egg.\n\n\
                 Fill the reason FIRST — one short sentence — and let the answer follow from \
                 it.\n\n\
                 Respond with ONLY a minified JSON object and nothing else.",
                name = self.food_name,
                given = self.given,
            );
        }
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
        if self.step.as_plant() {
            match crate::services::ai::parse_one::<PlantAnswer>(&raw) {
                Ok(a) => {
                    // «Овощи и фрукты» — это растение, но НЕ боб, не зерно и не семя.
                    // Решает наш код, а не модель: у неё спрашивают только факты о
                    // растении.
                    //
                    // КОРНЕПЛОДЫ СЧИТАЮТСЯ. Раньше «выросло под землёй» выбрасывало
                    // их из планки целиком, и вместе с картошкой из неё выпадали
                    // редиска, морковь и свёкла: 800 г редиски давали ноль овощей за
                    // день. Сам признак корня никуда не делся — он остаётся в разборе
                    // продукта («растение: корень»), просто больше не фильтрует.
                    let veg = a.is_product_a_part_of_some_plant
                        && !a.is_legume
                        && !a.is_grain
                        && !a.is_seed;
                    self.step.set(&mut ctx.flags, veg);
                    // Клетчатка из СПРАВОЧНИКА берётся как есть: числа модель помнит
                    // плохо — варёной чечевице давала 3.8 г против справочных 7.9.
                    // Её работа — узнать продукт и назвать запись.
                    let (fibre, fibre_src) =
                        match crate::services::ai::fibre_reference_hit(&a.fibre_reference_key) {
                            Some((name, g)) => (*g, format!("справочник «{name}»")),
                            None => (a.fibre_g_per_100g.max(0.0), "своё знание".to_string()),
                        };
                    ctx.fibre_g = Some(fibre);
                    let part = if a.is_root { "корень" }
                        else if a.is_leaf { "лист" }
                        else if a.is_fruit { "плод" }
                        else if a.is_seed { "семя" }
                        else if a.is_legume { "боб" }
                        else if a.is_grain { "зерно" }
                        else { "не растение" };
                    ctx.reasons.push(format!("растение: {part}"));
                    ctx.last_error = None;
                    leptos::logging::log!(
                        "фрукты/овощи «{}»: {veg} — {part}, клетчатка {fibre} г [{fibre_src}]",
                        ctx.food_name,
                    );
                    crate::services::telemetry::report_detection(
                        self.step.kind(),
                        &ctx.food_name,
                        &veg.to_string(),
                        &format!(
                            "{} → {part}",
                            ctx.identity.clone().unwrap_or_else(|| "(не опознано)".to_string())
                        ),
                        &[fibre],
                    );
                }
                Err(e) => {
                    ctx.last_error =
                        Some(format!("растение не разобрано: {e}, ответ: {raw}"))
                }
            }
            return ctx;
        }
        let parsed: Result<(bool, String), String> = if self.step.as_egg() {
            crate::services::ai::parse_one::<EggAnswer>(&raw)
                .map(|a| (a.is_this_product_of_bird_eggs, a.reason))
                .map_err(|e| format!("яйцо не разобрано: {e}, ответ: {raw}"))
        } else if self.step.as_category() {
            // Вердикт выводим САМИ из названной категории: пока его ставила модель,
            // она называла «FRUITS» и отвечала «да» на вишню в сиропе.
            crate::services::ai::parse_one::<CategoryAnswer>(&raw)
                .map_err(|e| format!("категория не разобрана: {e}, ответ: {raw}"))
                .and_then(|a| {
                    veg_fruit_from_category(&a.category_that_fits)
                        .map(|v| (v, a.category_that_fits))
                })
        } else {
            crate::services::ai::parse_one::<FlagAnswer>(&raw)
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
        if self.step.as_plant() {
            run_structured::<E, PlantAnswer>(executor, self.serialize(), self.name())
        } else if self.step.as_egg() {
            run_structured::<E, EggAnswer>(executor, self.serialize(), self.name())
        } else if self.step.as_category() {
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
        // Иначе — к следующему ЗАПРОШЕННОМУ шагу. Упавший признак не мешает
        // остальным: он останется пустым и будет переспрошен, когда пройдут сутки.
        let pos = ctx.wanted.iter().position(|s| *s == self.step)?;
        ctx.wanted.get(pos + 1).map(|next| {
            Box::new(NodeWrapper::new(FlagNode {
                executor: self.executor.clone(),
                step: *next,
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
pub struct Recognised {
    pub flags: Flags,
    pub fibre_g: Option<f64>,
    pub identity: Option<String>,
    /// Вес опознания после штрафа — по нему решается, стоит ли класть опознание в
    /// кэш (см. `food_probe`). У опознания, взятого из кэша, это его прежний вес.
    pub identity_weight: f64,
    /// false — продукт не опознан: признаков нет и спрашивать их нечем.
    pub recognised: bool,
}

/// Признак, который можно попросить у конвейера. Снаружи шаги называются так же,
/// как поля продукта, — чтобы вызывающий не знал о внутреннем порядке узлов.
#[derive(Clone, Copy, PartialEq)]
pub enum Aspect {
    VegFruit,
    Heme,
    MilkGlobule,
    RedMeat,
    ProcessedMeat,
    Egg,
}

impl Aspect {
    fn step(self) -> Step {
        match self {
            Aspect::VegFruit => Step::VegFruit,
            Aspect::Heme => Step::Heme,
            Aspect::MilkGlobule => Step::MilkGlobule,
            Aspect::RedMeat => Step::RedMeat,
            Aspect::ProcessedMeat => Step::ProcessedMeat,
            Aspect::Egg => Step::Egg,
        }
    }

    /// Имя для следа попыток — устойчивое, менять нельзя.
    pub fn slug(self) -> &'static str {
        match self {
            Aspect::VegFruit => "veg_fruit",
            Aspect::Heme => "heme",
            Aspect::MilkGlobule => "milk_globule",
            Aspect::RedMeat => "red_meat",
            Aspect::ProcessedMeat => "processed_meat",
            Aspect::Egg => "egg",
        }
    }
}

/// Спросить перечисленные признаки. `wanted` — только те, которых не хватает и
/// которые сегодня ещё не спрашивали: остальные не трогаем, чтобы готовый вердикт не
/// переписывался новым, который может с ним и разойтись.
pub async fn classify_all(food_name: &str, wanted: &[Aspect]) -> Result<Recognised, String> {
    classify_all_with_identity(food_name, wanted, None).await
}

/// То же, но с УЖЕ ИЗВЕСТНЫМ опознанием: `(что это за еда, вес опознания)`.
///
/// Опознание — самый дорогой вопрос прохода: его задают модели покрупнее, и он
/// повторяется для одного и того же продукта каждый раз, когда не хватает
/// очередного признака. Между этими разами продукт не меняется — меняется только
/// то, чего мы о нём ещё не спросили. Поэтому уверенное опознание кладётся в
/// `food_probe` и отсюда подставляется готовым: конвейер начинается сразу с
/// признаков, узел опознания не запускается вовсе.
pub async fn classify_all_with_identity(
    food_name: &str,
    wanted: &[Aspect],
    known: Option<(String, f64)>,
) -> Result<Recognised, String> {
    // Thinking ON. В остальных запросах он выключен: qwen3 паркует короткий ответ в
    // канал размышления и отдаёт пустой контент. Здесь пробуем включить, потому что
    // узлам достались настоящие противоречия — «язык это мышца, но орган», «копчёная
    // рыба консервирована, но не мясо», — и их надо продумать, а не угадать. Лимит
    // токенов в режиме рассуждения вчетверо больше (8000), так что ответу есть место.
    let flags_executor = build_executor_think(true)?;

    let mut ctx = FlagsCtx::new(food_name);
    // Порядок узлов сохраняем свой, а не тот, в котором попросили: у шагов он выверен
    // замерами (мясные идут после гема).
    ctx.wanted = [
        Step::VegFruit, Step::Heme, Step::RedMeat,
        Step::ProcessedMeat, Step::MilkGlobule, Step::Egg,
    ]
    .into_iter()
    .filter(|s| wanted.iter().any(|a| a.step() == *s))
    .collect();

    // С готовым опознанием конвейер начинается с ПЕРВОГО ЗАПРОШЕННОГО признака.
    // Гейт `recognised()` ниже смотрит на вес, поэтому кэш кладёт и его.
    let pipeline = match &known {
        Some((identity, weight)) => {
            let Some(first) = ctx.wanted.first().copied() else {
                return Err("опознание есть, а спрашивать нечего".to_string());
            };
            ctx.identity = Some(identity.clone());
            ctx.identity_confidence = *weight;
            // Штрафу неоткуда взяться: этот вес уже прошёл через него, когда
            // опознание получали живьём.
            ctx.identity_no_food_named = false;
            ctx.identity_dictionary_empty = false;
            ctx.identity_food_word = true;
            Pipeline::new(Box::new(NodeWrapper::new(FlagNode {
                executor: flags_executor.clone(),
                step: first,
            })))
        }
        None => Pipeline::new(Box::new(NodeWrapper::new(IdentifyNode {
            executor: build_identity_executor()?,
            fallback: build_identity_fallback_executor(),
            flags_executor: flags_executor.clone(),
        }))),
    };

    let mut stream = run_pipeline(pipeline, ctx);
    let mut last: Option<FlagsCtx> = None;
    while let Some(ev) = stream.next().await {
        if let NodeEvent::Completed(ctx) = ev {
            last = Some(ctx);
        }
    }

    let ctx = last.ok_or_else(|| "конвейер признаков не дал результата".to_string())?;
    // ПРОДУКТ НЕ ОПОЗНАН — говорим об этом вслух и не отдаём ни одного признака.
    // Раньше низкая уверенность молча превращалась в факт: «Кыстыбый» с версией
    // «мясное блюдо» на 0.30 получал гем и красное мясо.
    if !ctx.recognised() {
        let what = ctx.identity.clone().unwrap_or_else(|| "(нет версий)".to_string());
        leptos::logging::log!(
            "НЕ ОПОЗНАНО «{food_name}»: вес {:.2} (уверенность {:.2}, еду не назвать: {}, \
             словарь пуст: {}) — {what}",
            ctx.identity_weight(),
            ctx.identity_confidence,
            ctx.identity_no_food_named,
            ctx.identity_dictionary_empty
        );
        crate::services::telemetry::report_detection(
            "identity.unknown",
            food_name,
            "unknown",
            &what,
            &[ctx.identity_confidence],
        );
        return Ok(Recognised {
            flags: Flags::default(),
            fibre_g: None,
            identity: None,
            identity_weight: ctx.identity_weight(),
            recognised: false,
        });
    }
    let f = ctx.flags;
    let fibre = ctx.fibre_g;
    let identity = ctx.identity.clone();
    let weight = ctx.identity_weight();
    let got = [f.veg_fruit, f.heme, f.red_meat, f.processed_meat, f.milk_globule, f.egg]
        .iter()
        .filter(|v| v.is_some())
        .count();
    if got == 0 {
        return Err(ctx
            .last_error
            .unwrap_or_else(|| "конвейер признаков не дал ни одного вердикта".to_string()));
    }
    leptos::logging::log!(
        "признаки «{food_name}»: {} — выяснено {got} из 6 ({})",
        ctx.identity.clone().unwrap_or_else(|| "(не опознано)".to_string()),
        ctx.reasons.join(" · ")
    );
    Ok(Recognised { flags: f, fibre_g: fibre, identity, identity_weight: weight, recognised: true })
}

/// ТОЛЬКО опознание, без признаков.
///
/// Признаки продукта могут быть уже собраны, а кальций, железо и жиры — ещё нет: так
/// бывает после миграции, стирающей одно и оставляющей другое, и после конфликта
/// синхронизации. Раньше в этом случае конвейер не запускался вовсе, и все трое шли к
/// модели с голым именем из дневника — тем самым, из-за которого «Голец» получал
/// 120 мг кальция как «жидкий молочный продукт вроде кефира».
pub async fn identify(food_name: &str) -> Result<Option<String>, String> {
    Ok(identify_weighed(food_name).await?.map(|(identity, _)| identity))
}

/// То же опознание, но с ВЕСОМ — по нему решается, класть ли ответ в кэш
/// (`food_probe::record_identity`). Без веса кэшировать нельзя: продукт, разобранный
/// впритык к порогу, запоминать не за чем.
pub async fn identify_weighed(food_name: &str) -> Result<Option<(String, f64)>, String> {
    let mut ctx = FlagsCtx::new(food_name);
    ctx.identify_only = true;
    let pipeline = Pipeline::new(Box::new(NodeWrapper::new(IdentifyNode {
        executor: build_identity_executor()?,
        fallback: build_identity_fallback_executor(),
        flags_executor: build_executor_think(true)?,
    })));

    let mut stream = run_pipeline(pipeline, ctx);
    let mut last: Option<FlagsCtx> = None;
    while let Some(ev) = stream.next().await {
        if let NodeEvent::Completed(ctx) = ev {
            last = Some(ctx);
        }
    }
    let ctx = last.ok_or_else(|| "опознание не дало результата".to_string())?;
    // Тот же порог, что и в полном конвейере: неуверенное опознание не лучше его
    // отсутствия — кальций, железо и жиры пусть работают по одному имени, чем по
    // выдуманному определению.
    if !ctx.recognised() {
        leptos::logging::log!(
            "НЕ ОПОЗНАНО «{food_name}»: лучшая версия {:.2}",
            ctx.identity_confidence
        );
        return Ok(None);
    }
    leptos::logging::log!(
        "опознание «{food_name}»: {}",
        ctx.identity.clone().unwrap_or_else(|| "(не опознано)".to_string())
    );
    let weight = ctx.identity_weight();
    Ok(ctx.identity.map(|i| (i, weight)))
}

// ── Выгрузка промптов для замеров ────────────────────────────────────────────
//
// ЗАМЕРЫ ОБЯЗАНЫ ГОНЯТЬ ТО ЖЕ, ЧТО РАБОТАЕТ. Пока скрипты держали рукописные копии
// промптов, копии расходились с кодом — и расхождение было не видно, потому что
// обе стороны «работали». Дважды это стоило дня: сперва в коде оказалось три поля
// опознания против одного в копии, потом выяснилось, что ai-worker вклеивает в
// промпт СХЕМУ, которой в копии не было вовсе, — и именно она пропускала выдуманные
// слова как еду.
//
// Отсюда выгружаются и текст, и схема — ровно те, что уходят в модель. Имя продукта
// и опознание подставлены маркерами, чтобы замер мог поставить свои.
#[cfg(test)]
mod prompt_dump {
    use super::*;

    /// Маркеры, которые замер заменяет своими значениями.
    const FOOD: &str = "{{FOOD}}";
    const IDENT: &str = "{{IDENTITY}}";

    fn ctx() -> FlagsCtx {
        let mut c = FlagsCtx::new(FOOD);
        c.identity = Some(IDENT.to_string());
        c
    }

    /// Все промпты конвейера признаков + их схемы, как JSON.
    pub fn dump() -> serde_json::Value {
        let c = ctx();
        let mut nodes = serde_json::Map::new();

        let identify = IdentifyPrompt { food_name: FOOD.to_string() };
        nodes.insert(
            "identify".to_string(),
            serde_json::json!({
                "prompt": identify.serialize(),
                "schema": schemars::schema_for!(IdentityAnswer),
            }),
        );

        for (name, step) in [
            ("plant", Step::VegFruit),
            ("heme", Step::Heme),
            ("red_meat", Step::RedMeat),
            ("processed_meat", Step::ProcessedMeat),
            ("milk_globule", Step::MilkGlobule),
            ("egg", Step::Egg),
        ] {
            let p = FlagPrompt {
                food_name: c.food_name.clone(),
                given: c.given(),
                step,
            };
            let schema = if step.as_plant() {
                schemars::schema_for!(PlantAnswer)
            } else if step.as_egg() {
                schemars::schema_for!(EggAnswer)
            } else if step.as_category() {
                schemars::schema_for!(CategoryAnswer)
            } else {
                schemars::schema_for!(FlagAnswer)
            };
            nodes.insert(
                name.to_string(),
                serde_json::json!({ "prompt": p.serialize(), "schema": schema }),
            );
        }
        serde_json::Value::Object(nodes)
    }

    /// Пишет выгрузку в `scripts/prompts.json` — оттуда её читают замеры.
    /// Это не проверка, а генерация: тест ничего не утверждает, кроме того, что
    /// промпты вообще строятся.
    #[test]
    fn write_prompts_json() {
        let all = serde_json::json!({ "flags": dump(), "nutrients": crate::services::ai::prompt_dump() });
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/prompts.json");
        std::fs::write(path, serde_json::to_string_pretty(&all).expect("сериализация"))
            .expect("записать scripts/prompts.json");
        assert!(all["flags"]["identify"]["prompt"].as_str().unwrap().contains("{{FOOD}}"));
    }
}
