use actix_web::{FromRequest, HttpRequest, dev::Payload, web::Bytes};
use serde::de::DeserializeOwned;
use std::marker::PhantomData;

pub struct Postcard<T>(pub T, PhantomData<T>);

impl<T> Postcard<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: DeserializeOwned + 'static> FromRequest for Postcard<T> {
    type Error = actix_web::Error;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let content_type = req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes_fut = Bytes::from_request(req, payload);

        Box::pin(async move {
            if content_type != api_types::CONTENT_TYPE {
                return Err(actix_web::error::ErrorBadRequest(format!(
                    "expected content-type {}, got {content_type}",
                    api_types::CONTENT_TYPE
                )));
            }

            let body = bytes_fut.await?;
            let value: T = api_types::decode(&body)
                .map_err(|e| actix_web::error::ErrorBadRequest(format!("decode error: {e}")))?;

            Ok(Postcard(value, PhantomData))
        })
    }
}
