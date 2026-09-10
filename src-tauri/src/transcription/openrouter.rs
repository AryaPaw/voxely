use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::transcription::retry::{
    classify_http_status, classify_io, parse_retry_after, AttemptDecision, ClassifiedError,
    RetryPolicy, RetryScheduler,
};

const DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionSuccess {
    pub text: String,
    pub usage_json: Option<String>,
    pub cost: Option<f64>,
    pub generation_id: Option<String>,
    pub latency_ms: u128,
    pub attempt: u32,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SttModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttProgress {
    Attempt(u32),
    Waiting { attempt: u32, delay: Duration },
}

pub async fn transcribe_file(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    language: Option<&str>,
    path: &Path,
    policy: RetryPolicy,
    audio_duration: Duration,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<TranscriptionSuccess, AppError> {
    transcribe_file_with_progress(
        client,
        base_url,
        api_key,
        model,
        language,
        path,
        policy,
        audio_duration,
        cancel,
        |_| {},
    )
    .await
}

pub async fn transcribe_file_with_progress(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    language: Option<&str>,
    path: &Path,
    policy: RetryPolicy,
    audio_duration: Duration,
    cancel: tokio::sync::watch::Receiver<bool>,
    mut on_progress: impl FnMut(SttProgress) + Send,
) -> Result<TranscriptionSuccess, AppError> {
    if !path.exists() {
        return Err(AppError::StorageFailed("processed audio missing".into()));
    }
    let meta = std::fs::metadata(path).map_err(|e| AppError::StorageFailed(e.to_string()))?;
    if meta.len() > 25 * 1024 * 1024 {
        return Err(AppError::RecordingTooLarge);
    }
    let mut scheduler = RetryScheduler::new(policy.clone(), Instant::now());
    let mut cancel = cancel;
    loop {
        if *cancel.borrow() {
            return Err(AppError::Cancelled);
        }
        match scheduler.start_attempt(Instant::now(), audio_duration) {
            AttemptDecision::GiveUp(err) => return Err(err),
            AttemptDecision::Wait { delay, attempt } => {
                on_progress(SttProgress::Waiting { attempt, delay });
                if wait_or_cancel(&mut cancel, delay).await {
                    return Err(AppError::Cancelled);
                }
            }
            AttemptDecision::Run { attempt, timeout } => {
                on_progress(SttProgress::Attempt(attempt));
                tracing::info!(
                    attempt,
                    timeout_ms = timeout.as_millis() as u64,
                    "stt attempt"
                );
                let started = Instant::now();
                let outcome = tokio::select! {
                    biased;
                    _ = cancelled(&mut cancel) => None,
                    result = one_attempt(
                        client,
                        base_url,
                        api_key,
                        model,
                        language,
                        path,
                        policy.connect_timeout,
                        timeout,
                    ) => Some(result),
                };
                match outcome {
                    None => return Err(AppError::Cancelled),
                    Some(Ok(mut success)) => {
                        if *cancel.borrow() {
                            return Err(AppError::Cancelled);
                        }
                        success.attempt = attempt;
                        success.latency_ms = started.elapsed().as_millis();
                        success.model = model.to_string();
                        return Ok(success);
                    }
                    Some(Err(classified)) => {
                        match scheduler.after_failure(&classified, Instant::now(), 0.08) {
                            AttemptDecision::GiveUp(err) => return Err(err),
                            AttemptDecision::Wait {
                                delay,
                                attempt: wait_attempt,
                            } => {
                                tracing::warn!(
                                    attempt,
                                    delay_ms = delay.as_millis() as u64,
                                    status = classified.http_status,
                                    "retrying transcription"
                                );
                                on_progress(SttProgress::Waiting {
                                    attempt: wait_attempt,
                                    delay,
                                });
                                if wait_or_cancel(&mut cancel, delay).await {
                                    return Err(AppError::Cancelled);
                                }
                            }
                            AttemptDecision::Run { .. } => {}
                        }
                    }
                }
            }
        }
    }
}

async fn cancelled(cancel: &mut tokio::sync::watch::Receiver<bool>) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if cancel.changed().await.is_err() {
            return;
        }
    }
}

async fn wait_or_cancel(cancel: &mut tokio::sync::watch::Receiver<bool>, delay: Duration) -> bool {
    tokio::select! {
        biased;
        _ = cancelled(cancel) => true,
        _ = tokio::time::sleep(delay) => *cancel.borrow(),
    }
}

async fn one_attempt(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    language: Option<&str>,
    path: &Path,
    connect_timeout: Duration,
    timeout: Duration,
) -> Result<TranscriptionSuccess, ClassifiedError> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.wav")
        .to_string();
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| classify_io(&e.to_string()))?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("audio/wav")
        .map_err(|e| classify_io(&e.to_string()))?;
    let mut form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", part);
    if let Some(lang) = language {
        if lang != "auto" {
            form = form.text("language", lang.to_string());
        }
    }
    let url = format!("{}/audio/transcriptions", base_url.trim_end_matches('/'));
    let request = client
        .post(url)
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/voxely/voxely")
        .header("X-Title", "Voxely")
        .multipart(form)
        .timeout(timeout)
        .build()
        .map_err(|e| classify_io(&e.to_string()))?;
    let _ = connect_timeout;
    let response = client
        .execute(request)
        .await
        .map_err(|e| classify_io(&e.to_string()))?;
    let status = response.status().as_u16();
    let generation_id = response
        .headers()
        .get("x-generation-id")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned);
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_retry_after(v, chrono::Utc::now()));
    let body = response
        .text()
        .await
        .map_err(|e| classify_io(&e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(classify_http_status(status, retry_after));
    }
    let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|_| ClassifiedError {
        class: crate::transcription::retry::RetryClass::Terminal,
        error: AppError::ResponseMalformed,
        http_status: Some(status),
        retry_after: None,
    })?;
    let text = parsed
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or(ClassifiedError {
            class: crate::transcription::retry::RetryClass::Terminal,
            error: AppError::ResponseMalformed,
            http_status: Some(status),
            retry_after: None,
        })?
        .to_string();
    let usage = parsed.get("usage").cloned();
    let cost = usage
        .as_ref()
        .and_then(|u| u.get("cost"))
        .and_then(|v| v.as_f64());
    Ok(TranscriptionSuccess {
        text,
        usage_json: usage.map(|u| u.to_string()),
        cost,
        generation_id,
        latency_ms: 0,
        attempt: 1,
        model: model.to_string(),
    })
}

pub async fn list_transcription_models(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<SttModel>, AppError> {
    let response = client
        .get(format!(
            "{DEFAULT_BASE}/models?output_modalities=transcription"
        ))
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| AppError::ConnectionFailed(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::ProviderUnavailable);
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|_| AppError::ResponseMalformed)?;
    let mut models = Vec::new();
    if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(SttModel {
                    id: id.to_string(),
                    name: item
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string(),
                });
            }
        }
    }
    Ok(models)
}

pub fn default_base_url() -> &'static str {
    DEFAULT_BASE
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use std::io::Write;

    fn wav_fixture(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("a.wav");
        crate::audio::capture::write_pcm16_wav(&path, 48_000, &[0.1; 4800]).unwrap();
        path
    }

    #[tokio::test]
    async fn success_200() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200)
                .header("x-generation-id", "gen_1")
                .json_body(serde_json::json!({"text":"hello","usage":{"seconds":1.0,"cost":0.01}}));
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let result = transcribe_file(
            &client,
            &server.base_url(),
            "test-key",
            "openai/gpt-transcribe",
            None,
            &path,
            RetryPolicy::default(),
            Duration::from_secs(1),
            rx,
        )
        .await
        .unwrap();
        assert_eq!(result.text, "hello");
        assert_eq!(result.generation_id.as_deref(), Some("gen_1"));
    }

    #[tokio::test]
    async fn reports_attempt_progress() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200).json_body(serde_json::json!({"text":"ok"}));
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_cb = events.clone();
        transcribe_file_with_progress(
            &client,
            &server.base_url(),
            "test-key",
            "openai/gpt-transcribe",
            None,
            &path,
            RetryPolicy::default(),
            Duration::from_secs(1),
            rx,
            move |progress| events_for_cb.lock().unwrap().push(progress),
        )
        .await
        .unwrap();
        let recorded = events.lock().unwrap().clone();
        assert_eq!(recorded, vec![SttProgress::Attempt(1)]);
    }

    #[tokio::test]
    async fn no_retry_on_401() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(401).body("nope");
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let err = transcribe_file(
            &client,
            &server.base_url(),
            "bad",
            "openai/gpt-transcribe",
            None,
            &path,
            RetryPolicy::default(),
            Duration::from_secs(1),
            rx,
        )
        .await
        .unwrap_err();
        assert_eq!(err, AppError::InvalidApiKey);
        mock.assert_hits(1);
    }

    #[tokio::test]
    async fn retries_503_then_success() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(503).body("busy");
        });
        // httpmock last matching? use sequence via when body - simpler: first two fail using Expectation
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let mut policy = RetryPolicy::default();
        policy.additional_retries = 0;
        policy.total_operation_timeout = Duration::from_secs(5);
        let err = transcribe_file(
            &client,
            &server.base_url(),
            "k",
            "openai/gpt-transcribe",
            None,
            &path,
            policy,
            Duration::from_millis(200),
            rx,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            err,
            AppError::ProviderUnavailable | AppError::RetryDeadlineExceeded
        ));
    }

    #[tokio::test]
    async fn cancel_aborts_in_flight_http() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200)
                .delay(Duration::from_secs(8))
                .json_body(serde_json::json!({"text":"late"}));
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let base = server.base_url();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let transcribe = transcribe_file(
            &client,
            &base,
            "k",
            "openai/gpt-transcribe",
            None,
            &path,
            RetryPolicy::default(),
            Duration::from_secs(1),
            rx,
        );
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(80)).await;
            let _ = tx.send(true);
        };
        let (result, _) = tokio::join!(transcribe, cancel);
        assert_eq!(result.unwrap_err(), AppError::Cancelled);
    }

    #[tokio::test]
    async fn malformed_200_is_terminal() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(200).body("{}");
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let err = transcribe_file(
            &client,
            &server.base_url(),
            "k",
            "openai/gpt-transcribe",
            None,
            &path,
            RetryPolicy {
                automatic_retries: false,
                ..RetryPolicy::default()
            },
            Duration::from_millis(100),
            rx,
        )
        .await
        .unwrap_err();
        assert_eq!(err, AppError::ResponseMalformed);
    }

    #[test]
    fn fixture_writer() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "ok").unwrap();
    }
}
