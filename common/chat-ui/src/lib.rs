//! Вид живого чата: обои, пузыри, системные плашки, поле ввода.
//!
//! Раньше всё это жило только в приложении худеющего, а кураторское рисовало
//! свой чат — с теми же данными, но другой на вид. Разговор при этом ОДИН: два
//! человека смотрят в одну переписку с разных концов, и расходиться их экраны не
//! должны.
//!
//! Значения перенесены сюда ДОСЛОВНО из `frontend/src/pages/chat.rs` и
//! `components/live_message.rs`. Там, где стояли переменные Bulma и её классы
//! размеров, подставлены ЗАМЕРЕННЫЕ в браузере значения светлой темы, а не
//! угаданные: `is-size-6` = 16px, `is-size-7 has-text-grey` = 12px и #69748C,
//! `--bulma-text` = #404654, `--bulma-border` = #D6DAE2,
//! `--bulma-scheme-main-bis` = #F9FAFB.
//!
//! Зашиты именно светлые: обои чата — пастельный градиент при любой теме, и
//! части, которые следовали за темой, в тёмной ломались. Чужой пузырь белый
//! жёстко, а текст в нём брался из `--bulma-text` — то есть светло-серое по
//! белому. Теперь экран цельный, и его можно повторить в кураторском
//! приложении, которое тёмное по умолчанию.
//!
//! Что НЕ переехало и почему: панель запроса данных с кнопкой «Поделиться» —
//! она только у худеющего (куратору делиться нечем); очередь отправки с
//! повтором — тоже его; разделитель тредов — у него лента ОДНА на всех
//! собеседников, а у куратора тред на клиента, и делить нечего.

use leptos::*;

/// Обои чата: пастельный градиент. Только фон — как его разместить, решает
/// приложение: у худеющего чат занимает весь экран, у куратора живёт под
/// шапкой с кнопкой «назад».
pub const WALLPAPER: &str = "background: \
    radial-gradient(120% 80% at 0% 12%, #E7CCFB 0%, rgba(231,204,251,0) 60%), \
    radial-gradient(120% 90% at 0% 100%, #A5B3F9 0%, rgba(165,179,249,0) 60%), \
    radial-gradient(120% 90% at 100% 100%, #DDE9CE 0%, rgba(221,233,206,0) 62%), \
    #C1E1FC;";

/// Прокручиваемая область ленты. `min-height: 0` ОБЯЗАТЕЛЕН, чтобы `flex: 1`
/// прокручивался, а не растил родителя.
pub const SCROLL: &str = "flex: 1; min-height: 0; overflow-y: auto; \
    -webkit-overflow-scrolling: touch; overscroll-behavior: contain; \
    max-width: 30rem; width: 100%; margin: 0 auto;";

/// Узор поверх градиента, ПОД сообщениями — чтобы пузыри оставались чёткими.
pub const PATTERN: &str = "position: absolute; inset: 0; z-index: 0; pointer-events: none; \
    background-image: url('/chat-bg-pattern.svg'); background-repeat: repeat-y; \
    background-size: 100% auto; background-position: top center; mix-blend-mode: overlay;";

/// Обёртка над лентой: держит узор (`position: absolute`) и растягивается на всю
/// высоту прокрутки.
///
/// Сама колонка, а не просто `position: relative`, — иначе `min-height: 100%` у
/// ленты внутри не разрешается: процент считается от высоты родителя, а она
/// `auto`. Из-за этого короткий разговор висел вверху, хотя задумано было
/// обратное (и так и написано в исходном комментарии у худеющего). Теперь высоту
/// задаёт flex, а не процент, и прижатие работает.
pub const WRAP: &str = "position: relative; min-height: 100%; display: flex; flex-direction: column;";

/// Сама лента. `justify-content: flex-end` прижимает короткий разговор к низу —
/// новое сообщение видно без прыжка, а длинный разговор просто прокручивается.
pub const LIST: &str = "position: relative; z-index: 1; flex: 1; display: flex; \
    flex-direction: column; justify-content: flex-end;";

/// Поле ввода: плавающая карточка над лентой.
pub const COMPOSER: &str = "position: absolute; bottom: 0; left: 50%; transform: translateX(-50%); \
    z-index: 35; width: min(26rem, calc(100% - 1.5rem)); background: #FFFFFF; \
    border-radius: 1.25rem; box-shadow: 0 4px 24px rgba(0,0,0,0.15); padding: 0.5rem 0.6rem;";

/// Строка ввода внутри карточки.
pub const TEXTAREA: &str = "flex: 1; min-width: 0; min-height: 2.5rem; max-height: 9rem; \
    padding: 0.55rem 0.85rem; border: 1px solid #D6DAE2; border-radius: 1.25rem; \
    background: #F9FAFB; color: #404654; outline: none; resize: none; line-height: 1.4; \
    overflow-y: auto; font: inherit; box-sizing: border-box;";

/// Пузырь сообщения.
///
/// `mine` — не «от кого пришло», а «моё ли»: у худеющего своё — от него, у
/// куратора — от куратора, и цвет обязан следовать за этим, а не за полем
/// `sender`. Иначе у одной из сторон весь разговор оказался бы чужим.
///
/// Палитра re:Norma: своё — мягкий изумрудный тон марки (а не дежурный синий),
/// чужое — белая карточка. У обоих светлая рамка в тон, чтобы читались на
/// пастельных обоях.
#[component]
pub fn Bubble(
    text: String,
    mine: bool,
    /// Подпись над чужим пузырём. Без неё куратор неотличим от поддержки.
    /// `None` — подписи нет (своё сообщение, или собеседник и так один).
    sender_name: Option<String>,
    /// Полупрозрачность для неотправленного (очередь отправки у худеющего).
    #[prop(optional)]
    pending: bool,
) -> impl IntoView {
    let base = if mine {
        "background: #DEF7EC; color: #04603F; border: 1px solid #A7E3CD; border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-left: auto;"
    } else {
        "background: #FFFFFF; color: #404654; border: 1px solid #E4E8F0; border-radius: 12px; padding: 14px 16px; max-width: 80%; margin-right: auto;"
    };
    let style = if pending { format!("{base} opacity: 0.6;") } else { base.to_string() };
    let name_line = sender_name.map(|n| view! {
        <span attr:data-testid="live-sender-name"
            style="font-size: 12px; color: #69748C; margin: 0 0 3px 4px;">{n}</span>
    });
    view! {
        <div style="display: flex; flex-direction: column; margin-bottom: 10px;">
            {name_line}
            <div style=style>
                <p style="font-size: 16px; white-space: pre-wrap; line-height: 1.45; margin: 0;">{text}</p>
            </div>
        </div>
    }
}

/// Системная плашка по центру: директива куратора, смена темы, смена куратора.
/// Не разговор, поэтому и не пузырь.
#[component]
pub fn Note(text: String) -> impl IntoView {
    view! {
        <div attr:data-testid="live-message" attr:data-role="system"
            style="display: flex; justify-content: center; margin-bottom: 10px;">
            <div style="background: #EEF6FF; color: #1E4E79; border: 1px solid #CFE2F7; \
                        border-radius: 10px; padding: 8px 14px; max-width: 88%; text-align: center;">
                <p style="font-size: 12px; margin: 0;">{text}</p>
            </div>
        </div>
    }
}
