// codeks 0008: retry temporary capacity failures without losing their classification.
use codex_api::ApiError;
use codex_api::TransportError;
use codex_api::map_api_error;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use codex_protocol::protocol::CodexErrorInfo;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

fn http_error(status: StatusCode, code: &str, retry_after: Option<RetryAfter>) -> CodexErr {
    map_api_error(ApiError::Transport(TransportError::Http {
        status,
        url: None,
        headers: None,
        body: Some(json!({"error": {"code": code}}).to_string()),
        retry_after,
    }))
}

#[tokio::test(start_paused = true)]
async fn capacity_errors_preserve_classification_and_retry_with_or_without_advice() {
    for (status, code, expected) in [
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "policy_capacity_unavailable",
            CodexErrorInfo::PolicyCapacityUnavailable,
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "server_is_overloaded",
            CodexErrorInfo::ServerOverloaded,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "flex_unavailable",
            CodexErrorInfo::FlexUnavailable,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            CodexErrorInfo::ResponseTooManyFailedAttempts {
                http_status_code: Some(429),
            },
        ),
    ] {
        for advice in [None, RetryAfter::from_delay(Duration::from_secs(3))] {
            let error = http_error(status, code, advice);
            assert_eq!(error.to_codex_protocol_error(), expected);
            assert_eq!(error.retry_after(), advice);
            let delays = [1, 4, 14]
                .map(|attempt| error.retry_delay(attempt).expect("capacity is retryable"));
            if advice.is_some() {
                assert_eq!(delays, [Duration::from_secs(3); 3]);
            } else {
                assert!(delays[0] > Duration::ZERO);
                // The wide margin tolerates jitter while detecting a fixed delay or cap.
                assert!(delays[1] > delays[0] * 4);
                assert!(delays[2] > delays[1] * 4);
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn request_and_quota_errors_remain_terminal_even_with_retry_advice() {
    for (status, code, expected) in [
        (
            StatusCode::BAD_REQUEST,
            "invalid_prompt",
            CodexErrorInfo::InvalidPrompt,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "insufficient_quota",
            CodexErrorInfo::UsageLimitExceeded,
        ),
    ] {
        let error = http_error(status, code, RetryAfter::from_delay(Duration::from_secs(3)));
        assert_eq!(error.to_codex_protocol_error(), expected);
        assert_eq!(error.retry_delay(1), None);
        assert_eq!(error.retry_delay(14), None);
    }
}
