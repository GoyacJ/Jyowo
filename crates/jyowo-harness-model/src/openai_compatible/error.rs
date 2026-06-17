use std::time::Duration;

use harness_contracts::ModelError;
use http::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde_json::Value;

use crate::ErrorClass;

pub(crate) struct OpenAiCompatibleError {
    pub(crate) error: ModelError,
    pub(crate) class: ErrorClass,
    pub(crate) retry_after: Option<Duration>,
}

pub(crate) fn map_transport_error(error: reqwest::Error) -> OpenAiCompatibleError {
    if error.is_timeout() {
        return OpenAiCompatibleError {
            error: ModelError::DeadlineExceeded(Duration::ZERO),
            class: ErrorClass::Fatal,
            retry_after: None,
        };
    }

    OpenAiCompatibleError {
        error: ModelError::ProviderUnavailable(error.to_string()),
        class: ErrorClass::Transient,
        retry_after: None,
    }
}

pub(crate) async fn map_response_error(response: reqwest::Response) -> OpenAiCompatibleError {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_retry_after);
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str::<Value>(&body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.pointer("/error/code"))
                .and_then(Value::as_str)
        })
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("provider error"))
        .to_owned();

    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => OpenAiCompatibleError {
            error: ModelError::AuthExpired(message),
            class: ErrorClass::AuthExpired,
            retry_after: None,
        },
        StatusCode::TOO_MANY_REQUESTS => OpenAiCompatibleError {
            error: ModelError::RateLimited(message),
            class: ErrorClass::RateLimited { retry_after },
            retry_after,
        },
        StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => OpenAiCompatibleError {
            error: ModelError::InvalidRequest(message),
            class: ErrorClass::Fatal,
            retry_after: None,
        },
        status if status.is_server_error() => OpenAiCompatibleError {
            error: ModelError::ProviderUnavailable(message),
            class: ErrorClass::Transient,
            retry_after: None,
        },
        _ => OpenAiCompatibleError {
            error: ModelError::UnexpectedResponse(message),
            class: ErrorClass::Fatal,
            retry_after: None,
        },
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    value.parse::<u64>().ok().map(Duration::from_secs)
}
