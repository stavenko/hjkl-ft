use leptos::*;

use crate::services::i18n::t;
use crate::services::net;

/// A small amber ⚠ badge shown on a control that triggers a network action while
/// the server is unreachable — a heads-up that tapping it won't reach the network
/// right now. Render it INSIDE a `position: relative` button/container; it pins to
/// the top-right corner and renders nothing while online.
#[component]
pub fn NetOfflineBadge() -> impl IntoView {
    move || {
        // Only when we KNOW the server is unreachable (Some(false)) — not on the
        // unprobed None state.
        (net::is_online().get() == Some(false)).then(|| {
            view! {
                <span attr:data-testid="net-offline-badge"
                    title=t("net.offline_title")
                    style="position: absolute; top: -5px; right: -5px; z-index: 2; color: #E0A100; background: var(--bulma-scheme-main); border-radius: 50%; padding: 1px; line-height: 0; display: inline-flex; box-shadow: 0 0 0 1px var(--bulma-scheme-main);">
                    <svg xmlns="http://www.w3.org/2000/svg" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>
                        <line x1="12" y1="9" x2="12" y2="13"/>
                        <line x1="12" y1="17" x2="12.01" y2="17"/>
                    </svg>
                </span>
            }
        })
    }
}
