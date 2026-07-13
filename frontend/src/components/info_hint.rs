//! A small "?" button that toggles a floating explanation popup. Dismissed by a
//! tap ANYWHERE — the full-screen backdrop AND the card itself — via `pointerup`
//! (which fires on iOS, unlike a delegated click on a bare element).

use leptos::*;

#[component]
pub fn InfoHint(text: String) -> impl IntoView {
    let open = create_rw_signal(false);
    view! {
        <span style="display: inline-flex;">
            <button
                attr:aria-label="?"
                on:click=move |_| open.update(|o| *o = !*o)
                style="width: 16px; height: 16px; min-width: 16px; border-radius: 50%; \
                    border: 1px solid var(--bulma-border); background: transparent; \
                    color: var(--bulma-text-weak); font-size: 0.62rem; font-weight: 700; \
                    line-height: 1; cursor: pointer; padding: 0; display: inline-flex; \
                    align-items: center; justify-content: center;">
                "?"
            </button>
            {move || open.get().then(|| {
                let t = text.clone();
                view! {
                    <div on:pointerup=move |_| open.set(false)
                        style="position: fixed; inset: 0; z-index: 40; cursor: pointer;"></div>
                    <div on:pointerup=move |_| open.set(false)
                        style="position: fixed; z-index: 41; left: 50%; top: 28%; transform: translateX(-50%); \
                            width: min(320px, 90vw); cursor: pointer; \
                            background: var(--bulma-scheme-main); border: 0.5px solid var(--bulma-border-weak); \
                            box-shadow: 0 10px 30px rgba(0,0,0,0.22); border-radius: 12px; padding: 14px 16px;">
                        <span class="is-size-7 has-text-grey" style="line-height: 1.45; white-space: pre-line;">{t}</span>
                    </div>
                }
            })}
        </span>
    }
}
