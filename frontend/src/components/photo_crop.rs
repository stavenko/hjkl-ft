//! Просмотр снимка и обрезка — по образцу «Фото» на айфоне.
//!
//! Порядок управления там такой, и он здесь повторён, потому что человек его уже
//! знает руками:
//!
//! * **Рамка стоит.** Она не ездит по экрану целиком — её тянут за углы и стороны.
//!   Взялись за угол — двигается только он, противоположный остаётся на месте.
//! * **Снимок ходит под рамкой.** Одним пальцем — тащим, двумя — приближаем и
//!   отдаляем, и заодно тащим. Рамка при этом не шевелится.
//! * **Пустоты в рамке не бывает.** Снимок нельзя увести или уменьшить так, чтобы
//!   в рамке оказался край. Поэтому нижний предел приближения не постоянный: он
//!   считается от нынешней рамки и растёт, когда рамку раздвигают.
//!
//! Сетка в треть показывается ТОЛЬКО пока идёт жест — как там же. Стоящая всё
//! время, она читается как часть снимка.
//!
//! Обрезка настоящая: подтвердили — снимок перерисовывается в `<canvas>` по
//! выбранному куску и заменяется. Модели уезжает уже обрезанный кадр, а не
//! исходный с приложенной рамкой, — в этом весь смысл: человек убирает со стола
//! то, что путает разбор.

use leptos::*;
use wasm_bindgen::{JsCast, JsValue};

use crate::services::i18n::t;

/// За что взялись. Углы двигают две стороны сразу, стороны — одну.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Grip {
    Nw,
    Ne,
    Sw,
    Se,
    N,
    S,
    W,
    E,
}

/// Что сейчас происходит под пальцем.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    /// Тянут рамку за `Grip`.
    Frame(Grip),
    /// Тащат снимок одним пальцем.
    Pan,
    /// Двумя пальцами: расстояние между ними и середина на момент начала.
    Pinch { dist: f64, mid: (f64, f64) },
}

/// Рамка в координатах сцены (CSS-пиксели от её левого верхнего угла).
#[derive(Clone, Copy, PartialEq, Debug)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Меньше рамку не пускаем: за ней должно оставаться что тянуть пальцем.
const MIN_FRAME: f64 = 64.0;
/// Толщина уголка и стороны — палец должен попадать, не целясь.
const GRIP: f64 = 28.0;
/// Поля вокруг снимка на сцене. Без них рамка открывается впритык к краю экрана,
/// и за её уголки нечем взяться: половина уголка оказывается за пределами.
const PAD: f64 = 20.0;
/// Тот же предел, что и у снимка с камеры (`food_editor::file_to_jpeg_base64`):
/// обрезанный кадр не должен вдруг оказаться крупнее исходного.
const MAX_DIM: f64 = 1536.0;

/// Где снимок лежит на сцене при нынешних приближении и сдвиге.
///
/// Исходно он вписан в сцену целиком («contain»), дальше `zoom` умножает, а
/// `tx`/`ty` сдвигают от середины.
fn img_rect(nat: (f64, f64), stage: (f64, f64), zoom: f64, tx: f64, ty: f64) -> Rect {
    let fit = fit_scale(nat, stage);
    let w = nat.0 * fit * zoom;
    let h = nat.1 * fit * zoom;
    Rect { x: (stage.0 - w) / 2.0 + tx, y: (stage.1 - h) / 2.0 + ty, w, h }
}

fn fit_scale(nat: (f64, f64), stage: (f64, f64)) -> f64 {
    if nat.0 <= 0.0 || nat.1 <= 0.0 || stage.0 <= 0.0 || stage.1 <= 0.0 {
        return 1.0;
    }
    let (w, h) = ((stage.0 - 2.0 * PAD).max(1.0), (stage.1 - 2.0 * PAD).max(1.0));
    (w / nat.0).min(h / nat.1)
}

/// Наименьшее приближение, при котором снимок ещё накрывает рамку. Растёт вместе
/// с рамкой — раздвинули рамку шире снимка, и отдалять дальше уже нельзя.
fn min_zoom(nat: (f64, f64), stage: (f64, f64), frame: Rect) -> f64 {
    let fit = fit_scale(nat, stage);
    if fit <= 0.0 {
        return 1.0;
    }
    (frame.w / (nat.0 * fit)).max(frame.h / (nat.1 * fit)).max(0.05)
}

/// Подвинуть снимок так, чтобы в рамке не было пустоты. Возвращает поправленные
/// сдвиги — приближение к этому моменту уже не меньше `min_zoom`.
fn clamp_pan(nat: (f64, f64), stage: (f64, f64), zoom: f64, tx: f64, ty: f64, frame: Rect) -> (f64, f64) {
    let r = img_rect(nat, stage, zoom, tx, ty);
    let mut dx = 0.0;
    let mut dy = 0.0;
    if r.x > frame.x {
        dx = frame.x - r.x;
    } else if r.x + r.w < frame.x + frame.w {
        dx = (frame.x + frame.w) - (r.x + r.w);
    }
    if r.y > frame.y {
        dy = frame.y - r.y;
    } else if r.y + r.h < frame.y + frame.h {
        dy = (frame.y + frame.h) - (r.y + r.h);
    }
    (tx + dx, ty + dy)
}

/// Новая рамка после протяжки за `grip` на (dx, dy). Держится внутри снимка и не
/// схлопывается меньше `MIN_FRAME`.
fn resize(frame: Rect, grip: Grip, dx: f64, dy: f64, bounds: Rect) -> Rect {
    let (mut l, mut t) = (frame.x, frame.y);
    let (mut r, mut b) = (frame.x + frame.w, frame.y + frame.h);
    match grip {
        Grip::Nw => {
            l += dx;
            t += dy;
        }
        Grip::Ne => {
            r += dx;
            t += dy;
        }
        Grip::Sw => {
            l += dx;
            b += dy;
        }
        Grip::Se => {
            r += dx;
            b += dy;
        }
        Grip::N => t += dy,
        Grip::S => b += dy,
        Grip::W => l += dx,
        Grip::E => r += dx,
    }
    // Сначала в границы снимка, потом — не тоньше допустимого. Порядок важен: иначе
    // упёршаяся в край сторона утаскивала бы за собой противоположную.
    l = l.max(bounds.x).min(r - MIN_FRAME);
    t = t.max(bounds.y).min(b - MIN_FRAME);
    r = r.min(bounds.x + bounds.w).max(l + MIN_FRAME);
    b = b.min(bounds.y + bounds.h).max(t + MIN_FRAME);
    Rect { x: l, y: t, w: r - l, h: b - t }
}

/// Перерисовать выбранный кусок в новый JPEG (base64 без префикса).
fn cut(
    img: &web_sys::HtmlImageElement,
    nat: (f64, f64),
    stage: (f64, f64),
    zoom: f64,
    tx: f64,
    ty: f64,
    frame: Rect,
) -> Result<String, String> {
    let r = img_rect(nat, stage, zoom, tx, ty);
    let k = fit_scale(nat, stage) * zoom;
    if k <= 0.0 {
        return Err("нулевой масштаб".into());
    }
    // Из координат сцены — в пиксели исходника.
    let sx = ((frame.x - r.x) / k).max(0.0);
    let sy = ((frame.y - r.y) / k).max(0.0);
    let sw = (frame.w / k).min(nat.0 - sx).max(1.0);
    let sh = (frame.h / k).min(nat.1 - sy).max(1.0);

    let scale = (MAX_DIM / sw.max(sh)).min(1.0);
    let (dw, dh) = ((sw * scale).round().max(1.0), (sh * scale).round().max(1.0));

    let document = web_sys::window().and_then(|w| w.document()).ok_or("нет документа")?;
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|e| format!("canvas: {e:?}"))?
        .unchecked_into();
    canvas.set_width(dw as u32);
    canvas.set_height(dh as u32);
    let ctx: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .map_err(|e| format!("2d: {e:?}"))?
        .ok_or("нет контекста 2d")?
        .unchecked_into();
    ctx.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(
        img, sx, sy, sw, sh, 0.0, 0.0, dw, dh,
    )
    .map_err(|e| format!("draw: {e:?}"))?;
    let url = canvas
        .to_data_url_with_type_and_encoder_options("image/jpeg", &JsValue::from_f64(0.85))
        .map_err(|e| format!("toDataURL: {e:?}"))?;
    url.split_once(',')
        .map(|(_, b64)| b64.to_string())
        .ok_or_else(|| "плохой data URL".to_string())
}

#[component]
pub fn PhotoCrop(
    /// Снимок: base64 JPEG без префикса `data:`.
    src: String,
    /// Обрезали и подтвердили — новый base64 на замену.
    on_done: Callback<String>,
    /// Удалить снимок целиком.
    on_delete: Callback<()>,
    /// Закрыть, ничего не меняя.
    on_cancel: Callback<()>,
) -> impl IntoView {
    let src = store_value(src);

    let nat = create_rw_signal((0.0_f64, 0.0_f64));
    let stage = create_rw_signal((0.0_f64, 0.0_f64));
    let zoom = create_rw_signal(1.0_f64);
    let tx = create_rw_signal(0.0_f64);
    let ty = create_rw_signal(0.0_f64);
    let frame = create_rw_signal(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 });
    let drag = create_rw_signal(None::<Drag>);
    // Пальцы: (id, x, y). Второй палец превращает протяжку в щипок.
    let pointers = create_rw_signal(Vec::<(i32, f64, f64)>::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    let stage_ref = create_node_ref::<leptos::html::Div>();
    let img_ref = create_node_ref::<leptos::html::Img>();

    // Открываемся так же, как «Фото»: весь снимок выбран целиком.
    let reset = move || {
        zoom.set(1.0);
        tx.set(0.0);
        ty.set(0.0);
        frame.set(img_rect(nat.get_untracked(), stage.get_untracked(), 1.0, 0.0, 0.0));
    };

    // Размеры известны только после загрузки картинки И раскладки сцены — ждём обоих.
    let measure = move || {
        if let Some(el) = stage_ref.get_untracked() {
            let r = el.get_bounding_client_rect();
            stage.set((r.width(), r.height()));
        }
        if nat.get_untracked().0 > 0.0 && stage.get_untracked().0 > 0.0 {
            reset();
        }
    };

    let on_img_load = move |_| {
        if let Some(el) = img_ref.get_untracked() {
            nat.set((el.natural_width() as f64, el.natural_height() as f64));
        }
        measure();
    };

    // Точка события в координатах сцены.
    let local = move |ev: &web_sys::PointerEvent| -> (f64, f64) {
        match stage_ref.get_untracked() {
            Some(el) => {
                let r = el.get_bounding_client_rect();
                (ev.client_x() as f64 - r.left(), ev.client_y() as f64 - r.top())
            }
            None => (ev.client_x() as f64, ev.client_y() as f64),
        }
    };

    let on_stage_down = move |ev: web_sys::PointerEvent| {
        let p = local(&ev);
        pointers.update(|v| {
            v.retain(|(id, _, _)| *id != ev.pointer_id());
            v.push((ev.pointer_id(), p.0, p.1));
        });
        let ps = pointers.get_untracked();
        if ps.len() >= 2 {
            let (a, b) = (ps[0], ps[1]);
            let dist = ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt().max(1.0);
            drag.set(Some(Drag::Pinch { dist, mid: ((a.1 + b.1) / 2.0, (a.2 + b.2) / 2.0) }));
        } else if drag.get_untracked().is_none() {
            drag.set(Some(Drag::Pan));
        }
    };

    let on_stage_move = move |ev: web_sys::PointerEvent| {
        let Some(mode) = drag.get_untracked() else { return };
        let p = local(&ev);
        let prev = pointers
            .get_untracked()
            .iter()
            .find(|(id, _, _)| *id == ev.pointer_id())
            .map(|(_, x, y)| (*x, *y));
        pointers.update(|v| {
            if let Some(slot) = v.iter_mut().find(|(id, _, _)| *id == ev.pointer_id()) {
                slot.1 = p.0;
                slot.2 = p.1;
            }
        });
        let (n, st) = (nat.get_untracked(), stage.get_untracked());
        if n.0 <= 0.0 {
            return;
        }
        match mode {
            Drag::Frame(grip) => {
                let Some((px, py)) = prev else { return };
                let bounds = img_rect(n, st, zoom.get_untracked(), tx.get_untracked(), ty.get_untracked());
                let next = resize(frame.get_untracked(), grip, p.0 - px, p.1 - py, bounds);
                frame.set(next);
                // Рамку раздвинули — нижний предел приближения мог подрасти.
                let mz = min_zoom(n, st, next);
                if zoom.get_untracked() < mz {
                    zoom.set(mz);
                }
                let (nx, ny) =
                    clamp_pan(n, st, zoom.get_untracked(), tx.get_untracked(), ty.get_untracked(), next);
                tx.set(nx);
                ty.set(ny);
            }
            Drag::Pan => {
                let Some((px, py)) = prev else { return };
                let f = frame.get_untracked();
                let (nx, ny) = clamp_pan(
                    n,
                    st,
                    zoom.get_untracked(),
                    tx.get_untracked() + (p.0 - px),
                    ty.get_untracked() + (p.1 - py),
                    f,
                );
                tx.set(nx);
                ty.set(ny);
            }
            Drag::Pinch { dist, mid } => {
                let ps = pointers.get_untracked();
                if ps.len() < 2 {
                    return;
                }
                let (a, b) = (ps[0], ps[1]);
                let d = ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt().max(1.0);
                let m = ((a.1 + b.1) / 2.0, (a.2 + b.2) / 2.0);
                let f = frame.get_untracked();
                let mz = min_zoom(n, st, f);
                let z0 = zoom.get_untracked();
                let z = (z0 * (d / dist)).max(mz).min(12.0);
                // Приближаем ОТ СЕРЕДИНЫ между пальцами, а не от центра сцены:
                // иначе то, что человек держит, уезжает у него из-под рук.
                let c = (st.0 / 2.0, st.1 / 2.0);
                let k = z / z0;
                let nx = (tx.get_untracked() - (mid.0 - c.0)) * k + (m.0 - c.0);
                let ny = (ty.get_untracked() - (mid.1 - c.1)) * k + (m.1 - c.1);
                zoom.set(z);
                let (nx, ny) = clamp_pan(n, st, z, nx, ny, f);
                tx.set(nx);
                ty.set(ny);
                drag.set(Some(Drag::Pinch { dist: d, mid: m }));
            }
        }
    };

    let on_stage_up = move |ev: web_sys::PointerEvent| {
        pointers.update(|v| v.retain(|(id, _, _)| *id != ev.pointer_id()));
        if pointers.get_untracked().is_empty() {
            drag.set(None);
        } else {
            // Один палец отпустили из двух — щипок кончился, тащим оставшимся.
            drag.set(Some(Drag::Pan));
        }
    };

    let apply = move |_| {
        if busy.get_untracked() {
            return;
        }
        let Some(img) = img_ref.get_untracked() else { return };
        busy.set(true);
        match cut(
            &img,
            nat.get_untracked(),
            stage.get_untracked(),
            zoom.get_untracked(),
            tx.get_untracked(),
            ty.get_untracked(),
            frame.get_untracked(),
        ) {
            Ok(b64) => on_done.call(b64),
            Err(e) => {
                error.set(Some(e));
                busy.set(false);
            }
        }
    };

    // Уголок или сторона. Каждый сам объявляет, за что взялись, и НЕ пускает
    // событие дальше — иначе сцена сочла бы это протяжкой снимка.
    let grip_at = move |grip: Grip, style: String| {
        view! {
            <div
                attr:data-testid=match grip {
                    Grip::Nw => "photo-crop-grip-nw",
                    Grip::Ne => "photo-crop-grip-ne",
                    Grip::Sw => "photo-crop-grip-sw",
                    Grip::Se => "photo-crop-grip-se",
                    Grip::N => "photo-crop-grip-n",
                    Grip::S => "photo-crop-grip-s",
                    Grip::W => "photo-crop-grip-w",
                    Grip::E => "photo-crop-grip-e",
                }
                style=style
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    ev.stop_propagation();
                    pointers.update(|v| {
                        v.retain(|(id, _, _)| *id != ev.pointer_id());
                        v.push((ev.pointer_id(), 0.0, 0.0));
                    });
                    // Точку берём той же меркой, что и сцена, — иначе первый сдвиг
                    // прыгнул бы на разницу систем координат.
                    let p = local(&ev);
                    pointers.update(|v| {
                        if let Some(slot) = v.iter_mut().find(|(id, _, _)| *id == ev.pointer_id()) {
                            slot.1 = p.0;
                            slot.2 = p.1;
                        }
                    });
                    drag.set(Some(Drag::Frame(grip)));
                }
            ></div>
        }
    };

    view! {
      // В <body>, как полноэкранный просмотр историй (`story_tray.rs`) и по той же
      // причине: оболочка приложения — `position: fixed`, а значит СВОЙ контекст
      // наложения, и вложенный в неё экран не может лечь поверх нижнего меню, какой
      // бы z-index ему ни поставить. Меню при этом остаётся на своих 40.
      <Portal>
        <div attr:data-testid="photo-crop"
            style="position: fixed; inset: 0; z-index: 100; display: flex; flex-direction: column; background: var(--bulma-black);">

            // ── Шапка ───────────────────────────────────────────────────────
            <div style="display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 16px; flex: none;">
                <button
                    attr:data-testid="photo-crop-cancel"
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; padding: 4px; font: inherit; cursor: pointer; color: var(--bulma-white);"
                    on:click=move |_| on_cancel.call(())
                >{move || t("common.cancel")}</button>

                <button
                    attr:data-testid="photo-crop-reset"
                    class="is-size-7"
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; padding: 4px 8px; font: inherit; cursor: pointer; color: var(--bulma-white);"
                    on:click=move |_| reset()
                >{move || t("photo_crop.reset")}</button>

                <button
                    attr:data-testid="photo-crop-done"
                    class="has-text-weight-semibold"
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; padding: 4px; font: inherit; cursor: pointer; color: var(--bulma-link);"
                    disabled=move || busy.get()
                    on:click=apply
                >{move || t("photo_crop.done")}</button>
            </div>

            // ── Сцена ───────────────────────────────────────────────────────
            <div node_ref=stage_ref
                attr:data-testid="photo-crop-stage"
                style="position: relative; flex: 1; overflow: hidden; touch-action: none; user-select: none; -webkit-user-select: none;"
                on:pointerdown=on_stage_down
                on:pointermove=on_stage_move
                on:pointerup=on_stage_up
                on:pointercancel=on_stage_up
            >
                <img node_ref=img_ref
                    attr:data-testid="photo-crop-image"
                    src=move || format!("data:image/jpeg;base64,{}", src.get_value())
                    on:load=on_img_load
                    draggable="false"
                    style=move || {
                        let r = img_rect(nat.get(), stage.get(), zoom.get(), tx.get(), ty.get());
                        format!("position: absolute; left: {}px; top: {}px; width: {}px; height: {}px; \
                                 max-width: none; pointer-events: none;", r.x, r.y, r.w, r.h)
                    }
                />

                // Затемнение вокруг рамки — четырьмя полосами. Тон берём у модального
                // фона Bulma, чтобы он жил вместе с темой, а не спорил с ней.
                {move || {
                    let f = frame.get();
                    let (sw, sh) = stage.get();
                    if f.w <= 0.0 { return ().into_view(); }
                    let band = |x: f64, y: f64, w: f64, h: f64| view! {
                        <div class="modal-background"
                            style=format!("position: absolute; left: {x}px; top: {y}px; width: {}px; height: {}px; pointer-events: none;",
                                          w.max(0.0), h.max(0.0)) ></div>
                    };
                    view! {
                        {band(0.0, 0.0, sw, f.y)}
                        {band(0.0, f.y + f.h, sw, sh - (f.y + f.h))}
                        {band(0.0, f.y, f.x, f.h)}
                        {band(f.x + f.w, f.y, sw - (f.x + f.w), f.h)}
                    }.into_view()
                }}

                // Рамка, её уголки и стороны.
                {move || {
                    let f = frame.get();
                    if f.w <= 0.0 { return ().into_view(); }
                    let gridding = drag.get().is_some();
                    let edge = format!("position: absolute; left: {}px; top: {}px; width: {}px; height: {}px;",
                                       f.x, f.y, f.w, f.h);
                    let g = GRIP;
                    view! {
                        <div attr:data-testid="photo-crop-frame"
                            style=format!("{edge} border: 1px solid var(--bulma-white); pointer-events: none;")></div>

                        // Сетка в треть — только пока идёт жест, как в «Фото».
                        {gridding.then(|| {
                            let line_v = |k: f64| format!(
                                "position: absolute; left: {}px; top: {}px; width: 1px; height: {}px; background: var(--bulma-white); opacity: 0.4; pointer-events: none;",
                                f.x + f.w * k, f.y, f.h);
                            let line_h = |k: f64| format!(
                                "position: absolute; left: {}px; top: {}px; width: {}px; height: 1px; background: var(--bulma-white); opacity: 0.4; pointer-events: none;",
                                f.x, f.y + f.h * k, f.w);
                            view! {
                                <div style=line_v(1.0/3.0)></div>
                                <div style=line_v(2.0/3.0)></div>
                                <div style=line_h(1.0/3.0)></div>
                                <div style=line_h(2.0/3.0)></div>
                            }
                        })}

                        {grip_at(Grip::Nw, format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {g}px; cursor: nwse-resize;", f.x - g/2.0, f.y - g/2.0))}
                        {grip_at(Grip::Ne, format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {g}px; cursor: nesw-resize;", f.x + f.w - g/2.0, f.y - g/2.0))}
                        {grip_at(Grip::Sw, format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {g}px; cursor: nesw-resize;", f.x - g/2.0, f.y + f.h - g/2.0))}
                        {grip_at(Grip::Se, format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {g}px; cursor: nwse-resize;", f.x + f.w - g/2.0, f.y + f.h - g/2.0))}
                        {grip_at(Grip::N,  format!("position: absolute; left: {}px; top: {}px; width: {}px; height: {g}px; cursor: ns-resize;", f.x + g/2.0, f.y - g/2.0, (f.w - g).max(1.0)))}
                        {grip_at(Grip::S,  format!("position: absolute; left: {}px; top: {}px; width: {}px; height: {g}px; cursor: ns-resize;", f.x + g/2.0, f.y + f.h - g/2.0, (f.w - g).max(1.0)))}
                        {grip_at(Grip::W,  format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {}px; cursor: ew-resize;", f.x - g/2.0, f.y + g/2.0, (f.h - g).max(1.0)))}
                        {grip_at(Grip::E,  format!("position: absolute; left: {}px; top: {}px; width: {g}px; height: {}px; cursor: ew-resize;", f.x + f.w - g/2.0, f.y + g/2.0, (f.h - g).max(1.0)))}

                        // Уголки видимыми скобками — палец должен знать, куда целиться.
                        {[(f.x, f.y, 1.0, 1.0), (f.x + f.w, f.y, -1.0, 1.0),
                          (f.x, f.y + f.h, 1.0, -1.0), (f.x + f.w, f.y + f.h, -1.0, -1.0)]
                            .into_iter().map(|(cx, cy, sx, sy)| {
                                let arm = 22.0_f64;
                                let th = 3.0_f64;
                                let hx = if sx > 0.0 { cx - th/2.0 } else { cx - arm + th/2.0 };
                                let vy = if sy > 0.0 { cy - th/2.0 } else { cy - arm + th/2.0 };
                                view! {
                                    <div style=format!("position: absolute; left: {hx}px; top: {}px; width: {arm}px; height: {th}px; background: var(--bulma-white); pointer-events: none;", cy - th/2.0)></div>
                                    <div style=format!("position: absolute; left: {}px; top: {vy}px; width: {th}px; height: {arm}px; background: var(--bulma-white); pointer-events: none;", cx - th/2.0)></div>
                                }
                            }).collect_view()}
                    }.into_view()
                }}
            </div>

            // ── Подвал ──────────────────────────────────────────────────────
            <div style="display: flex; flex-direction: column; gap: 6px; padding: 12px 16px 20px; flex: none;">
                {move || error.get().map(|e| view! {
                    <p class="help is-danger" attr:data-testid="photo-crop-error">{e}</p>
                })}
                <p class="is-size-7 has-text-centered" style="margin: 0 0 2px; line-height: 1.35; color: var(--bulma-white); opacity: 0.55;">
                    {move || t("photo_crop.hint")}
                </p>
                <button
                    attr:data-testid="photo-crop-delete"
                    class="button is-danger is-light is-fullwidth"
                    on:click=move |_| on_delete.call(())
                >{move || t("photo_crop.delete")}</button>
            </div>
        </div>
      </Portal>
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.001
    }

    #[test]
    fn snimok_vpisyvaetsya_celikom_s_polyami_i_stoit_po_seredine() {
        // Сцена 400×800, снимок 1000×500 — упирается в ширину ЗА вычетом полей и
        // стоит по центру. Поля не украшение: без них за уголки рамки не взяться.
        let r = img_rect((1000.0, 500.0), (400.0, 800.0), 1.0, 0.0, 0.0);
        assert!(approx(r.w, 360.0), "ширина {}", r.w);
        assert!(approx(r.h, 180.0), "высота {}", r.h);
        assert!(approx(r.x, PAD), "левый край {} должен отступать на поле", r.x);
        assert!(approx(r.y, 310.0), "верх {}", r.y);
    }

    #[test]
    fn v_ramke_ne_byvaet_pustoty() {
        let nat = (1000.0, 500.0);
        let st = (400.0, 800.0);
        let frame = Rect { x: PAD, y: 310.0, w: 360.0, h: 180.0 };
        // Уводим снимок далеко вправо — поправка обязана вернуть его на место.
        let (tx, ty) = clamp_pan(nat, st, 1.0, 500.0, 0.0, frame);
        let r = img_rect(nat, st, 1.0, tx, ty);
        assert!(r.x <= frame.x + 0.001, "левый край {} правее рамки", r.x);
        assert!(r.x + r.w >= frame.x + frame.w - 0.001, "правый край не достаёт");
        assert!(approx(ty, 0.0), "по вертикали двигать было незачем");
    }

    #[test]
    fn nizhnij_predel_priblizheniya_rastet_vmeste_s_ramkoj() {
        let nat = (1000.0, 500.0);
        let st = (400.0, 800.0);
        // Рамка во весь вписанный снимок — отдалять некуда, предел ровно 1.
        let full = Rect { x: PAD, y: 310.0, w: 360.0, h: 180.0 };
        assert!(approx(min_zoom(nat, st, full), 1.0));
        // Рамка вдвое уже — можно отдалить вдвое.
        let half = Rect { x: PAD, y: 310.0, w: 180.0, h: 90.0 };
        assert!(approx(min_zoom(nat, st, half), 0.5), "{}", min_zoom(nat, st, half));
    }

    #[test]
    fn ugol_tyanet_svoyu_storonu_a_protivopolozhnaya_stoit() {
        let bounds = Rect { x: 0.0, y: 0.0, w: 400.0, h: 400.0 };
        let f = Rect { x: 100.0, y: 100.0, w: 200.0, h: 200.0 };
        let out = resize(f, Grip::Nw, 40.0, 20.0, bounds);
        assert!(approx(out.x, 140.0), "левая {}", out.x);
        assert!(approx(out.y, 120.0), "верхняя {}", out.y);
        // Правая и нижняя не сдвинулись.
        assert!(approx(out.x + out.w, 300.0));
        assert!(approx(out.y + out.h, 300.0));
    }

    #[test]
    fn ramka_ne_vyhodit_za_snimok_i_ne_shlopyvaetsya() {
        let bounds = Rect { x: 50.0, y: 50.0, w: 200.0, h: 200.0 };
        let f = Rect { x: 100.0, y: 100.0, w: 100.0, h: 100.0 };
        // Тянем далеко влево-вверх: упрёмся в край снимка, а не уедем за него.
        let out = resize(f, Grip::Nw, -500.0, -500.0, bounds);
        assert!(out.x >= 50.0 - 0.001 && out.y >= 50.0 - 0.001, "{out:?}");
        // Тянем правый край внутрь до схлопывания — остаётся минимум.
        let tight = resize(f, Grip::E, -500.0, 0.0, bounds);
        assert!(approx(tight.w, MIN_FRAME), "ширина {}", tight.w);
    }

    #[test]
    fn storona_dvigaet_tolko_sebya() {
        let bounds = Rect { x: 0.0, y: 0.0, w: 400.0, h: 400.0 };
        let f = Rect { x: 100.0, y: 100.0, w: 200.0, h: 200.0 };
        let out = resize(f, Grip::S, 0.0, 50.0, bounds);
        assert!(approx(out.x, 100.0) && approx(out.w, 200.0), "ширина не должна меняться");
        assert!(approx(out.y, 100.0), "верх не должен меняться");
        assert!(approx(out.h, 250.0), "высота {}", out.h);
    }
}
