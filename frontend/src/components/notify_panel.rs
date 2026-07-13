//! Push-notification enable/test button + the reminder-time schedule, extracted
//! from the Settings page so the Dashboard's "Notifications" widget can render the
//! exact same UI. Fully self-contained: owns its signals, reads/writes the schedule
//! from IndexedDB, and derives visibility from the story flags.
//!
//! `hide_check_after_received`: when true, the enable/test button disappears once a
//! notification has been received in the background (the widget wants only the time
//! settings afterwards); Settings passes false to keep the button always available.

use leptos::*;
use serde::Serialize;

use crate::services::{db, i18n::t, push, story};

const IOS_CARD: &str = "background: var(--bulma-scheme-main); border-radius: 12px; overflow: hidden;";
const IOS_SECTION_LABEL: &str =
    "text-transform: uppercase; letter-spacing: 0.02em; padding: 24px 0 8px 16px; margin: 0;";
const IOS_SEPARATOR: &str = "border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;";
const TOGGLE_BASE: &str = "appearance: none; -webkit-appearance: none; width: 51px; min-width: 51px; height: 31px; border-radius: 16px; border: none; padding: 2px; cursor: pointer; margin-left: 12px;";
const TOGGLE_KNOB: &str = "width: 27px; height: 27px; border-radius: 14px; background: var(--bulma-scheme-main); box-shadow: 0 1px 3px rgba(0,0,0,0.2); pointer-events: none;";

#[component]
pub fn NotifyPanel(#[prop(default = false)] hide_check_after_received: bool) -> impl IntoView {
    // Whether a notification has been received in the background — only used to
    // optionally hide the enable/test button (see `hide_check_after_received`).
    // Owned by the push service (per device), not the story.
    let notif_received = push::received_signal();

    // Enable/test button state.
    let push_ver = create_rw_signal(0u32);
    let subscribed = move || {
        push_ver.get();
        push::is_subscribed()
    };
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let push_supported = push::is_supported();

    // The enable/test button is hidden once received only when the caller asked.
    let show_check = move || !(hide_check_after_received && notif_received.get());

    // Schedule signals, hydrated from the stored record below.
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
    // Global kill-switch: rides to the backend with every schedule push.
    let disabled = create_rw_signal(false);
    spawn_local(async move {
        if let Some(val) = db::get::<serde_json::Value>("_sync_meta", "notification_schedule").await {
            let load = |key: &str, on: RwSignal<bool>, time: RwSignal<String>| {
                if let Some(s) = val.get(key) {
                    if let Some(v) = s.get("enabled").and_then(|v| v.as_bool()) {
                        on.set(v);
                    }
                    if let Some(v) = s.get("time").and_then(|v| v.as_str()) {
                        time.set(v.to_string());
                    }
                }
            };
            load("weigh_in", wi_on, wi_time);
            load("breakfast", bf_on, bf_time);
            load("lunch", lu_on, lu_time);
            load("dinner", di_on, di_time);
            load("steps", st_on, st_time);
            if let Some(v) = val.get("disabled").and_then(|v| v.as_bool()) {
                disabled.set(v);
            }
        }
    });

    view! {
        {move || show_check().then(|| {
            let btn = if !push_supported {
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
                                crate::services::diag::note("notif button pressed");
                                spawn_local(async move {
                                    let result = async {
                                        // Re-subscribe before testing: the local flag can go
                                        // stale (Chrome rotates/drops subscriptions); re-subscribing
                                        // is idempotent and refreshes the server's endpoint.
                                        push::subscribe().await?;
                                        crate::services::diag::note("subscribe ok");
                                        let (body, url): (&str, String) = if push::was_received() {
                                            (t("settings.notif_push_plain"), "/".to_string())
                                        } else {
                                            // First test push: carry a receipt code so the
                                            // poll marks this device as "received", and land
                                            // on the dashboard (not the story).
                                            let rand = format!(
                                                "{:04x}",
                                                0x1000 + (js_sys::Math::random() * 61439.0) as u32,
                                            );
                                            (
                                                t("settings.notif_push_task"),
                                                format!("/?ntf=tc.setup.notif.{rand}"),
                                            )
                                        };
                                        push::send_test(body, &url).await
                                    }.await;
                                    match &result {
                                        Ok(()) => crate::services::diag::note("send_test ok"),
                                        Err(e) => crate::services::diag::note(&format!("ERR {e}")),
                                    }
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
            };
            view! {
                <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.notifications")}</p>
                <div style=IOS_CARD>{btn}</div>
            }
        })}

        <p class="is-size-7 has-text-grey-light" style=IOS_SECTION_LABEL>{move || t("settings.schedule")}</p>
            <div style=IOS_CARD>
                // Master «turn off all notifications» toggle. Sending it with the
                // full schedule on every change means the backend never misses it.
                <div style="padding: 12px 16px; display: flex; align-items: center; gap: 12px;">
                    <span class="is-size-6" style="flex: 1;">{move || t("settings.notif_enabled")}</span>
                    // The toggle now reads "notifications ENABLED": ON (green, knob
                    // right) = enabled = `!disabled`. The stored `disabled` flag and
                    // the payload are unchanged — only the UI polarity is inverted.
                    <button attr:data-testid="notif-toggle-disabled"
                        style=move || format!(
                            "{}background: {};",
                            TOGGLE_BASE,
                            if disabled.get() { "var(--bulma-border)" } else { "var(--bulma-success)" }
                        )
                        on:click=move |_| {
                            disabled.update(|v| *v = !*v);
                            save_notification_schedule(disabled, wi_on, bf_on, lu_on, di_on, st_on, wi_time, bf_time, lu_time, di_time, st_time);
                        }>
                        <div style=move || format!(
                            "{}transform: {};",
                            TOGGLE_KNOB,
                            if disabled.get() { "translateX(0)" } else { "translateX(20px)" }
                        )></div>
                    </button>
                </div>

                {move || (!disabled.get()).then(|| view! {
                    <div style=IOS_SEPARATOR></div>
                    <ScheduleRow label="settings.weigh_in" slot_id="weigh_in" enabled=wi_on time=wi_time
                        disabled wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                    <div style=IOS_SEPARATOR></div>
                    <ScheduleRow label="settings.breakfast" slot_id="breakfast" enabled=bf_on time=bf_time
                        disabled wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                    <div style=IOS_SEPARATOR></div>
                    <ScheduleRow label="settings.lunch" slot_id="lunch" enabled=lu_on time=lu_time
                        disabled wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                    <div style=IOS_SEPARATOR></div>
                    <ScheduleRow label="settings.dinner" slot_id="dinner" enabled=di_on time=di_time
                        disabled wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                    <div style=IOS_SEPARATOR></div>
                    <ScheduleRow label="settings.steps" slot_id="steps" enabled=st_on time=st_time
                        disabled wi_on bf_on lu_on di_on st_on wi_time bf_time lu_time di_time st_time />
                })}
            </div>
    }
}

#[component]
fn ScheduleRow(
    label: &'static str,
    slot_id: &'static str,
    enabled: RwSignal<bool>,
    time: RwSignal<String>,
    disabled: RwSignal<bool>,
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
        save_notification_schedule(disabled, wi_on, bf_on, lu_on, di_on, st_on, wi_time, bf_time, lu_time, di_time, st_time);
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
    disabled: bool,
    weigh_in: SlotData,
    breakfast: SlotData,
    lunch: SlotData,
    dinner: SlotData,
    steps: SlotData,
}

fn save_notification_schedule(
    disabled: RwSignal<bool>,
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
        disabled: disabled.get_untracked(),
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
            "disabled": record.disabled,
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
