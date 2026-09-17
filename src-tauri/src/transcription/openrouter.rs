use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::transcription::retry::{
    classify_http, classify_io, classify_reqwest, error_category, error_chain, parse_retry_after,
    truncated_body, AttemptDecision, ClassifiedError, RetryClass, RetryPolicy, RetryScheduler,
};

const DEFAULT_BASE: &str = "https://openrouter.ai/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SttHttpOptions {
    pub http1_only: bool,
    pub pool_max_idle_per_host: usize,
}

pub fn stt_http_options() -> SttHttpOptions {
    SttHttpOptions {
        http1_only: true,
        pool_max_idle_per_host: 2,
    }
}

pub fn build_stt_client(connect_timeout: Duration) -> Result<reqwest::Client, AppError> {
    let opts = stt_http_options();
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .pool_max_idle_per_host(opts.pool_max_idle_per_host)
        .connect_timeout(connect_timeout);
    if opts.http1_only {
        builder = builder.http1_only();
    }
    builder
        .build()
        .map_err(|e| AppError::ConnectionFailed(e.to_string()))
}

pub struct OpenRouterTransport {
    client: reqwest::Client,
    connect_timeout: Duration,
}

impl OpenRouterTransport {
    pub fn new(connect_timeout: Duration) -> Result<Self, AppError> {
        Ok(Self {
            client: build_stt_client(connect_timeout)?,
            connect_timeout,
        })
    }

    pub fn sync(&mut self, connect_timeout: Duration) -> Result<(), AppError> {
        if self.connect_timeout != connect_timeout {
            self.client = build_stt_client(connect_timeout)?;
            self.connect_timeout = connect_timeout;
        }
        Ok(())
    }

    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }
}

const PREWARM_BODY_LIMIT: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrewarmOutcome {
    Ready {
        status: u16,
        latency_ms: u64,
    },
    Failed {
        category: &'static str,
        latency_ms: u64,
    },
}

pub async fn prewarm_connection(client: &reqwest::Client) {
    match prewarm_connection_at(client, DEFAULT_BASE).await {
        PrewarmOutcome::Ready { status, latency_ms } => {
            tracing::debug!(status, latency_ms, "openrouter prewarm ready");
        }
        PrewarmOutcome::Failed {
            category,
            latency_ms,
        } => {
            tracing::debug!(category, latency_ms, "openrouter prewarm failed");
        }
    }
}

pub async fn prewarm_connection_at(client: &reqwest::Client, base_url: &str) -> PrewarmOutcome {
    let started = Instant::now();
    let base = base_url.trim_end_matches('/');
    let models = format!("{base}/models");
    let filtered = format!("{base}/models?output_modalities=transcription");
    match send_prewarm(client, &models, reqwest::Method::HEAD).await {
        Ok(status) if status != 405 && status != 501 => {
            return PrewarmOutcome::Ready {
                status,
                latency_ms: elapsed_ms(started),
            };
        }
        Ok(_) | Err(_) => {}
    }
    match send_prewarm(client, &filtered, reqwest::Method::GET).await {
        Ok(status) => PrewarmOutcome::Ready {
            status,
            latency_ms: elapsed_ms(started),
        },
        Err(err) => PrewarmOutcome::Failed {
            category: error_category(&classify_reqwest(&err).error),
            latency_ms: elapsed_ms(started),
        },
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

async fn send_prewarm(
    client: &reqwest::Client,
    url: &str,
    method: reqwest::Method,
) -> Result<u16, reqwest::Error> {
    let response = client
        .request(method, url)
        .timeout(Duration::from_secs(8))
        .send()
        .await?;
    let status = response.status().as_u16();
    finish_prewarm_body(response).await;
    Ok(status)
}

async fn finish_prewarm_body(response: reqwest::Response) {
    if let Some(len) = response.content_length() {
        if len > PREWARM_BODY_LIMIT as u64 {
            drop(response);
            return;
        }
    }
    let _ = response.bytes().await;
}

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
    Waiting {
        attempt: u32,
        delay: Duration,
    },
    Finished {
        attempt: u32,
        outcome: &'static str,
        http_status: Option<u16>,
        latency_ms: u128,
        category: Option<String>,
    },
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
                    None => {
                        on_progress(SttProgress::Finished {
                            attempt,
                            outcome: "cancelled",
                            http_status: None,
                            latency_ms: started.elapsed().as_millis(),
                            category: Some("cancelled".into()),
                        });
                        return Err(AppError::Cancelled);
                    }
                    Some(Ok(mut success)) => {
                        if *cancel.borrow() {
                            on_progress(SttProgress::Finished {
                                attempt,
                                outcome: "cancelled",
                                http_status: None,
                                latency_ms: started.elapsed().as_millis(),
                                category: Some("cancelled".into()),
                            });
                            return Err(AppError::Cancelled);
                        }
                        success.attempt = attempt;
                        success.latency_ms = started.elapsed().as_millis();
                        success.model = model.to_string();
                        on_progress(SttProgress::Finished {
                            attempt,
                            outcome: "success",
                            http_status: Some(200),
                            latency_ms: success.latency_ms,
                            category: None,
                        });
                        return Ok(success);
                    }
                    Some(Err(classified)) => {
                        tracing::warn!(
                            attempt,
                            status = classified.http_status,
                            category = error_category(&classified.error),
                            retryable = classified.class == RetryClass::Retryable,
                            latency_ms = started.elapsed().as_millis() as u64,
                            error = %classified.error,
                            "stt attempt failed"
                        );
                        match scheduler.after_failure(&classified, Instant::now(), 0.08) {
                            AttemptDecision::GiveUp(err) => {
                                on_progress(SttProgress::Finished {
                                    attempt,
                                    outcome: "error",
                                    http_status: classified.http_status,
                                    latency_ms: started.elapsed().as_millis(),
                                    category: Some(error_category(&classified.error).into()),
                                });
                                return Err(err);
                            }
                            AttemptDecision::Wait {
                                delay,
                                attempt: wait_attempt,
                            } => {
                                on_progress(SttProgress::Finished {
                                    attempt,
                                    outcome: "error",
                                    http_status: classified.http_status,
                                    latency_ms: started.elapsed().as_millis(),
                                    category: Some(error_category(&classified.error).into()),
                                });
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
    let attempt_started = Instant::now();
    let request = client
        .post(url)
        .bearer_auth(api_key)
        .header("HTTP-Referer", crate::app::lifecycle::GITHUB_REPO_URL)
        .header("X-Title", "Voxely")
        .multipart(form)
        .timeout(timeout)
        .build()
        .map_err(|e| classify_reqwest(&e))?;
    tracing::debug!(
        connect_timeout_ms = connect_timeout.as_millis() as u64,
        request_timeout_ms = timeout.as_millis() as u64,
        "stt http execute"
    );
    let response = client.execute(request).await.map_err(|e| {
        let classified = classify_reqwest(&e);
        tracing::info!(
            latency_ms = attempt_started.elapsed().as_millis() as u64,
            category = error_category(&classified.error),
            retryable = classified.class == RetryClass::Retryable,
            "stt transport attempt failed"
        );
        classified
    })?;
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
    let body = response.text().await.map_err(|e| classify_reqwest(&e))?;
    if !(200..300).contains(&status) {
        let snippet = truncated_body(&body);
        tracing::warn!(status, body = %snippet, "stt http error");
        return Err(classify_http(status, retry_after, &body));
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
        .header("HTTP-Referer", crate::app::lifecycle::GITHUB_REPO_URL)
        .header("X-Title", "Voxely")
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .map_err(|e| AppError::ConnectionFailed(error_chain(&e)))?;
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
        assert!(matches!(recorded.first(), Some(SttProgress::Attempt(1))));
        assert!(recorded.iter().any(|event| matches!(
            event,
            SttProgress::Finished {
                attempt: 1,
                outcome: "success",
                ..
            }
        )));
    }

    #[tokio::test]
    async fn reports_waiting_then_second_attempt_after_retryable_failure() {
        let server = MockServer::start();
        let mut busy = server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(503).body("busy");
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let mut policy = RetryPolicy::default();
        policy.additional_retries = 3;
        policy.initial_retry_delay = Duration::from_millis(20);
        policy.max_retry_delay = Duration::from_millis(50);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_for_cb = events.clone();
        let base = server.base_url();
        let transcribe = transcribe_file_with_progress(
            &client,
            &base,
            "k",
            "openai/gpt-transcribe",
            None,
            &path,
            policy,
            Duration::from_millis(200),
            rx,
            move |progress| events_for_cb.lock().unwrap().push(progress),
        );
        let switch = async {
            loop {
                if busy.hits() >= 1 {
                    busy.delete();
                    server.mock(|when, then| {
                        when.method(POST).path("/audio/transcriptions");
                        then.status(200)
                            .json_body(serde_json::json!({"text": "ok"}));
                    });
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let (result, _) = tokio::join!(transcribe, switch);
        assert_eq!(result.unwrap().text, "ok");
        let recorded = events.lock().unwrap().clone();
        assert!(matches!(recorded.first(), Some(SttProgress::Attempt(1))));
        assert!(recorded
            .iter()
            .any(|event| matches!(event, SttProgress::Waiting { attempt: 1, .. })));
        assert!(recorded
            .iter()
            .any(|event| matches!(event, SttProgress::Attempt(2))));
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
        let mut busy = server.mock(|when, then| {
            when.method(POST).path("/audio/transcriptions");
            then.status(503).body("busy");
        });
        let dir = tempfile::tempdir().unwrap();
        let path = wav_fixture(dir.path());
        let client = reqwest::Client::new();
        let (_tx, rx) = tokio::sync::watch::channel(false);
        let mut policy = RetryPolicy::default();
        policy.additional_retries = 2;
        policy.initial_retry_delay = Duration::from_millis(20);
        policy.max_retry_delay = Duration::from_millis(50);
        policy.total_operation_timeout = Duration::from_secs(5);
        let base = server.base_url();
        let transcribe = transcribe_file(
            &client,
            &base,
            "k",
            "openai/gpt-transcribe",
            None,
            &path,
            policy,
            Duration::from_millis(200),
            rx,
        );
        let switch = async {
            loop {
                if busy.hits() >= 1 {
                    busy.delete();
                    server.mock(|when, then| {
                        when.method(POST).path("/audio/transcriptions");
                        then.status(200)
                            .json_body(serde_json::json!({"text": "ok"}));
                    });
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let (result, _) = tokio::join!(transcribe, switch);
        let result = result.unwrap();
        assert_eq!(result.text, "ok");
        assert!(result.attempt >= 2);
    }

    #[test]
    fn stt_client_reuses_idle_http1_pool() {
        let opts = stt_http_options();
        assert!(opts.http1_only);
        assert_eq!(opts.pool_max_idle_per_host, 2);
        let client = build_stt_client(Duration::from_secs(8)).expect("production STT client");
        drop(client);
    }

    #[test]
    fn transport_rebuilds_only_when_connect_timeout_changes() {
        let mut transport = OpenRouterTransport::new(Duration::from_secs(8)).unwrap();
        transport.sync(Duration::from_secs(8)).unwrap();
        assert_eq!(transport.connect_timeout(), Duration::from_secs(8));
        transport.sync(Duration::from_secs(3)).unwrap();
        assert_eq!(transport.connect_timeout(), Duration::from_secs(3));
    }

    async fn serve_keep_alive_http(
        listener: tokio::net::TcpListener,
        accepted: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::spawn(handle_keep_alive_conn(stream));
        }
    }

    async fn handle_keep_alive_conn(mut stream: tokio::net::TcpStream) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = Vec::new();
        let mut tmp = [0u8; 2048];
        loop {
            let n = match stream.read(&mut tmp).await {
                Ok(0) => return,
                Ok(n) => n,
                Err(_) => return,
            };
            buf.extend_from_slice(&tmp[..n]);
            while let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header = String::from_utf8_lossy(&buf[..end]);
                let is_head = header.starts_with("HEAD ");
                buf.drain(..end + 4);
                let body = b"{\"data\":[]}";
                let mut response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                    body.len()
                );
                if !is_head {
                    response.push_str(std::str::from_utf8(body).unwrap());
                }
                if stream.write_all(response.as_bytes()).await.is_err() {
                    return;
                }
            }
        }
    }

    #[tokio::test]
    async fn prewarm_and_follow_up_reuse_one_http1_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let accepted_for_server = accepted.clone();
        tokio::spawn(serve_keep_alive_http(listener, accepted_for_server));
        let client = build_stt_client(Duration::from_secs(2)).unwrap();
        let base = format!("http://{addr}");
        let outcome = prewarm_connection_at(&client, &base).await;
        assert!(
            matches!(outcome, PrewarmOutcome::Ready { status: 200, .. }),
            "prewarm outcome {outcome:?}"
        );
        let follow = client.get(format!("{base}/models")).send().await.unwrap();
        assert_eq!(follow.status().as_u16(), 200);
        let _ = follow.bytes().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            accepted.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "prewarm and follow-up must share one HTTP/1 socket"
        );
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
