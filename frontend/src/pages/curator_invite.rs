//! Экран согласия: «Куратор такой-то хочет добавить вас в список своих клиентов».
//!
//! Открывается по ссылке, которую куратор прислал человеку. Данные худеющего
//! лежат у него, и получить к ним доступ куратор может только с его согласия —
//! этот экран и есть то место, где согласие даётся.
//!
//! Ссылка рассчитана на УСТАНОВЛЕННОЕ приложение: приглашают тех, кто уже внутри.
//! Открыл в браузере или без входа — отправляем регистрироваться, а не пытаемся
//! завести аккаунт по дороге.

use leptos::*;

use crate::services::i18n::t;
use crate::services::{auth, config, curator};

/// Код приглашения из `?c=`.
fn param_code() -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get("c").filter(|s| !s.is_empty())
}

fn go_home() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href("/");
    }
}

#[derive(Clone, PartialEq)]
enum Step {
    Loading,
    /// Приложение не установлено или входа нет — приглашение не для этого случая.
    NeedApp,
    /// Ссылка не найдена или уже погашена.
    Dead,
    Ask { name: String, replaces: bool },
    Accepted { name: String },
    Failed(String),
}

#[component]
pub fn CuratorInvitePage() -> impl IntoView {
    let step = create_rw_signal(Step::Loading);
    let busy = create_rw_signal(false);
    let code = store_value(param_code().unwrap_or_default());

    create_effect(move |_| {
        let c = code.get_value();
        spawn_local(async move {
            if c.is_empty() {
                step.set(Step::Dead);
                return;
            }
            // Приглашение — для тех, кто уже пользуется приложением. Нет сессии
            // (или это браузер, а не установленное приложение) — говорим об этом
            // прямо и отправляем по обычному пути, а не пытаемся войти по дороге.
            if !auth::session_valid_here() {
                step.set(Step::NeedApp);
                return;
            }
            if let Err(e) = config::ensure_ready().await {
                step.set(Step::Failed(e));
                return;
            }
            match curator::peek(&c).await {
                Ok(p) if p.found => step.set(Step::Ask {
                    name: p.curator_name,
                    replaces: p.current_curator_id.is_some(),
                }),
                Ok(_) => step.set(Step::Dead),
                Err(e) => {
                    leptos::logging::error!("curator invite peek: {e}");
                    step.set(Step::Failed(e));
                }
            }
        });
    });

    let accept = move |_| {
        if busy.get() {
            return;
        }
        busy.set(true);
        let c = code.get_value();
        let name = match step.get_untracked() {
            Step::Ask { name, .. } => name,
            _ => String::new(),
        };
        spawn_local(async move {
            match curator::accept(&c).await {
                Ok(_) => {
                    // Привязка меняет адресата чата и включает виджет отчёта —
                    // и то и другое приложение узнаёт из ближайшего опроса.
                    let _ = crate::services::support_chat::poll().await;
                    step.set(Step::Accepted { name });
                }
                Err(e) => {
                    leptos::logging::error!("curator invite accept: {e}");
                    step.set(Step::Failed(e));
                }
            }
            busy.set(false);
        });
    };

    let card = "border: 0.5px solid var(--bulma-border-weak); border-radius: 16px; \
                padding: 20px; background: var(--bulma-scheme-main); margin-top: 2rem;";

    view! {
        <div style="padding: 1rem; max-width: 480px; margin: 0 auto;"
            attr:data-testid="curator-invite">
            {move || match step.get() {
                Step::Loading => view! {
                    <div style="display: flex; justify-content: center; padding: 3rem;">
                        <div class="ft-spinner"></div>
                    </div>
                }.into_view(),

                Step::NeedApp => view! {
                    <div style=card attr:data-testid="curator-invite-need-app">
                        <p class="is-size-5 has-text-weight-semibold">{move || t("curator.invite.need_app_title")}</p>
                        <p class="is-size-6" style="margin-top: 10px; line-height: 1.5;">
                            {move || t("curator.invite.need_app_body")}
                        </p>
                        <a class="button is-link is-fullwidth" style="margin-top: 16px;"
                            href=move || config::get().landing_url.clone()>
                            {move || t("curator.invite.need_app_cta")}
                        </a>
                    </div>
                }.into_view(),

                Step::Dead => view! {
                    <div style=card attr:data-testid="curator-invite-dead">
                        <p class="is-size-5 has-text-weight-semibold">{move || t("curator.invite.dead_title")}</p>
                        <p class="is-size-6" style="margin-top: 10px; line-height: 1.5;">
                            {move || t("curator.invite.dead_body")}
                        </p>
                        <button class="button is-fullwidth" style="margin-top: 16px;"
                            on:click=move |_| go_home()>
                            {move || t("curator.invite.to_app")}
                        </button>
                    </div>
                }.into_view(),

                Step::Ask { name, replaces } => {
                    let name_line = t("curator.invite.ask").replace("{name}", &name);
                    view! {
                        <div style=card attr:data-testid="curator-invite-ask">
                            <p class="is-size-5 has-text-weight-semibold" style="line-height: 1.4;">
                                {name_line}
                            </p>
                            <p class="is-size-6 has-text-grey" style="margin-top: 12px; line-height: 1.5;">
                                {move || t("curator.invite.explain")}
                            </p>
                            // Согласие оборвёт прежнюю связь — сказать об этом надо
                            // ДО, а не после.
                            {replaces.then(|| view! {
                                <p class="is-size-6 has-text-warning-dark"
                                    attr:data-testid="curator-invite-replaces"
                                    style="margin-top: 12px; line-height: 1.5;">
                                    {move || t("curator.invite.replaces")}
                                </p>
                            })}
                            <button class="button is-link is-fullwidth" style="margin-top: 18px;"
                                attr:data-testid="curator-invite-accept"
                                prop:disabled=move || busy.get()
                                on:click=accept>
                                {move || t("curator.invite.accept")}
                            </button>
                            <button class="button is-fullwidth" style="margin-top: 10px;"
                                prop:disabled=move || busy.get()
                                on:click=move |_| go_home()>
                                {move || t("curator.invite.decline")}
                            </button>
                        </div>
                    }.into_view()
                }

                Step::Accepted { name } => {
                    let line = t("curator.invite.done").replace("{name}", &name);
                    view! {
                        <div style=card attr:data-testid="curator-invite-done">
                            <p class="is-size-5 has-text-weight-semibold" style="line-height: 1.4;">{line}</p>
                            <p class="is-size-6 has-text-grey" style="margin-top: 12px; line-height: 1.5;">
                                {move || t("curator.invite.done_body")}
                            </p>
                            <button class="button is-link is-fullwidth" style="margin-top: 18px;"
                                on:click=move |_| go_home()>
                                {move || t("curator.invite.to_app")}
                            </button>
                        </div>
                    }.into_view()
                }

                Step::Failed(e) => view! {
                    <div style=card attr:data-testid="curator-invite-failed">
                        <p class="is-size-5 has-text-weight-semibold">{move || t("curator.invite.failed")}</p>
                        <p class="is-size-7 has-text-grey" style="margin-top: 10px;">{e}</p>
                        <button class="button is-fullwidth" style="margin-top: 16px;"
                            on:click=move |_| go_home()>
                            {move || t("curator.invite.to_app")}
                        </button>
                    </div>
                }.into_view(),
            }}
        </div>
    }
}
