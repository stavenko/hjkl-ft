use serde::{de::DeserializeOwned, Serialize};
use api_types::{ApiResponseEnvelope, ApiError};

use super::config;

pub async fn post<I: Serialize, O: DeserializeOwned>(
    path: &str,
    input: &I,
) -> Result<O, ApiError> {
    let base = &config::get().api_base_url;
    let url = format!("{base}{path}");
    let body = api_types::encode(input);

    let resp = gloo_net::http::Request::post(&url)
        .header("content-type", api_types::CONTENT_TYPE)
        .body(body)
        .expect("failed to build request")
        .send()
        .await
        .map_err(|_| ApiError::InternalError)?;

    let bytes = resp.binary().await.map_err(|_| ApiError::InternalError)?;
    let envelope: ApiResponseEnvelope<O> =
        api_types::decode(&bytes).map_err(|_| ApiError::InternalError)?;

    match envelope {
        ApiResponseEnvelope::Ok(value) => Ok(value),
        ApiResponseEnvelope::Err(err) => Err(err),
    }
}
