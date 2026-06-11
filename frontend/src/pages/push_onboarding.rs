use leptos::*;
use serde::Serialize;

use crate::services::{db, i18n::t, push};

const TOGGLE_BASE: &str = "appearance: none; -webkit-appearance: none; width: 51px; min-width: 51px; height: 31px; border-radius: 16px; border: none; padding: 2px; cursor: pointer; margin-left: 12px;";
const TOGGLE_KNOB: &str = "width: 27px; height: 27px; border-radius: 14px; background: var(--bulma-scheme-main); box-shadow: 0 1px 3px rgba(0,0,0,0.2); pointer-events: none;";

#[component]
pub fn PushOnboarding(on_done: Callback<()>) -> impl IntoView {
    let step = create_rw_signal(1u8);
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    let allow = move |_| {
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match push::subscribe().await {
                Ok(()) => step.set(2),
                Err(e) => {
                    error.set(Some(e));
                    loading.set(false);
                }
            }
        });
    };

    let skip = move |_| {
        push::dismiss_onboarding();
        on_done.call(());
    };

    let wi_on = create_rw_signal(true);
    let wi_time = create_rw_signal("07:00".to_string());
    let bf_on = create_rw_signal(true);
    let bf_time = create_rw_signal("09:00".to_string());
    let lu_on = create_rw_signal(true);
    let lu_time = create_rw_signal("13:00".to_string());
    let di_on = create_rw_signal(true);
    let di_time = create_rw_signal("19:00".to_string());

    let done_schedule = move |_| {
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
            on_done.call(());
        });
    };

    let skip_schedule = move |_| {
        on_done.call(());
    };

    view! {
        <div
            attr:data-testid="push-onboarding"
            style="display: flex; flex-direction: column; align-items: center; justify-content: center; min-height: 100vh; padding: 32px; background: var(--bulma-scheme-main); text-align: center;"
        >
            {move || if step.get() == 1 {
                view! {
                    <div attr:data-testid="push-onboarding-step-1">
                        <div style="font-size: 64px; margin-bottom: 24px;">"🔔"</div>

                        <h1
                            attr:data-testid="push-onboarding-title"
                            class="is-size-3 has-text-weight-bold"
                            style="margin: 0 0 16px 0;"
                        >
                            {move || t("push_onboarding.title")}
                        </h1>

                        <p
                            attr:data-testid="push-onboarding-description"
                            class="is-size-6 has-text-grey-light"
                            style="max-width: 320px; line-height: 1.5; margin: 0 0 32px 0;"
                        >
                            {move || t("push_onboarding.description")}
                        </p>

                        {move || error.get().map(|e| view! {
                            <p class="is-size-7 has-text-danger" style="margin-bottom: 16px;">{e}</p>
                        })}

                        <button
                            attr:data-testid="push-onboarding-btn-allow"
                            class="button is-link is-size-5 has-text-weight-semibold"
                            style="border: none; border-radius: 12px; padding: 14px 32px; cursor: pointer; width: 100%; max-width: 320px;"
                            prop:disabled=move || loading.get()
                            on:click=allow
                        >
                            {move || t("push_onboarding.allow")}
                        </button>

                        <button
                            attr:data-testid="push-onboarding-btn-skip"
                            class="has-text-grey-light is-size-6"
                            style="background: none; border: none; cursor: pointer; margin-top: 16px; padding: 8px;"
                            on:click=skip
                        >
                            {move || t("push_onboarding.skip")}
                        </button>
                    </div>
                }.into_view()
            } else {
                view! {
                    <div attr:data-testid="push-onboarding-step-2" style="width: 100%; max-width: 360px;">
                        <div style="font-size: 64px; margin-bottom: 24px;">"⏰"</div>

                        <h1 class="is-size-3 has-text-weight-bold" style="margin: 0 0 8px 0;">
                            {move || t("push_onboarding.schedule_title")}
                        </h1>

                        <p class="is-size-6 has-text-grey-light" style="line-height: 1.5; margin: 0 0 24px 0;">
                            {move || t("push_onboarding.schedule_description")}
                        </p>

                        <div style="background: var(--bulma-background); border-radius: 12px; overflow: hidden; text-align: left;">
                            <OnboardingSlot label="settings.weigh_in" slot_id="weigh_in" enabled=wi_on time=wi_time />
                            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;"></div>
                            <OnboardingSlot label="settings.breakfast" slot_id="breakfast" enabled=bf_on time=bf_time />
                            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;"></div>
                            <OnboardingSlot label="settings.lunch" slot_id="lunch" enabled=lu_on time=lu_time />
                            <div style="border-bottom: 0.5px solid var(--bulma-border-weak); margin-left: 16px;"></div>
                            <OnboardingSlot label="settings.dinner" slot_id="dinner" enabled=di_on time=di_time />
                        </div>

                        <button
                            attr:data-testid="push-onboarding-btn-done"
                            class="button is-link is-size-5 has-text-weight-semibold"
                            style="border: none; border-radius: 12px; padding: 14px 32px; cursor: pointer; width: 100%; margin-top: 24px;"
                            on:click=done_schedule
                        >
                            {move || t("push_onboarding.done")}
                        </button>

                        <button
                            attr:data-testid="push-onboarding-btn-skip-schedule"
                            class="has-text-grey-light is-size-6"
                            style="background: none; border: none; cursor: pointer; margin-top: 16px; padding: 8px;"
                            on:click=skip_schedule
                        >
                            {move || t("push_onboarding.skip_schedule")}
                        </button>
                    </div>
                }.into_view()
            }}
        </div>
    }
}

#[component]
fn OnboardingSlot(
    label: &'static str,
    slot_id: &'static str,
    enabled: RwSignal<bool>,
    time: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div style="padding: 12px 16px; display: flex; align-items: center; gap: 12px; background: var(--bulma-scheme-main);">
            <span class="is-size-6" style="flex: 1;">{move || t(label)}</span>
            <input
                type="time"
                attr:data-testid=format!("push-schedule-time-{}", slot_id)
                style="background: var(--bulma-background); border: none; border-radius: 8px; padding: 4px 8px; color: var(--bulma-link); width: 90px; text-align: center;"
                class="is-size-6"
                prop:value=move || time.get()
                on:change=move |ev| {
                    time.set(event_target_value(&ev));
                }
            />
            <button
                attr:data-testid=format!("push-schedule-toggle-{}", slot_id)
                style=move || format!(
                    "{}background: {};",
                    TOGGLE_BASE,
                    if enabled.get() { "var(--bulma-success)" } else { "var(--bulma-border)" }
                )
                on:click=move |_| {
                    enabled.update(|v| *v = !*v);
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
