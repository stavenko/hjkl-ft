use leptos::*;
use crate::components::pwa_icons::*;
use crate::services::i18n::t;
use crate::services::platform;

fn detect_platform() -> &'static str {
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
    let is_yandex = ua.contains("yabrowser");

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
                        {t("pwa.inst.ios_safari.1")} " " <IosShareIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {t("pwa.inst.ios_safari.2")} " " <AddToHomeIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{t("pwa.inst.ios_safari.3")}</div>
                </div>
            </div>
        }.into_view(),

        "ios_chrome" | "ios_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{t("pwa.inst.ios_other.1")}</div>
                </div>
                <div class="step">
                    <div class="step-body">{t("pwa.inst.ios_other.2")}</div>
                </div>
            </div>
        }.into_view(),

        "android_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.android_chrome.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">
                        {t("pwa.inst.android_chrome.2")} " " <AddToHomeIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{t("pwa.inst.android_chrome.3")}</div>
                </div>
            </div>
        }.into_view(),

        "android_samsung" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.android_samsung.1")} " " <HamburgerIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.android_samsung.2")}</div>
                </div>
            </div>
        }.into_view(),

        "android_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.android_firefox.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.android_firefox.2")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"3"</span>
                    <div class="step-body">{t("pwa.inst.android_firefox.3")}</div>
                </div>
            </div>
        }.into_view(),

        "android_yandex" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.android_yandex.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.android_yandex.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_safari" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">{t("pwa.inst.macos_safari.1")}</div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.macos_safari.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_chrome" | "desktop_chrome" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_edge" | "desktop_edge" => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.edge.1")} " " <ThreeDotsIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.edge.2")}</div>
                </div>
            </div>
        }.into_view(),

        "macos_firefox" | "desktop_firefox" => view! {
            <div class="steps">
                <div class="step">
                    <div class="step-body has-text-warning-dark">{t("pwa.inst.firefox.1")}</div>
                </div>
            </div>
        }.into_view(),

        _ => view! {
            <div class="steps">
                <div class="step">
                    <span class="step-num">"1"</span>
                    <div class="step-body">
                        {t("pwa.inst.chrome.1")} " " <InstallIcon />
                    </div>
                </div>
                <div class="step">
                    <span class="step-num">"2"</span>
                    <div class="step-body">{t("pwa.inst.chrome.2")}</div>
                </div>
            </div>
        }.into_view(),
    }
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

    view! {
        <style>"
            .steps { display: flex; flex-direction: column; gap: 0.75rem; }
            .step { display: flex; align-items: flex-start; gap: 0.75rem; }
            .step-num {
                flex-shrink: 0; width: 1.75rem; height: 1.75rem;
                border-radius: 50%; background: #485fc7; color: white;
                display: flex; align-items: center; justify-content: center;
                font-size: 0.85rem; font-weight: 600;
            }
            .step-body { font-size: 0.95rem; line-height: 1.5; padding-top: 0.15rem; }
        "</style>
        <div style="min-height: 100vh; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 2rem; text-align: center; background: white;">
            <div style="max-width: 24rem;">
                <img src="/icon-192.png" alt="Food Tracker" style="width: 80px; height: 80px; border-radius: 16px; margin-bottom: 1rem;" />
                <h1 class="title is-3" style="margin-bottom: 0.5rem;">"Food Tracker"</h1>
                <p class="has-text-grey mb-5" style="font-size: 1.05rem; line-height: 1.6;">
                    {t("pwa.description")}
                </p>

                <div class="box" style="text-align: left; margin-bottom: 2rem;">
                    <p class="has-text-weight-semibold mb-4">{t(title)}</p>
                    {steps}
                </div>

                <button
                    attr:data-testid="pwa-btn-dismiss"
                    class="button is-ghost has-text-grey"
                    style="text-decoration: underline; font-size: 0.85rem;"
                    on:click=dismiss
                >
                    {t("pwa.use_browser")}
                </button>
            </div>
        </div>
    }
}
