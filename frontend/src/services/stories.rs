//! The Stories engine: bundled, Instagram-style stories shown as a tray of
//! circles on the dashboard and opened into a fullscreen frame viewer.
//!
//! Content (text + which bundled image) is authored HERE as static data — on an
//! app update the whole set is overwritten. Each frame carries a content hash;
//! the set of SEEN hashes is persisted per-device in `app_flags`, so the tray
//! ring shows the fraction of a story's frames the user hasn't seen yet. When a
//! frame's content changes its hash changes, so the ring re-arms automatically.

use leptos::*;
use std::cell::RefCell;
use std::collections::HashSet;

use crate::services::app_flags;
use crate::services::i18n::{get_lang, Lang};

// --- Authoring model --------------------------------------------------------

/// A bilingual string literal.
#[derive(Clone, Copy)]
pub struct Loc {
    pub en: &'static str,
    pub ru: &'static str,
}
impl Loc {
    pub fn get(&self) -> &'static str {
        match get_lang() {
            Lang::En => self.en,
            Lang::Ru => self.ru,
        }
    }
}

/// A frame's background layer.
#[derive(Clone, Copy)]
pub enum Bg {
    /// Dark gradient backdrop (used behind the chart and screenshot cards).
    Dark,
    /// Тёмно-мясной градиент — фон недели красного мяса. Тот же рисунок, что у
    /// `Dark`, но в бордово-винных тонах: тема узнаётся до того, как прочитан
    /// первый кадр, и не путается с предыдущими главами.
    Meat,
    /// Full-bleed photo — asset path served under `/story-img/`.
    Photo(&'static str),
}

/// A frame's foreground media.
#[derive(Clone, Copy)]
pub enum Media {
    None,
    /// An app screenshot shown as a centred rounded card (`/story-img/…`).
    Shot(&'static str),
    /// Like `Shot`, but the image is nudged up by N% of its own height, so a
    /// GIF whose highlight sits lower in the frame is panned up to a shared
    /// focal point across a run of frames (same widget, different framing).
    ShotUp(&'static str, u8),
    /// Узкая ПОЛОСА снимка (подсветка и её ближайшее окружение), а не карточка
    /// целиком. Для кадров с длинным текстом: высокая карточка в оставшееся над
    /// текстом место влезает только нечитаемо мелкой, а не уменьшенная — уходит
    /// под текст. Полоса коротка и широка, поэтому ставится выше и целиком в
    /// свободном месте, а подсветки оказываются примерно в его середине.
    ShotBand(&'static str),
    /// A full-bleed photo anchored to the TOP of the frame (no rounded corners,
    /// scaled slightly past the screen edges). The text sits at the bottom on a
    /// gradient whose translucency begins at the kicker line. Used for editorial
    /// "topic photo" frames (e.g. dairy sources).
    Cover(&'static str),
    /// The bundled weight-trend SVG chart.
    Chart,
    /// A large centred emoji (e.g. a celebration).
    Emoji(&'static str),
    /// A small lightly-tinted panel listing benefit bullets, set in the Unbounded
    /// display font (bilingual per item). Used by the «benefit of walking» frame.
    Bonuses(&'static [Loc]),
}

/// One story frame: a background, optional media, and the text overlay.
#[derive(Clone, Copy)]
pub struct Frame {
    pub bg: Bg,
    pub media: Media,
    /// Kicker (eyebrow) colour, e.g. accent green or warning amber.
    pub accent: &'static str,
    pub kicker: Loc,
    pub title: Loc,
    pub body: Loc,
}

impl Frame {
    /// Stable content hash. Changes iff the frame's text or media change, so
    /// replacing a frame re-arms the tray ring for everyone who'd seen the old one.
    pub fn hash(&self) -> String {
        let mut s = String::with_capacity(256);
        s.push_str(self.kicker.ru);
        s.push('|');
        s.push_str(self.title.ru);
        s.push('|');
        s.push_str(self.body.ru);
        s.push('|');
        match self.bg {
            Bg::Dark => s.push_str("dark"),
            Bg::Meat => s.push_str("meat"),
            Bg::Photo(p) => {
                s.push_str("photo:");
                s.push_str(p);
            }
        }
        s.push('|');
        match self.media {
            Media::None => s.push_str("none"),
            Media::Shot(p) => {
                s.push_str("shot:");
                s.push_str(p);
            }
            Media::ShotUp(p, up) => {
                s.push_str("shotup:");
                s.push_str(p);
                s.push(':');
                s.push_str(&up.to_string());
            }
            Media::ShotBand(p) => {
                s.push_str("shotband:");
                s.push_str(p);
            }
            Media::Cover(p) => {
                s.push_str("cover:");
                s.push_str(p);
            }
            Media::Chart => s.push_str("chart"),
            Media::Emoji(e) => {
                s.push_str("emoji:");
                s.push_str(e);
            }
            Media::Bonuses(items) => {
                s.push_str("bonuses:");
                for it in items {
                    s.push_str(it.ru);
                    s.push(';');
                }
            }
        }
        format!("{:016x}", fnv1a(&s))
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// When a story becomes visible in the tray.
#[derive(Clone, Copy, PartialEq)]
pub enum Appears {
    /// Visible from the very first launch.
    Always,
    /// Visible once the first weekly calorie planka has been calculated.
    AfterCaloriePlanka,
    /// Visible once the activity week (step planka) has been unlocked.
    AfterActivityWeek,
    /// Visible once the calcium week (calcium goal + indicator) has been unlocked.
    AfterCalciumWeek,
    /// Visible once the iron week (weekly iron gauge + indicator) has been unlocked.
    AfterIronWeek,
    /// Видна, когда открылись ЖИРЫ — то есть после закрытой недельной планки железа.
    AfterFatWeek,
    /// Видна, когда открылась неделя КРАСНОГО МЯСА — после закрытой недели жиров.
    AfterRedMeatWeek,
}

pub struct Story {
    pub id: &'static str,
    pub appears: Appears,
    /// The glyph shown in the tray circle (story 1 reads as the numeral "1").
    pub badge: Loc,
    pub frames: &'static [Frame],
}

// --- Viewed-state (per-device, persisted in app_flags) ----------------------

const VIEWED_KEY: &str = "story_viewed";

struct ViewedState {
    set: HashSet<String>,
    /// Bumped whenever a hash is marked seen, so the tray re-computes rings.
    ver: RwSignal<u32>,
}

thread_local! {
    static VIEWED: RefCell<Option<ViewedState>> = const { RefCell::new(None) };
    /// The currently-open story in the fullscreen viewer. Lives in the root scope
    /// (not in the tray component) so it survives dashboard re-renders — otherwise
    /// a tapped story closes the moment the dashboard re-renders.
    static OPEN: RefCell<Option<RwSignal<Option<&'static Story>>>> = const { RefCell::new(None) };
    /// Reactive: true until the welcome story has been opened. Drives the tray
    /// circle's attention jiggle.
    static WELCOME_PENDING_SIG: RefCell<Option<RwSignal<bool>>> = const { RefCell::new(None) };
}

/// Seed the seen-set from `app_flags` and create the reactive signals.
/// Call once from `main()` inside the Leptos runtime.
pub fn init() {
    let set: HashSet<String> = app_flags::get(VIEWED_KEY)
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let ver = create_rw_signal(0u32);
    VIEWED.with(|v| *v.borrow_mut() = Some(ViewedState { set, ver }));
    OPEN.with(|o| *o.borrow_mut() = Some(create_rw_signal(None)));
    let pending = !app_flags::get_bool(WELCOME_KEY);
    WELCOME_PENDING_SIG.with(|s| *s.borrow_mut() = Some(create_rw_signal(pending)));
}

/// Re-seed the seen-set from `app_flags` after SYNC applied a remote change.
/// [`init`] seeds it once at launch — before the first sync of the session — so
/// without this the tray keeps drawing the pre-sync state until the next launch
/// (progress made on another device shows a launch late). Keeps the existing
/// signals (they belong to the root owner) and bumps the version so the tray
/// redraws; a no-op when nothing actually changed.
pub fn reseed() {
    let set: HashSet<String> = app_flags::get(VIEWED_KEY)
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let changed = VIEWED.with(|v| {
        let mut b = v.borrow_mut();
        match b.as_mut() {
            Some(st) if st.set != set => {
                st.set = set;
                true
            }
            _ => false,
        }
    });
    if changed {
        version().update(|v| *v += 1);
    }
    let pending = !app_flags::get_bool(WELCOME_KEY);
    if let Some(sig) = WELCOME_PENDING_SIG.with(|s| *s.borrow()) {
        if sig.get_untracked() != pending {
            sig.set(pending);
        }
    }
}

/// The root-scope signal holding the story currently open in the viewer.
pub fn open_signal() -> RwSignal<Option<&'static Story>> {
    OPEN.with(|o| *o.borrow()).expect("stories::init() must run first")
}

fn version() -> RwSignal<u32> {
    VIEWED.with(|v| v.borrow().as_ref().expect("stories::init() must run first").ver)
}

fn is_viewed(hash: &str) -> bool {
    VIEWED.with(|v| v.borrow().as_ref().is_some_and(|s| s.set.contains(hash)))
}

/// Record a frame's hash as seen (idempotent). Persists the set and bumps the
/// reactive version so the tray ring updates live.
pub fn mark_viewed(hash: &str) {
    let snapshot = VIEWED.with(|v| {
        let mut b = v.borrow_mut();
        let st = b.as_mut().expect("stories::init() must run first");
        if st.set.insert(hash.to_string()) {
            Some(st.set.iter().cloned().collect::<Vec<_>>())
        } else {
            None
        }
    });
    if let Some(list) = snapshot {
        if let Ok(json) = serde_json::to_string(&list) {
            app_flags::set(VIEWED_KEY, &json);
        }
        version().update(|v| *v += 1);
    }
}

/// How many of a story's frames the user hasn't seen. Subscribes to the version
/// signal, so callers re-render when frames are viewed.
pub fn unviewed_count(story: &Story) -> usize {
    version().track();
    story.frames.iter().filter(|f| !is_viewed(&f.hash())).count()
}

/// The stories currently eligible to show, in order. `planka_set` = the weekly
/// calorie planka has been calculated (gates the second-week story).
pub fn visible(
    planka_set: bool,
    activity_unlocked: bool,
    calcium_unlocked: bool,
    iron_unlocked: bool,
    fat_unlocked: bool,
    red_meat_unlocked: bool,
) -> Vec<&'static Story> {
    STORIES
        .iter()
        .filter(|s| match s.appears {
            Appears::Always => true,
            Appears::AfterCaloriePlanka => planka_set,
            Appears::AfterActivityWeek => activity_unlocked,
            Appears::AfterCalciumWeek => calcium_unlocked,
            Appears::AfterIronWeek => iron_unlocked,
            Appears::AfterFatWeek => fat_unlocked,
            Appears::AfterRedMeatWeek => red_meat_unlocked,
        })
        .collect()
}

pub fn by_id(id: &str) -> Option<&'static Story> {
    STORIES.iter().find(|s| s.id == id)
}

/// Every bundled image referenced by any story frame (`/story-img/…`).
fn all_image_paths() -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for story in STORIES {
        for f in story.frames {
            match f.media {
                Media::Shot(p) | Media::ShotUp(p, _) | Media::ShotBand(p) | Media::Cover(p) => {
                    set.insert(format!("/story-img/{p}"));
                }
                Media::Chart => {
                    set.insert("/story-img/weight-chart.svg".to_string());
                }
                Media::None | Media::Emoji(_) | Media::Bonuses(_) => {}
            }
            if let Bg::Photo(p) = f.bg {
                set.insert(format!("/story-img/{p}"));
            }
        }
    }
    set.into_iter().collect()
}

/// Warm the cache for every story image so the FIRST story open shows them
/// instantly instead of fetching each on demand (the "loads from outside" flash).
/// Fire-and-forget: fetches each same-origin asset in the background; the service
/// worker caches every response cache-first thereafter. Idempotent and cheap —
/// call once after launch, off the critical path.
pub fn prefetch_images() {
    let paths = all_image_paths();
    wasm_bindgen_futures::spawn_local(async move {
        for p in paths {
            let opts = web_sys::RequestInit::new();
            opts.set_method("GET");
            let Ok(req) = web_sys::Request::new_with_str_and_init(&p, &opts) else { continue };
            let Some(window) = web_sys::window() else { break };
            // Await each so we don't fire a burst of parallel requests at launch;
            // errors (offline) are ignored — the on-demand fetch remains the fallback.
            let _ = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&req)).await;
        }
    });
}

// --- Welcome story auto-open (once, on first launch) ------------------------

const WELCOME_KEY: &str = "welcome_shown";

/// Reactive: true until the user has opened the welcome story on this device.
/// The tray circle jiggles while this holds.
pub fn welcome_pending() -> bool {
    WELCOME_PENDING_SIG
        .with(|s| *s.borrow())
        .map(|sig| sig.get())
        .unwrap_or(false)
}

/// Record that the welcome story has been opened (stops the jiggle, persists).
pub fn mark_welcome_shown() {
    app_flags::set(WELCOME_KEY, "true");
    if let Some(sig) = WELCOME_PENDING_SIG.with(|s| *s.borrow()) {
        sig.set(false);
    }
}

// --- Authored content -------------------------------------------------------

const GREEN: &str = "#34d399";
const AMBER: &str = "#f0b968";

const S1: &[Frame] = &[
    // 1 — intro, weight chart
    Frame {
        bg: Bg::Dark,
        media: Media::Chart,
        accent: GREEN,
        kicker: Loc { en: "Week of discipline", ru: "Неделя дисциплины" },
        title: Loc { en: "The first week is about the habit", ru: "Первая неделя — про привычку" },
        body: Loc {
            en: "Log your weight, steps and food every day — that's all you need. The app does the rest.",
            ru: "Каждый день вносите вес, шаги и еду — это всё, что от вас нужно. Остальное приложение посчитает само.",
        },
    },
    // 2 — weight: tap widget (animated hint highlights the weight widget)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("dashboard-weight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Weight", ru: "Вес" },
        title: Loc { en: "Start with weight", ru: "Начинаем с веса" },
        body: Loc {
            en: "Weigh in every day, in the morning. On the home screen tap the «Weight» widget.",
            ru: "Взвешивайтесь каждый день с утра. На главном экране нажмите на виджет «Вес».",
        },
    },
    // 3 — weight: the widget expands, with the «Взвеситься» button
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("weight-expand.png"),
        accent: GREEN,
        kicker: Loc { en: "Weight", ru: "Вес" },
        title: Loc { en: "The widget opens up", ru: "Виджет раскроется" },
        body: Loc {
            en: "You'll see your chart and history. Tap «Weigh in» to log a new weight.",
            ru: "Откроется график и история. Нажмите «Взвеситься», чтобы внести новый вес.",
        },
    },
    // 4 — weight: form
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("weight-form.png"),
        accent: GREEN,
        kicker: Loc { en: "Weight", ru: "Вес" },
        title: Loc { en: "The weigh-in form", ru: "Форма взвешивания" },
        body: Loc {
            en: "Enter your weight. And try to tick the checkboxes too, honestly matching reality.",
            ru: "Введите свой вес. И старайтесь делать так, чтобы галочки тоже были проставлены и соответствовали действительности.",
        },
    },
    // 5 — weight: save (checkboxes ticked)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("weight-form-checked.png"),
        accent: GREEN,
        kicker: Loc { en: "Weight", ru: "Вес" },
        title: Loc { en: "Save it", ru: "Сохраните" },
        body: Loc {
            en: "Press «Save». Well done — do this every morning.",
            ru: "Нажмите «Сохранить». Вы молодец — так и делайте каждое утро.",
        },
    },
    // 6 — steps: tap widget (animated hint highlights the empty steps widget)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("dashboard-steps.gif"),
        accent: GREEN,
        kicker: Loc { en: "Steps", ru: "Шаги" },
        title: Loc { en: "Now steps", ru: "Теперь шаги" },
        body: Loc {
            en: "On the home screen tap the «Steps» widget.",
            ru: "На главном экране нажмите на виджет «Шаги».",
        },
    },
    // 7 — steps: form
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("steps-form.png"),
        accent: GREEN,
        kicker: Loc { en: "Steps", ru: "Шаги" },
        title: Loc { en: "Enter your steps", ru: "Внесите шаги" },
        body: Loc {
            en: "Log your steps in the evening before bed, or in the morning for the previous day.",
            ru: "Шаги можно внести вечером перед сном или утром, за вчерашний день.",
        },
    },
    // 8 — steps: save
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("steps-form.png"),
        accent: GREEN,
        kicker: Loc { en: "Steps", ru: "Шаги" },
        title: Loc { en: "Save it", ru: "Сохраните" },
        body: Loc {
            en: "Press «Save». Steps need logging every day too. Activity really matters!",
            ru: "Нажмите «Сохранить». Шаги тоже надо записывать каждый день. Активность — это очень важно!",
        },
    },
    // 9 — food: open the diary (highlight the «Дневник» nav button)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("diary-nav.gif"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "Open the diary", ru: "Откройте дневник" },
        body: Loc {
            en: "Tap «Diary» in the bottom bar.",
            ru: "Внизу нажмите «Дневник».",
        },
    },
    // 10 — food: add an entry (highlight a meal panel's «+»)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("diary-plus.gif"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "Add an entry", ru: "Добавьте запись" },
        body: Loc {
            en: "Tap «+» on a meal — breakfast, lunch or dinner (or its title).",
            ru: "Нажмите «+» на приёме — завтрак, обед или ужин (или на его названии).",
        },
    },
    // 11 — food: new product (highlight «Добавить новый продукт»)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("diary-addnew.gif"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "A new product", ru: "Новый продукт" },
        body: Loc {
            en: "Nothing found yet — tap «Add a new product».",
            ru: "Пока ничего нет — нажмите «Добавить новый продукт».",
        },
    },
    // 12 — food by description
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-by-name-card.png"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "Food by description", ru: "Еда по описанию" },
        body: Loc {
            en: "Describe the dish in words — the app fills in the calories & macros. Check it and press «Add».",
            ru: "Опишите блюдо словами — приложение подставит КБЖУ. Проверьте и нажмите «Добавить».",
        },
    },
    // 13 — food by label
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-by-photo.png"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "By the label", ru: "По этикетке" },
        body: Loc {
            en: "Shoot the nutrition table up close — the numbers fill in themselves.",
            ru: "Снимите таблицу КБЖУ крупно — цифры заполнятся сами.",
        },
    },
    // 14 — food by dish photo
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("foodphoto-top.png"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "By a photo of the dish", ru: "По фото еды" },
        body: Loc {
            en: "Photograph the whole dish — it's broken down into products.",
            ru: "Сфотографируйте блюдо целиком — оно разберётся на продукты.",
        },
    },
    // 15 — warning: dish photo is a draft (right after «по фото еды»)
    Frame {
        bg: Bg::Photo("dish-bowl.jpeg"),
        media: Media::None,
        accent: AMBER,
        kicker: Loc { en: "Important", ru: "Важно" },
        title: Loc { en: "A dish photo is a draft", ru: "Фото тарелки — черновик" },
        body: Loc {
            en: "Photo recognition can be wrong about the contents and grams — always check the numbers. Description and label are more accurate.",
            ru: "Распознавание по фото может ошибиться в составе и граммах — всегда проверяйте цифры. Описание и этикетка точнее.",
        },
    },
    // 16 — repeat: copy from yesterday (shows the ⇄ repeat icon)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-duplicate-popup.png"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "Copy from yesterday", ru: "Копируем из вчера" },
        body: Loc {
            en: "Eating the same thing? Open the diary, swipe to «Yesterday» and tap the ⇄ icon on the entry — «Repeat today».",
            ru: "Едите одно и то же? Откройте дневник, перелистните на «Вчера» и нажмите у записи иконку ⇄ «Повторить сегодня».",
        },
    },
    // 17 — repeat: duplicate today (via the «⋮» menu)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("diary-duplicate.gif"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "Duplicate for today", ru: "Дублируем сегодня" },
        body: Loc {
            en: "For today's food, open the «⋮» menu on the entry and choose «Duplicate».",
            ru: "Съеденное сегодня — откройте меню «⋮» у записи и выберите «Дублировать».",
        },
    },
    // 18 — food search
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("food-search.gif"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "Food search", ru: "Поиск по еде" },
        body: Loc {
            en: "Already logged this product? Start typing its name — say «Ap» — and pick it from the list.",
            ru: "Уже вносили этот продукт? Начните вводить название — например «Яб» — и выберите из списка.",
        },
    },
    // 19 — always log caloric drinks
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("drinks-collage.png"),
        accent: AMBER,
        kicker: Loc { en: "Important", ru: "Важно" },
        title: Loc { en: "Log caloric drinks", ru: "Записывайте напитки" },
        body: Loc {
            en: "Always log caloric drinks. Juice, sugary soda, or sugar in your tea or coffee — that's a real amount of calories, and it has to be counted.",
            ru: "Обязательно записывайте калорийные напитки. Если пьёте сок, сладкую газировку с сахаром или добавляете сахар в чай или кофе — это существенное количество калорий, его нужно учитывать.",
        },
    },
    // 20 — always log oils
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("oils-collage.png"),
        accent: AMBER,
        kicker: Loc { en: "Important", ru: "Важно" },
        title: Loc { en: "Log the oils", ru: "Записывайте масла" },
        body: Loc {
            en: "Always log the oils: olive, sunflower, butter. They're packed with calories — skip them and in a week your planka will be a very hungry one.",
            ru: "Обязательно записываем масла: оливковое, подсолнечное, сливочное. В них очень много калорий; если их не записывать, то через неделю у вас будет очень голодная планка.",
        },
    },
];

// The welcome / dashboard tour. Auto-opens once on first launch and stays in the
// tray for re-watching.
const WELCOME: &[Frame] = &[
    // 1 — hello
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-intro.png"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Hello!", ru: "Привет!" },
        body: Loc {
            en: "This is re:Norma — a weight-loss app.",
            ru: "Это re:Norma. Приложение по похудению.",
        },
    },
    // 2 — persona
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-persona.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Your details", ru: "Ваши данные" },
        body: Loc {
            en: "Set your personal details here — height, weight, age — and what you want to achieve: lose, gain or maintain.",
            ru: "Вот здесь настройте свои персональные данные — рост, вес, возраст — и чего вы хотите достичь: похудеть, набрать или сохранить.",
        },
    },
    // 3 — notifications
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-bell.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Notifications", ru: "Уведомления" },
        body: Loc {
            en: "Set up notifications here — so the app can remind you to log something, or tell you it's been updated.",
            ru: "Вот здесь настройте уведомления — чтобы приложение могло напомнить внести данные или сообщить, что программа обновилась.",
        },
    },
    // 4 — the errors / warning tile
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-errors.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Problems", ru: "Проблемы" },
        body: Loc {
            en: "If anything goes wrong, you'll be able to see it here.",
            ru: "Если какие-то проблемы произойдут, здесь их можно будет посмотреть.",
        },
    },
    // 5 — settings / language
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-settings.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Settings", ru: "Настройки" },
        body: Loc {
            en: "Here you can set the language. App updates show up here too.",
            ru: "Вот здесь вы можете настроить язык. Также там будут обновления.",
        },
    },
    // 6 — support
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-support.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "Support", ru: "Поддержка" },
        body: Loc {
            en: "And here's the support chat. You'll always get an answer — though you may have to wait.",
            ru: "А вот здесь чат поддержки. Вам обязательно ответят, но, может быть, придётся подождать.",
        },
    },
    // 7 — the main thing
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("welcome-main.gif"),
        accent: GREEN,
        kicker: Loc { en: "re:Norma", ru: "re:Norma" },
        title: Loc { en: "The main thing", ru: "Самое главное" },
        body: Loc {
            en: "And here's what matters most: your weight, activity, and your food-diary entries.",
            ru: "А вот здесь всё самое главное: ваш вес, активность, а также записи вашего дневника питания.",
        },
    },
];

// The second-week story. Appears once the first weekly calorie planka has been
// calculated. Product frames (protein / veg-fruit / oils / drinks) are plain text
// on the dark card for now; real product photos can be dropped in later.
const S2: &[Frame] = &[
    // 1 — first week done, planka calculated (celebration)
    Frame {
        bg: Bg::Dark,
        media: Media::Emoji("🎉"),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "The first week is behind you", ru: "Первая неделя позади" },
        body: Loc {
            en: "Congratulations — the first week is done, we have your first data, and your first calorie planka is calculated.",
            ru: "Поздравляем — первая неделя прошла, у нас появились первые данные, и ваша первая планка по калориям посчитана.",
        },
    },
    // 2 — the calorie planka (highlighted on the widget) + weekly recalculation
    Frame {
        bg: Bg::Dark,
        media: Media::ShotUp("dashboard-planka-cal.gif", 0),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Your calorie planka", ru: "Планка по калориям" },
        body: Loc {
            en: "From now on, try not to eat above this planka — the indicator shows how many calories you have left. We recalculate and adjust it every week.",
            ru: "Отныне старайтесь не превышать калорийность выше этой планки — индикатор показывает, сколько калорий вам ещё осталось. Мы пересчитываем и корректируем её каждую неделю.",
        },
    },
    // 3 — not only calories: protein + veg/fruit plankas (highlighted)
    Frame {
        bg: Bg::Dark,
        media: Media::ShotUp("dashboard-planka-macros.gif", 14),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Not only calories", ru: "Не только калории" },
        body: Loc {
            en: "Besides the calorie planka, we also give you a protein planka and a vegetables-and-fruit planka.",
            ru: "Кроме планки по калориям, мы также выдаём планку по белку и планку по овощам и фруктам.",
        },
    },
    // 4 — the indicators
    Frame {
        bg: Bg::Dark,
        media: Media::ShotUp("dashboard-indicators.gif", 44),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Indicators", ru: "Индикаторы" },
        body: Loc {
            en: "You now have indicators — they show how well you're keeping to your plankas. There are just two for now, but there will be more. They help you see how healthy your diet is.",
            ru: "У вас появились индикаторы — они показывают, как хорошо вы придерживаетесь ваших целей на планке. Пока их здесь только два, но будет больше. С их помощью вы будете понимать, насколько здоров ваш рацион.",
        },
    },
    // 5 — protein → satiety
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("protein-collage.png"),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Eat more protein", ru: "Ешьте больше белка" },
        body: Loc {
            en: "Protein is very filling. The more protein you eat, the less hungry you are. Use it as a tool to control hunger.",
            ru: "Белок даёт очень хорошее насыщение. Чем больше белка вы едите, тем меньше ваш голод. Используйте этот инструмент для контроля голода.",
        },
    },
    // 6 — veg/fruit → volume, low calories
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("veg-collage.png"),
        accent: GREEN,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Eat plenty of vegetables and fruit", ru: "Ешьте много овощей и фруктов" },
        body: Loc {
            en: "Vegetables and fruit are low in calories and full of water, so they satisfy hunger too. The more of them, the easier it is to fill your stomach.",
            ru: "Овощи и фрукты обладают низкой калорийностью и содержат много воды, поэтому тоже хорошо утоляют голод. Чем больше фруктов и овощей, тем легче наполнить желудок.",
        },
    },
    // 7 — go easy on oils
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("oils-collage.png"),
        accent: AMBER,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Use less fat", ru: "Используйте меньше жира" },
        body: Loc {
            en: "Still not fitting your planka? Use less oil — butter, vegetable oil, mayonnaise are very high-calorie. Try to limit them.",
            ru: "Если всё равно не влезаете в планку — используйте меньше масла: сливочное, растительное, майонез очень калорийны. Постарайтесь их ограничивать.",
        },
    },
    // 8 — caloric drinks leave you hungry
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("drinks-collage.png"),
        accent: AMBER,
        kicker: Loc { en: "Week 2", ru: "Вторая неделя" },
        title: Loc { en: "Don't drink caloric drinks", ru: "Не пейте калорийные напитки" },
        body: Loc {
            en: "Caloric drinks — juice, sugary cola, beer — can leave you hungry, because the calories run out very fast.",
            ru: "Калорийные напитки — соки, кола с сахаром, пиво — могут оставить вас голодными, потому что калории заканчиваются очень быстро.",
        },
    },
];

// --- Story 3 «Неделя активности» — unlocked once the week-2 gate is cleared and
// the step planka is set (Appears::AfterActivityWeek). ------------------------
/// Health benefits of regular walking, listed on the «Польза ходьбы» frame.
const WALK_BONUSES: &[Loc] = &[
    Loc { en: "Eases anxiety", ru: "Помогает от тревожности" },
    Loc { en: "Reduces depression symptoms", ru: "Снижает симптомы депрессии" },
    Loc { en: "Cuts mortality by 47%", ru: "Снижает смертность на 47%" },
    Loc { en: "Guards against dementia", ru: "Защищает от деменции" },
    Loc { en: "Guards against heart disease", ru: "Защищает от сердечно-сосудистых" },
    Loc { en: "Lowers cancer risk", ru: "Снижает риск онкологии" },
];

const S3: &[Frame] = &[
    // 1 — congrats + intro
    Frame {
        bg: Bg::Dark,
        media: Media::Emoji("🎉"),
        accent: GREEN,
        kicker: Loc { en: "Activity", ru: "Активность" },
        // No big title — the message is the body, with the phrase emphasized inline
        // (`~…~` = bold gradient, same size as the rest of the text).
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "You've been in the program for two weeks now, and you've kept your indicators green for a whole week. So we begin the next week: ~the activity week~.",
            ru: "Вы уже две недели в программе и целую неделю держите индикаторы зелёными. Поэтому мы начинаем следующую неделю: ~неделю активности~.",
        },
    },
    // 2 — the step planka
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("walk-park.png"),
        accent: GREEN,
        kicker: Loc { en: "Activity", ru: "Активность" },
        title: Loc { en: "Your step planka", ru: "Планка по шагам" },
        body: Loc {
            en: "You now have a step planka. You need not only to log your steps, but also to walk no less than the set planka.",
            ru: "У вас теперь появляется планка по шагам. Вам необходимо не только записывать шаги, но ещё и ходить не меньше установленной планки.",
        },
    },
    // 3 — why walk at all (deficit / metabolism)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("couch-cola.png"),
        accent: GREEN,
        kicker: Loc { en: "Activity", ru: "Активность" },
        title: Loc { en: "Why walk at all?", ru: "Зачем вообще ходить?" },
        body: Loc {
            en: "When you start living in a calorie deficit, your body begins to slow down to get out of that deficit. To stay in it, you have to spend calories.",
            ru: "Если вы начинаете жить в дефиците калорий, ваш организм начинает замедляться так, чтобы из этого дефицита уйти. Чтобы не уходить из дефицита, необходимо тратить калории.",
        },
    },
    // 4 — separate health benefit of walking (with the benefits panel)
    Frame {
        bg: Bg::Dark,
        media: Media::Bonuses(WALK_BONUSES),
        accent: GREEN,
        kicker: Loc { en: "Activity", ru: "Активность" },
        title: Loc { en: "The benefit of walking", ru: "Польза ходьбы" },
        body: Loc {
            en: "Walking has a separate benefit of its own. Every study confirms the health benefit of walking more than 7000 steps a day.",
            ru: "У ходьбы есть отдельная польза. Все исследования подтверждают пользу для здоровья, если человек ходит более 7000 шагов в день.",
        },
    },
    // 5 — walking, running or the gym
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("walk-run-gym.png"),
        accent: GREEN,
        kicker: Loc { en: "Activity", ru: "Активность" },
        title: Loc { en: "Walking, running or CrossFit", ru: "Ходьба, бег или Crossfit" },
        body: Loc {
            en: "It depends on your athletic background. Already a runner — keep running. Doing CrossFit regularly — don't stop. But if you haven't done sport before — start with walking.",
            ru: "Это зависит от вашего спортивного опыта. Если вы уже бегаете — можно оставить бег. Если вы регулярно занимаетесь кроссфитом — не надо останавливаться. Но если вы до этого не занимались спортом — начните с ходьбы.",
        },
    },
];

// --- Story 4 «Неделя кальция» — unlocked once the activity (steps) gate is cleared
// and the calcium goal is set (Appears::AfterCalciumWeek).
//
// PLACEHOLDER CONTENT: the copy below is a draft skeleton following the user's
// outline (congrats → why calcium → dairy → plant sources → canned fish → the 1 g
// planka + gauge/indicator). Exact wording and real photos are still to be found —
// every image points at `calcium-placeholder.png` for now, and the text is marked
// with a leading «(черновик)» so it reads as unfinished in the viewer. -----------
const CALCIUM_PH: &str = "calcium-placeholder.svg";

const S4: &[Frame] = &[
    // 1 — congrats: steps task done, and the «strong bones» week begins. The phrase
    // is emphasised with the white→yellow gradient marker `^…^` (bones).
    Frame {
        bg: Bg::Dark,
        media: Media::Emoji("🎉"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "You've successfully completed the task of keeping your step planka green for 7 days. That's excellent.\n\n\
                 From now on we'll recalculate your step planka much like your calorie planka. Because movement is life. It's far easier to lose weight and stay in shape when you walk a lot. But now we go further with you.\n\n\
                 And a new week begins — the ^strong-bones^ week.",
            ru: "Вы успешно выполнили задание держать планку по шагам в течение 7 дней. Это очень хорошо.\n\n\
                 В дальнейшем мы будем пересчитывать вашу планку по шагам примерно так же, как и планку по калориям. Потому что движение — это жизнь. Намного легче худеть и держать вес в норме, когда вы много ходите. Но мы сейчас с вами пойдём дальше.\n\n\
                 И у нас начинается неделя ^«крепких костей»^.",
        },
    },
    // 2 — why calcium matters (why we start watching it)
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("calcium-bones.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Strong bones need calcium. You were most likely already getting too little of it before the diet, and on a diet most people cut their calcium intake even further. That puts your bones at risk.\n\n\
                 So we start keeping an eye on your calcium intake.",
            ru: "Для крепких костей необходим кальций. Скорее всего, и до диеты вы употребляли его недостаточно. А на диете большинство людей снижают потребление кальция ещё сильнее. Это несёт риски для ваших костей.\n\n\
                 Поэтому начинаем следить за потреблением кальция.",
        },
    },
    // 3 — calcium from dairy (incl. low-fat; lactose-intolerant options)
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("calcium-dairy.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Dairy, including low-fat: milk, yoghurt, cottage cheese, hard cheese — a simple and cheap source of calcium. Low-fat products have no less calcium than full-fat ones; calcium is absorbed regardless of the product's fat content.\n\n\
                 If you're lactose-intolerant, you can have aged cheeses or some fermented-milk products. They contain less lactose.",
            ru: "Молочные продукты, в том числе обезжиренные: молоко, йогурт, творог, твёрдый сыр — это простой и дешёвый источник кальция. В обезжиренных продуктах кальция не меньше, чем в жирных. Кальций усвоится независимо от жирности продукта.\n\n\
                 Если у вас непереносимость лактозы, вы можете употреблять выдержанные сыры или некоторое количество кисломолочных продуктов. Там лактозы меньше.",
        },
    },
    // 4 — calcium from plant sources
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("calcium-plants.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "If dairy doesn't suit you, there are good plant sources. For example: Chinese cabbage, arugula, tofu, various greens, fermented or heat-treated cabbage.",
            ru: "Если молочные продукты вам не подходят, есть хорошие растительные источники. Например: пекинская капуста, руккола, тофу, разнообразная зелень, ферментированная или термообработанная капуста.",
        },
    },
    // 5 — calcium from canned fish (eat the soft bones)
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("calcium-fish.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "You'll find an excellent source in canned fish — the kind where the fish are small and cooked so that you can eat the bones too. Eat the fish together with the bones. The calcium from their bones is now your calcium.",
            ru: "Отличный источник вы можете найти в рыбных консервах. Там, где рыбки маленькие и сварены так, что кости тоже можно есть. Ешьте рыбу вместе с костями. Кальций из их костей теперь ваш кальций.",
        },
    },
    // 6 — the new calcium indicator + gauge
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("calcium-highlight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Calcium", ru: "Кальций" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Now, to track calcium, you have a new indicator — it shows how well you're meeting your daily calcium planka.\n\n\
                 Plus a daily guide: how much more calcium you still need to eat.",
            ru: "Теперь для отслеживания кальция у вас появился новый индикатор, который покажет, как вы выполняете ежедневную планку по кальцию.\n\n\
                 Ну и ежедневный ориентир — сколько вам ещё нужно съесть кальция.",
        },
    },
];

// --- Story 5 «Неделя железа» — unlocked once the calcium gate is cleared
// (Appears::AfterIronWeek).
//
// The Russian copy is the user's own text, carried over verbatim (orthography and
// typography only).

const S5: &[Frame] = &[
    // 1 — congrats: the calcium week is done, the iron week begins.
    Frame {
        bg: Bg::Dark,
        media: Media::Emoji("🎉"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Congratulations, the calcium week is over. You kept the indicator green for a whole week. Keep holding that indicator green from here on.\n\n\
                 And we go further — the #iron# week begins.",
            ru: "Поздравляем, неделя кальция закончилась. Вы держали индикатор зелёным целую неделю. Продолжайте и дальше держать этот индикатор зелёным.\n\n\
                 А мы идём дальше и начинаем #неделю железа#.",
        },
    },
    // 2 — why iron matters
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-blood.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Iron is a very important element for our body. Our blood and our muscles need it most of all. When iron is low, we feel tired and weak.",
            ru: "Железо — это очень важный элемент для нашего организма. Больше всего он нужен нашей крови и мышцам. Если железа мало, мы чувствуем усталость и слабость.",
        },
    },
    // 3 — red meat
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-meat.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "One of the best sources of iron is red meat: beef, lamb, pork. It also carries an enormous amount of protein. Eating it regularly closes the requirement very quickly.",
            ru: "Один из лучших источников железа — это красное мясо: говядина, баранина, свинина. В них также есть огромное количество белка. Регулярное употребление очень быстро закрывает потребность.",
        },
    },
    // 4 — liver
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-liver.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Liver is also a very good source. Chicken liver, at that, is better than beef liver. This food is low in calories too. But you can't eat too much of it, because of the large amount of vitamin A.",
            ru: "Печень — это тоже очень хороший источник. Причём куриная печень лучше говяжьей. В этой еде ещё и мало калорий. Но слишком много есть нельзя из-за большого количества витамина А.",
        },
    },
    // 5 — shellfish and roe
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-seafood.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Mussels, oysters, red and black caviar — this is iron for the rich. If your wallet allows it, eat plenty. And with pleasure. Enjoy your meal.",
            ru: "Мидии, устрицы, красная и чёрная икра — это железо для богатых. Если вам позволяет кошелёк — ешьте много. И с удовольствием. Приятного аппетита.",
        },
    },
    // 6 — plant sources + the absorption gap (the reason iron carries a coefficient)
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-legumes.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "If your conscience won't let you eat animals, try closing your iron with legumes: beans, chickpeas, lentils. You can also try nuts, but they carry too many calories: sesame or cashew, for instance. Plant sources are not the best ones, and the app will account for #bioavailability#.",
            ru: "Если есть животных вам не позволяет совесть, попробуйте закрывать железо из бобовых: фасоли, нута, чечевицы. Можно ещё попробовать орехи, но в них слишком много калорий: например, кунжут или кешью. Растительные источники не самые лучшие, и программа будет учитывать #биологическую доступность#.",
        },
    },
    // 7 — the new WEEKLY indicator + gauge
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("iron-highlight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Your new iron indicator will be WEEKLY. Over the coming week, eat your iron norm. Your norm depends on your sex and your age.",
            ru: "Ваш новый индикатор по железу будет недельным. За ближайшую неделю съешьте норму вашего железа. Ваша норма будет зависеть от вашего пола и возраста.",
        },
    },
    // 8 — и второй индикатор: не только ради железа
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("heme-highlight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "And one more indicator: heme iron. We make you eat liver, meat and molluscs not only for the iron.",
            ru: "И ещё индикатор потребления гемового железа. Мы заставляем вас есть печень, мясо и моллюсков не только ради железа.",
        },
    },
    // 9 — calcium vs iron: the interaction you may ignore
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("iron-ca-vs-fe.jpg"),
        accent: AMBER,
        kicker: Loc { en: "Iron", ru: "Железо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Calcium and iron use the same transport. When we eat a lot of calcium, iron is absorbed worse. Most people need to do nothing and need not account for it. But if you do have trouble with iron, try splitting your meals like this: calcium in one meal, and iron in another.",
            ru: "Кальций и железо используют один и тот же транспорт. Когда едим много кальция, железо усваивается хуже. Большинству людей не нужно ничего делать и не нужно это учитывать. Но если у вас проблемы с железом, попробуйте разделять приёмы пищи так: кальций в один приём, а железо в другой.",
        },
    },
];

// --- Story 6 «Неделя жиров» — открывается, когда ЗАКРЫТА недельная планка железа
// (Appears::AfterFatWeek).
//
// Русский текст — авторский, перенесён дословно. Правлены только опечатки диктовки
// («бальшая», «состоят и жирных кислот», «Страиваются», «декатор») и раскрыта
// пометка «(перечислить те, что в рыбе)» — ЭПК и ДГК.

const S6: &[Frame] = &[
    // 1 — поздравление: железо позади, впереди жиры. Кадр-поздравление, как в
    // остальных историях, идёт с хлопушкой, а не с фотографией еды.
    Frame {
        bg: Bg::Dark,
        media: Media::Emoji("🎉"),
        accent: GREEN,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Congratulations, you have dealt with iron! That, I hope, was not too hard. \
                 Ahead lies a very big and important subject — the @fats@ week.",
            ru: "Поздравляем, вы справились с железом! Это, надеюсь, было не очень сложно. \
                 Впереди очень большая и важная тема — @неделя жиров@.",
        },
    },
    // 2 — из чего вообще состоят жиры.
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("fats-lard.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "There are a great many different fats, and living with them is rather hard. \
                 All fats are made of fatty acids. And those come as saturated, \
                 mono-unsaturated and poly-unsaturated.",
            ru: "Жиров очень много разных и с ними жить довольно сложно. Все жиры состоят из \
                 жирных кислот. А они бывают насыщенные, моно-ненасыщенные и \
                 поли-ненасыщенные.",
        },
    },
    // 3 — незаменимые: организм их не делает, взять можно только из рыбы.
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("fats-omega-foods.jpg"),
        accent: GREEN,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Besides that, there are special fatty acids — EPA and DHA — which we need and \
                 which our body does not make. They are also called @omega-3@. As it happens, \
                 we can get them only from fish.",
            ru: "Кроме того, есть особые жирные кислоты: ЭПК и ДГК, которые нужны, и которые наш \
                 организм не производит. Их ещё называют @омега-3@. Так вышло, что их мы можем \
                 получить только из рыбы.",
        },
    },
    // 4 — чем оборачивается дефицит.
    Frame {
        bg: Bg::Dark,
        media: Media::Cover("fats-heart.jpg"),
        accent: AMBER,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "A deficit of omega-3 raises mortality from cardiovascular and oncological \
                 disease. These acids are built into the membranes of our brain and of the \
                 retina, keeping them working. They regulate inflammation and clotting.",
            ru: "Дефицит омега-3 вызывает повышенную смертность из-за сердечно-сосудистых и \
                 онкологических заболеваний. Эти кислоты встраиваются в оболочку мембран нашего \
                 мозга и сетчатки глаза, обеспечивая их работу. Они регулируют воспаление и \
                 тромбы.",
        },
    },
    // 5 — беда не в жире как таковом, а в перекосе.
    Frame {
        bg: Bg::Dark,
        // Триптих: сало, орехи, скумбрия — три рода жирных кислот РЯДОМ. Разговор
        // здесь про баланс, а баланс виден только когда стороны показаны разом.
        media: Media::Cover("fats-triptych.jpg"),
        accent: AMBER,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "And the whole problem here is exactly the *imbalance*: when there are too many \
                 saturated fatty acids — from animal products such as beef, pork and lamb.",
            ru: "И вся проблема здесь именно в *перекосе*: когда слишком много насыщенных жирных \
                 кислот: из животных продуктов, таких как говядина, свинина, баранина.",
        },
    },
    // 6 — первый индикатор: омега-3 из рыбы.
    Frame {
        bg: Bg::Dark,
        media: Media::ShotBand("fats-omega-highlight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "That is why we give you two indicators. One of them is for omega-3 from fish — \
                 you need to close a minimum amount.\n\n\
                 Keep this indicator green for a whole week to discover the next story in this \
                 journey.",
            ru: "Поэтому мы даём вам два индикатора: один из них по омега-3 из рыбы — необходимо \
                 закрывать минимальное количество.\n\n\
                 Держите этот индикатор зелёным целую неделю, чтобы открыть следующую историю \
                 этого пути.",
        },
    },
    // 7 — второй индикатор: соотношение, то есть баланс.
    Frame {
        bg: Bg::Dark,
        media: Media::ShotBand("fats-ratio-highlight.gif"),
        accent: GREEN,
        kicker: Loc { en: "Fats", ru: "Жиры" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "The other indicator is the ratio of saturated to unsaturated fatty acids. It \
                 shows how well you are in balance. If this indicator is green, your risks of \
                 cardiovascular and oncological disease are minimal.\n\n\
                 If the bar has turned red, cut the animal and dairy fats. Or add oily fish.",
            ru: "Другой индикатор — это соотношение насыщенных к ненасыщенным жирным кислотам. \
                 Он показывает, насколько вы в балансе. Если этот индикатор зелёный, значит, ваши \
                 риски сердечно-сосудистых и онкологических заболеваний минимальны.\n\n\
                 Если полоска стала красной, надо убрать животные и молочные жиры. Или добавить \
                 жирной рыбы.",
        },
    },
    // Отдельного кадра «держите неделю» здесь НЕТ: голый текст на пустом фоне
    // ничего не показывал, а сказать это надо там, где виден сам индикатор, —
    // поэтому фраза стоит на кадре с омега-3, по которому и открывается следующая
    // глава.
];

// --- Story 7 «Неделя красного мяса» — открывается по ЗАКРЫТОЙ неделе жиров
// (Appears::AfterRedMeatWeek).
//
// Русский текст авторский, перенесён дословно; правлена только типографика —
// запятые, тире и «ё». Акцент амбровый, а не зелёный: это первая история про
// ограничение, и цвет отличает её от предыдущих, где речь шла о целях.

const S7: &[Frame] = &[
    // 1 — поздравление: жиры позади. Как и в остальных историях, с хлопушкой.
    Frame {
        bg: Bg::Meat,
        media: Media::Emoji("🎉"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Congratulations, you have dealt with fats — the biggest subject of them all. \
                 Ahead lies a shorter one, but no less important: the @red meat@ week.",
            ru: "Поздравляем, вы справились с жирами — это была самая большая тема. Впереди \
                 тема покороче, но не менее важная — @неделя красного мяса@.",
        },
    },
    // 2 — сама планка и почему она есть.
    Frame {
        bg: Bg::Meat,
        media: Media::Cover("meat-steaks.jpg"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Red meat is associated with cancer. Across the world the recommendation is to \
                 eat no more than 700 g a week. Above that figure the risks of cancer keep \
                 rising.",
            ru: "Красное мясо ассоциировано с онкологией. Во всём мире рекомендуют употреблять \
                 не более 700 г в неделю. Выше этой цифры риски рака постоянно возрастают.",
        },
    },
    // 3 — переработанное мясо: тот же разговор, но отдельный.
    Frame {
        bg: Bg::Meat,
        media: Media::Cover("meat-sausages.jpg"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Frankfurters, sausages and the other convenience foods we are used to buying \
                 at the shop are also linked to cancer risks.",
            ru: "Сосиски, колбасы и прочие полуфабрикаты, которые мы привыкли покупать в \
                 магазине, также связаны с онкологическими рисками.",
        },
    },
    // 4 — почему мы ничего не запрещаем.
    Frame {
        bg: Bg::Meat,
        media: Media::Emoji("🎂"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "Banning frankfurters and sausages is the wrong thing to do. We are all human, \
                 we like eating them. We like them on holidays. We know that medicine differs \
                 from poison by the dose.",
            ru: "Запрещать сосиски, колбасы — это неправильно. Все мы люди, мы любим это есть. \
                 Мы любим это на праздники. Мы знаем, что лекарство от яда отличается дозой.",
        },
    },
    // 5 — что мы делаем вместо запрета. Кадр про сам индикатор, поэтому здесь не
    // метафора, а снимок дашборда: шкала недели и ряд значков, по которым человек
    // и будет судить.
    Frame {
        bg: Bg::Meat,
        media: Media::Shot("red-meat-highlight.gif"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "So we simply give you an indicator. It will honestly show you that you are \
                 taking a risk. Your task is to learn to follow it.",
            ru: "Поэтому мы просто вводим индикатор. Он честно покажет вам, что вы рискуете. \
                 Ваша задача — научиться ему следовать.",
        },
    },
    // 6 — задание недели, по которому откроется следующая глава.
    Frame {
        bg: Bg::Meat,
        media: Media::Emoji("🎯"),
        accent: AMBER,
        kicker: Loc { en: "Red meat", ru: "Красное мясо" },
        title: Loc { en: "", ru: "" },
        body: Loc {
            en: "This week, try to eat red meat so that you stay within your limit. After that \
                 the progress will carry on.",
            ru: "На этой неделе постарайтесь съесть красного мяса так, чтобы уложиться в вашу \
                 планку. После этого прогресс пойдёт дальше.",
        },
    },
];

static STORIES: &[Story] = &[
    Story {
        id: "welcome",
        appears: Appears::Always,
        badge: Loc { en: "?", ru: "?" },
        frames: WELCOME,
    },
    Story {
        id: "week1",
        appears: Appears::Always,
        badge: Loc { en: "1", ru: "1" },
        frames: S1,
    },
    Story {
        id: "week2",
        appears: Appears::AfterCaloriePlanka,
        badge: Loc { en: "2", ru: "2" },
        frames: S2,
    },
    Story {
        id: "week3",
        appears: Appears::AfterActivityWeek,
        badge: Loc { en: "3", ru: "3" },
        frames: S3,
    },
    Story {
        id: "week4",
        appears: Appears::AfterCalciumWeek,
        badge: Loc { en: "4", ru: "4" },
        frames: S4,
    },
    Story {
        id: "week5",
        appears: Appears::AfterIronWeek,
        badge: Loc { en: "5", ru: "5" },
        frames: S5,
    },
    Story {
        id: "week6",
        appears: Appears::AfterFatWeek,
        badge: Loc { en: "6", ru: "6" },
        frames: S6,
    },
    Story {
        id: "week7",
        appears: Appears::AfterRedMeatWeek,
        badge: Loc { en: "7", ru: "7" },
        frames: S7,
    },
];
