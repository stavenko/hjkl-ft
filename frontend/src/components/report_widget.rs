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

/// Панель виджета: имя куратора, состояние, отправка и отвязка.
#[component]
pub fn ReportPanel(
    /// Закрыть панель (после отвязки смотреть в ней больше не на что).
    on_done: Callback<()>,
) -> impl IntoView {
    let status = create_rw_signal(support_chat::ReportStatus::default());
    let curator_name = create_rw_signal(String::new());
    let busy = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let days = create_rw_signal(DEFAULT_DAYS.to_string());
    let sent = create_rw_signal(false);

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

    let send = move |requested: Option<u32>| {
        if busy.get_untracked() {
            return;
        }
        let n = requested.unwrap_or_else(|| {
            days.get_untracked().trim().parse::<u32>().unwrap_or(DEFAULT_DAYS)
        });
        if n == 0 || n > 366 {
            error.set(Some(t("curator.report.bad_period").to_string()));
            return;
        }
        busy.set(true);
        error.set(None);
        spawn_local(async move {
            match support_chat::send_report(n).await {
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
                {move || match status.get().request_days {
                    // Куратор ждёт данные за названный им срок — одна кнопка, и
                    // выбирать человеку нечего: он отвечает на конкретную просьбу.
                    Some(d) => {
                        let label = t("curator.report.send_requested")
                            .replace("{days}", &d.to_string());
                        view! {
                            <p class="is-size-6" style="line-height: 1.5;">
                                {t("curator.report.requested").replace("{days}", &d.to_string())}
                            </p>
                            <button class="button is-link is-fullwidth" style="margin-top: 14px;"
                                attr:data-testid="report-send-requested"
                                prop:disabled=move || busy.get()
                                on:click=move |_| send(Some(d))>
                                {label}
                            </button>
                        }.into_view()
                    }
                    // Запроса нет — человек отправляет по своей воле и сам
                    // называет срок.
                    None => view! {
                        <p class="is-size-6" style="line-height: 1.5;">
                            {move || match status.get().last_report_at {
                                Some(at) => t("curator.report.last_sent")
                                    .replace("{date}", at.get(0..10).unwrap_or("")),
                                None => t("curator.report.never_sent").to_string(),
                            }}
                        </p>
                        <div style="display: flex; gap: 8px; align-items: center; margin-top: 14px;">
                            <input class="input" type="number" min="1" max="366"
                                attr:data-testid="report-days"
                                style="max-width: 110px;"
                                prop:value=move || days.get()
                                on:input=move |ev| days.set(event_target_value(&ev)) />
                            <span class="is-size-7 has-text-grey">
                                {move || t("curator.report.days_hint")}
                            </span>
                        </div>
                        <button class="button is-link is-fullwidth" style="margin-top: 12px;"
                            attr:data-testid="report-send"
                            prop:disabled=move || busy.get()
                            on:click=move |_| send(None)>
                            {move || t("curator.report.send")}
                        </button>
                    }.into_view(),
                }}
                {move || sent.get().then(|| view! {
                    <p class="is-size-7" style="margin-top: 10px; color: #1fa463;"
                        attr:data-testid="report-sent">
                        {move || t("curator.report_sent")}
                    </p>
                })}
                {move || error.get().map(|e| view! {
                    <p class="is-size-7 has-text-danger" style="margin-top: 10px;">{e}</p>
                })}
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
