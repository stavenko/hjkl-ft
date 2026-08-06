pub mod app;
pub mod components;
pub mod pages;
pub mod services;

#[cfg(not(test))]
use wasm_bindgen::prelude::wasm_bindgen;

/// Interval for the connectivity re-probe (ms). RU networks flap, so we re-check
/// server reachability regularly to keep the offline warning honest. Pings ONLY
/// the AI worker (one request per tick).
#[cfg(not(test))]
const PROBE_POLL_MS: i32 = 15_000;

#[cfg(not(test))]
#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::spawn_local(async {
        // ---- Critical path (under the splash): LOCAL ONLY, no network. <1s. ----
        // Config synchronously from cache-or-default so `config::get()` never
        // panics and network code has a base to read (empty until the background
        // fetch REPLACES it). The UI must not wait for the network or the config.
        services::config::load_or_default();
        // Profile reactivity BEFORE db::init (which hydrates the profile cache).
        services::profile::init();

        // Сигналы версий сторов нужны раньше самой базы: на них подписаны ресурсы
        // виджетов, а те строятся до того, как выяснится, есть ли сессия.
        services::db::init_signals();

        // База принадлежит ПОЛЬЗОВАТЕЛЮ и открывается, только когда он известен.
        // Данных вне его базы не существует: устройство живёт в localStorage, а
        // персону спрашивают виджеты уже внутри приложения. Открытие/апгрейд
        // IndexedDB — единственный шаг критического пути, который может ЗАБЛОКИРОВАТЬ
        // (апгрейд схемы, придержанный не закрытой сессией при обновлении PWA);
        // он ограничен таймаутом и возвращает ошибку вместо вечного сплэша.
        if let Some(uid) = services::auth::get_user_id() {
            if let Err(e) = services::db::activate_for_user(&uid).await {
                show_critical_error(&e);
                return;
            }
            services::app_flags::activate().await;
            // Разовый перенос планки шагов из `goals` в поле профиля. Только здесь:
            // без базы читать нечего, а флаг о выполнении принадлежит пользователю.
            // Выполняется ДО первой отрисовки, чтобы `goals` были чисты к моменту,
            // когда любой продуктовый экран начнёт их перебирать.
            if !services::app_flags::get_bool("steps_planka_migrated_v1") {
                services::local::migrate_steps_goal_to_planka().await;
                services::app_flags::set_bool("steps_planka_migrated_v1", true);
            }
        }
        services::i18n::init_lang();
        services::i18n::init_weight_unit();
        services::update::init(); // update-available signal at the root
        services::net::init(); // connectivity signals at the root
        services::subscription::init(); // subscription gate signal at the root
        services::push::init_received(); // "notification received on this device" signal
        services::local::init_planka_stale(); // "planka needs recompute" signal at the root
        services::classify::init(); // reset the background food-classification queue
        services::errors::init(); // background-error log signal at the root
        services::stories::init(); // stories seen-set + tray-ring version signal
        services::letters::init(); // program-letters inbox version signal (mail widget)
        services::sync::init(); // migration-progress signal at the root

        // The database is ready → drop the splash and show the UI IMMEDIATELY.
        // Everything below is background and MUST NOT block the first paint.
        if let Some(splash) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("splash"))
        {
            splash.remove();
        }

        leptos::mount_to_body(app::App);

        // Background listeners/timers: focus re-sync, notification receipts,
        // connectivity re-probe on online/offline + a periodic probe. The update
        // check runs ONCE at start (in bootstrap_network below) — no polling; a
        // manual "check for update" lives in Settings.
        install_foreground_sync();
        install_notif_receipt_poll();
        install_connectivity_listeners();
        install_periodic_probe();

        // ---- Background network bootstrap: prepare the connection, then use it. ----
        leptos::spawn_local(bootstrap_network());
    });
}

/// A short, stable code for an error message (6 hex, FNV-1a) — the same failure
/// always yields the same «Код ошибки», so a user's screenshot pins it down.
#[cfg(not(test))]
fn error_code(msg: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in msg.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:06X}", h & 0xFF_FFFF)
}

/// Render a critical-error screen IN PLACE of the splash when the launch (schema
/// upgrade / DB open) fails or stalls — so the user sees an actionable message and
/// a code instead of an eternal splash. Plain DOM (the Leptos app isn't mounted).
#[cfg(not(test))]
fn show_critical_error(msg: &str) {
    let code = error_code(msg);
    let short = msg
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let html = format!(
        r#"<div style="min-height:100vh;box-sizing:border-box;display:flex;flex-direction:column;\
           align-items:center;justify-content:center;text-align:center;padding:32px 24px;\
           background:#F1F4F9;color:#0E1630;font-family:'Golos Text',system-ui,sans-serif;">
          <div style="font-size:20px;font-weight:800;line-height:1.25;margin-bottom:18px;max-width:340px;">
            Возникла ошибка при обновлении приложения</div>
          <div style="font-size:15px;line-height:1.5;color:#39425F;max-width:340px;margin-bottom:10px;">
            Полностью закройте приложение и откройте снова.</div>
          <div style="font-size:15px;line-height:1.5;color:#39425F;max-width:340px;margin-bottom:20px;">
            Если не помогло — сделайте скриншот этого экрана для диагностики.</div>
          <div style="font-size:13px;line-height:1.45;color:#0E1630;background:#fff;border:1px solid #dfe4ee;\
               border-radius:12px;padding:12px 14px;max-width:340px;margin-bottom:16px;">{short}</div>
          <div style="font-size:14px;font-weight:700;letter-spacing:0.04em;color:#F4536B;">
            Код ошибки: {code}</div>
        </div>"#
    );
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        if let Some(splash) = doc.get_element_by_id("splash") {
            splash.set_inner_html(&html);
        } else if let Some(body) = doc.body() {
            body.set_inner_html(&html);
        }
    }
}

/// Prepare the network in the background and, once the server is reachable, start
/// the network interactions. Never blocks the UI.
#[cfg(not(test))]
async fn bootstrap_network() {
    // 1. Fetch the real config from the network and swap it in live (+cache for
    //    next launch). Until this lands the base URLs may be the cached/empty set.
    if let Some(cfg) = services::config::fetch_from_network().await {
        services::config::save_to_cache(&cfg);
        services::config::set(cfg);
    }

    // 2. Probe reachability: AI worker → is_online; secondary workers → degraded.
    services::net::probe().await;

    // Warm the story-image cache from the same origin so the first story open isn't
    // a from-network flash. Independent of worker reachability — the assets are
    // bundled and served locally, so this runs regardless of `online_now()`.
    services::stories::prefetch_images();

    // 3. Server reachable → begin network interactions.
    if services::net::online_now() {
        // First update check right after establishing the connection.
        services::update::check().await;
        // Subscription: first-ever verify or the daily re-check (flips the gate).
        services::subscription::maybe_recheck().await;
        // Reconcile with the server when signed in: push then pull the merged dump.
        if services::auth::get_token().is_some() {
            if let Err(e) = services::sync::sync_now().await {
                leptos::logging::warn!("Launch sync failed: {e}");
            }
        }
    }

    // Дальше — обслуживание ДАННЫХ пользователя: разметка приёмов пищи, разбор
    // необработанной еды, открытие недель, заморозка дней, пересчёт планок.
    // Без базы всему этому не над чем работать: считать не по чему, а результат
    // некуда положить — каждая запись просто терялась. Вошедшему в этой же
    // сессии обслуживание достанется на следующем запуске; у только что
    // заведённого аккаунта замораживать и открывать всё равно нечего.
    if services::db::current_name().is_none() {
        leptos::logging::log!("обслуживание данных пропущено: базы пользователя нет");
        return;
    }

    if services::net::online_now() {
        // One-time backfill of explicit meal labels on pre-existing entries (runs
        // on the merged dump, after the pull, so server entries get labelled too).
        services::local::migrate_meal_labels().await;
        // Миграции локальной базы здесь НЕ запускаются: им положено идти только в
        // работающем приложении — вне онбординга и после того, как обновление
        // применилось. Условие держит `app.rs`.
        // Classify any recent/untagged food now that the AI worker is reachable.
        leptos::spawn_local(services::classify::sweep_unprocessed());
        // Poll the support thread so a curator `set_planka` directive is applied by
        // the app even if the user never opens /chat (the poll applies it locally).
        leptos::spawn_local(async {
            if let Err(e) = services::support_chat::poll().await {
                leptos::logging::warn!("Launch support poll failed: {e}");
            }
        });
    }

    // Open the activity week if the week-2 gate is now cleared (local-only; runs
    // regardless of connectivity).
    services::indicators::maybe_unlock_activity_week().await;
    // Then the calcium week, once the activity (steps) gate is cleared.
    services::indicators::maybe_unlock_calcium_week().await;

    // Then the iron week, once the calcium gate is cleared.
    services::indicators::maybe_unlock_iron_week().await;

    // Freeze each recent completed day's calorie result (its planka) BEFORE the
    // weekly recompute below might change the planka — so past-day diary gauges keep
    // the planka that actually applied. Independent of the recompute itself.
    services::indicators::freeze_calories_recent().await;
    // То же для шагов — перед недельным пересчётом планки по шагам ниже.
    services::indicators::freeze_steps_recent().await;

    // Weekly calorie-planka recompute: one week after the planka was set (and every
    // week after), recompute it from the last 7 days and post a "letter". Local-only;
    // self-limits via a stored anchor, so calling it every launch is safe.
    services::letters::maybe_recompute_weekly_planka().await;

    // Same, for the steps planka — raised by the step indicator's own colour.
    services::letters::maybe_recompute_weekly_steps_planka().await;
}

#[cfg(not(test))]
fn install_foreground_sync() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
    let cb = Closure::<dyn FnMut()>::new(move || {
        // `document.hidden` is false when the tab/PWA is in the foreground.
        let hidden = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| js_sys::Reflect::get(d.as_ref(), &"hidden".into()).ok())
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !hidden {
            // Re-probe connectivity FIRST: a resumed device may have lost/regained
            // the network (VPN toggled, tunnel dropped) — refresh is_online so the
            // warning is honest and the follow-ups gate correctly.
            services::net::probe_background();
            leptos::spawn_local(async {
                // iOS force-closes a backgrounded PWA's IndexedDB connection; the
                // cached one then wedges (writes silently fail, reads return
                // nothing) until a reload. Reopen it BEFORE any resume work — and
                // before the user touches it (saving weight, opening the diary).
                services::db::reopen().await;
                if services::auth::get_token().is_some() {
                    services::sync::sync_now_background();
                    // Apply a pending curator planka directive on resume too.
                    leptos::spawn_local(async {
                        let _ = services::support_chat::poll().await;
                    });
                }
                // Daily subscription re-check (no-op unless a day has passed).
                services::subscription::maybe_recheck().await;
                // Classify any still-untagged recent food on resume too.
                services::classify::sweep_unprocessed().await;
                // A day may have rolled over while backgrounded — freeze recent days'
                // calorie planka, then check the weekly recompute (self-limits).
                services::indicators::freeze_calories_recent().await;
                // То же для шагов: недельный пересчёт ниже может поднять планку, и
                // незамороженные дни рассудились бы уже по новой.
                services::indicators::freeze_steps_recent().await;
                services::letters::maybe_recompute_weekly_planka().await;
                services::letters::maybe_recompute_weekly_steps_planka().await;
            });
        }
    });
    let _ = document
        .add_event_listener_with_callback("visibilitychange", cb.as_ref().unchecked_ref());
    // Leak the closure: it must live for the whole app session.
    cb.forget();
}

#[cfg(not(test))]
fn install_notif_receipt_poll() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;
    // A push notification's arrival confirms this device's notification setup works.
    // The service worker records the code in Cache; index.html bridges it into
    // localStorage `rn_notif_received`. Poll it and mark the device as having
    // received a push. No tap / navigation.
    let cb = Closure::<dyn Fn()>::new(move || {
        let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
        else {
            return;
        };
        let Some(code) = storage.get_item("rn_notif_received").ok().flatten() else { return };
        if code.is_empty() {
            return;
        }
        let _ = storage.remove_item("rn_notif_received");
        services::diag::note(&format!("WASM poll consumed {code}"));
        // Receiving ANY push confirms this device's notification setup works.
        services::push::mark_received();
    });
    if let Some(win) = web_sys::window() {
        let _ = win.set_interval_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            1000,
        );
    }
    cb.forget();
}

/// Re-probe server reachability on the browser's `online`/`offline` events. These
/// events are only a TRIGGER (they report the NIC, not our servers) — the probe
/// is the source of truth.
#[cfg(not(test))]
fn install_connectivity_listeners() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else { return };
    let cb = Closure::<dyn FnMut()>::new(move || {
        services::net::probe_background();
    });
    let _ = window.add_event_listener_with_callback("online", cb.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback("offline", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Periodically re-probe reachability so a silently dropped connection (common on
/// RU mobile networks) surfaces the warning without needing a resume.
#[cfg(not(test))]
fn install_periodic_probe() {
    use wasm_bindgen::prelude::Closure;
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else { return };
    let cb = Closure::<dyn Fn()>::new(move || {
        services::net::probe_background();
    });
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        cb.as_ref().unchecked_ref(),
        PROBE_POLL_MS,
    );
    cb.forget();
}
