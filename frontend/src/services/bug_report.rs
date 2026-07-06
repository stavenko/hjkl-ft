//! Bug-report submission to the bug-report worker.
//!
//! Reports are composed by the support-chat assistant (its `file_bug_report`
//! tool). The tool can't do the network itself (a `Tool::call` future must be
//! `Send`, web_sys fetch isn't), so it captures the structured report and the
//! chat loop submits it here. JWT-authed — only signed-in users can file.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

use super::{auth, config};

/// The report payload sent to the worker. `app_version` is filled in by the chat
/// loop just before submitting; the worker adds the user id (from JWT) and time.
#[derive(Debug, Clone, Serialize)]
pub struct BugReport {
    pub title: String,
    pub area: String,
    pub steps_to_reproduce: String,
    pub expected: String,
    pub actual: String,
    pub severity: String,
    pub app_version: String,
}

#[derive(Debug, Deserialize)]
struct ReportAck {
    id: String,
}

/// Submit a bug report. Returns the server-assigned report id on success.
/// FAIL LOUDLY: any transport / auth / HTTP error is returned as `Err`.
pub async fn submit(report: &BugReport) -> Result<String, String> {
    let base = &config::get().bug_report_base_url;
    if base.is_empty() {
        return Err("bug_report_base_url is not configured".to_string());
    }
    let url = format!("{base}/report");
    let token = auth::get_token().ok_or_else(|| "not authenticated".to_string())?;

    let body = serde_json::to_string(report).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body));

    let headers = web_sys::Headers::new().map_err(|e| format!("{e:?}"))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{e:?}"))?;
    headers.set("Authorization", &format!("Bearer {token}")).map_err(|e| format!("{e:?}"))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response".to_string())?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }
    let ack: ReportAck = serde_json::from_str(&text).map_err(|e| format!("parse error: {e}"))?;
    Ok(ack.id)
}
