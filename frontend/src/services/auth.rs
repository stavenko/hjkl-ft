use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

const KEY_USER_ID: &str = "user_id";
const KEY_AUTH_TOKEN: &str = "auth_token";
const KEY_TOKEN_ID: &str = "token_id";

fn storage() -> web_sys::Storage {
    web_sys::window()
        .expect("no window")
        .local_storage()
        .ok()
        .flatten()
        .expect("no localStorage")
}

pub fn get_user_id() -> Option<String> {
    storage().get_item(KEY_USER_ID).ok().flatten()
}

fn set_user(user_id: &str) {
    storage().set_item(KEY_USER_ID, user_id).expect("write user_id");
}

fn set_token(token: &str) {
    storage().set_item(KEY_AUTH_TOKEN, token).expect("write token");
    // Extract token_id from JWT payload and store separately
    if let Some(token_id) = extract_token_id_from_jwt(token) {
        storage().set_item(KEY_TOKEN_ID, &token_id).expect("write token_id");
    }
}

fn extract_token_id_from_jwt(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(parts[1]))
        .ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    json.get("token_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub fn get_token() -> Option<String> {
    storage().get_item(KEY_AUTH_TOKEN).ok().flatten()
}

pub fn logout() {
    let s = storage();
    let _ = s.remove_item(KEY_USER_ID);
    let _ = s.remove_item(KEY_AUTH_TOKEN);
    let _ = s.remove_item(KEY_TOKEN_ID);
}

/// Return the token_id stored in localStorage (extracted from JWT on login).
pub fn current_token_id() -> Option<String> {
    storage().get_item(KEY_TOKEN_ID).ok().flatten()
}

fn generate_fingerprint() -> String {
    let window = web_sys::window().expect("no window");
    let ua = window.navigator().user_agent().unwrap_or_default();
    let screen = window.screen().ok();
    let w = screen.as_ref().map(|s| s.width().unwrap_or(0)).unwrap_or(0);
    let h = screen.as_ref().map(|s| s.height().unwrap_or(0)).unwrap_or(0);
    let lang = window.navigator().language().unwrap_or_default();
    let tz = js_sys::Reflect::get(
        &js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new())
            .resolved_options(),
        &"timeZone".into(),
    )
    .ok()
    .and_then(|v| v.as_string())
    .unwrap_or_default();

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("{ua}|{w}x{h}|{lang}|{tz}"));
    let hash = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&hash[..16])
}

fn auth_base_url() -> String {
    let cfg = crate::services::config::get();
    if cfg.auth_base_url.is_empty() {
        cfg.api_base_url.clone()
    } else {
        cfg.auth_base_url.clone()
    }
}

async fn post_json(path: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", auth_base_url(), path);
    let body_str = serde_json::to_string(body).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body_str));

    let headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{:?}", e))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response")?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn b64url_to_u8array(b64: &str) -> js_sys::Uint8Array {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(b64))
        .unwrap_or_default();
    let arr = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    arr.copy_from(&bytes);
    arr
}

fn u8array_to_b64url(arr: &js_sys::Uint8Array) -> String {
    use base64::Engine;
    let bytes = arr.to_vec();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&bytes)
}

/// Build publicKey JS object from server JSON, converting base64url → ArrayBuffer
fn build_create_options(public_key: &serde_json::Value) -> Result<JsValue, String> {
    let challenge = public_key.get("challenge")
        .and_then(|v| v.as_str())
        .ok_or("missing challenge")?;

    let user = public_key.get("user").ok_or("missing user")?;
    let user_id = user.get("id").and_then(|v| v.as_str()).ok_or("missing user.id")?;
    let user_name = user.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let user_display = user.get("displayName").and_then(|v| v.as_str()).unwrap_or(user_name);

    let rp = public_key.get("rp").ok_or("missing rp")?;

    // Build user object with ArrayBuffer id
    let user_obj = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&user_obj, &"id".into(), &b64url_to_u8array(user_id).buffer());
    let _ = js_sys::Reflect::set(&user_obj, &"name".into(), &JsValue::from_str(user_name));
    let _ = js_sys::Reflect::set(&user_obj, &"displayName".into(), &JsValue::from_str(user_display));

    // Build rp object manually
    let rp_obj = js_sys::Object::new();
    if let Some(rp_id) = rp.get("id").and_then(|v| v.as_str()) {
        let _ = js_sys::Reflect::set(&rp_obj, &"id".into(), &JsValue::from_str(rp_id));
    }
    if let Some(rp_name) = rp.get("name").and_then(|v| v.as_str()) {
        let _ = js_sys::Reflect::set(&rp_obj, &"name".into(), &JsValue::from_str(rp_name));
    }

    // Build pubKeyCredParams manually (serde_wasm_bindgen may lose i64 alg values)
    let params = public_key.get("pubKeyCredParams")
        .and_then(|v| v.as_array())
        .ok_or("missing pubKeyCredParams")?;
    let params_js = js_sys::Array::new();
    for p in params {
        let obj = js_sys::Object::new();
        if let Some(alg) = p.get("alg").and_then(|v| v.as_f64()) {
            let _ = js_sys::Reflect::set(&obj, &"alg".into(), &JsValue::from_f64(alg));
        }
        if let Some(t) = p.get("type").and_then(|v| v.as_str()) {
            let _ = js_sys::Reflect::set(&obj, &"type".into(), &JsValue::from_str(t));
        }
        params_js.push(&obj);
    }

    // Build the main object
    let pk = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&pk, &"challenge".into(), &b64url_to_u8array(challenge).buffer());
    let _ = js_sys::Reflect::set(&pk, &"rp".into(), &rp_obj);
    let _ = js_sys::Reflect::set(&pk, &"user".into(), &user_obj);
    let _ = js_sys::Reflect::set(&pk, &"pubKeyCredParams".into(), &params_js);

    if let Some(timeout) = public_key.get("timeout").and_then(|v| v.as_f64()) {
        let _ = js_sys::Reflect::set(&pk, &"timeout".into(), &JsValue::from_f64(timeout));
    }
    if let Some(attestation) = public_key.get("attestation").and_then(|v| v.as_str()) {
        let _ = js_sys::Reflect::set(&pk, &"attestation".into(), &JsValue::from_str(attestation));
    }
    {
        let sel = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&sel, &"authenticatorAttachment".into(), &JsValue::from_str("platform"));
        let _ = js_sys::Reflect::set(&sel, &"residentKey".into(), &JsValue::from_str("required"));
        let _ = js_sys::Reflect::set(&sel, &"requireResidentKey".into(), &JsValue::from_bool(true));
        let _ = js_sys::Reflect::set(&sel, &"userVerification".into(), &JsValue::from_str("required"));
        let _ = js_sys::Reflect::set(&pk, &"authenticatorSelection".into(), &sel);
    }

    Ok(pk.into())
}

/// Build publicKey JS object for authentication
fn build_get_options(public_key: &serde_json::Value) -> Result<JsValue, String> {
    let challenge = public_key.get("challenge")
        .and_then(|v| v.as_str())
        .ok_or("missing challenge")?;

    let pk = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&pk, &"challenge".into(), &b64url_to_u8array(challenge).buffer());

    if let Some(timeout) = public_key.get("timeout").and_then(|v| v.as_f64()) {
        let _ = js_sys::Reflect::set(&pk, &"timeout".into(), &JsValue::from_f64(timeout));
    }
    if let Some(rp_id) = public_key.get("rpId").and_then(|v| v.as_str()) {
        let _ = js_sys::Reflect::set(&pk, &"rpId".into(), &JsValue::from_str(rp_id));
    }
    if let Some(uv) = public_key.get("userVerification").and_then(|v| v.as_str()) {
        let _ = js_sys::Reflect::set(&pk, &"userVerification".into(), &JsValue::from_str(uv));
    }

    // Convert allowCredentials[].id from base64url to ArrayBuffer
    if let Some(allow) = public_key.get("allowCredentials").and_then(|v| v.as_array()) {
        let arr = js_sys::Array::new();
        for cred in allow {
            let obj = js_sys::Object::new();
            if let Some(id) = cred.get("id").and_then(|v| v.as_str()) {
                let _ = js_sys::Reflect::set(&obj, &"id".into(), &b64url_to_u8array(id).buffer());
            }
            if let Some(t) = cred.get("type").and_then(|v| v.as_str()) {
                let _ = js_sys::Reflect::set(&obj, &"type".into(), &JsValue::from_str(t));
            }
            arr.push(&obj);
        }
        let _ = js_sys::Reflect::set(&pk, &"allowCredentials".into(), &arr);
    }

    Ok(pk.into())
}

/// Serialize PublicKeyCredential response → JSON with ArrayBuffer → base64url
fn serialize_credential(credential: &JsValue) -> Result<serde_json::Value, String> {
    let id = js_sys::Reflect::get(credential, &"id".into())
        .ok().and_then(|v| v.as_string())
        .unwrap_or_default();
    let cred_type = js_sys::Reflect::get(credential, &"type".into())
        .ok().and_then(|v| v.as_string())
        .unwrap_or_else(|| "public-key".to_string());

    let response = js_sys::Reflect::get(credential, &"response".into())
        .map_err(|e| format!("{:?}", e))?;

    let client_data_json = js_sys::Reflect::get(&response, &"clientDataJSON".into())
        .map_err(|e| format!("{:?}", e))?;
    let client_data_b64 = u8array_to_b64url(&js_sys::Uint8Array::new(&client_data_json));

    let attestation_object = js_sys::Reflect::get(&response, &"attestationObject".into());
    let authenticator_data = js_sys::Reflect::get(&response, &"authenticatorData".into());
    let signature = js_sys::Reflect::get(&response, &"signature".into());

    let mut resp_json = serde_json::json!({
        "clientDataJSON": client_data_b64,
    });

    if let Ok(att) = attestation_object {
        if !att.is_undefined() {
            resp_json["attestationObject"] = serde_json::Value::String(
                u8array_to_b64url(&js_sys::Uint8Array::new(&att))
            );
        }
    }
    if let Ok(auth_data) = authenticator_data {
        if !auth_data.is_undefined() {
            resp_json["authenticatorData"] = serde_json::Value::String(
                u8array_to_b64url(&js_sys::Uint8Array::new(&auth_data))
            );
        }
    }
    if let Ok(sig) = signature {
        if !sig.is_undefined() {
            resp_json["signature"] = serde_json::Value::String(
                u8array_to_b64url(&js_sys::Uint8Array::new(&sig))
            );
        }
    }

    let raw_id = js_sys::Reflect::get(credential, &"rawId".into())
        .map_err(|e| format!("{:?}", e))?;
    let raw_id_b64 = u8array_to_b64url(&js_sys::Uint8Array::new(&raw_id));

    Ok(serde_json::json!({
        "id": id,
        "rawId": raw_id_b64,
        "type": cred_type,
        "response": resp_json,
    }))
}

/// Register a new account
pub async fn register(display_name: &str) -> Result<String, String> {
    let fingerprint = generate_fingerprint();
    let begin_resp = post_json("/register/begin", &serde_json::json!({
        "display_name": display_name,
        "fingerprint": fingerprint
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let user_id = begin_resp.get("user_id")
        .and_then(|v| v.as_str())
        .ok_or("server did not return user_id")?
        .to_string();

    let public_key = begin_resp.get("publicKey")
        .ok_or("missing publicKey")?;

    let pk_js = build_create_options(public_key)?;

    let create_opts = js_sys::Object::new();
    js_sys::Reflect::set(&create_opts, &"publicKey".into(), &pk_js)
        .map_err(|e| format!("{:?}", e))?;

    let cred_promise = web_sys::window().expect("no window")
        .navigator()
        .credentials()
        .create_with_options(create_opts.unchecked_ref())
        .map_err(|e| {
            leptos::logging::error!("credentials.create error: {:?}", e);
            crate::services::i18n::t("auth.error_passkey").to_string()
        })?;

    let credential = JsFuture::from(cred_promise)
        .await
        .map_err(|e| {
            leptos::logging::error!("PassKey create rejected: {:?}", e);
            crate::services::i18n::t("auth.error_cancelled").to_string()
        })?;

    let credential_json = serialize_credential(&credential)?;

    let finish_resp = post_json("/register/finish", &serde_json::json!({
        "user_id": user_id,
        "credential": credential_json,
        "fingerprint": fingerprint
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let user_id = finish_resp.get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&user_id)
        .to_string();

    set_user(&user_id);

    if let Some(token) = finish_resp.get("token").and_then(|v| v.as_str()) {
        set_token(token);
    }

    Ok(user_id)
}

/// Authenticate with existing PassKey (discoverable credential)
pub async fn authenticate() -> Result<String, String> {
    let fingerprint = generate_fingerprint();
    let begin_resp = post_json("/authenticate/begin", &serde_json::json!({
        "fingerprint": fingerprint
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let public_key = begin_resp.get("publicKey")
        .ok_or("missing publicKey")?;

    let pk_js = build_get_options(public_key)?;

    let get_opts = js_sys::Object::new();
    js_sys::Reflect::set(&get_opts, &"publicKey".into(), &pk_js)
        .map_err(|e| format!("{:?}", e))?;

    let cred_promise = web_sys::window().expect("no window")
        .navigator()
        .credentials()
        .get_with_options(get_opts.unchecked_ref())
        .map_err(|e| {
            leptos::logging::error!("credentials.get error: {:?}", e);
            crate::services::i18n::t("auth.error_passkey").to_string()
        })?;

    let credential = JsFuture::from(cred_promise)
        .await
        .map_err(|e| {
            leptos::logging::error!("PassKey auth rejected: {:?}", e);
            crate::services::i18n::t("auth.error_cancelled").to_string()
        })?;

    let credential_json = serialize_credential(&credential)?;

    let finish_resp = post_json("/authenticate/finish", &serde_json::json!({
        "credential": credential_json,
        "fingerprint": fingerprint
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let user_id = finish_resp.get("user_id")
        .and_then(|v| v.as_str())
        .ok_or("missing user_id")?;
    let token = finish_resp.get("token")
        .and_then(|v| v.as_str())
        .ok_or("missing token")?;

    set_user(user_id);
    set_token(token);
    Ok(user_id.to_string())
}

// ---------------------------------------------------------------------------
// Device pairing
// ---------------------------------------------------------------------------

async fn post_json_auth(path: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let token = get_token().ok_or_else(|| "not authenticated".to_string())?;
    let url = format!("{}{}", auth_base_url(), path);
    let body_str = serde_json::to_string(body).map_err(|e| e.to_string())?;

    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&JsValue::from_str(&body_str));

    let headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
    headers.set("Content-Type", "application/json").map_err(|e| format!("{:?}", e))?;
    headers.set("Authorization", &format!("Bearer {}", token))
        .map_err(|e| format!("{:?}", e))?;
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response")?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    serde_json::from_str(&text).map_err(|e| e.to_string())
}

async fn get_json(path: &str) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", auth_base_url(), path);

    let opts = web_sys::RequestInit::new();
    opts.set_method("GET");

    let headers = web_sys::Headers::new().map_err(|e| format!("{:?}", e))?;
    if let Some(token) = get_token() {
        headers.set("Authorization", &format!("Bearer {}", token))
            .map_err(|e| format!("{:?}", e))?;
    }
    opts.set_headers(&headers);

    let request = web_sys::Request::new_with_str_and_init(&url, &opts)
        .map_err(|e| format!("{:?}", e))?;

    let window = web_sys::window().expect("no window");
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{:?}", e))?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "not a Response")?;

    let text = JsFuture::from(resp.text().map_err(|e| format!("{:?}", e))?)
        .await
        .map_err(|e| format!("{:?}", e))?;
    let text = text.as_string().ok_or("response not string")?;

    if !resp.ok() {
        return Err(format!("HTTP {}: {}", resp.status(), text));
    }

    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Logged-in device creates a pairing invitation.
/// POST /pair/create (authenticated) -> { pairing_id, secret, qr_url }
pub async fn pair_create() -> Result<serde_json::Value, String> {
    post_json_auth("/pair/create", &serde_json::json!({})).await
}

/// New (unauthenticated) device requests a pairing.
/// POST /pair/request -> { pairing_id, secret, qr_url }
pub async fn pair_request() -> Result<serde_json::Value, String> {
    post_json("/pair/request", &serde_json::json!({})).await
}

/// Unauthenticated status check for polling.
pub async fn pair_check(pairing_id: &str, secret: &str) -> Result<serde_json::Value, String> {
    post_json("/pair/check", &serde_json::json!({
        "pairing_id": pairing_id,
        "secret": secret,
    })).await
}

/// New device claims a pairing created by the logged-in device.
/// Receives a publicKey challenge, creates a PassKey, finishes registration.
pub async fn pair_claim(pairing_id: &str, secret: &str) -> Result<String, String> {
    let claim_resp = post_json("/pair/claim", &serde_json::json!({
        "pairing_id": pairing_id,
        "secret": secret,
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let public_key = claim_resp.get("publicKey")
        .ok_or("missing publicKey in claim response")?;
    let user_id = claim_resp.get("user_id")
        .and_then(|v| v.as_str())
        .ok_or("missing user_id in claim response")?
        .to_string();

    let pk_js = build_create_options(public_key)?;

    let create_opts = js_sys::Object::new();
    js_sys::Reflect::set(&create_opts, &"publicKey".into(), &pk_js)
        .map_err(|e| format!("{:?}", e))?;

    let cred_promise = web_sys::window().expect("no window")
        .navigator()
        .credentials()
        .create_with_options(create_opts.unchecked_ref())
        .map_err(|e| {
            leptos::logging::error!("credentials.create error: {:?}", e);
            crate::services::i18n::t("auth.error_passkey").to_string()
        })?;

    let credential = JsFuture::from(cred_promise)
        .await
        .map_err(|e| {
            leptos::logging::error!("PassKey create rejected: {:?}", e);
            crate::services::i18n::t("auth.error_cancelled").to_string()
        })?;

    let credential_json = serialize_credential(&credential)?;

    let finish_resp = post_json("/pair/finish", &serde_json::json!({
        "pairing_id": pairing_id,
        "credential": credential_json,
    })).await
        .map_err(|_| crate::services::i18n::t("auth.error_network").to_string())?;

    let user_id = finish_resp.get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or(&user_id)
        .to_string();

    set_user(&user_id);

    if let Some(token) = finish_resp.get("token").and_then(|v| v.as_str()) {
        set_token(token);
    }

    Ok(user_id)
}

/// Logged-in device approves a pairing request from the new device.
pub async fn pair_approve(pairing_id: &str, secret: &str) -> Result<serde_json::Value, String> {
    post_json_auth("/pair/approve", &serde_json::json!({
        "pairing_id": pairing_id,
        "secret": secret,
    })).await
}

/// Poll pairing status. Returns the JSON with a "status" field.
pub async fn pair_status(pairing_id: &str) -> Result<serde_json::Value, String> {
    get_json(&format!("/pair/status/{}", pairing_id)).await
}

/// Fetch active tokens/sessions for the current user.
pub async fn fetch_tokens() -> Result<Vec<serde_json::Value>, String> {
    let resp = get_json("/tokens").await?;
    // The API returns {"tokens": [...]}, unwrap the inner array
    if let Some(arr) = resp.get("tokens").and_then(|v| v.as_array()) {
        return Ok(arr.clone());
    }
    // Fallback: maybe it's already a flat array
    resp.as_array()
        .cloned()
        .ok_or_else(|| "expected array or {tokens:[...]} from /tokens".to_string())
}

/// Return the fingerprint of the current device (for highlighting in session list).
pub fn current_fingerprint() -> String {
    generate_fingerprint()
}
