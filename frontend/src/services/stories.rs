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
    /// Full-bleed photo — asset path served under `/story-img/`.
    Photo(&'static str),
}

/// A frame's foreground media.
#[derive(Clone, Copy)]
pub enum Media {
    None,
    /// An app screenshot shown as a centred rounded card (`/story-img/…`).
    Shot(&'static str),
    /// The bundled weight-trend SVG chart.
    Chart,
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
            Media::Chart => s.push_str("chart"),
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

/// The stories currently eligible to show, in order.
pub fn visible() -> Vec<&'static Story> {
    STORIES
        .iter()
        .filter(|s| match s.appears {
            Appears::Always => true,
        })
        .collect()
}

pub fn by_id(id: &str) -> Option<&'static Story> {
    STORIES.iter().find(|s| s.id == id)
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
            en: "Weigh in once, in the morning. On the home screen tap the «Weight» widget.",
            ru: "Взвешивайтесь один раз, с утра. На главном экране нажмите на виджет «Вес».",
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
    // 10 — food: add an entry (highlight the green «+» button)
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("diary-plus.gif"),
        accent: GREEN,
        kicker: Loc { en: "Food", ru: "Еда" },
        title: Loc { en: "Add an entry", ru: "Добавьте запись" },
        body: Loc {
            en: "Tap the green «+» button.",
            ru: "Нажмите зелёную кнопку «+».",
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
];
