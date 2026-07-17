//! The Stories tray (row of ring-circles) + the fullscreen frame viewer.
//!
//! The tray renders one circle per visible story with a partial ring showing the
//! fraction of that story's frames not yet seen (ring gone once all are seen).
//! Tapping a circle opens the viewer: top progress bars, an ×, and tap-to-advance.
//! Clickable elements are `<button>`s (href-less `<a on:click>` is dead on iOS).

use leptos::*;

use crate::services::stories::{self, Bg, Frame, Media, Story};

/// Full-width row of story circles. Mount once on the dashboard.
#[component]
pub fn StoryTray() -> impl IntoView {
    let open = create_rw_signal(None::<&'static Story>);
    let list = stories::visible();

    view! {
        <div style="width: 100%; overflow-x: auto; -webkit-overflow-scrolling: touch;">
            <div style="display: flex; gap: 14px; padding: 4px 2px 8px;">
                {list.into_iter().map(|s| view! { <TrayCircle story=s open=open /> }).collect_view()}
            </div>
        </div>
        <Show when=move || open.get().is_some()>
            <StoryViewer story=open.get().unwrap() on_close=Callback::new(move |_| open.set(None)) />
        </Show>
    }
}

#[component]
fn TrayCircle(story: &'static Story, open: RwSignal<Option<&'static Story>>) -> impl IntoView {
    // r=31 ring in a 68×68 box; the arc length is the unseen fraction of the circle.
    let c = std::f64::consts::PI * 2.0 * 31.0;
    let ring = move || {
        let total = story.frames.len().max(1);
        let unseen = stories::unviewed_count(story);
        let frac = unseen as f64 / total as f64;
        (frac * c, c)
    };
    let badge = story.badge.get();

    view! {
        <button
            on:click=move |_| open.set(Some(story))
            style="flex: 0 0 auto; background: none; border: none; padding: 0; cursor: pointer; \
                   width: 68px; height: 68px; position: relative;"
        >
            // Ring (SVG): faint full track + accent unseen arc, starting at the top.
            <svg width="68" height="68" viewBox="0 0 68 68" style="position: absolute; inset: 0;">
                <circle cx="34" cy="34" r="31" fill="none" stroke="rgba(52,211,153,0.18)" stroke-width="3" />
                {move || {
                    let (dash, total) = ring();
                    (dash > 0.5).then(|| view! {
                        <circle cx="34" cy="34" r="31" fill="none" stroke="#34d399" stroke-width="3"
                            stroke-linecap="round" transform="rotate(-90 34 34)"
                            stroke-dasharray=format!("{dash:.2} {total:.2}") />
                    })
                }}
            </svg>
            // Inner disc with the badge numeral (Anticva, outline-only) on a grey face.
            <div style="position: absolute; inset: 6px; border-radius: 50%; \
                        background: #e5e7eb; \
                        display: flex; align-items: center; justify-content: center;">
                <span style="font-family: 'Anticva', serif; font-size: 40px; line-height: 1; \
                             color: transparent; -webkit-text-stroke: 1.5px #334155; \
                             text-stroke: 1.5px #334155;">{badge}</span>
            </div>
        </button>
    }
}

#[component]
fn StoryViewer(story: &'static Story, on_close: Callback<()>) -> impl IntoView {
    let frames = story.frames;
    let n = frames.len();
    let cur = create_rw_signal(0usize);

    // Mark the currently-shown frame seen (fires on entry and on every advance).
    create_effect(move |_| {
        let i = cur.get();
        if let Some(f) = frames.get(i) {
            stories::mark_viewed(&f.hash());
        }
    });

    let next = move || {
        let i = cur.get_untracked();
        if i + 1 < n {
            cur.set(i + 1);
        } else {
            on_close.call(());
        }
    };
    let prev = move || {
        let i = cur.get_untracked();
        if i > 0 {
            cur.set(i - 1);
        }
    };

    // Portal to <body>: the dashboard lives inside a scrolling app-shell that forms
    // its own stacking context, so a nested fixed overlay can't paint over the bottom
    // nav. Mounting at the document root lets z-index cover the whole app.
    view! {
      <Portal>
        <div style="position: fixed; inset: 0; z-index: 100; background: #000; overflow: hidden;">
            // Current frame body.
            {move || view! { <FrameView frame=frames[cur.get()] /> }}

            // Tap zones: left third = back, rest = next. Transparent buttons.
            <button on:click=move |_| prev()
                style="position: absolute; top: 64px; left: 0; width: 30%; height: calc(100% - 64px); \
                       background: none; border: none; padding: 0; z-index: 3; cursor: pointer;" />
            <button on:click=move |_| next()
                style="position: absolute; top: 64px; left: 30%; width: 70%; height: calc(100% - 64px); \
                       background: none; border: none; padding: 0; z-index: 3; cursor: pointer;" />

            // Progress bars.
            <div style="position: absolute; top: 14px; left: 14px; right: 14px; display: flex; gap: 5px; z-index: 6;">
                {(0..n).map(|i| {
                    let filled = move || i <= cur.get();
                    view! {
                        <i style=move || format!(
                            "flex: 1; height: 3px; border-radius: 2px; background: {};",
                            if filled() { "#fff" } else { "rgba(255,255,255,0.4)" }
                        ) />
                    }
                }).collect_view()}
            </div>

            // Close.
            <button on:click=move |_| on_close.call(())
                style="position: absolute; top: 24px; right: 16px; z-index: 7; background: none; border: none; \
                       color: #fff; font-size: 28px; line-height: 1; cursor: pointer; opacity: 0.95;">
                "×"
            </button>
        </div>
      </Portal>
    }
}

#[component]
fn FrameView(frame: Frame) -> impl IntoView {
    let content = view! {
        <div style="position: absolute; inset: 0; z-index: 2; display: flex; flex-direction: column; \
                    justify-content: flex-end; padding: 30px 28px 92px;">
            <div style=format!("color: {}; font-weight: 700; font-size: 14px; letter-spacing: 0.05em; \
                                text-transform: uppercase; margin-bottom: 10px;", frame.accent)>
                {frame.kicker.get()}
            </div>
            <div style="color: #fff; font-size: 34px; line-height: 1.1; font-weight: 800; margin-bottom: 14px; \
                        text-shadow: 0 2px 18px rgba(0,0,0,0.55);">
                {frame.title.get()}
            </div>
            <div style="color: rgba(255,255,255,0.93); font-size: 18px; line-height: 1.45; \
                        text-shadow: 0 1px 12px rgba(0,0,0,0.6);">
                {frame.body.get()}
            </div>
        </div>
    };

    // Background layer.
    let bg = match frame.bg {
        Bg::Dark => view! {
            <div style="position: absolute; inset: 0; \
                        background: radial-gradient(120% 80% at 50% 15%, #14314a 0%, #0b1622 60%, #070d14 100%);" />
        }.into_view(),
        Bg::Photo(p) => view! {
            <img src=format!("/story-img/{p}")
                style="position: absolute; inset: 0; width: 100%; height: 100%; object-fit: cover;" />
            <div style="position: absolute; inset: 0; background: linear-gradient(180deg, \
                        rgba(0,0,0,0.32) 0%, rgba(0,0,0,0) 25%, rgba(0,0,0,0) 40%, rgba(0,0,0,0.86) 100%);" />
        }.into_view(),
    };

    // Foreground media.
    let media = match frame.media {
        Media::None => ().into_view(),
        Media::Chart => view! {
            <div style="position: absolute; top: 20%; left: 0; right: 0; z-index: 1; display: flex; justify-content: center;">
                <img src="/story-img/weight-chart.svg" style="width: 340px; max-width: 88%; height: auto;" />
            </div>
        }.into_view(),
        Media::Shot(p) => view! {
            <div style="position: absolute; top: 10%; left: 0; right: 0; bottom: 34%; z-index: 1; \
                        display: flex; align-items: center; justify-content: center; padding: 0 28px;">
                <img src=format!("/story-img/{p}")
                    style="max-width: 100%; max-height: 100%; border-radius: 18px; \
                           box-shadow: 0 18px 50px rgba(0,0,0,0.5);" />
            </div>
        }.into_view(),
    };

    view! {
        <div style="position: absolute; inset: 0;">
            {bg}
            {media}
            {content}
        </div>
    }
}
