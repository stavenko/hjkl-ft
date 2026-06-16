use leptos::*;
use leptos_router::*;
use wasm_bindgen::JsCast;

use crate::services::{db, i18n::t, story};

const PAGE_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 0; margin: -0.75rem;";
const CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";

#[component]
pub fn StorySetupPage() -> impl IntoView {
    let navigate = use_navigate();

    // Reload task flags whenever the story store changes.
    let story_ver = db::version("story");
    let lang_done = create_rw_signal(false);
    let notif_done = create_rw_signal(false);
    let sex_done = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            lang_done.set(story::get_flag(story::LANGUAGE_CONFIGURED).await);
            notif_done.set(story::get_flag(story::NOTIFICATION_RECEIVED).await);
            sex_done.set(story::get_flag(story::SEX_SELECTED).await);
        });
    });

    let toggle_lang = move |_| {
        let new_val = !lang_done.get_untracked();
        lang_done.set(new_val);
        spawn_local(async move {
            story::set_flag(story::LANGUAGE_CONFIGURED, new_val).await;
        });
    };

    // Opened by tapping the test push (URL carries `?notif_test=1`): mark the
    // notification task done, drop the marker without leaving the route, and
    // scroll down to the tasks block.
    let win = web_sys::window().expect("no window");
    let search = js_sys::Reflect::get(&win, &"location".into())
        .ok()
        .and_then(|loc| js_sys::Reflect::get(&loc, &"search".into()).ok())
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    if search.contains("notif=1") {
        spawn_local(async move {
            story::set_flag(story::NOTIFICATION_RECEIVED, true).await;
        });
        if let Ok(history) = js_sys::Reflect::get(&win, &"history".into()) {
            if let Ok(func) = js_sys::Reflect::get(&history, &"replaceState".into())
                .and_then(|f| f.dyn_into::<js_sys::Function>().map_err(|_| wasm_bindgen::JsValue::NULL))
            {
                let args = js_sys::Array::of3(
                    &wasm_bindgen::JsValue::NULL,
                    &wasm_bindgen::JsValue::from_str(""),
                    &wasm_bindgen::JsValue::from_str("/story/setup"),
                );
                let _ = js_sys::Reflect::apply(&func, &history, &args);
            }
        }
        request_animation_frame(move || {
            if let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("setup-tasks"))
            {
                if let Ok(func) = js_sys::Reflect::get(&el, &"scrollIntoView".into())
                    .ok()
                    .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                    .ok_or(())
                {
                    let opts = js_sys::Object::new();
                    let _ = js_sys::Reflect::set(&opts, &"behavior".into(), &"smooth".into());
                    let _ = js_sys::Reflect::set(&opts, &"block".into(), &"start".into());
                    let _ = js_sys::Reflect::apply(&func, &el, &js_sys::Array::of1(&opts));
                }
            }
        });
    }

    let sections = ["story.setup.s_story", "story.setup.s_diary", "story.setup.s_recipes", "story.setup.s_settings"];
    let section_items = sections.iter().map(|&key| view! {
        <li class="is-size-6" style="margin-bottom: 6px; line-height: 1.5;">
            {move || match t(key).split_once(" \u{2014} ") {
                Some((name, rest)) => view! { <strong>{name}</strong>" \u{2014} "{rest} }.into_view(),
                None => view! { {t(key)} }.into_view(),
            }}
        </li>
    }).collect_view();

    view! {
        <div style=PAGE_BG>
            <div style="display: flex; align-items: center; padding: 12px 16px;">
                <button
                    style="appearance: none; -webkit-appearance: none; border: none; background: none; cursor: pointer; padding: 4px; font: inherit;"
                    class="is-size-5"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/", Default::default())
                    }
                >
                    <span class="has-text-link">{move || t("common.back")}</span>
                </button>
            </div>

            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 16px 16px 16px;">{move || t("story.ch1.setup")}</h1>

            <div style="padding: 0 16px 8px 16px;">
                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.setup.intro")}</p>
                <ul style="margin: 0 0 14px 0; padding-left: 22px; list-style: disc;">
                    {section_items}
                </ul>

                <p class="is-size-6" style="line-height: 1.55; margin: 0 0 8px 0;">{move || t("story.setup.task_intro")}</p>
                <ul style="margin: 0 0 14px 0; padding-left: 22px; list-style: disc;">
                    <li class="is-size-6" style="margin-bottom: 6px; line-height: 1.5;">{move || t("story.setup.check_lang_line")}</li>
                    <li class="is-size-6" style="line-height: 1.5;">{move || t("story.setup.check_notif_line")}</li>
                </ul>

                <p class="is-size-6" style="line-height: 1.55; margin: 0;">{move || t("story.setup.instructions")}</p>
            </div>

            // ---- Tasks ----
            <div id="setup-tasks" style="padding: 16px 16px 0 16px; scroll-margin-top: 12px;">
                <p class="is-size-7 has-text-grey-light" style="text-transform: uppercase; letter-spacing: 0.02em; margin: 0 0 8px 4px;">
                    {move || t("story.setup.task_label")}
                </p>
                <div style=CARD>
                    <label style="display: flex; align-items: center; gap: 12px; padding: 14px 16px; cursor: pointer;">
                        <input type="checkbox"
                            attr:data-testid="story-setup-language-configured"
                            style="width: 22px; height: 22px; accent-color: var(--bulma-link);"
                            prop:checked=move || lang_done.get()
                            on:change=toggle_lang
                        />
                        <span class="is-size-6 has-text-weight-semibold">{move || t("story.setup.checkbox_lang")}</span>
                    </label>
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                    <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                        {move || if notif_done.get() {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{2705}"</span> }.into_view()
                        } else {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{23f3}"</span> }.into_view()
                        }}
                        <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                            {move || if notif_done.get() { t("story.setup.notif_status_done") } else { t("story.setup.notif_status_pending") }}
                        </span>
                    </div>
                    <div style="border-bottom: 0.5px solid var(--bulma-border-weak);"></div>
                    // Sex task: completed once the user picks their sex in settings.
                    <div style="display: flex; align-items: center; gap: 12px; padding: 14px 16px;">
                        {move || if sex_done.get() {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{2705}"</span> }.into_view()
                        } else {
                            view! { <span style="font-size: 22px; width: 22px; text-align: center;">"\u{23f3}"</span> }.into_view()
                        }}
                        <span class="is-size-6 has-text-weight-semibold" style="flex: 1;">
                            {move || if sex_done.get() { t("story.setup.sex_status_done") } else { t("story.setup.sex_status_pending") }}
                        </span>
                    </div>
                </div>

                {move || (lang_done.get() && notif_done.get()).then(|| view! {
                    <p class="is-size-6 has-text-weight-semibold has-text-success" style="margin-top: 16px;">
                        {move || t("story.setup.next_unlocked")}
                    </p>
                })}

                <button
                    class="button is-link is-fullwidth is-medium"
                    style="margin-top: 16px;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| nav("/settings", Default::default())
                    }
                >
                    {move || t("story.setup.open_settings")}
                </button>
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}
