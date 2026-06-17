use std::time::Duration;

use harness_contracts::ModelError;
use http::HeaderMap;
use serde_json::Value;

use crate::ErrorClass;

pub(super) struct AnthropicError {
    pub error: ModelError,
    pub class: ErrorClass,
    pub retry_after: Option<Duration>,
}

pub(super) fn map_transport_error(error: reqwest::Error) -> AnthropicError {
    let model_error = if error.is_timeout() {
        ModelError::DeadlineExceeded(Duration::ZERO)
    } else {
        ModelError::ProviderUnavailable(error.to_string())
    };

    AnthropicError {
        error: model_error,
        class: ErrorClass::Transient,
        retry_after: None,
    }
}

pub(super) async fn map_response_error(response: reqwest::Response) -> AnthropicError {
    let status = response.status();
    let retry_after = retry_after(response.headers());
    let body = response.text().await.unwrap_or_default();
    let message = error_message(&body);

    match status.as_u16() {
        401 | 403 => AnthropicError {
            error: ModelError::AuthExpired(message),
            class: ErrorClass::AuthExpired,
            retry_after: None,
        },
        429 => AnthropicError {
            error: ModelError::RateLimited(message),
            class: ErrorClass::RateLimited { retry_after },
            retry_after,
        },
        400 | 422 => AnthropicError {
            error: ModelError::InvalidRequest(message),
            class: ErrorClass::Fatal,
            retry_after: None,
        },
        500..=599 => AnthropicError {
            error: ModelError::ProviderUnavailable(message),
            class: ErrorClass::Transient,
            retry_after,
        },
        _ => AnthropicError {
            error: ModelError::UnexpectedResponse(format!("status={status}: {message}")),
            class: ErrorClass::Fatal,
            retry_after: None,
        },
    }
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.pointer("/message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| body.to_owned())
}
