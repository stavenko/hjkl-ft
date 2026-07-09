use worker::*;
use worker::durable::State;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationSlot {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSchedule {
    pub utc_offset_minutes: i32,
    pub weigh_in: NotificationSlot,
    pub breakfast: NotificationSlot,
    pub lunch: NotificationSlot,
    pub dinner: NotificationSlot,
    #[serde(default)]
    pub steps: NotificationSlot,
    /// Global kill-switch. When true, ALL reminders are off regardless of the
    /// per-slot `enabled` flags. The client sends the full schedule (incl. this
    /// flag) on every change, so we never miss the signal.
    #[serde(default)]
    pub disabled: bool,
}

const STORAGE_SCHEDULE: &str = "schedule";
const STORAGE_USER_ID: &str = "user_id";
const STORAGE_NEXT_SLOT: &str = "next_slot";

#[durable_object]
pub struct ScheduleDO {
    state: State,
    env: Env,
}

impl DurableObject for ScheduleDO {
    fn new(state: State, env: Env) -> Self {
        Self { state, env }
    }

    async fn fetch(&self, mut req: Request) -> Result<Response> {
        let url = req.url()?;
        let path = url.path();

        match path {
            "/update" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .ok_or_else(|| Error::RustError("missing user_id".into()))?;
                let schedule: UserSchedule = serde_json::from_value(
                    body.get("schedule").cloned()
                        .ok_or_else(|| Error::RustError("missing schedule".into()))?,
                ).map_err(|e| Error::RustError(format!("parse schedule: {e}")))?;
                self.handle_update(user_id, schedule).await
            }
            "/get" => self.handle_get().await,
            "/test-alarm" => {
                let body: serde_json::Value = req.json().await?;
                let user_id = body.get("user_id").and_then(|v| v.as_str())
                    .unwrap_or("test-user");
                self.state.storage().put(STORAGE_USER_ID, user_id.to_string()).await?;
                self.state.storage().put(STORAGE_NEXT_SLOT, "weigh_in".to_string()).await?;
                let sched = UserSchedule {
                    utc_offset_minutes: 0,
                    weigh_in: NotificationSlot { enabled: true, time: "00:00".into() },
                    breakfast: NotificationSlot::default(),
                    lunch: NotificationSlot::default(),
                    dinner: NotificationSlot::default(),
                    steps: NotificationSlot::default(),
                    disabled: false,
                };
                let sched_json = serde_json::to_string(&sched)
                    .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
                self.state.storage().put(STORAGE_SCHEDULE, &sched_json).await?;

                let delay_ms = body.get("delay_s").and_then(|v| v.as_u64()).unwrap_or(90);
                let fire_ms = Date::now().as_millis() as f64 + (delay_ms as f64 * 1000.0);
                let alarm_date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(fire_ms));
                self.state.storage().set_alarm(worker::durable::ScheduledTime::new(alarm_date)).await?;
                console_log!("test-alarm: set for {}ms from now (fire_ms={})", delay_ms * 1000, fire_ms);
                Response::from_json(&serde_json::json!({"ok": true, "fire_in_s": delay_ms, "fire_ms": fire_ms}))
            }
            _ => Response::error(format!("unknown path: {path}"), 404),
        }
    }

    async fn alarm(&self) -> Result<Response> {
        self.handle_alarm().await
    }
}

impl ScheduleDO {
    async fn handle_update(&self, user_id: &str, schedule: UserSchedule) -> Result<Response> {
        self.state.storage().delete_alarm().await?;
        let _ = self.state.storage().delete(STORAGE_NEXT_SLOT).await;

        let schedule_json = serde_json::to_string(&schedule)
            .map_err(|e| Error::RustError(format!("serialize: {e}")))?;
        self.state.storage().put(STORAGE_SCHEDULE, &schedule_json).await?;
        self.state.storage().put(STORAGE_USER_ID, user_id.to_string()).await?;

        self.schedule_next_alarm(&schedule, 0).await?;

        Response::from_json(&serde_json::json!({"ok": true}))
    }

    async fn handle_get(&self) -> Result<Response> {
        let stored: Option<String> = self.state.storage().get(STORAGE_SCHEDULE).await?;
        match stored {
            Some(json) => {
                let schedule: UserSchedule = serde_json::from_str(&json)
                    .map_err(|e| Error::RustError(format!("parse: {e}")))?;
                Response::from_json(&schedule)
            }
            None => Response::from_json(&serde_json::json!(null)),
        }
    }

    async fn handle_alarm(&self) -> Result<Response> {
        console_log!("ScheduleDO: alarm fired at {}", Date::now().as_millis());

        let slot_name: Option<String> = self.state.storage().get(STORAGE_NEXT_SLOT).await?;
        let schedule_json: Option<String> = self.state.storage().get(STORAGE_SCHEDULE).await?;
        let user_id: Option<String> = self.state.storage().get(STORAGE_USER_ID).await?;

        console_log!(
            "ScheduleDO: slot={:?} schedule={} user={:?}",
            slot_name,
            schedule_json.is_some(),
            user_id
        );

        if let (Some(slot), Some(sched_json), Some(uid)) = (slot_name, schedule_json, user_id) {
            let schedule: UserSchedule = serde_json::from_str(&sched_json)
                .map_err(|e| Error::RustError(format!("parse: {e}")))?;

            let slot_enabled = match slot.as_str() {
                "weigh_in" => schedule.weigh_in.enabled,
                "breakfast" => schedule.breakfast.enabled,
                "lunch" => schedule.lunch.enabled,
                "dinner" => schedule.dinner.enabled,
                "steps" => schedule.steps.enabled,
                _ => false,
            };

            console_log!("ScheduleDO: slot '{}' enabled={} for user {}", slot, slot_enabled, uid);

            if slot_enabled {
                let payload = notification_payload(&slot);
                match crate::send_push_to_user(&self.env, &uid, &payload).await {
                    Ok(()) => console_log!("ScheduleDO: push sent for slot '{}'", slot),
                    Err(e) => console_log!("ScheduleDO alarm push failed for user {}: {}", uid, e),
                }
            }

            self.schedule_next_alarm(&schedule, 30_000).await?;
        } else {
            console_log!("ScheduleDO: alarm fired but missing data, no action taken");
        }

        Response::ok("alarm handled")
    }

    async fn schedule_next_alarm(&self, schedule: &UserSchedule, buffer_ms: u64) -> Result<()> {
        // Global kill-switch: no alarm at all when the user disabled notifications.
        if schedule.disabled {
            console_log!("ScheduleDO: notifications disabled — no alarm scheduled");
            return Ok(());
        }
        let slots: [(&str, &NotificationSlot); 5] = [
            ("weigh_in", &schedule.weigh_in),
            ("breakfast", &schedule.breakfast),
            ("lunch", &schedule.lunch),
            ("dinner", &schedule.dinner),
            ("steps", &schedule.steps),
        ];

        let now_ms = Date::now().as_millis() as u64;
        let day_ms = 24 * 60 * 60 * 1000u64;
        let day_start_ms = now_ms - (now_ms % day_ms);

        let mut earliest: Option<(u64, String)> = None;

        for (name, slot) in &slots {
            if !slot.enabled {
                continue;
            }
            let Some((h, m)) = parse_time(&slot.time) else { continue };
            let local_min = h as i64 * 60 + m as i64;
            let utc_min = (local_min - schedule.utc_offset_minutes as i64).rem_euclid(24 * 60);

            let mut fire_ms = day_start_ms + utc_min as u64 * 60_000;
            if fire_ms <= now_ms + buffer_ms {
                fire_ms += day_ms;
            }

            match &earliest {
                None => earliest = Some((fire_ms, name.to_string())),
                Some((t, _)) if fire_ms < *t => earliest = Some((fire_ms, name.to_string())),
                _ => {}
            }
        }

        if let Some((fire_ms, slot_name)) = earliest {
            self.state.storage().put(STORAGE_NEXT_SLOT, &slot_name).await?;
            let alarm_date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(fire_ms as f64));
            self.state.storage().set_alarm(worker::durable::ScheduledTime::new(alarm_date)).await?;
            console_log!(
                "ScheduleDO: next alarm for slot '{}' at {} (in {}s)",
                slot_name,
                fire_ms,
                (fire_ms - now_ms) / 1000
            );
        } else {
            self.state.storage().delete_alarm().await?;
            let _ = self.state.storage().delete(STORAGE_NEXT_SLOT).await;
        }

        Ok(())
    }
}

fn notification_payload(slot: &str) -> String {
    let (body, tag, url) = match slot {
        "weigh_in" => ("\u{2696}\u{fe0f} Время взвеситься!", "weigh-in", "/weight"),
        "breakfast" => ("\u{1f950} Время записать завтрак!", "breakfast", "/diary"),
        "lunch" => ("\u{1f37d}\u{fe0f} Время записать обед!", "lunch", "/diary"),
        "dinner" => ("\u{1f37d}\u{fe0f} Время записать ужин!", "dinner", "/diary"),
        "steps" => ("\u{1f6b6} Время записать шаги!", "steps", "/steps"),
        _ => ("Напоминание", "reminder", "/"),
    };

    serde_json::json!({
        "title": "Food Tracker",
        "body": body,
        "icon": "/icon-192.png",
        "tag": tag,
        "renotify": true,
        "requireInteraction": true,
        "url": url,
        "actions": [{"action": "open", "title": "Открыть"}],
    }).to_string()
}

fn parse_time(time: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 2 { return None; }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    if h >= 24 || m >= 60 { return None; }
    Some((h, m))
}
