//! Weekly AI report. Computed via ai-worker (qwen3) and cached in IndexedDB
//! (`summaries` store, id `week:<monday>`). The week report is computed once the
//! week has ended (the following Monday).
//!
//! The former daily «Оценка» (per-day AI assessment + background snack tagging)
//! was removed: food is now classified per-product in the background as soon as
//! it's logged (see the `classify` service), so there's no daily-report step.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::{ai, db, i18n, local};

#[derive(Clone, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub date: String,
    pub text: String,
    /// Last generation error, if the most recent attempt failed. Kept alongside
    /// the last good `text` so a failed regeneration never wipes a success.
    #[serde(default)]
    pub error: Option<String>,
    pub created_at: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn lang_name() -> &'static str {
    match i18n::get_lang() {
        i18n::Lang::Ru => "Russian",
        i18n::Lang::En => "English",
    }
}

// ---- Week date helpers (weeks are Mon–Sun) ----

pub fn week_start_of(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| (d - Duration::days(d.weekday().num_days_from_monday() as i64)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| date.to_string())
}

/// The Monday after `week_start` — when that week's report becomes available.
pub fn next_monday(week_start: &str) -> String {
    NaiveDate::parse_from_str(week_start, "%Y-%m-%d")
        .map(|d| (d + Duration::days(7)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| week_start.to_string())
}

/// True once the week containing `week_start` has fully ended.
pub fn week_ready(week_start: &str) -> bool {
    let today = chrono::Local::now().date_naive();
    NaiveDate::parse_from_str(&next_monday(week_start), "%Y-%m-%d")
        .map(|nm| today >= nm)
        .unwrap_or(false)
}

// ---- Storage ----

pub async fn get_week(week_start: &str) -> Option<Summary> {
    db::get("summaries", &format!("week:{week_start}")).await
}

async fn store(id: String, date: String, text: String) -> Summary {
    let s = Summary { id, date, text, error: None, created_at: now() };
    db::put("summaries", &s).await;
    s
}

// ---- Context assembly ----

/// Last 15 weight + step entries, used by the weekly context.
async fn recent_body_block() -> String {
    let mut w = local::list_weight_entries().await;
    w.sort_by(|a, b| a.date.cmp(&b.date));
    let w: Vec<String> = w.iter().rev().take(15).rev()
        .map(|e| format!("{}: {:.1}kg", e.date, e.weight_kg)).collect();
    let mut s = local::list_step_entries().await;
    s.sort_by(|a, b| a.date.cmp(&b.date));
    let s: Vec<String> = s.iter().rev().take(15).rev()
        .map(|e| format!("{}: {}", e.date, e.steps)).collect();
    let mut out = String::new();
    if !w.is_empty() {
        out.push_str(&format!("\nRecent weight (last {}): {}", w.len(), w.join(", ")));
    }
    if !s.is_empty() {
        out.push_str(&format!("\nRecent steps (last {}): {}", s.len(), s.join(", ")));
    }
    out
}

async fn week_context(week_start: &str) -> Option<String> {
    let start = NaiveDate::parse_from_str(week_start, "%Y-%m-%d").ok()?;
    let mut any = false;
    let mut days = Vec::new();
    for i in 0..7 {
        let d = (start + Duration::days(i)).format("%Y-%m-%d").to_string();
        let diary = local::list_diary(&d).await;
        if diary.is_empty() {
            continue;
        }
        any = true;
        days.push(format!("{}: {} entries", d, diary.len()));
    }
    if !any {
        return None;
    }
    let mut ctx = format!("Week {} – {}\n", week_start, (start + Duration::days(6)).format("%Y-%m-%d"));
    ctx.push_str(&days.join("\n"));
    ctx.push_str(&recent_body_block().await);
    Some(ctx)
}

// ---- Generation (AI, cached) ----

pub async fn ensure_week(week_start: &str) -> Option<Summary> {
    if let Some(s) = get_week(week_start).await {
        return Some(s);
    }
    if !week_ready(week_start) {
        return None;
    }
    let ctx = week_context(week_start).await?;
    let prompt = format!(
        "You are a supportive weight-loss coach. Write a SHORT weekly report (3–5 sentences) in \
         {lang}: overall trend, what went well, and one focus for next week. Do not invent numbers. \
         Plain text only.\n\n{ctx}",
        lang = lang_name(),
    );
    let text = ai::summarize(&prompt).await.ok()?;
    if text.is_empty() {
        return None;
    }
    Some(store(format!("week:{week_start}"), week_start.to_string(), text).await)
}
