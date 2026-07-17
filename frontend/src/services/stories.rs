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
}

/// Seed the seen-set from `app_flags` and create the reactive version signal.
/// Call once from `main()` inside the Leptos runtime.
pub fn init() {
    let set: HashSet<String> = app_flags::get(VIEWED_KEY)
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    let ver = create_rw_signal(0u32);
    VIEWED.with(|v| *v.borrow_mut() = Some(ViewedState { set, ver }));
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
    // 6 — steps: tap widget
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("dashboard.png"),
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
            en: "Take the step count from your phone's pedometer and type it in.",
            ru: "Возьмите число шагов из шагомера на телефоне и впишите его.",
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
            en: "Press «Save». Log steps every day — it's half of all your activity.",
            ru: "Нажмите «Сохранить». Записывайте шаги каждый день — это половина всей активности.",
        },
    },
    // 9 — food by description
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
    // 10 — food by label
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
    // 11 — food by dish photo
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
    // 12 — repeat: go to yesterday
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-duplicate-popup.png"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "Copy from yesterday", ru: "Копируем из вчера" },
        body: Loc {
            en: "Eating the same thing? Open the diary and swipe to «Yesterday».",
            ru: "Едите одно и то же? Откройте дневник и перелистните на «Вчера».",
        },
    },
    // 13 — repeat: the icon
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-duplicate-card.png"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "The ⇄ icon", ru: "Иконка ⇄" },
        body: Loc {
            en: "Every entry has a ⇄ icon on the right. Tap it on the product you want.",
            ru: "У каждой записи справа есть иконка ⇄. Нажмите её у нужного продукта.",
        },
    },
    // 14 — repeat: repeat today
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-duplicate-popup.png"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "Repeat today", ru: "Повторить сегодня" },
        body: Loc {
            en: "Choose «Repeat today» — the product, with its grams and macros, moves to today.",
            ru: "Выберите «Повторить сегодня» — продукт со всеми граммами и КБЖУ перенесётся в сегодня.",
        },
    },
    // 15 — duplicate today
    Frame {
        bg: Bg::Dark,
        media: Media::Shot("shot-duplicate-card.png"),
        accent: GREEN,
        kicker: Loc { en: "Repeat", ru: "Повтор" },
        title: Loc { en: "Duplicate within today", ru: "Дублируем сегодня" },
        body: Loc {
            en: "The same works for what you already ate today: the same ⇄ on today's entry.",
            ru: "Так же дублируется съеденное сегодня: тот же значок ⇄ у записи за сегодня.",
        },
    },
    // 16 — warning: dish photo is a draft
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
];

static STORIES: &[Story] = &[Story {
    id: "week1",
    appears: Appears::Always,
    badge: Loc { en: "1", ru: "1" },
    frames: S1,
}];
