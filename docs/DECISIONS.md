# Retry and timeout decisions

## Problem

Spokenly-class tools can wait a minute or more on a hung HTTP request before Retry is possible. Forced new TLS on every dictation also amplified VPN handshake drops (~5 s peer close).

## Decision

A dedicated `RetryScheduler` owns attempts. Defaults:

- Automatic retries on
- Additional retries: 3 (1 initial + 3)
- Connect timeout: 8s (stored values below 8s are lifted on load)
- Request timeout: 20s, scaled with audio duration, cap 15 min
- Backoff: 500ms, 1s, 2s, jitter <= 20%, cap 3s
- Connect-stage TLS EOF (`tls handshake eof`): retryable with ~80ms delay instead of the exponential backoff
- Certificate trust failures (`UnknownIssuer` and similar): terminal, no extra handshake attempts
- HTTP 429/5xx: retryable with the usual backoff and Retry-After
- Total operation timeout: 12 min (hard deadline)

Retry-After is honored but never past the remaining deadline.

All live STT, History retry, Compare, and connection checks share one process `OpenRouterTransport`: HTTP/1.1, rustls, idle pool of 2. The client is rebuilt only when the connect timeout changes. Recording start prewarms with completed `HEAD /models` (GET fallback with a bounded body) so HTTP/1 can return the socket to the pool. Prewarm does not use an API key. Connect-stage TLS EOF and connect timeouts stay `ConnectionFailed`; only a total request timeout is `RequestTimeout`.

## Alternatives

Unbounded reqwest default timeouts: rejected.

TCP-level connectivity ping loops: rejected; the STT request is the source of truth.

Per-job `reqwest::Client` with `pool_max_idle_per_host(0)`: rejected; it forced a new TLS handshake on every attempt.

Switching to Schannel/`native-tls`: rejected. Windows curl/Schannel A/B reproduced the same ~5.02 s TLS handshake failure against OpenRouter; the peer close is outside rustls.

Idempotency-Key: not documented by OpenRouter for STT, so retries remain bounded and may double-bill if the server succeeded and the response was lost.

## Installer and updates

Official Tauri NSIS + signed updater. Inno Setup and the custom GitHub-asset updater were removed. Authenticode is deferred. Version SSOT is `package.json`; `tauri.conf.json` reads that path; `Cargo.toml` is synced by `scripts/set-version.ps1`.

## UI foundation

Official shadcn CLI with Radix. Product strings live in `src/lib/i18n.ts`.
