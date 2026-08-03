use leptos::*;
use crate::components::pwa_icons::*;
use crate::services::i18n::t;
use crate::services::platform;

pub fn detect_platform() -> &'static str {
    let ua = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default()
        .to_lowercase();

    let is_ios = ua.contains("iphone") || ua.contains("ipad") || ua.contains("ipod");
    let is_android = ua.contains("android");
    let is_mac = ua.contains("macintosh") || ua.contains("mac os");

    let is_chrome = ua.contains("chrome") && !ua.contains("edg") && !ua.contains("opr");
    let is_firefox = ua.contains("firefox");
    let is_edge = ua.contains("edg/");
    let is_safari = ua.contains("safari") && !ua.contains("chrome") && !ua.contains("chromium");
    let is_samsung = ua.contains("samsungbrowser");
    // Yandex Browser (yabrowser) AND the Yandex app's built-in browser (yasearchbrowser /
    // yaapp_android) — the latter has NO "yabrowser" in its UA but does contain "chrome",
    // so without this it would fall through to the Chrome instructions.
    let is_yandex = ua.contains("yabrowser") || ua.contains("yasearchbrowser") || ua.contains("yaapp_android");

    if is_ios && is_safari { return "ios_safari"; }
    if is_ios && is_chrome { return "ios_chrome"; }
    if is_ios && is_firefox { return "ios_firefox"; }
    if is_ios { return "ios_safari"; }

    if is_android && is_samsung { return "android_samsung"; }
    if is_android && is_yandex { return "android_yandex"; }
    if is_android && is_firefox { return "android_firefox"; }
    if is_android && is_chrome { return "android_chrome"; }
    if is_android { return "android_chrome"; }

    if is_mac && is_safari { return "macos_safari"; }
    if is_mac && is_chrome { return "macos_chrome"; }
    if is_mac && is_edge { return "macos_edge"; }
    if is_mac && is_firefox { return "macos_firefox"; }
    if is_mac { return "macos_chrome"; }

    if is_chrome { return "desktop_chrome"; }
    if is_edge { return "desktop_edge"; }
    if is_firefox { return "desktop_firefox"; }

    "desktop_chrome"
}

fn title_key(platform: &str) -> &'static str {
    match platform {
        s if s.starts_with("ios") => "pwa.title.ios",
        s if s.starts_with("android") => "pwa.title.android",
        s if s.starts_with("macos") => "pwa.title.macos",
        _ => "pwa.title.desktop",
    }
}

fn render_steps(platform: &str) -> View {
    match platform {
        "ios_safari" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.1")} " " <IosShareIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.ios_safari.2")} " " <AddToHomeIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{move || t("pwa.inst.ios_safari.3")}</div>
                </div>
            </div>
        }.into_view(),

        "ios_chrome" | "ios_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{move || t("pwa.inst.ios_other.1")}</div>
                </div>
                <div class="step">
                    <div class="step-body">{move || t("pwa.inst.ios_other.2")}</div>
                </div>
            </div>
        }.into_view(),

        // Chrome on Android — the flow the user actually sees, illustrated with
        // screenshots of the live UI and a blinking hint on what to tap
        // (scripts/shot-yandex-hop-gifs.mjs builds them).
        "android_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.1")}
                        <img src="/onboard-img/pwa-menu.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.2")}
                        <img src="/onboard-img/pwa-addscreen.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_chrome.3")}
                        <img src="/onboard-img/pwa-install.gif" alt="" class="step-shot" />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"4"</span>
                    <div class="step-body">{move || t("pwa.inst.android_chrome.4")}</div>
                </div>
            </div>
        }.into_view(),

        "android_samsung" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_samsung.1")} " " <HamburgerIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_samsung.2")}</div>
                </div>
            </div>
        }.into_view(),

        "android_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_firefox.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_firefox.2")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{move || t("pwa.inst.android_firefox.3")}</div>
                </div>
            </div>
        }.into_view(),

        "android_yandex" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.android_yandex.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.android_yandex.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_safari" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">{move || t("pwa.inst.macos_safari.1")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.macos_safari.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_chrome" | "desktop_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_edge" | "desktop_edge" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.edge.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.edge.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_firefox" | "desktop_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{move || t("pwa.inst.firefox.1")}</div>
                </div>
            </div>
        }.into_view(),

        _ => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {move || t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{move || t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),
    }
}

/// Android intent URL that opens the app in Chrome SPECIFICALLY (package=), ignoring
/// the default browser. Carries `?u=<user_id>` so Chrome (fresh localStorage, no
/// session) can offer the Telegram-code login for this account. Falls back to the
/// plain https URL when Chrome is absent.
fn system_browser_intent_url() -> String {
    let win = web_sys::window().expect("no window");
    let host = win.location().host().unwrap_or_default();
    // Account id: the signed-in session first, else the `?u=` this page was
    // opened with (unauthenticated launch of a `?u=`-carrying link).
    let url_u = win
        .location()
        .search()
        .ok()
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
        .and_then(|p| p.get("u"))
        .filter(|s| !s.is_empty());
    let uid_q = crate::services::auth::get_user_id()
        .or(url_u)
        .map(|u| format!("?u={u}"))
        .unwrap_or_default();
    let target = format!("https://{host}/{uid_q}");
    let fallback = js_sys::encode_uri_component(&target);
    format!(
        "intent://{host}/{uid_q}#Intent;scheme=https;package=com.android.chrome;S.browser_fallback_url={fallback};end"
    )
}

#[component]
pub fn PwaPrompt(on_dismiss: Callback<()>) -> impl IntoView {
    let platform = detect_platform();
    let title = title_key(platform);
    let steps = render_steps(platform);

    let dismiss = move |_| {
        platform::dismiss_pwa_prompt();
        on_dismiss.call(());
    };

    // Yandex Browser (and the Yandex app): a passkey cannot be created there
    // (`navigator.credentials` is absent) and a PWA cannot be installed, so the
    // user is walked into Chrome instead. An intent hand-off does NOT work —
    // Chromium refuses to launch an intent whose target is another browser and
    // silently follows `browser_fallback_url`, i.e. reopens the page HERE — so
    // the screen teaches the only route that does work: the browser's own share
    // button → pick Chrome. Blocking by design: there is no way past it.
    // iOS is untouched: this branch is Android-Yandex only.
    if platform == "android_yandex" {
        return view! {
            <div attr:data-testid="pwa-yandex-screen"
                 style="min-height: 100vh; padding: 28px 20px 40px; text-align: center; \
                        background: var(--bulma-scheme-main); overflow-y: auto;">
                <div style="max-width: 26rem; margin: 0 auto;">
                    <img src="/icon-192.png" alt="re:Norma"
                         style="width: 72px; height: 72px; border-radius: 16px; margin-bottom: 18px;" />
                    <h1 class="title is-5" style="line-height: 1.3; margin-bottom: 10px;">
                        {move || t("pwa.yandex.title")}
                    </h1>
                    <p class="has-text-grey" style="line-height: 1.55; margin-bottom: 26px;">
                        {move || t("pwa.yandex.lead")}
                    </p>

                    <div style="text-align: left; margin-bottom: 22px;">
                        <p style="line-height: 1.5; margin-bottom: 10px;">
                            <span style="display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 50%; background: var(--bulma-text-strong); color: var(--bulma-scheme-main); font-size: 13px; font-weight: 700; margin-right: 8px; vertical-align: 1px;">"1"</span>
                            {move || t("pwa.yandex.step1")}
                        </p>
                        <img src="/onboard-img/hop-share.gif" alt=""
                             style="display: block; width: 100%; border-radius: 14px; border: 1px solid var(--bulma-border);" />
                    </div>

                    <div style="text-align: left;">
                        <p style="line-height: 1.5; margin-bottom: 10px;">
                            <span style="display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; border-radius: 50%; background: var(--bulma-text-strong); color: var(--bulma-scheme-main); font-size: 13px; font-weight: 700; margin-right: 8px; vertical-align: 1px;">"2"</span>
                            {move || t("pwa.yandex.step2")}
                        </p>
                        <img src="/onboard-img/hop-chrome.gif" alt=""
                             style="display: block; width: 100%; border-radius: 14px; border: 1px solid var(--bulma-border);" />
                    </div>
                </div>
            </div>
        }
        .into_view();
    }

    view! {
        <style>"
            .steps { display: flex; flex-direction: column; gap: 0.75rem; }
            .step { display: flex; align-items: flex-start; gap: 0.75rem; }
            .step-num {
                flex-shrink: 0; width: 1.75rem; height: 1.75rem;
                border-radius: 50%; background: var(--bulma-link); color: var(--bulma-link-invert);
                display: flex; align-items: center; justify-content: center;
                font-size: 0.85rem; font-weight: 600;
            }
            .step-body { font-size: 0.95rem; line-height: 1.5; padding-top: 0.15rem; }
            /* Скриншот живого интерфейса под текстом шага — мигающая подсказка
               показывает, куда нажимать. */
            .step-shot {
                display: block; width: 100%; margin-top: 0.5rem;
                border-radius: 10px; border: 1px solid var(--bulma-border);
            }
        "</style>
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: var(--bulma-scheme-main); overflow-y: auto;">
            <div style="max-width: 24rem;">
                <img src="/icon-192.png" alt="Food Tracker" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                <h1 class="title is-3" style="margin-bottom: 0.5rem;">"Food Tracker"</h1>
                <p class="has-text-grey mb-5" style="font-size: 1.05rem; line-height: 1.6;">
                    {move || t("pwa.description")}
                </p>

                <div class="box" style="text-align: left; margin-bottom: 2rem;">
                    <p class="has-text-weight-semibold mb-4">{t(title)}</p>
                    {steps}
                </div>

                // «Продолжить в браузере» есть только на десктопе. На телефоне
                // приложение обязано стоять как PWA: браузерная вкладка на
                // Android открывается системным браузером (у части людей это
                // Яндекс, где приложение не работает), поэтому выхода из
                // инструкции нет — ставим PWA.
                {(!platform.starts_with("android") && !platform.starts_with("ios")).then(|| view! {
                    <button
                        attr:data-testid="pwa-btn-dismiss"
                        class="button is-ghost has-text-grey"
                        style="text-decoration: underline; font-size: 0.85rem;"
                        on:click=dismiss
                    >
                        {move || t("pwa.use_browser")}
                    </button>
                })}
            </div>
        </div>
    }
    .into_view()
}
