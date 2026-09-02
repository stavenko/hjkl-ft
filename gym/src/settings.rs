//! Настройки — единственный пока раздел за иконкой в нижнем меню.
//!
//! Содержимое перенесено из настроек приложения худеющего и урезано до того, что
//! у приложения тренировок и правда есть: язык, версия с обновлением, ключи и
//! выход. Устройство разделов и их порядок оставлены прежними — человек ходит
//! между двумя приложениями одного продукта, и настройки в них не должны
//! ощущаться чужими.

use leptos::*;

use crate::i18n::{t, Lang};
use crate::{ai, auth, i18n, subscription, update};

/// Что показывать в разделе аккаунта: сам список или раскрытую фразу.
#[derive(Clone, Copy, PartialEq)]
enum Pane {
    Root,
    Phrase,
}

#[component]
pub fn Settings(on_logout: Callback<()>) -> impl IntoView {
    let pane = create_rw_signal(Pane::Root);

    view! {
        <div class="screen" attr:data-testid="settings">
            <div class="pad">
                {move || match pane.get() {
                    Pane::Root => view! { <Root pane=pane on_logout=on_logout /> }.into_view(),
                    Pane::Phrase => view! { <Phrase pane=pane /> }.into_view(),
                }}
            </div>
        </div>
    }
}

#[component]
fn Root(pane: RwSignal<Pane>, on_logout: Callback<()>) -> impl IntoView {
    // Ручная проверка обновления. Фонового опроса нет: одна проверка при запуске,
    // одна при возвращении на передний план и вот эта кнопка. Опрашивать чаще
    // нечего — сборка выкатывается руками.
    let checking = create_rw_signal(false);
    let check = move |_| {
        if checking.get_untracked() {
            return;
        }
        checking.set(true);
        spawn_local(async move {
            update::check().await;
            checking.set(false);
        });
    };

    // «Обновить» — это `location.reload()`, а он ходит в сеть: до подмены
    // страницы проходит около секунды, в течение которой кнопка выглядела бы
    // мёртвой. Поэтому спиннер зажигается сразу, и лишь потом идёт перезагрузка.
    let updating = create_rw_signal(false);
    let do_update = move |_| {
        if updating.get_untracked() {
            return;
        }
        updating.set(true);
        update::reload();
    };

    // Ключ на этом устройстве.
    let adding_key = create_rw_signal(false);
    let key_msg = create_rw_signal(None::<Result<String, String>>);
    let add_key = move |_| {
        if adding_key.get_untracked() {
            return;
        }
        adding_key.set(true);
        key_msg.set(None);
        spawn_local(async move {
            let name = auth::get_user_id().unwrap_or_default();
            let r = auth::add_passkey(&name).await;
            key_msg.set(Some(match r {
                Ok(()) => Ok(t("set.key_added").to_string()),
                Err(e) => Err(e),
            }));
            adding_key.set(false);
        });
    };

    view! {
        <h1 class="h1" style="text-align: left; margin-bottom: 20px;">{move || t("set.title")}</h1>

        // ── Язык ──
        <p class="section">{move || t("set.language")}</p>
        <div class="card card--rows">
            <button class="row-btn" attr:data-testid="set-lang-ru"
                on:click=move |_| i18n::set(Lang::Ru)>
                <span>"Русский"</span>
                {move || (i18n::get() == Lang::Ru).then(|| view! { <span class="tick">"✓"</span> })}
            </button>
            <button class="row-btn" attr:data-testid="set-lang-en"
                on:click=move |_| i18n::set(Lang::En)>
                <span>"English"</span>
                {move || (i18n::get() == Lang::En).then(|| view! { <span class="tick">"✓"</span> })}
            </button>
        </div>

        // ── Версия ──
        <p class="section">{move || t("set.version")}</p>
        <div class="card card--rows">
            <div class="row" attr:data-testid="set-version">
                <div style="min-width: 0;">
                    <div style="display: flex; align-items: center; gap: 8px;">
                        {move || update::available().get().then(|| view! {
                            <span class="dot" attr:data-testid="set-update-dot"></span>
                        })}
                        <span>
                            {move || if update::available().get() {
                                t("set.version_available")
                            } else {
                                t("set.version_current")
                            }}
                        </span>
                    </div>
                    <p class="hint mono" style="margin-top: 3px;">{move || update::current_version()}</p>
                </div>
                // Есть обновление → «Обновить»; иначе — ручная проверка.
                {move || if update::available().get() {
                    view! {
                        <button class="btn btn--primary btn--sm" attr:data-testid="set-btn-update"
                            prop:disabled=move || updating.get() on:click=do_update>
                            {move || if updating.get() {
                                view! { <span class="spinner spinner--btn"></span> }.into_view()
                            } else {
                                t("set.version_update").into_view()
                            }}
                        </button>
                    }.into_view()
                } else {
                    view! {
                        <button class="btn btn--sm" attr:data-testid="set-btn-check"
                            prop:disabled=move || checking.get() on:click=check>
                            {move || if checking.get() { t("set.version_checking") } else { t("set.version_check") }}
                        </button>
                    }.into_view()
                }}
            </div>
        </div>

        // ── Аккаунт ──
        <p class="section">{move || t("set.account")}</p>
        <div class="card card--rows">
            <button class="row-btn" attr:data-testid="set-btn-add-key"
                prop:disabled=move || adding_key.get() on:click=add_key>
                <span>{move || if adding_key.get() { t("set.key_adding") } else { t("set.key_add") }}</span>
            </button>
            <button class="row-btn" attr:data-testid="set-btn-phrase"
                on:click=move |_| pane.set(Pane::Phrase)>
                <span>{move || t("set.phrase")}</span>
                <span class="chevron">"›"</span>
            </button>
        </div>
        <p class="hint">{move || t("set.key_hint")}</p>
        {move || key_msg.get().map(|r| match r {
            Ok(m) => view! { <div class="banner banner--ok" attr:data-testid="set-key-ok">{m}</div> }.into_view(),
            Err(e) => view! { <div class="banner" attr:data-testid="set-key-err">{e}</div> }.into_view(),
        })}

        <div class="card card--rows" style="margin-top: 18px;">
            // Выход стирает и сессию, и запомненный ответ о подписке: следующий
            // вход может оказаться другим аккаунтом, и чужое «подписка активна»
            // пустило бы его внутрь без проверки.
            <button class="row-btn row-btn--danger" attr:data-testid="set-btn-logout"
                on:click=move |_| {
                    let Some(win) = web_sys::window() else { return };
                    if win.confirm_with_message(t("set.logout_confirm")).unwrap_or(false) {
                        auth::logout();
                        subscription::forget();
                        on_logout.call(());
                    }
                }>
                <span>{move || t("set.logout")}</span>
            </button>
        </div>

        <div style="height: 24px;"></div>
    }
}

/// Фраза восстановления. Показывается как есть, если её уже заводили — в том
/// числе в приложении питания: фраза принадлежит аккаунту, а не приложению.
#[component]
fn Phrase(pane: RwSignal<Pane>) -> impl IntoView {
    let phrase = create_rw_signal(None::<String>);
    let loading = create_rw_signal(true);
    let generating = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);

    spawn_local(async move {
        match auth::get_backup_phrase().await {
            Ok(p) => phrase.set(p),
            Err(e) => error.set(Some(e)),
        }
        loading.set(false);
    });

    // Придумать новую и записать. Совпадение с чужой фразой (`taken`) — редкость,
    // но не ошибка: пробуем ещё, а не сдаёмся с непонятным сообщением.
    let generate = move |_| {
        if generating.get_untracked() {
            return;
        }
        generating.set(true);
        error.set(None);
        spawn_local(async move {
            for _ in 0..3 {
                let p = match ai::generate_backup_phrase().await {
                    Ok(p) => p,
                    Err(e) => {
                        error.set(Some(e));
                        generating.set(false);
                        return;
                    }
                };
                match auth::set_backup_phrase(&p).await {
                    Ok(s) if s == "ok" => {
                        phrase.set(Some(p));
                        generating.set(false);
                        return;
                    }
                    Ok(s) if s == "taken" || s == "too_short" => continue,
                    Ok(s) => {
                        error.set(Some(format!("{}: {s}", t("set.phrase_failed"))));
                        generating.set(false);
                        return;
                    }
                    Err(e) => {
                        error.set(Some(e));
                        generating.set(false);
                        return;
                    }
                }
            }
            error.set(Some(t("set.phrase_failed").to_string()));
            generating.set(false);
        });
    };

    view! {
        <button class="linkbtn" style="margin-bottom: 14px;" attr:data-testid="set-btn-phrase-back"
            on:click=move |_| pane.set(Pane::Root)>
            {move || t("set.back")}
        </button>
        <h1 class="h1" style="text-align: left; margin-bottom: 10px;">{move || t("set.phrase")}</h1>
        <p class="sub" style="text-align: left;">{move || t("set.phrase_desc")}</p>

        {move || error.get().map(|e| view! { <div class="banner" attr:role="alert">{e}</div> })}

        {move || if loading.get() {
            view! { <div class="spinner"></div> }.into_view()
        } else {
            view! {
                {move || phrase.get().map(|p| view! {
                    <div class="card phrase" attr:data-testid="set-phrase-value">{p}</div>
                    <p class="hint" style="margin-bottom: 18px;">{move || t("set.phrase_warning")}</p>
                })}
                <button class="btn btn--primary btn--block" attr:data-testid="set-btn-phrase-generate"
                    prop:disabled=move || generating.get() on:click=generate>
                    {move || if generating.get() {
                        t("set.phrase_generating")
                    } else if phrase.get().is_some() {
                        t("set.phrase_regenerate")
                    } else {
                        t("set.phrase_generate")
                    }}
                </button>
            }.into_view()
        }}
    }
}
