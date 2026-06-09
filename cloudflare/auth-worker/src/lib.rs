use worker::*;

mod auth;
mod recovery;
mod token;
mod types;
mod user_do;

pub use user_do::UserDO;

fn add_cors(resp: Response) -> Result<Response> {
    let mut headers = Headers::new();
    let _ = headers.set("Access-Control-Allow-Origin", "*");
    let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
    // Copy original headers
    for (k, v) in resp.headers() {
        let _ = headers.set(&k, &v);
    }
    let status = resp.status_code();
    Ok(Response::from_body(resp.body().clone())?.with_headers(headers).with_status(status))
}

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    if req.method() == Method::Options {
        let mut headers = Headers::new();
        let _ = headers.set("Access-Control-Allow-Origin", "*");
        let _ = headers.set("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
        let _ = headers.set("Access-Control-Allow-Headers", "Content-Type, Authorization");
        let _ = headers.set("Access-Control-Max-Age", "86400");
        return Ok(Response::empty()?.with_headers(headers).with_status(204));
    }

    let router = Router::new();

    let result = router
        .post_async("/register/begin", auth::register_begin)
        .post_async("/register/finish", auth::register_finish)
        .post_async("/authenticate/begin", auth::authenticate_begin)
        .post_async("/authenticate/finish", auth::authenticate_finish)
        .post_async("/add-device/begin", auth::add_device_begin)
        .post_async("/add-device/finish", auth::add_device_finish)
        .post_async("/recovery/set", recovery::set_recovery_key)
        .post_async("/recovery/authenticate", recovery::authenticate_with_recovery)
        .post_async("/token/validate", token::validate_token)
        .run(req, env)
        .await;

    match result {
        Ok(resp) => add_cors(resp),
        Err(e) => {
            let body = serde_json::json!({ "error": e.to_string() });
            let mut resp = Response::from_json(&body)?.with_status(500);
            let headers = resp.headers_mut();
            let _ = headers.set("Access-Control-Allow-Origin", "*");
            Ok(resp)
        }
    }
}
