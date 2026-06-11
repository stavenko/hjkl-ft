use leptos::*;
use leptos_router::*;
use serde::Serialize;

use crate::services::{push, db, i18n::t};

const IOS_BG: &str = "background: var(--bulma-background); min-height: 100vh; padding: 16px; margin: -0.75rem;";
const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str = "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;";

#[component]
pub fn SettingsPage() -> impl IntoView {
    let navigate = use_navigate();

    view! {
        <div style=IOS_BG>
            <h1 class="is-size-1 has-text-weight-bold" style="margin: 0 0 8px 0;">{move || t("settings.title")}</h1>

            // ---- Goals row ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.goals")}</p>
            <div style=IOS_CARD>
                <button
                    attr:data-testid="settings-btn-goals"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; display: flex; align-items: center; justify-content: space-between; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click={
                        let nav = navigate.clone();
                        move |_| {
                            let nav = nav.clone();
                            nav("/settings/goals", Default::default());
                        }
                    }
                >
                    <span class="is-size-6">{move || t("settings.goals")}</span>
                    <span style="color: var(--bulma-text-weak); font-size: 18px;">"›"</span>
                </button>
            </div>

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
                    let push_subscribed = move || { push_ver.get(); push::is_subscribed() };
                    let push_loading = create_rw_signal(false);
                    let push_supported = push::is_supported();
                    view! {
                        <div style="padding: 12px 16px; display: flex; align-items: center; justify-content: space-between;">
                            <span class="is-size-6">{move || t("settings.notifications")}</span>
                            {if !push_supported {
                                view! {
                                    <span class="is-size-7 has-text-grey-light">{move || t("settings.push_not_supported")}</span>
                                }.into_view()
                            } else {
                                view! {
                                    <div style="display: flex; align-items: center; gap: 8px;">
                                        <button
                                            class=move || if push_subscribed() {
                                                "button is-danger is-size-7"
                                            } else {
                                                "button is-link is-size-7"
                                            }
                                            style="border-radius: 16px; padding: 6px 14px;"
                                            prop:disabled=move || push_loading.get()
                                            on:click=move |_| {
                                                push_loading.set(true);
                                                let is_on = push_subscribed();
                                                spawn_local(async move {
                                                    if is_on {
                                                        match push::unsubscribe().await {
                                                            Ok(()) => {},
                                                            Err(e) => leptos::logging::error!("push unsubscribe: {}", e),
                                                        }
                                                    } else {
                                                        match push::subscribe().await {
                                                            Ok(()) => {},
                                                            Err(e) => leptos::logging::error!("push subscribe: {}", e),
                                                        }
                                                    }
                                                    push_ver.update(|v| *v += 1);
                                                    push_loading.set(false);
                                                });
                                            }
                                        >
                                            {move || if push_subscribed() {
                                                t("settings.push_disable")
                                            } else {
                                                t("settings.push_enable")
                                            }}
                                        </button>
                                    </div>
                                }.into_view()
                            }}
                        </div>
                    }
                }
            </div>

            // ---- Notification schedule section ----
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
                        }
                    });

                    view! {
                        <ScheduleRow label="settings.weigh_in" slot_id="weigh_in" enabled=wi_on time=wi_time
                            wi_on bf_on lu_on di_on wi_time bf_time lu_time di_time />
                        <div style=IOS_SEPARATOR></div>
                        <ScheduleRow label="settings.breakfast" slot_id="breakfast" enabled=bf_on time=bf_time
                            wi_on bf_on lu_on di_on wi_time bf_time lu_time di_time />
                        <div style=IOS_SEPARATOR></div>
                        <ScheduleRow label="settings.lunch" slot_id="lunch" enabled=lu_on time=lu_time
                            wi_on bf_on lu_on di_on wi_time bf_time lu_time di_time />
                        <div style=IOS_SEPARATOR></div>
                        <ScheduleRow label="settings.dinner" slot_id="dinner" enabled=di_on time=di_time
                            wi_on bf_on lu_on di_on wi_time bf_time lu_time di_time />
                    }
                }
            </div>

            // ---- Data section ----
            <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.data")}</p>
            <div style=IOS_CARD>
                <button
                    attr:data-testid="settings-btn-wipe-all"
                    style="appearance: none; -webkit-appearance: none; width: 100%; padding: 12px 16px; cursor: pointer; border: none; background: none; font: inherit; text-align: left;"
                    on:click=move |_| {
                        let win = web_sys::window().unwrap();
                        if win.confirm_with_message(&t("settings.wipe_confirm")).unwrap_or(false) {
                            spawn_local(async move {
                                crate::services::db::wipe_all().await;
                            });
                        }
                    }
                >
                    <span class="is-size-6 has-text-danger">{move || t("settings.wipe_all")}</span>
                </button>
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
    wi_time: RwSignal<String>,
    bf_time: RwSignal<String>,
    lu_time: RwSignal<String>,
    di_time: RwSignal<String>,
) -> impl IntoView {
    let save = move || {
        save_notification_schedule(wi_on, bf_on, lu_on, di_on, wi_time, bf_time, lu_time, di_time);
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
}

fn save_notification_schedule(
    wi_on: RwSignal<bool>,
    bf_on: RwSignal<bool>,
    lu_on: RwSignal<bool>,
    di_on: RwSignal<bool>,
    wi_time: RwSignal<String>,
    bf_time: RwSignal<String>,
    lu_time: RwSignal<String>,
    di_time: RwSignal<String>,
) {
    let record = ScheduleRecord {
        key: "notification_schedule".to_string(),
        weigh_in: SlotData { enabled: wi_on.get_untracked(), time: wi_time.get_untracked() },
        breakfast: SlotData { enabled: bf_on.get_untracked(), time: bf_time.get_untracked() },
        lunch: SlotData { enabled: lu_on.get_untracked(), time: lu_time.get_untracked() },
        dinner: SlotData { enabled: di_on.get_untracked(), time: di_time.get_untracked() },
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
        });
        if let Err(e) = push::sync_notification_schedule(payload).await {
            leptos::logging::error!("sync schedule: {}", e);
        }
    });
}
