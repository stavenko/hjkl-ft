// receipt-worker — catches inbound payment-receipt emails (Cloudflare Email Routing →
// this Worker's `email()` handler) and (1) ARCHIVES each one whole to R2: the raw RFC822
// (which already contains every attachment, inline) plus a `meta/*.json` header sidecar,
// and (2) BINDS it to its payment via payment-worker `/internal/receipt` (address → claim,
// amount, full text). The raw archive is the source of truth; binding is best-effort and
// only happens when the sender verifies as lava.
//
// "Handler must act": Email Routing DROPS the message if the handler returns without
// consuming `raw`, forwarding, or rejecting — so we always read `raw_bytes()`.
//
// R2 holds the blobs (a receipt PDF can be large); pull a capture with
// `wrangler r2 object get renorma-receipts-prod/<key> --remote`.

use worker::*;

#[event(email)]
async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> Result<()> {
    let to = message.to();
    let from = message.from();
    let headers = message.headers();
    let hget = |k: &str| headers.get(k).ok().flatten().unwrap_or_default();
    let subject = hget("subject");
    let message_id = hget("message-id");
    // Cloudflare stamps SPF/DKIM/DMARC results here — the sender-verification source.
    let auth_results = hget("authentication-results");
    let size = message.raw_size() as u64;

    // The raw stream is single-use; buffer it once into the full RFC822 bytes.
    let raw = message.raw_bytes().await?;
    let now = Date::now().as_millis();

    // Parse BEFORE the archive move: verify sender, decode the body text, read the amount.
    let verified = sender_is_lava(&auth_results);
    let body_text = decode_body(&raw);
    let (amount_minor, currency) = parse_amount(&body_text);

    // Key: a sanitized Message-ID makes a retry of the SAME email overwrite the same
    // object (idempotent archive); fall back to time+size when there's no id.
    let slug = slug_of(&message_id);
    let base = if slug.is_empty() { format!("{now}-{size}") } else { slug };
    let raw_key = format!("raw/{base}.eml");
    let meta_key = format!("meta/{base}.json");

    console_log!(
        "receipt-worker: CAUGHT to={to} from={from} subject={subject:?} msgid={message_id:?} \
         size={size} verified={verified} amount_minor={amount_minor:?} currency={currency:?} key={raw_key}"
    );

    let meta = serde_json::json!({
        "to": to,
        "from": from,
        "subject": subject,
        "messageId": message_id,
        "authResults": auth_results,
        "verified": verified,
        "amountMinor": amount_minor,
        "currency": currency,
        "size": size,
        "receivedMs": now,
        "rawKey": raw_key,
    })
    .to_string();

    match env.bucket("RECEIPTS") {
        Ok(bucket) => {
            put_r2(&bucket, &raw_key, raw.clone()).await;
            put_r2(&bucket, &meta_key, meta.clone().into_bytes()).await;
            // Stable `latest/` copy (overwritten each time) for easy retrieval by hand.
            put_r2(&bucket, "latest/raw.eml", raw).await;
            put_r2(&bucket, "latest/meta.json", meta.into_bytes()).await;
        }
        Err(e) => console_error!("receipt-worker: RECEIPTS bucket binding missing: {e}"),
    }

    // Bind to the payment — ONLY when the sender is a verified lava address (money-safety:
    // never let a spoofed email fabricate a receipt on a payment). Best-effort: a failure
    // here never errors the handler; the raw stays archived either way.
    if verified {
        bind_receipt(&env, &to, &message_id, amount_minor, currency.as_deref(), &body_text).await;
    } else {
        console_warn!("receipt-worker: sender NOT verified as lava — archived only (auth={auth_results:?})");
    }

    Ok(())
}

/// True when Cloudflare's `Authentication-Results` show a PASSING DKIM signature from a
/// lava domain. Envelope/header addresses alone are spoofable; a passing DKIM over a
/// lava-owned domain is the trustworthy signal. lava signs with `header.d=lavatop.app`.
fn sender_is_lava(auth_results: &str) -> bool {
    let a = auth_results.to_ascii_lowercase();
    a.contains("dkim=pass")
        && (a.contains("header.d=lavatop.app") || a.contains("header.d=lava.top"))
}

/// Extract the receipt amount as (minor units ×100, currency). lava writes it as
/// `RUB 50` (ISO code, space, number — no symbol, no decimals). Scans for the first
/// `<CUR> <number>`; returns None if absent. No regex crate (keeps the wasm small).
fn parse_amount(text: &str) -> (Option<i64>, Option<String>) {
    for cur in ["RUB", "USD", "EUR"] {
        let mut from = 0;
        while let Some(rel) = text[from..].find(cur) {
            let after = from + rel + cur.len();
            let rest = &text[after..];
            // Require whitespace between the code and the number.
            let trimmed = rest.trim_start_matches([' ', '\u{a0}', '\t']);
            if trimmed.len() < rest.len() {
                let num: String = trimmed
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
                    .collect();
                let normalized = num.replace(',', ".");
                if let Ok(v) = normalized.trim_end_matches('.').parse::<f64>() {
                    if v.is_finite() {
                        return (Some((v * 100.0).round() as i64), Some(cur.to_string()));
                    }
                }
            }
            from = after;
        }
    }
    (None, None)
}

/// Decode the email's body to readable text. Handles the single-part `text/html`
/// quoted-printable receipt lava sends: split headers/body at the first blank line, then
/// decode per the top-level Content-Transfer-Encoding (quoted-printable / base64 / raw).
/// Multipart bodies aren't split here — the whole raw is preserved in R2 regardless.
fn decode_body(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let (head, body) = match s.find("\r\n\r\n").or_else(|| s.find("\n\n")) {
        Some(i) => {
            let skip = if s[i..].starts_with("\r\n\r\n") { 4 } else { 2 };
            (&s[..i], &s[i + skip..])
        }
        None => return s.into_owned(),
    };
    let cte = head.to_ascii_lowercase();
    if cte.contains("content-transfer-encoding: quoted-printable") {
        String::from_utf8_lossy(&qp_decode(body.as_bytes())).into_owned()
    } else if cte.contains("content-transfer-encoding: base64") {
        let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        match base64_decode(&compact) {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => body.to_string(),
        }
    } else {
        body.to_string()
    }
}

/// Minimal quoted-printable decode: `=\r\n`/`=\n` soft line breaks are dropped, `=XX`
/// hex escapes become their byte, everything else is literal.
fn qp_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'=' {
            if i + 2 < input.len() && input[i + 1] == b'\r' && input[i + 2] == b'\n' {
                i += 3;
                continue;
            }
            if i + 1 < input.len() && input[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            if i + 2 < input.len() {
                if let (Some(h), Some(l)) = (hexval(input[i + 1]), hexval(input[i + 2])) {
                    out.push((h << 4) | l);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

fn hexval(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Standard base64 decode (no external crate). Returns None on malformed input.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut buf = 0u32;
        let mut n = 0;
        for &b in chunk {
            buf = (buf << 6) | val(b)? as u32;
            n += 1;
        }
        buf <<= 6 * (4 - n);
        for k in 0..(n - 1) {
            out.push((buf >> (16 - 8 * k)) as u8);
        }
    }
    Some(out)
}

/// POST the parsed receipt to payment-worker `/internal/receipt` over the service binding,
/// authenticated by the shared INTERNAL_PUSH_KEY. Best-effort: every failure is logged, none
/// propagates. `to` is the (lowercased) recipient address → resolved to a claim on that side;
/// `body_text` is the full decoded receipt, stored on the claim for the admin view.
async fn bind_receipt(
    env: &Env,
    to: &str,
    message_id: &str,
    amount_minor: Option<i64>,
    currency: Option<&str>,
    body_text: &str,
) {
    let key = match internal_key(env).await {
        Some(k) => k,
        None => {
            console_error!("receipt-worker: INTERNAL_PUSH_KEY not configured — cannot bind receipt");
            return;
        }
    };
    let payload = serde_json::json!({
        "email": to,
        "messageId": message_id,
        "amount": amount_minor,
        "currency": currency,
        "bodyText": body_text,
    })
    .to_string();
    let headers = Headers::new();
    let _ = headers.set("Content-Type", "application/json");
    let _ = headers.set("X-Internal-Key", &key);
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload)));
    let req = match Request::new_with_init("https://payment-worker/internal/receipt", &init) {
        Ok(r) => r,
        Err(e) => {
            console_error!("receipt-worker: build bind request failed: {e}");
            return;
        }
    };
    let payment = match env.service("PAYMENT_WORKER") {
        Ok(s) => s,
        Err(e) => {
            console_error!("receipt-worker: PAYMENT_WORKER binding error: {e}");
            return;
        }
    };
    match payment.fetch_request(req).await {
        Ok(mut res) => {
            let status = res.status_code();
            let txt = res.text().await.unwrap_or_default();
            if (200..300).contains(&status) {
                console_log!("receipt-worker: bind {to} -> {status} {txt}");
            } else {
                console_error!("receipt-worker: bind {to} -> {status} {txt}");
            }
        }
        Err(e) => console_error!("receipt-worker: bind {to} failed: {e}"),
    }
}

/// INTERNAL_PUSH_KEY from the Secrets Store (prod) or `[vars]` (dev).
async fn internal_key(env: &Env) -> Option<String> {
    if let Ok(store) = env.secret_store("INTERNAL_PUSH_KEY") {
        if let Ok(Some(v)) = store.get().await {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    env.var("INTERNAL_PUSH_KEY").ok().map(|v| v.to_string()).filter(|s| !s.is_empty())
}

/// FAIL LOUD (never swallow) — but never error the caller, or Email Routing would
/// retry/bounce the message; the log is the signal that a capture was lost.
async fn put_r2(bucket: &worker::Bucket, key: &str, value: Vec<u8>) {
    if let Err(e) = bucket.put(key, value).execute().await {
        console_error!("receipt-worker: R2 put failed key={key}: {e}");
    }
}

/// Reduce a Message-ID to an R2-key-safe slug: drop the angle brackets, keep
/// `[A-Za-z0-9._-]`, replace the rest with `-`. Empty when there's no id.
fn slug_of(message_id: &str) -> String {
    message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
