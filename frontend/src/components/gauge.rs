//! Horizontal progress gauge: a grey track that fills with `color` from the left
//! as `value` approaches `target`. Used on the dashboard progress widget for
//! calories and the daily-nutrient goals. Empty (all grey) at value 0, so a
//! stack of gauges reads as "greyed out, filling in as the day's data arrives".

use leptos::*;

/// (bar colour, value colour) for an "at least" daily gauge (protein / veg-fruit):
/// a NEUTRAL grey fill while the day's amount is below the target, turning GREEN
/// (bar + value number) once the target is met. `value colour` is `None` when the
/// value keeps its default colour.
pub fn at_least_colors(value: f64, target: f64) -> (&'static str, Option<&'static str>) {
    if target > 0.0 && value >= target {
        ("#1fa463", Some("#1fa463"))
    } else {
        ("#9aa0a6", None)
    }
}

/// Pace markers for a gauge that fills over a PERIOD rather than in one day.
///
/// The track is divided into `segments` equal parts by `segments - 1` dots (a
/// week → 7 parts, 6 dots). Each dot marks where the amount would stand if the
/// period's target were eaten evenly, day by day. Dots for days already behind us
/// are lit, so the last lit dot is "where you should be about now".
///
/// From that same expectation the gauge takes its outline: an ORANGE glow while
/// the value trails the lit dots, a GREEN one once it runs ahead of them.
#[derive(Clone, Copy, PartialEq)]
pub struct GaugePace {
    /// Parts the period is divided into (7 for a week).
    pub segments: u32,
    /// Parts already COMPLETED (day 3 of 7 → 2). `0` on the first day: nothing is
    /// owed yet, so the gauge can't be behind.
    pub passed: u32,
}

impl GaugePace {
    /// The amount expected by now: the completed fraction of the period's target.
    fn expected(&self, target: f64) -> f64 {
        if self.segments == 0 {
            return 0.0;
        }
        target * self.passed as f64 / self.segments as f64
    }
}

/// Шкала ОТ СЕРЕДИНЫ: влево красным, вправо зелёным.
///
/// Нужна там, где показатель — не количество, а ОТНОШЕНИЕ, и у него есть не «сколько
/// набрал», а «в какую сторону сдвинут». Обычная полоса такому врёт: «1.12 из 2.00»
/// заливкой читается как «половина пути», хотя это не половина нормы, а вдвое худший
/// баланс, и никакого «пути» тут нет — значение ходит в обе стороны каждый день.
///
/// Середина — сама норма. Влево (красным) откладывается, насколько баланс хуже
/// нужного: это значит, что насыщенного жира слишком много. Вправо (зелёным) —
/// насколько лучше. Обе половины меряются в долях нормы, поэтому шкала симметрична:
/// левый край — ноль, правый — двойная норма.
///
/// Число показывается СО ЗНАКОМ и считается от нормы, а не от нуля: «−0.88» значит
/// «на 0.88 хуже нормы», «+0.17» — «на столько лучше». Само отношение (1.12) человеку
/// ничего не говорит без второго числа рядом, а отклонение говорит сразу, и знак
/// совпадает со стороной полосы.
///
/// Точек хода недели здесь НЕТ: на короткой двусторонней полосе они только рябят, а
/// догонять у отношения всё равно нечего.
/// `value` — `None`, когда мерить нечего: у продуктов ещё нет профиля жира. Тогда
/// шкала серая и показывает прочерк. Подставлять сюда ноль нельзя: это дало бы
/// полную красную полосу на пустом месте — «рацион ужасен» вместо «мы ещё не знаем».
#[component]
pub fn BalanceGauge(
    value: Option<f64>,
    target: f64,
    label: String,
    #[prop(default = 8.0)] height: f64,
    #[prop(optional, into)] hint: Option<String>,
    #[prop(default = 2)] decimals: usize,
) -> impl IntoView {
    let known = value.is_some();
    let value = value.unwrap_or(target) + 0.0;
    let good = target > 0.0 && value >= target;
    // Отклонение от нормы со знаком — то, что видит человек.
    let signed = value - target;
    // Отклонение в ДОЛЯХ нормы, зажатое единицей: левый край шкалы — ноль, правый —
    // двойная норма. Без зажима вегетарианский рацион с отношением 8 уехал бы за край.
    let dev = if target > 0.0 {
        ((value - target).abs() / target).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let half = if known { dev * 50.0 } else { 0.0 };
    let radius = height / 2.0;
    let bar_color = if good { "#1fa463" } else { "#e0304f" };
    let value_color = if !known {
        "#9aa0a6"
    } else if good {
        "#1fa463"
    } else {
        "#e0304f"
    };

    let open = create_rw_signal(false);
    let hint_btn = hint.clone().map(|_| {
        view! {
            <button
                attr:aria-label="?"
                on:click=move |_| open.update(|o| *o = !*o)
                style="width: 16px; height: 16px; min-width: 16px; border-radius: 50%; \
                    border: 1px solid var(--bulma-border); background: transparent; \
                    color: var(--bulma-text-weak); font-size: 0.62rem; font-weight: 700; \
                    line-height: 1; cursor: pointer; padding: 0; display: inline-flex; \
                    align-items: center; justify-content: center;">
                "?"
            </button>
        }
    });
    let hint_popup = hint.map(|text| {
        view! {
            {move || open.get().then(|| {
                let t = text.clone();
                view! {
                    <div on:pointerup=move |_| open.set(false)
                        style="position: fixed; inset: 0; z-index: 40; cursor: pointer;"></div>
                    <div on:pointerup=move |_| open.set(false)
                        style="position: absolute; z-index: 41; top: 24px; left: 0; right: 0; cursor: pointer; \
                            background: var(--bulma-scheme-main); border: 0.5px solid var(--bulma-border-weak); \
                            box-shadow: 0 10px 30px rgba(0,0,0,0.22); border-radius: 12px; padding: 12px 14px;">
                        <span class="is-size-7 has-text-grey" style="line-height: 1.45; white-space: pre-line;">{t}</span>
                    </div>
                }
            })}
        }
    });

    // Полоса растёт ОТ СЕРЕДИНЫ: влево — левым краем в 50 % минус ширина, вправо —
    // левым краем ровно в 50 %.
    let bar_left = if good { 50.0 } else { 50.0 - half };
    // Скругляется только ВНЕШНИЙ конец. У внутреннего скругления быть не должно:
    // полоса упирается в риску нормы, и закруглённый торец отходит от неё, будто
    // между значением и нормой есть зазор.
    let bar_radius = if good {
        format!("0 {radius}px {radius}px 0")
    } else {
        format!("{radius}px 0 0 {radius}px")
    };
    view! {
        <div attr:data-gauge=label.clone() attr:data-balance-side=if good { "right" } else { "left" }
            style="position: relative; display: flex; flex-direction: column; gap: 5px; width: 100%; min-width: 0;">
            <div style="display: flex; justify-content: space-between; align-items: baseline; gap: 8px;">
                <span style="display: inline-flex; align-items: center; gap: 6px;">
                    <span class="is-size-7 has-text-weight-medium" style="color: var(--bulma-text-weak);">{label}</span>
                    {hint_btn}
                </span>
                <span class="is-size-7" style="white-space: nowrap;">
                    <span class="has-text-weight-bold" style=format!("color: {value_color};")>
                        // Минус — типографский, а не дефис с клавиатуры.
                        {if known {
                            format!("{signed:+.*}", decimals).replace('-', "\u{2212}")
                        } else {
                            "—".to_string()
                        }}
                    </span>
                </span>
            </div>
            <div style=format!("height: {height}px; border-radius: {radius}px; background: var(--bulma-border-weak); \
                    overflow: hidden; position: relative;")>
                <div style=format!("position: absolute; top: 0; height: 100%; left: {bar_left:.1}%; \
                    width: {half:.1}%; background: {bar_color}; border-radius: {bar_radius}; \
                    transition: left 0.4s, width 0.4s;")></div>
                // Риска посередине — сама норма, то есть ноль отклонения. Без неё
                // непонятно, от чего отсчёт.
                <div style=format!("position: absolute; top: 0; bottom: 0; left: 50%; width: 1.5px; \
                    margin-left: -0.75px; background: rgba(0,0,0,0.35); z-index: 3;")></div>
            </div>
            {hint_popup}
        </div>
    }
}

/// A full-width bar. `value`/`target` in the same unit; the fill spans
/// `min(value/target, 1)` of the track. `color` is the fill (the metric's
/// colour); the track is grey. The header line shows `label` on the left and
/// `value / target unit` on the right.
///
/// `hint`: when set, a "?" button appears next to the label; tapping it toggles a
/// tooltip (below the bar) that explains where this target came from.
#[component]
pub fn Gauge(
    value: f64,
    target: f64,
    label: String,
    unit: String,
    color: String,
    /// Bar thickness in px (calories get a thicker bar than the daily goals).
    #[prop(default = 8.0)] height: f64,
    /// Optional "why this target" explanation, shown via a "?" tooltip.
    #[prop(optional, into)] hint: Option<String>,
    /// Optional colour for the VALUE number (the eaten amount) — e.g. red when the
    /// calorie planka is exceeded, green when an "at least" goal is met. The target
    /// (`/ NNN`) always keeps its usual grey.
    #[prop(default = None)] value_color: Option<String>,
    /// Digits after the decimal point in the value/target numbers. Default 0 —
    /// calories and grams read best whole. Iron is the exception: its numbers are
    /// single-digit milligrams, where rounding to a whole number would hide a whole
    /// day's worth of progress.
    #[prop(default = 0)] decimals: usize,
    /// Pace markers for a period gauge (see [`GaugePace`]). `None` → a plain bar,
    /// exactly as before.
    #[prop(default = None)] pace: Option<GaugePace>,
    /// ЧИСЛА ВНУТРИ ПОЛОСЫ, а не строкой над ней.
    ///
    /// Три дневные шкалы стоят в один ряд, и на ширине телефона строка «подпись —
    /// число» в каждой трети не помещается: подписи переносятся, числа упираются в
    /// край. Внутри полосы место есть — над ней остаётся только название.
    #[prop(default = false)] value_inside: bool,
) -> impl IntoView {
    let value_style = value_color
        .map(|c| format!("color: {c};"))
        .unwrap_or_default();
    // Normalize negative zero (an empty nutrient sum can be -0.0) so the label
    // reads "0", not "-0".
    let value = value + 0.0;
    let frac = if target > 0.0 { (value / target).clamp(0.0, 1.0) } else { 0.0 };
    let pct = frac * 100.0;
    let radius = height / 2.0;

    let open = create_rw_signal(false);
    let hint_btn = hint.clone().map(|_| {
        view! {
            <button
                attr:aria-label="?"
                on:click=move |_| open.update(|o| *o = !*o)
                style="width: 16px; height: 16px; min-width: 16px; border-radius: 50%; \
                    border: 1px solid var(--bulma-border); background: transparent; \
                    color: var(--bulma-text-weak); font-size: 0.62rem; font-weight: 700; \
                    line-height: 1; cursor: pointer; padding: 0; display: inline-flex; \
                    align-items: center; justify-content: center;">
                "?"
            </button>
        }
    });
    // A floating popup (not inline): a full-screen backdrop to dismiss + a card
    // anchored under the gauge header, overlaying the content rather than pushing it.
    let hint_popup = hint.map(|text| {
        view! {
            {move || open.get().then(|| {
                let t = text.clone();
                // Dismiss on a tap ANYWHERE — the full-screen backdrop AND the card
                // itself. `pointerup` (not `click`) so it fires on iOS, where a tap on
                // a bare <div> never raises a delegated click.
                view! {
                    <div on:pointerup=move |_| open.set(false)
                        style="position: fixed; inset: 0; z-index: 40; cursor: pointer;"></div>
                    <div on:pointerup=move |_| open.set(false)
                        style="position: absolute; z-index: 41; top: 24px; left: 0; right: 0; cursor: pointer; \
                            background: var(--bulma-scheme-main); border: 0.5px solid var(--bulma-border-weak); \
                            box-shadow: 0 10px 30px rgba(0,0,0,0.22); border-radius: 12px; padding: 12px 14px;">
                        <span class="is-size-7 has-text-grey" style="line-height: 1.45; white-space: pre-line;">{t}</span>
                    </div>
                }
            })}
        }
    });

    // Pace markers + the outline that says whether we're keeping up. Both are
    // empty for an ordinary (single-day) gauge.
    let ahead = pace.map(|p| value >= p.expected(target));
    let track_glow = match ahead {
        // Ahead of the daily pace → green outline; behind → orange. Ring + soft
        // glow so it reads on both light and dark schemes.
        Some(true) => "box-shadow: 0 0 0 1.5px #1fa463, 0 0 8px rgba(31,164,99,0.45);".to_string(),
        Some(false) => "box-shadow: 0 0 0 1.5px #e8850d, 0 0 8px rgba(232,133,13,0.45);".to_string(),
        None => String::new(),
    };
    let dot = height * 0.45;
    let pace_dots = pace.map(|p| {
        (1..p.segments)
            .map(|i| {
                let left = i as f64 * 100.0 / p.segments as f64;
                // A dot is LIT once its day is behind us — the last lit one marks
                // where an even, day-by-day intake would have put the value by now.
                let lit = i <= p.passed;
                // Contrast is taken from what lies UNDER the dot: white over the
                // coloured fill, dark over the pale track. A single colour would
                // vanish on one of the two (a white dot on the grey track reads as
                // an empty hole, a dark one on the fill reads as a gap).
                let on_fill = left <= pct;
                let fill_color = match (on_fill, lit) {
                    (true, true) => "#ffffff",
                    (true, false) => "rgba(255,255,255,0.42)",
                    (false, true) => "rgba(0,0,0,0.55)",
                    (false, false) => "rgba(0,0,0,0.14)",
                };
                view! {
                    <div attr:data-pace-dot=if lit { "lit" } else { "dim" }
                        style=format!(
                            "position: absolute; top: 50%; left: {left:.4}%; width: {dot:.1}px; \
                             height: {dot:.1}px; margin-left: {half:.1}px; margin-top: {half:.1}px; \
                             border-radius: 50%; background: {fill_color};",
                            half = -dot / 2.0,
                        )></div>
                }
            })
            .collect_view()
    });

    // Числа: либо строкой над полосой, либо внутри неё. Текст один и тот же, меняется
    // только место, поэтому и собирается он один раз.
    let numbers = format!(
        "{v} / {t} {unit}",
        v = format!("{value:.*}", decimals),
        t = format!("{target:.*}", decimals),
    );
    // Внутри полосы она должна быть достаточно толстой, чтобы буквы не резались.
    let bar_h = if value_inside { height.max(18.0) } else { height };
    let radius = if value_inside { bar_h / 2.0 } else { radius };
    let inside = value_inside.then(|| {
        // ПЛАНКА ВЫПОЛНЕНА — полоса залита целиком, и текст на ней белый. Пока она
        // не набрана, цифры лежат частью на заливке, частью на пустом фоне, и
        // белый на светлом пропал бы — там обычный тёмный.
        //
        // Прежде здесь стоял `mix-blend-mode: luminosity`, чтобы один цвет годился
        // для обоих фонов. На половине заполнения он рвал строку ровно по границе
        // заливки: половина цифр читалась, половина мылилась.
        let met = frac >= 1.0;
        let color = if met { "#ffffff" } else { "var(--bulma-text)" };
        view! {
            <span class="is-size-7 has-text-weight-semibold"
                style=format!(
                    "position: absolute; inset: 0; display: flex; align-items: center; \
                     justify-content: center; white-space: nowrap; pointer-events: none; \
                     color: {color};"
                )>
                {numbers.clone()}
            </span>
        }
    });
    let above = (!value_inside).then(|| {
        view! {
            <span class="is-size-7" style="white-space: nowrap;">
                <span class="has-text-weight-bold" style=value_style>{format!("{value:.*}", decimals)}</span>
                <span class="has-text-grey">{format!(" / {:.*} {unit}", decimals, target)}</span>
            </span>
        }
    });

    view! {
        <div attr:data-gauge=label.clone() style="position: relative; display: flex; flex-direction: column; gap: 5px; width: 100%; min-width: 0;">
            // Когда числа внутри полосы, справа в этой строке пусто — и подпись
            // встаёт ПО ЦЕНТРУ над своей шкалой, а не жмётся к левому краю.
            <div style=format!(
                "display: flex; justify-content: {}; align-items: baseline; gap: 8px;",
                if value_inside { "center" } else { "space-between" },
            )>
                <span style="display: inline-flex; align-items: center; gap: 6px;">
                    <span class="is-size-7 has-text-weight-medium" style="color: var(--bulma-text-weak);">{label}</span>
                    {hint_btn}
                </span>
                {above}
            </div>
            <div style=format!("height: {bar_h}px; border-radius: {radius}px; background: var(--bulma-border-weak); \
                    overflow: hidden; position: relative; {track_glow}")>
                <div style=format!("height: 100%; width: {pct:.1}%; background: {color}; border-radius: {radius}px; transition: width 0.4s;")></div>
                {pace_dots}
                {inside}
            </div>
            {hint_popup}
        </div>
    }
}
