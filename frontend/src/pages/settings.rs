use leptos::*;
use leptos_router::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::services::{auth, db, story, profile, local, sync, update, i18n::t};

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;";

// TODO: Показывать цели после того, как пользователь освоится с программой и
// выполнит некоторые задания из «Истории».
const SHOW_GOALS: bool = false;

// TODO: Панель диагностических логов (notif/deep-link). Скрыта до тех пор, пока
// снова не понадобится для отладки — вернуть в true, чтобы показать.
const SHOW_DIAG_LOG: bool = false;

/// Compose the «Разработка» view: build version + whether a service worker
/// controls the page, then the page journal (`rn_pj_txt`) — lifecycle and
/// notification-receipt breadcrumbs written by index.html / services::diag.
fn read_diag_log() -> String {
    let Some(win) = web_sys::window() else { return String::new() };
    let ver = js_sys::Reflect::get(win.as_ref(), &"__APP_VERSION__".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| "?".to_string());
    let ctrl = js_sys::Reflect::get(win.navigator().as_ref(), &"serviceWorker".into())
        .ok()
        .and_then(|sw| js_sys::Reflect::get(&sw, &"controller".into()).ok())
        .is_some_and(|c| !c.is_null() && !c.is_undefined());
    let journal = win
        .local_storage()
        .ok()
        .flatten()
        .and_then(|s| s.get_item("rn_pj_txt").ok().flatten())
        .unwrap_or_default();
    format!("ver={ver} ctrl={}\n{journal}", if ctrl { "y" } else { "n" })
}

#[component]
pub fn SettingsPage() -> impl IntoView {
    let navigate = use_navigate();

    // Diagnostics («Разработка»): the notif/deep-link breadcrumb log, refreshable.
    let diag_log = create_rw_signal(read_diag_log());
    let refresh_diag = move |_| diag_log.set(read_diag_log());
    // Live refresh: the SW / page JS write breadcrumbs to localStorage asynchronously
    // (outside Leptos's reactive graph), so poll them into the signal ~1/s. Cleared on
    // unmount so the interval never fires against a disposed signal.
    {
        let interval_id = store_value(None::<i32>);
        let cb = Closure::<dyn Fn()>::new(move || diag_log.set(read_diag_log()));
        if let Some(win) = web_sys::window() {
            if let Ok(id) = win.set_interval_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                1000,
            ) {
                interval_id.set_value(Some(id));
            }
        }
        cb.forget();
        on_cleanup(move || {
            if let (Some(win), Some(id)) = (web_sys::window(), interval_id.get_value()) {
                win.clear_interval_with_handle(id);
            }
        });
    }

    // Расписание уведомлений показываем после того, как пройдена секция
    // «Настроим приложение» (язык + проверка уведомлений) — её задание ведёт
    // сюда настраивать напоминания.
    let story_ver = db::version("story");
    // Danger zone: the "delete diary data" row expands into its two options.
    let show_diary_delete = create_rw_signal(false);


    // Course goal (lose / maintain). The chooser is shown ONLY after the relevant
    // chapter unlocks it; until then it's hidden and the goal defaults to lose.
    use crate::services::profile::CourseGoal;
    let goal = create_rw_signal(profile::get_goal());
    let pick_goal = move |g: CourseGoal| {
        profile::set_goal(g);
        goal.set(g);
    };
    let show_goal = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move { show_goal.set(story::UNLOCK_ALL || story::get_flag(story::COURSE_GOAL_UNLOCKED).await); });
    });


    view! {
        <div style=IOS_BG>
            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 0 8px 0;">{move || t("settings.title")}</h1>

            // ---- Goals row ----
            {SHOW_GOALS.then(|| {
                let nav = navigate.clone();
                view! {
                    <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.goals")}</p>
                    <div style=IOS_CARD>
                        <button
                            attr:data-testid="settings-btn-goals"
                            style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                            on:click=move |_| {
                                let nav = nav.clone();
                                nav("/settings/goals", Default::default());
                            }
                        >
                            <span class="is-size-6">{move || t("settings.goals")}</span>
                            <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                        </button>
                    </div>
                }
            })}

            // ---- Language section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.language")}</p>
            <div style=IOS_CARD>
                <button
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    attr:data-testid="settings-btn-lang-ru"
                    on:click=move |_| {
                        crate::services::i18n::set_lang(crate::services::i18n::Lang::Ru);
                    }
                >
                    <span class="is-size-6">"Русский"</span>
                    {move || if crate::services::i18n::get_lang() == crate::services::i18n::Lang::Ru {
                        view! { <span class="has-text-link is-size-5">"✓"</span> }.into_view()
                    } else {
                        view! {}.into_view()
                    }}
                </button>
                <div style=IOS_SEPARATOR></div>
                <button
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    attr:data-testid="settings-btn-lang-en"
                    on:click=move |_| {
                        crate::services::i18n::set_lang(crate::services::i18n::Lang::En);
                    }
                >
                    <span class="is-size-6">"English"</span>
                    {move || if crate::services::i18n::get_lang() == crate::services::i18n::Lang::En {
                        view! { <span class="has-text-link is-size-5">"✓"</span> }.into_view()
                    } else {
                        view! {}.into_view()
                    }}
                </button>
            </div>

            // Sex / birth year / height are configured in the dashboard «Персона»
            // widget now (see pages::dashboard::PersonaEditor).

            // ---- Course goal section (hidden until the relevant chapter unlocks) ----
            {move || show_goal.get().then(|| view! {
                <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.goal")}</p>
                <div style=IOS_CARD>
                    <button
                        style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                        attr:data-testid="settings-btn-goal-lose"
                        on:click=move |_| pick_goal(CourseGoal::Lose)
                    >
                        <span class="is-size-6">{move || t("settings.goal_lose")}</span>
                        {move || (goal.get() == CourseGoal::Lose).then(|| view! { <span class="has-text-link is-size-5">"✓"</span> })}
                    </button>
                    <div style=IOS_SEPARATOR></div>
                    <button
                        style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                        attr:data-testid="settings-btn-goal-maintain"
                        on:click=move |_| pick_goal(CourseGoal::Maintain)
                    >
                        <span class="is-size-6">{move || t("settings.goal_maintain")}</span>
                        {move || (goal.get() == CourseGoal::Maintain).then(|| view! { <span class="has-text-link is-size-5">"✓"</span> })}
                    </button>
                </div>
                <p class="is-size-7 has-text-grey" style="padding: 8px 16px 0 16px; margin: 0; line-height: 1.45;">
                    {move || t("settings.goal_why")}
                </p>
            })}

            // ---- Privacy row ----
            <div style="margin-top: 24px;">
                <div style=IOS_CARD>
                    <button
                        attr:data-testid="settings-btn-privacy"
                        style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                        on:click={
                            let nav = navigate.clone();
                            move |_| {
                                let nav = nav.clone();
                                nav("/settings/privacy", Default::default());
                            }
                        }
                    >
                        <span class="is-size-6">{move || t("settings.privacy")}</span>
                        <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                    </button>
                </div>
            </div>

            // Notifications + reminder schedule moved to the dashboard «Уведомления»
            // widget (see components::notify_panel::NotifyPanel).

            // ---- Version (manual update) ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.version")}</p>
            <div style=IOS_CARD>
                <div style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; gap: 12px;">
                    <div style="display: flex; align-items: center; gap: 8px; min-width: 0;">
                        {move || update::available().get().then(|| view! {
                            <span style="width: 9px; height: 9px; border-radius: 50%; background: var(--bulma-danger); flex-shrink: 0;"></span>
                        })}
                        <div style="min-width: 0;">
                            <span class="is-size-6">
                                {move || if update::available().get() { t("settings.version_available") } else { t("settings.version_up_to_date") }}
                            </span>
                            <p class="is-size-7 has-text-grey-light" style="margin: 2px 0 0 0;">
                                {move || format!("{} {}", t("settings.version_current"), update::current_version())}
                            </p>
                        </div>
                    </div>
                    {move || update::available().get().then(|| view! {
                        <button
                            attr:data-testid="settings-btn-update"
                            class="button is-link is-small"
                            style="flex-shrink: 0;"
                            on:click=move |_| update::reload()
                        >
                            {move || t("settings.version_update")}
                        </button>
                    })}
                </div>
            </div>

            // ---- Разработка (notif/deep-link diagnostics) ----
            {SHOW_DIAG_LOG.then(|| view! {
                <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.dev")}</p>
                <div style=IOS_CARD>
                    <div style="padding: 12px 16px;">
                        <div style="display: flex; gap: 8px; margin-bottom: 8px;">
                            <button class="button is-small" on:click=refresh_diag>{move || t("settings.dev_refresh")}</button>
                            // Copy the CURRENT log snapshot to the clipboard — selecting text in
                            // the <pre> is impossible because the 1s live refresh re-renders it
                            // (the heartbeat line changes every tick) and drops the selection.
                            <button class="button is-small" on:click=move |_| {
                                let text = read_diag_log();
                                spawn_local(async move {
                                    let clipboard = web_sys::window().unwrap().navigator().clipboard();
                                    let _ = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&text)).await;
                                });
                            }>{move || t("settings.dev_copy")}</button>
                        </div>
                        <pre style="max-height: 44vh; overflow: auto; white-space: pre-wrap; word-break: break-word; font-size: 11px; line-height: 1.4; margin: 0; background: var(--bulma-background); padding: 8px; border-radius: 8px; -webkit-user-select: text; user-select: text;">
                            {move || { let s = diag_log.get(); if s.is_empty() { t("settings.dev_empty").to_string() } else { s } }}
                        </pre>
                    </div>
                </div>
            })}

            // ---- Subscription row → /settings/subscription ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.subscription")}</p>
            <div style=IOS_CARD>
                <button
                    attr:data-testid="settings-btn-subscription"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| {
                            let nav = nav.clone();
                            nav("/settings/subscription", Default::default());
                        }
                    }
                >
                    <span class="is-size-6">{move || t("settings.sub_manage")}</span>
                    <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                </button>
            </div>

            // ---- Account ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.account")}</p>
            <div style=IOS_CARD>
                // Backup phrase → /settings/backup (username-less recovery).
                <button
                    attr:data-testid="settings-btn-backup"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left; border-bottom: 0.5px solid var(--bulma-border-weak);"
                    on:click={
                        let nav = navigate.clone();
                        move |_| { let nav = nav.clone(); nav("/settings/backup", Default::default()); }
                    }
                >
                    <span class="is-size-6">{move || t("settings.backup")}</span>
                    <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                </button>
                // Sign out: clears the whole localStorage, then reloads — which
                // resets the app to the auth screen and the active DB to bootstrap.
                // The per-user IndexedDB stays, so signing back in restores data.
                <button
                    attr:data-testid="settings-btn-logout"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click=move |_| {
                        let win = web_sys::window().unwrap();
                        if win.confirm_with_message(&t("settings.logout_confirm")).unwrap_or(false) {
                            auth::logout();
                            let _ = win.location().reload();
                        }
                    }
                >
                    <span class="is-size-6 has-text-danger">{move || t("settings.logout")}</span>
                </button>
            </div>

            // ---- Danger zone ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.danger_zone")}</p>
            <div style=IOS_CARD>
                // 1. Reset story progress. Soft-reset (flags set false + bumped),
                // then pushed so the reset propagates to the user's other devices.
                <button
                    attr:data-testid="settings-btn-reset-story"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click=move |_| {
                        let win = web_sys::window().unwrap();
                        if win.confirm_with_message(&t("settings.danger_confirm_story")).unwrap_or(false) {
                            spawn_local(async move {
                                local::delete_story_progress().await;
                                sync::push_background();
                            });
                        }
                    }
                >
                    <span class="is-size-6 has-text-danger">{move || t("settings.danger_reset_story")}</span>
                </button>

                <div style=IOS_SEPARATOR></div>

                // 2. Delete diary data — expands into two options.
                <button
                    attr:data-testid="settings-btn-delete-diary"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click=move |_| show_diary_delete.update(|v| *v = !*v)
                >
                    <span class="is-size-6 has-text-danger">{move || t("settings.danger_delete_diary")}</span>
                    <span style="color: var(--bulma-text-weak); font-size: 18px;">
                        {move || if show_diary_delete.get() { "⌄" } else { "›" }}
                    </span>
                </button>

                {move || show_diary_delete.get().then(|| view! {
                    <div style=IOS_SEPARATOR></div>
                    <button
                        attr:data-testid="settings-btn-delete-diary-old"
                        style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px 12px 32px; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                        on:click=move |_| {
                            let win = web_sys::window().unwrap();
                            if win.confirm_with_message(&t("settings.danger_confirm_old")).unwrap_or(false) {
                                let cutoff = (chrono::Local::now().date_naive() - chrono::Duration::days(365))
                                    .format("%Y-%m-%d").to_string();
                                spawn_local(async move {
                                    local::delete_diary_data(Some(&cutoff)).await;
                                    sync::push_background();
                                });
                            }
                        }
                    >
                        <span class="is-size-6 has-text-danger">{move || t("settings.danger_delete_old")}</span>
                    </button>

                    <div style=IOS_SEPARATOR></div>
                    <button
                        attr:data-testid="settings-btn-delete-diary-all"
                        style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px 12px 32px; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                        on:click=move |_| {
                            let win = web_sys::window().unwrap();
                            if win.confirm_with_message(&t("settings.danger_confirm_all")).unwrap_or(false) {
                                spawn_local(async move {
                                    local::delete_diary_data(None).await;
                                    sync::push_background();
                                });
                            }
                        }
                    >
                        <span class="is-size-6 has-text-danger">{move || t("settings.danger_delete_all")}</span>
                    </button>
                })}
            </div>

            <div style="height: 40px;"></div>
        </div>
    }
}


