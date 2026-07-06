use leptos::*;
use leptos_router::*;
use serde::Serialize;

use crate::services::{auth, push, db, story, profile, local, sync, update, i18n::t};
use crate::services::profile::Sex;

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;";

// TODO: Показывать цели после того, как пользователь освоится с программой и
// выполнит некоторые задания из «Истории».
const SHOW_GOALS: bool = false;

#[component]
pub fn SettingsPage() -> impl IntoView {
    let navigate = use_navigate();

    // Расписание уведомлений показываем после того, как пройдена секция
    // «Настроим приложение» (язык + проверка уведомлений) — её задание ведёт
    // сюда настраивать напоминания.
    let story_ver = db::version("story");
    let show_schedule = create_rw_signal(false);
    // The meal/steps reminders are unlocked later, in the "first food entries"
    // section; until then only the weigh-in reminder is shown.
    let show_meal_reminders = create_rw_signal(false);
    // Danger zone: the "delete diary data" row expands into its two options.
    let show_diary_delete = create_rw_signal(false);
    create_effect(move |_| {
        story_ver.get();
        spawn_local(async move {
            show_schedule.set(
                story::get_flag(story::LANGUAGE_CONFIGURED).await
                    && story::get_flag(story::NOTIFICATION_RECEIVED).await,
            );
            show_meal_reminders.set(story::get_flag(story::MEAL_REMINDERS_UNLOCKED).await);
        });
    });

    // Biological sex (localStorage). Picking it also marks the setup-section
    // story task. Reactive signal so the ✓ updates immediately.
    let sex = create_rw_signal(profile::get_sex());
    let pick_sex = move |s: Sex| {
        profile::set_sex(s);
        sex.set(Some(s));
        spawn_local(async { story::set_flag(story::SEX_SELECTED, true).await; });
    };

    // Height (cm, localStorage) + the latest logged weight → BMI. Lets us read how
    // much of the body mass is fat. Saving a valid height also marks the
    // setup-section `height` story task (like sex/birth_year).
    let height = create_rw_signal(profile::get_height_cm());
    let set_height = move |raw: String| {
        let cm = raw.trim().replace(',', ".").parse::<f64>().ok().filter(|h| *h > 0.0);
        profile::set_height_cm(cm.unwrap_or(0.0));
        height.set(cm);
        if cm.is_some() {
            spawn_local(async { story::set_flag(story::HEIGHT_SET, true).await; });
        }
    };
    // Year of birth (synced profile). Saving a valid year also marks the
    // setup-section `age` story task. Only a valid year completes the task —
    // a cleared/garbage entry must not.
    let birth_year = create_rw_signal(profile::get_birth_year());
    let set_birth_year = move |raw: String| {
        let y = raw.trim().parse::<i32>().ok().filter(|y| (1900..=2026).contains(y));
        profile::set_birth_year(y.unwrap_or(0));
        birth_year.set(y);
        if y.is_some() {
            spawn_local(async { story::set_flag(story::BIRTH_YEAR_SET, true).await; });
        }
    };

    let latest_weight = create_rw_signal(None::<f64>);
    let weight_ver = db::version("weight_entries");
    create_effect(move |_| {
        weight_ver.get();
        spawn_local(async move {
            latest_weight.set(local::list_weight_entries().await.last().map(|e| e.weight_kg));
        });
    });
    let bmi = move || height.get().zip(latest_weight.get()).and_then(|(h, w)| profile::bmi(w, h));

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
        spawn_local(async move { show_goal.set(story::get_flag(story::COURSE_GOAL_UNLOCKED).await); });
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

            // ---- Sex section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.sex")}</p>
            <div style=IOS_CARD>
                <button
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    attr:data-testid="settings-btn-sex-female"
                    on:click=move |_| pick_sex(Sex::Female)
                >
                    <span class="is-size-6">{move || t("settings.sex_female")}</span>
                    {move || (sex.get() == Some(Sex::Female)).then(|| view! { <span class="has-text-link is-size-5">"✓"</span> })}
                </button>
                <div style=IOS_SEPARATOR></div>
                <button
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    attr:data-testid="settings-btn-sex-male"
                    on:click=move |_| pick_sex(Sex::Male)
                >
                    <span class="is-size-6">{move || t("settings.sex_male")}</span>
                    {move || (sex.get() == Some(Sex::Male)).then(|| view! { <span class="has-text-link is-size-5">"✓"</span> })}
                </button>
            </div>
            <p class="is-size-7 has-text-grey" style="padding: 8px 16px 0 16px; margin: 0; line-height: 1.45;">
                {move || t("settings.sex_why")}
            </p>

            // ---- Birth year section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.birth_year")}</p>
            <div style=IOS_CARD>
                <div style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between;">
                    <span class="is-size-6">{move || t("settings.birth_year_label")}</span>
                    <input
                        type="number" inputmode="numeric" min="1900" step="1"
                        attr:data-testid="settings-input-birth-year"
                        style="appearance: none; -webkit-appearance: none; border: none; background: none; font: inherit; text-align: right; width: 96px; color: inherit;"
                        prop:value=move || birth_year.get().map(|y| y.to_string()).unwrap_or_default()
                        on:change=move |ev| set_birth_year(event_target_value(&ev))
                    />
                </div>
            </div>
            <p class="is-size-7 has-text-grey" style="padding: 8px 16px 0 16px; margin: 0; line-height: 1.45;">
                {move || t("settings.birth_year_why")}
            </p>

            // ---- Height / BMI section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.height")}</p>
            <div style=IOS_CARD>
                <div style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between;">
                    <span class="is-size-6">{move || t("settings.height_label")}</span>
                    <input
                        type="number" inputmode="numeric" min="0" step="1"
                        attr:data-testid="settings-input-height"
                        style="appearance: none; -webkit-appearance: none; border: none; background: none; font: inherit; text-align: right; width: 96px; color: inherit;"
                        prop:value=move || height.get().map(|h| (h.round() as i64).to_string()).unwrap_or_default()
                        on:change=move |ev| set_height(event_target_value(&ev))
                    />
                </div>
            </div>
            <p class="is-size-7 has-text-grey" style="padding: 8px 16px 0 16px; margin: 0; line-height: 1.45;">
                {move || match bmi() {
                    Some(v) => t("settings.bmi").replace("{n}", &format!("{v:.1}")),
                    None => t("settings.height_why").to_string(),
                }}
            </p>

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

            // ---- Notifications section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.notifications")}</p>
            <div style=IOS_CARD>
                {
                    let push_ver = create_rw_signal(0u32);
                    let subscribed = move || { push_ver.get(); push::is_subscribed() };
                    let loading = create_rw_signal(false);
                    let error = create_rw_signal(None::<String>);
                    let push_supported = push::is_supported();
                    if !push_supported {
                        view! {
                            <div style="padding: 12px 16px;">
                                <span class="is-size-7 has-text-grey-light">{move || t("settings.push_not_supported")}</span>
                            </div>
                        }.into_view()
                    } else {
                        view! {
                            <div style="padding: 12px 16px;">
                                <button
                                    attr:data-testid="settings-btn-notifications"
                                    class="button is-link is-fullwidth"
                                    prop:disabled=move || loading.get()
                                    on:click=move |_| {
                                        loading.set(true);
                                        error.set(None);
                                        spawn_local(async move {
                                            let result = async {
                                                // Always (re)subscribe before testing. The local
                                                // `push_subscribed` flag can go stale (Chrome rotates
                                                // or drops subscriptions), leaving the server with a
                                                // dead endpoint. Re-subscribing is idempotent and
                                                // refreshes the server's record; it also surfaces a
                                                // real "permission denied" instead of silently no-op.
                                                push::subscribe().await?;
                                                // The client picks the deep-link based on story
                                                // progress: while the setup section's check task
                                                // is pending, tapping the notification opens that
                                                // section and completes the task; once it's done,
                                                // the notification just confirms it works.
                                                let setup_done = story::get_flag(story::LANGUAGE_CONFIGURED).await
                                                    && story::get_flag(story::NOTIFICATION_RECEIVED).await;
                                                let (body, url) = if setup_done {
                                                    (t("settings.notif_push_plain"), "/")
                                                } else {
                                                    (t("settings.notif_push_task"), "/story/setup?notif=1")
                                                };
                                                push::send_test(body, url).await
                                            }.await;
                                            if let Err(e) = result {
                                                leptos::logging::error!("notifications: {}", e);
                                                error.set(Some(e));
                                            }
                                            push_ver.update(|v| *v += 1);
                                            loading.set(false);
                                        });
                                    }
                                >
                                    {move || if loading.get() {
                                        t("settings.sending")
                                    } else if subscribed() {
                                        t("settings.notif_check")
                                    } else {
                                        t("settings.notif_enable_check")
                                    }}
                                </button>
                                {move || error.get().map(|e| view! {
                                    <p class="is-size-7 has-text-danger" style="margin-top: 8px;">{e}</p>
                                })}
                            </div>
                        }.into_view()
                    }
                }
            </div>

            // ---- Notification schedule section ----
            {move || show_schedule.get().then(|| view! {
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.schedule")}</p>
            <div style=IOS_CARD>
                {
                    let wi_on = create_rw_signal(false);
                    let wi_time = create_rw_signal("07:00".to_string());
                    let bf_on = create_rw_signal(false);
                    let bf_time = create_rw_signal("09:00".to_string());
                    let lu_on = create_rw_signal(false);
                    let lu_time = create_rw_signal("13:00".to_string());
                    let di_on = create_rw_signal(false);
                    let di_time = create_rw_signal("19:00".to_string());
                    let st_on = create_rw_signal(false);
                    let st_time = create_rw_signal("22:00".to_string());

                    spawn_local(async move {
                        if let Some(val) = db::get::<serde_json::Value>("_sync_meta", "notification_schedule").await {
                            if let Some(s) = val.get("weigh_in") {
                                if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) { wi_on.set(v); }
                                if let Some(v) = s.get("time").and_then(|v| v.as_str()) { wi_time.set(v.to_string()); }
                            }
                            if let Some(s) = val.get("breakfast") {
                                if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) { bf_on.set(v); }
                                if let Some(v) = s.get("time").and_then(|v| v.as_str()) { bf_time.set(v.to_string()); }
                            }
                            if let Some(s) = val.get("lunch") {
                                if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) { lu_on.set(v); }
                                if let Some(v) = s.get("time").and_then(|v| v.as_str()) { lu_time.set(v.to_string()); }
                            }
                            if let Some(s) = val.get("dinner") {
                                if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) { di_on.set(v); }
                                if let Some(v) = s.get("time").and_then(|v| v.as_str()) { di_time.set(v.to_string()); }
                            }
                            if let Some(s) = val.get("steps") {
                                if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) { st_on.set(v); }
                                if let Some(v) = s.get("time").and_then(|v| v.as_str()) { st_time.set(v.to_string()); }
                            }
                        }
                    });

                    view! {
                        <ScheduleRow label="settings.weigh_in" slot_id="weigh_in" enabled=wi_on time=wi_time
                            wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                        {move || show_meal_reminders.get().then(|| view! {
                            <div style=IOS_SEPARATOR></div>
                            <ScheduleRow label="settings.breakfast" slot_id="breakfast" enabled=bf_on time=bf_time
                                wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                            <div style=IOS_SEPARATOR></div>
                            <ScheduleRow label="settings.lunch" slot_id="lunch" enabled=lu_on time=lu_time
                                wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                            <div style=IOS_SEPARATOR></div>
                            <ScheduleRow label="settings.dinner" slot_id="dinner" enabled=di_on time=di_time
                                wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                            <div style=IOS_SEPARATOR></div>
                            <ScheduleRow label="settings.steps" slot_id="steps" enabled=st_on time=st_time
                                wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                        })}
                    }
                }
            </div>
            })}

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

const TOGGLE_BASE: &str = "appearance: none; -webkit-appearance: none; width: 51px; min-width: 51px; height: 31px; border-radius: 16px; border: none; padding: 2px; cursor: pointer; margin-left: 12px;";
const TOGGLE_KNOB: &str = "width: 27px; height: 27px; border-radius: 14px; background: var(--bulma-scheme-main); box-shadow: 0 1px 3px rgba(0,0,0,0.2); pointer-events: none;";

#[component]
fn ScheduleRow(
    label: &'static str,
    slot_id: &'static str,
    enabled: RwSignal<bool>,
    time: RwSignal<String>,
    wi_on: RwSignal<bool>,
    bf_on: RwSignal<bool>,
    lu_on: RwSignal<bool>,
    di_on: RwSignal<bool>,
    st_on: RwSignal<bool>,
    wi_time: RwSignal<String>,
    bf_time: RwSignal<String>,
    lu_time: RwSignal<String>,
    di_time: RwSignal<String>,
    st_time: RwSignal<String>,
) -> impl IntoView {
    let save = move || {
        save_notification_schedule(wi_on, bf_on, lu_on, di_on, st_on, wi_time, bf_time, lu_time, di_time, st_time);
    };

    view! {
        <div style="padding: 12px 16px; display: flex; align-items: center; gap: 12px;">
            <span class="is-size-6" style="flex: 1;">{move || t(label)}</span>
            <input
                type="time"
                attr:data-testid=format!("schedule-time-{}", slot_id)
                style="background: var(--bulma-background); border: none; border-radius: 8px; padding: 4px 8px; color: var(--bulma-link); width: 90px; text-align: center;"
                class="is-size-6"
                prop:value=move || time.get()
                on:change=move |ev| {
                    time.set(event_target_value(&ev));
                    save();
                }
            />
            <button
                attr:data-testid=format!("schedule-toggle-{}", slot_id)
                style=move || format!(
                    "{}background: {};",
                    TOGGLE_BASE,
                    if enabled.get() { "var(--bulma-success)" } else { "var(--bulma-border)" }
                )
                on:click=move |_| {
                    enabled.update(|v| *v = !*v);
                    save();
                    // The act of enabling a reminder is a story milestone: record
                    // it in the story DB (an event), independent of the schedule's
                    // later on/off state.
                    if enabled.get_untracked() {
                        let flag = match slot_id {
                            "weigh_in" => Some(story::WEIGH_IN_REMINDER),
                            "steps" => Some(story::STEPS_REMINDER),
                            _ => None,
                        };
                        if let Some(flag) = flag {
                            spawn_local(async move {
                                story::set_flag(flag, true).await;
                            });
                        }
                    }
                }
            >
                <div style=move || format!(
                    "{}transform: {};",
                    TOGGLE_KNOB,
                    if enabled.get() { "translateX(20px)" } else { "translateX(0)" }
                )></div>
            </button>
        </div>
    }
}

#[derive(Serialize)]
struct SlotData {
    enabled: bool,
    time: String,
}

#[derive(Serialize)]
struct ScheduleRecord {
    key: String,
    weigh_in: SlotData,
    breakfast: SlotData,
    lunch: SlotData,
    dinner: SlotData,
    steps: SlotData,
}

fn save_notification_schedule(
    wi_on: RwSignal<bool>,
    bf_on: RwSignal<bool>,
    lu_on: RwSignal<bool>,
    di_on: RwSignal<bool>,
    st_on: RwSignal<bool>,
    wi_time: RwSignal<String>,
    bf_time: RwSignal<String>,
    lu_time: RwSignal<String>,
    di_time: RwSignal<String>,
    st_time: RwSignal<String>,
) {
    let record = ScheduleRecord {
        key: "notification_schedule".to_string(),
        weigh_in: SlotData { enabled: wi_on.get_untracked(), time: wi_time.get_untracked() },
        breakfast: SlotData { enabled: bf_on.get_untracked(), time: bf_time.get_untracked() },
        lunch: SlotData { enabled: lu_on.get_untracked(), time: lu_time.get_untracked() },
        dinner: SlotData { enabled: di_on.get_untracked(), time: di_time.get_untracked() },
        steps: SlotData { enabled: st_on.get_untracked(), time: st_time.get_untracked() },
    };
    spawn_local(async move {
        db::put("_sync_meta", &record).await;
        let offset = -(js_sys::Date::new_0().get_timezone_offset() as i32);
        let payload = serde_json::json!({
            "utc_offset_minutes": offset,
            "weigh_in": {"enabled": record.weigh_in.enabled, "time": record.weigh_in.time},
            "breakfast": {"enabled": record.breakfast.enabled, "time": record.breakfast.time},
            "lunch": {"enabled": record.lunch.enabled, "time": record.lunch.time},
            "dinner": {"enabled": record.dinner.enabled, "time": record.dinner.time},
            "steps": {"enabled": record.steps.enabled, "time": record.steps.time},
        });
        if let Err(e) = push::sync_notification_schedule(payload).await {
            leptos::logging::error!("sync schedule: {}", e);
        }
    });
}

