use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    pub automatic_retries: bool,
    pub additional_retries: u32,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub initial_retry_delay: Duration,
    pub max_retry_delay: Duration,
    pub total_operation_timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            automatic_retries: true,
            additional_retries: 3,
            connect_timeout: Duration::from_secs(8),
            request_timeout: Duration::from_secs(20),
            initial_retry_delay: Duration::from_millis(500),
            max_retry_delay: Duration::from_secs(3),
            total_operation_timeout: Duration::from_secs(12 * 60),
        }
    }
}

impl RetryPolicy {
    pub fn max_attempts(&self) -> u32 {
        if self.automatic_retries {
            1 + self.additional_retries
        } else {
            1
        }
    }

    pub fn scaled_request_timeout(&self, audio_duration: Duration) -> Duration {
        let extra_ms = (audio_duration.as_secs_f64() * 2.5 * 1000.0) as u64;
        let from_audio = Duration::from_millis(20_000 + extra_ms);
        let scaled = self.request_timeout.max(from_audio);
        scaled.min(Duration::from_secs(15 * 60))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    Retryable,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedError {
    pub class: RetryClass,
    pub error: AppError,
    pub http_status: Option<u16>,
    pub retry_after: Option<Duration>,
}

pub fn classify_http_status(status: u16, retry_after: Option<Duration>) -> ClassifiedError {
    match status {
        408 | 429 | 500 | 502 | 503 | 504 => ClassifiedError {
            class: RetryClass::Retryable,
            error: match status {
                408 => AppError::RequestTimeout,
                429 => AppError::RateLimited,
                502..=504 => AppError::ProviderUnavailable,
                _ => AppError::OpenRouterServerError,
            },
            http_status: Some(status),
            retry_after,
        },
        401 | 403 => ClassifiedError {
            class: RetryClass::Terminal,
            error: AppError::InvalidApiKey,
            http_status: Some(status),
            retry_after: None,
        },
        400 | 404 | 413 | 422 => ClassifiedError {
            class: RetryClass::Terminal,
            error: if status == 413 {
                AppError::RecordingTooLarge
            } else {
                AppError::RequestValidationFailed(format!("HTTP {status}"))
            },
            http_status: Some(status),
            retry_after: None,
        },
        other if (500..600).contains(&other) => ClassifiedError {
            class: RetryClass::Retryable,
            error: AppError::OpenRouterServerError,
            http_status: Some(other),
            retry_after,
        },
        other => ClassifiedError {
            class: RetryClass::Terminal,
            error: AppError::RequestValidationFailed(format!("HTTP {other}")),
            http_status: Some(other),
            retry_after: None,
        },
    }
}

pub fn classify_http(status: u16, retry_after: Option<Duration>, body: &str) -> ClassifiedError {
    let lower = body.to_ascii_lowercase();
    let provider_gap = lower.contains("could not be reached")
        || lower.contains("no available")
        || lower.contains("overloaded")
        || lower.contains("no endpoints")
        || lower.contains("provider returned error");
    if provider_gap && status != 401 && status != 403 {
        return ClassifiedError {
            class: RetryClass::Retryable,
            error: AppError::ProviderUnavailable,
            http_status: Some(status),
            retry_after,
        };
    }
    if (status == 400 || status == 404 || status == 422)
        && (lower.contains("invalid model")
            || lower.contains("model not found")
            || lower.contains("does not exist"))
    {
        return ClassifiedError {
            class: RetryClass::Terminal,
            error: AppError::InvalidModel(truncated_body(body)),
            http_status: Some(status),
            retry_after: None,
        };
    }
    classify_http_status(status, retry_after)
}

pub fn truncated_body(body: &str) -> String {
    const LIMIT: usize = 400;
    let compact: String = body.chars().take(LIMIT).collect();
    if body.chars().count() > LIMIT {
        format!("{compact}...")
    } else {
        compact
    }
}

pub fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(inner) = source {
        out.push_str(": ");
        out.push_str(&inner.to_string());
        source = inner.source();
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportCategory {
    TlsTrust,
    ConnectEof,
    Timeout,
    Reset,
    Http,
    Other,
}

impl TransportCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TlsTrust => "tls_trust",
            Self::ConnectEof => "connect_eof",
            Self::Timeout => "timeout",
            Self::Reset => "reset",
            Self::Http => "http",
            Self::Other => "other",
        }
    }
}

pub fn transport_category(message: &str) -> TransportCategory {
    let lower = message.to_ascii_lowercase();
    if lower.contains("unknown issuer")
        || lower.contains("unknownissuer")
        || lower.contains("invalid certificate")
        || lower.contains("invalid peer certificate")
        || lower.contains("certificate verify")
        || lower.contains("unknown ca")
        || lower.contains("notvalidforname")
        || lower.contains("webpki::error")
    {
        TransportCategory::TlsTrust
    } else if lower.contains("handshake eof")
        || (lower.contains("tls") && lower.contains("eof"))
        || (lower.contains("handshake") && lower.contains("eof"))
    {
        TransportCategory::ConnectEof
    } else if lower.contains("timed out") || lower.contains("timeout") {
        TransportCategory::Timeout
    } else if lower.contains("connection reset") || lower.contains("reset by peer") {
        TransportCategory::Reset
    } else {
        TransportCategory::Other
    }
}

pub fn error_category(error: &AppError) -> &'static str {
    match error {
        AppError::RequestTimeout => TransportCategory::Timeout.as_str(),
        AppError::RateLimited | AppError::ProviderUnavailable | AppError::OpenRouterServerError => {
            TransportCategory::Http.as_str()
        }
        AppError::ConnectionFailed(message) => transport_category(message).as_str(),
        AppError::NetworkUnavailable => TransportCategory::Other.as_str(),
        _ => TransportCategory::Other.as_str(),
    }
}

pub fn is_connect_stage_eof(error: &AppError) -> bool {
    matches!(
        error,
        AppError::ConnectionFailed(message)
            if transport_category(message) == TransportCategory::ConnectEof
    )
}

fn is_connect_stage_timeout(lower: &str) -> bool {
    lower.contains("handshake")
        || lower.contains("error trying to connect")
        || lower.contains("tcp connect")
        || lower.contains("connection timed out")
        || (lower.contains("connect") && lower.contains("timeout"))
}

pub fn classify_reqwest(err: &reqwest::Error) -> ClassifiedError {
    let message = error_chain(err);
    if err.is_timeout() {
        if err.is_connect() || is_connect_stage_timeout(&message.to_ascii_lowercase()) {
            return ClassifiedError {
                class: RetryClass::Retryable,
                error: AppError::ConnectionFailed(message),
                http_status: None,
                retry_after: None,
            };
        }
        return ClassifiedError {
            class: RetryClass::Retryable,
            error: AppError::RequestTimeout,
            http_status: None,
            retry_after: None,
        };
    }
    classify_io(&message)
}

pub fn classify_io(message: &str) -> ClassifiedError {
    let lower = message.to_ascii_lowercase();
    let category = transport_category(message);
    let retryable = match category {
        TransportCategory::TlsTrust => false,
        TransportCategory::ConnectEof | TransportCategory::Timeout | TransportCategory::Reset => {
            true
        }
        TransportCategory::Http | TransportCategory::Other => {
            lower.contains("timed out")
                || lower.contains("timeout")
                || lower.contains("connection refused")
                || lower.contains("connection reset")
                || lower.contains("dns")
                || lower.contains("network")
                || lower.contains("unreachable")
                || lower.contains("temporarily")
                || lower.contains("tls")
                || lower.contains("reset by peer")
                || lower.contains("error sending request")
                || lower.contains("error trying to connect")
                || lower.contains("tcp connect")
        }
    };
    ClassifiedError {
        class: if retryable {
            RetryClass::Retryable
        } else {
            RetryClass::Terminal
        },
        error: if category == TransportCategory::Timeout {
            if is_connect_stage_timeout(&lower) {
                AppError::ConnectionFailed(message.to_string())
            } else {
                AppError::RequestTimeout
            }
        } else if retryable && (lower.contains("dns") || lower.contains("network")) {
            AppError::NetworkUnavailable
        } else {
            AppError::ConnectionFailed(message.to_string())
        },
        http_status: None,
        retry_after: None,
    }
}

pub fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    if let Ok(seconds) = value.trim().parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(value).ok().map(|system| {
        let later = chrono::DateTime::<chrono::Utc>::from(system);
        let delta = later.signed_duration_since(now);
        Duration::from_millis(delta.num_milliseconds().max(0) as u64)
    })
}

pub fn backoff_delay(policy: &RetryPolicy, failed_attempts: u32, jitter_ratio: f64) -> Duration {
    let exp = failed_attempts.saturating_sub(1).min(8);
    let factor = 2u32.saturating_pow(exp);
    let raw = policy
        .initial_retry_delay
        .saturating_mul(factor)
        .min(policy.max_retry_delay);
    let jitter = (raw.as_secs_f64() * jitter_ratio.clamp(0.0, 0.2)).max(0.0);
    Duration::from_secs_f64(raw.as_secs_f64() + jitter).min(policy.max_retry_delay)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptDecision {
    Run { attempt: u32, timeout: Duration },
    Wait { attempt: u32, delay: Duration },
    GiveUp(AppError),
}

pub struct RetryScheduler {
    pub policy: RetryPolicy,
    pub deadline: Instant,
    pub attempts_used: u32,
}

impl RetryScheduler {
    pub fn new(policy: RetryPolicy, now: Instant) -> Self {
        let deadline = now + policy.total_operation_timeout;
        Self {
            policy,
            deadline,
            attempts_used: 0,
        }
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }

    pub fn start_attempt(&mut self, now: Instant, audio_duration: Duration) -> AttemptDecision {
        if now >= self.deadline {
            return AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded);
        }
        if self.attempts_used >= self.policy.max_attempts() {
            return AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded);
        }
        self.attempts_used += 1;
        let timeout = self
            .policy
            .scaled_request_timeout(audio_duration)
            .min(self.remaining(now));
        if timeout.is_zero() {
            return AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded);
        }
        AttemptDecision::Run {
            attempt: self.attempts_used,
            timeout,
        }
    }

    pub fn after_failure(
        &self,
        classified: &ClassifiedError,
        now: Instant,
        jitter_ratio: f64,
    ) -> AttemptDecision {
        if classified.class == RetryClass::Terminal {
            return AttemptDecision::GiveUp(classified.error.clone());
        }
        if self.attempts_used >= self.policy.max_attempts() {
            return AttemptDecision::GiveUp(classified.error.clone());
        }
        let remaining = self.remaining(now);
        if remaining.is_zero() {
            return AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded);
        }
        let mut delay = if is_connect_stage_eof(&classified.error) {
            let jitter = (0.08_f64 * jitter_ratio.clamp(0.0, 1.0)).max(0.0);
            Duration::from_secs_f64(0.08 + jitter)
        } else {
            backoff_delay(&self.policy, self.attempts_used, jitter_ratio)
        };
        if let Some(retry_after) = classified.retry_after {
            delay = delay.max(retry_after);
        }
        delay = delay.min(self.policy.max_retry_delay).min(remaining);
        if delay >= remaining {
            return AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded);
        }
        AttemptDecision::Wait {
            attempt: self.attempts_used,
            delay,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additional_retries_mean_initial_plus_n() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_attempts(), 4);
        let mut off = policy.clone();
        off.automatic_retries = false;
        assert_eq!(off.max_attempts(), 1);
    }

    #[test]
    fn terminal_statuses_do_not_retry() {
        for status in [400, 401, 403, 404, 413, 422] {
            assert_eq!(
                classify_http_status(status, None).class,
                RetryClass::Terminal
            );
        }
    }

    #[test]
    fn retryable_statuses() {
        for status in [408, 429, 500, 502, 503, 504] {
            assert_eq!(
                classify_http_status(status, None).class,
                RetryClass::Retryable
            );
        }
    }

    #[test]
    fn sending_request_failure_is_retryable() {
        let classified = classify_io(
            "error sending request for url (https://openrouter.ai/api/v1/audio/transcriptions)",
        );
        assert_eq!(classified.class, RetryClass::Retryable);
        assert!(matches!(classified.error, AppError::ConnectionFailed(_)));
    }

    #[test]
    fn tls_handshake_eof_is_retryable_connect_stage() {
        let classified = classify_io("error sending request: connection error: tls handshake eof");
        assert_eq!(classified.class, RetryClass::Retryable);
        assert!(is_connect_stage_eof(&classified.error));
        assert_eq!(error_category(&classified.error), "connect_eof");
        match classified.error {
            AppError::ConnectionFailed(message) => {
                assert!(message.contains("tls handshake eof"));
            }
            other => panic!("expected ConnectionFailed, got {other}"),
        }
    }

    #[test]
    fn connect_stage_timeout_is_connection_failed_not_request_timeout() {
        let classified = classify_io(
            "error sending request for url (https://openrouter.ai/api/v1/audio/transcriptions): error trying to connect: tcp connect error: connection timed out",
        );
        assert_eq!(classified.class, RetryClass::Retryable);
        match classified.error {
            AppError::ConnectionFailed(message) => {
                assert!(message.contains("connection timed out"));
            }
            other => panic!("expected ConnectionFailed, got {other}"),
        }
    }

    #[test]
    fn total_request_timeout_stays_request_timeout() {
        let classified = classify_io(
            "error sending request for url (https://openrouter.ai/api/v1/audio/transcriptions): operation timed out",
        );
        assert_eq!(classified.error, AppError::RequestTimeout);
        assert_eq!(classified.error.user_message(), "Превышено время ожидания");
    }

    #[test]
    fn certificate_trust_errors_are_terminal() {
        let classified = classify_io("invalid peer certificate: UnknownIssuer");
        assert_eq!(classified.class, RetryClass::Terminal);
        assert_eq!(error_category(&classified.error), "tls_trust");
    }

    #[test]
    fn connect_eof_uses_fast_backoff() {
        let policy = RetryPolicy::default();
        let start = Instant::now();
        let mut scheduler = RetryScheduler::new(policy, start);
        let _ = scheduler.start_attempt(start, Duration::from_secs(1));
        let classified = classify_io("tls handshake eof");
        let decision = scheduler.after_failure(&classified, start, 0.0);
        match decision {
            AttemptDecision::Wait { delay, .. } => {
                assert!(delay <= Duration::from_millis(200));
            }
            other => panic!("expected wait, got {other:?}"),
        }
    }

    #[test]
    fn http_and_timeout_categories() {
        assert_eq!(error_category(&AppError::RequestTimeout), "timeout");
        assert_eq!(error_category(&AppError::OpenRouterServerError), "http");
        assert_eq!(
            transport_category("operation timed out"),
            TransportCategory::Timeout
        );
    }

    #[test]
    fn provider_could_not_be_reached_is_retryable() {
        let classified = classify_http(
            400,
            None,
            r#"{"error":{"message":"Provider returned error: The model could not be reached"}}"#,
        );
        assert_eq!(classified.class, RetryClass::Retryable);
        assert_eq!(classified.error, AppError::ProviderUnavailable);
    }

    #[test]
    fn retry_after_seconds() {
        assert_eq!(
            parse_retry_after("2", chrono::Utc::now()),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn backoff_never_exceeds_max() {
        let policy = RetryPolicy::default();
        let delay = backoff_delay(&policy, 8, 0.2);
        assert!(delay <= policy.max_retry_delay);
    }

    #[test]
    fn overall_deadline_wins() {
        let mut policy = RetryPolicy::default();
        policy.total_operation_timeout = Duration::from_millis(10);
        let start = Instant::now();
        let mut scheduler = RetryScheduler::new(policy, start);
        let _ = scheduler.start_attempt(start, Duration::from_secs(1));
        let classified = classify_http_status(503, Some(Duration::from_secs(30)));
        let decision = scheduler.after_failure(&classified, start + Duration::from_millis(5), 0.0);
        assert_eq!(
            decision,
            AttemptDecision::GiveUp(AppError::RetryDeadlineExceeded)
        );
    }

    #[test]
    fn never_exceeds_max_attempts() {
        let policy = RetryPolicy::default();
        let start = Instant::now();
        let mut scheduler = RetryScheduler::new(policy.clone(), start);
        for _ in 0..policy.max_attempts() {
            match scheduler.start_attempt(start, Duration::ZERO) {
                AttemptDecision::Run { .. } => {}
                other => panic!("expected run, got {other:?}"),
            }
        }
        assert!(matches!(
            scheduler.start_attempt(start, Duration::ZERO),
            AttemptDecision::GiveUp(_)
        ));
    }

    #[test]
    fn ten_second_audio_exceeds_old_forty_second_cap() {
        let timeout = RetryPolicy::default().scaled_request_timeout(Duration::from_secs(10));
        assert!(timeout > Duration::from_secs(40));
    }

    #[test]
    fn eight_minute_audio_is_not_capped_at_forty() {
        let timeout = RetryPolicy::default().scaled_request_timeout(Duration::from_secs(8 * 60));
        assert!(timeout > Duration::from_secs(40));
        assert!(timeout <= Duration::from_secs(15 * 60));
    }

    use proptest::prelude::*;

    proptest::proptest! {
        #[test]
        fn jitter_bounded(failed in 1u32..6, jitter in 0.0f64..0.2) {
            let policy = RetryPolicy::default();
            let delay = backoff_delay(&policy, failed, jitter);
            prop_assert!(delay <= policy.max_retry_delay);
        }
    }
}
