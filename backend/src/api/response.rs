use actix_web::{HttpResponse, Responder};
use api_types::{ApiError, ApiResponseEnvelope};
use serde::Serialize;

pub struct ApiResponse<T>(pub Result<T, ApiError>);

impl<T: Serialize> Responder for ApiResponse<T> {
    type Body = actix_web::body::BoxBody;

    fn respond_to(self, _req: &actix_web::HttpRequest) -> HttpResponse<Self::Body> {
        let (status, envelope) = match self.0 {
            Ok(value) => (
                actix_web::http::StatusCode::OK,
                ApiResponseEnvelope::Ok(value),
            ),
            Err(ref err) => {
                let status = match err {
                    ApiError::NotFound => actix_web::http::StatusCode::NOT_FOUND,
                    ApiError::BadRequest(_) => actix_web::http::StatusCode::BAD_REQUEST,
                    ApiError::InternalError => {
                        actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, ApiResponseEnvelope::Err(err.clone()))
            }
        };

        let body = api_types::encode(&envelope);
        HttpResponse::build(status)
            .content_type(api_types::CONTENT_TYPE)
            .body(body)
    }
}

impl<T> From<T> for ApiResponse<T> {
    fn from(value: T) -> Self {
        ApiResponse(Ok(value))
    }
}
