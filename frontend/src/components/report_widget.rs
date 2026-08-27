//! Виджет отправки отчёта куратору: плитка на дашборде и её панель.
//!
//! Появляется только у человека, у которого куратор есть. Состояние берётся из
//! уже скачанного треда (`support_chat::report_status`), а не отдельным
//! запросом: приложение и так опрашивает чат, и второй источник правды
//! разошёлся бы с первым.
//!
//! Дребезжит, пока панель не открыли после запроса. Гасит дребезжание САМО
//! открытие: человек увидел — дальше его дело, отправлять сейчас или потом.
//! Запрос при этом остаётся невыполненным, и куратор видит, что ответа нет.

use leptos::*;

use crate::services::i18n::t;
use crate::services::{curator, support_chat};

/// Сколько дней предлагаем отправить, когда запроса нет. Свой выбор человека —
/// куратор срок не называл.
const DEFAULT_DAYS: u32 = 1;

/// Выбор, что отправить. Двумя кнопками, а не полем «за сколько дней».
///
/// Число дней человек не знает. Он знает «я уже присылал» и «я ещё ничего не
/// присылал» — и перевести это в 17 не может ни он, ни куратор. Поэтому выбор из
/// двух, и оба названы его словами.
///
/// Когда прошлого отчёта нет, «только новое» не показывается вовсе: отсчитывать
/// не от чего, и предлагать выбор, у которого один осмысленный ответ, — значит
/// заставлять человека думать впустую.
#[component]
fn SendChoice(
    status: RwSignal<support_chat::ReportStatus>,
    send: impl Fn(datashare::report::Scope) + Copy + 'static,
) -> impl IntoView {
    use datashare::report::Scope;
    let sheet = "position: fixed; inset: 0; z-index: 60; background: rgba(0,0,0,.45); \
                 display: flex; align-items: flex-end;";
    let card = "width: 100%; background: var(--bulma-scheme-main); border-radius: 18px 18px 0 0; \
                padding: 20px;";
    view! {
        <div style=sheet attr:data-testid="report-choice">
            <div style=card on:click=|ev| ev.stop_propagation()>
                <p class="is-size-6 has-text-weight-semibold">{move || t("curator.report.what")}</p>
                {move || status.get().last_report_through.map(|through| view! {
                    <button class="button is-link is-fullwidth" style="margin-top: 16px;"
                        attr:data-testid="report-send-new"
                        on:click=move |_| send(Scope::New)>
                        {move || t("curator.report.only_new")}
                    </button>
                    <p class="is-size-7 has-text-grey" style="margin-top: 6px;">
                        {t("curator.report.only_new_hint").replace("{date}", &through)}
                    </p>
                })}
                <button class="button is-fullwidth" style="margin-top: 12px;"
                    attr:data-testid="report-send-all"
                    on:click=move |_| send(Scope::All)>
                    {move || t("curator.report.everything")}
                </button>
                <p class="is-size-7 has-text-grey" style="margin-top: 6px;">
                    {move || t("curator.report.through_hint")}
                </p>
            </div>
        </div>
    }
}

/// Панель виджета: имя куратора, состояние, отправка и отвязка./// Панель виджета: имя куратора, состояние, отправка и отвязка.
#[component]
pub fn ReportPanel(
    /// Закрыть панель (после отвязки смотреть в ней больше не на что).
    on_done: Callback<()>,
) -> impl IntoView {
    let status = create_rw_signal(support_chat::ReportStatus::default());
    let curator_name = create_rw_signal(String::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let sent = create_rw_signal(false);
    /// Модалка выбора: что именно отправить. `false` — панель как была.
    let choosing = create_rw_signal(false);

    // Открытие панели гасит дребезжание — это и есть «увидел».
    create_effect(move |_| {
        spawn_local(async move {
            support_chat::mark_report_seen().await;
            status.set(support_chat::report_status().await);
            if let Ok(b) = curator::binding().await {
                curator_name.set(b.curator_name);
            }
        });
    });

    let send = move |scope: datashare::report::Scope| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        choosing.set(false);
        error.set(None);
        spawn_local(async move {
            match support_chat::send_report(scope).await {
                Ok(_) => {
                    sent.set(true);
                    status.set(support_chat::report_status().await);
                }
                Err(e) => {
                    leptos::logging::error!("отправка отчёта: {e}");
                    error.set(Some(e));
                }
            }
            busy.set(false);
        });
    };

    let unbind = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match curator::unbind().await {
                Ok(_) => {
                    crate::services::curator::forget_locally().await;
                    on_done.call(());
                }
                Err(e) => {
                    leptos::logging::error!("отвязка от куратора: {e}");
                    error.set(Some(e));
                }
            }
            busy.set(false);
        });
    };

    let card = "border: 0.5px solid var(--bulma-border-weak); border-radius: 14px; \
                padding: 16px; background: var(--bulma-scheme-main);";

    view! {
        <div style="display: flex; flex-direction: column; gap: 12px;"
            attr:data-testid="report-panel">
            <div style=card>
                <p class="is-size-7 has-text-grey">{move || t("curator.report.your_curator")}</p>
                <p class="is-size-5 has-text-weight-semibold" style="margin-top: 4px;">
                    {move || {
                        let n = curator_name.get();
                        if n.is_empty() { t("chat.peer_curator").to_string() } else { n }
                    }}
                </p>
            </div>

            <div style=card>
                <p class="is-size-6" style="line-height: 1.5;">
                    {move || match (status.get().request, status.get().last_report_at) {
                        // Куратор ждёт данные — говорим об этом прямо: человек
                        // отвечает на просьбу, а не действует по своей воле.
                        (Some(_), _) => t("curator.report.requested").to_string(),
                        (None, Some(at)) => t("curator.report.last_sent")
                            .replace("{date}", at.get(0..10).unwrap_or("")),
                        (None, None) => t("curator.report.never_sent").to_string(),
                    }}
                </p>
                <button class="button is-link is-fullwidth" style="margin-top: 14px;"
                    attr:data-testid="report-send"
                    prop:disabled=move || busy.get()
                    on:click=move |_| { error.set(None); choosing.set(true); }>
                    {move || t("curator.report.send")}
                </button>
                {move || sent.get().then(|| view! {
                    <p class="is-size-7" style="margin-top: 10px; color: #1fa463;"
                        attr:data-testid="report-sent">
                        {move || t("curator.report_sent")}
                    </p>
                })}
                {move || error.get().map(|e| view! {
                    <p class="is-size-7 has-text-danger" style="margin-top: 10px;">{e}</p>
                })}
                {move || choosing.get().then(|| view! { <SendChoice status=status send=send /> })}
            </div>

            <div style=card>
                <p class="is-size-7 has-text-grey" style="line-height: 1.5;">
                    {move || t("curator.report.unbind_hint")}
                </p>
                <button class="button is-fullwidth is-danger is-light" style="margin-top: 12px;"
                    attr:data-testid="report-unbind"
                    prop:disabled=move || busy.get()
                    on:click=unbind>
                    {move || t("curator.report.unbind")}
                </button>
            </div>
        </div>
    }
}

/// Значок плитки: стрелка вверх в скобке — «отправить».
pub fn icon_upload() -> View {
    view! {
        <svg width="22" height="22" viewBox="0 0 24 24" fill="none"
            stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M12 16V4" />
            <polyline points="7 9 12 4 17 9" />
            <path d="M5 16v2a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-2" />
        </svg>
    }
    .into_view()
}
