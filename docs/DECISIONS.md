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

All live STT, History retry, Compare, and connection checks share one process `OpenRouterTransport`: HTTP/1.1, rustls, idle pool of 2. The client is rebuilt only when the connect-timeout fingerprint changes. Recording start may prewarm TLS with `GET /models` and no API key.

## Alternatives

Unbounded reqwest default timeouts: rejected.

TCP-level connectivity ping loops: rejected; the STT request is the source of truth.

Per-job `reqwest::Client` with `pool_max_idle_per_host(0)`: rejected; it forced a new TLS handshake on every attempt.

Switching to Schannel/`native-tls`: rejected for the VPN handshake-eof; Schannel failed the same way.

Idempotency-Key: not documented by OpenRouter for STT, so retries remain bounded and may double-bill if the server succeeded and the response was lost.

## Installer and updates

Official Tauri NSIS + signed updater. Inno Setup and the custom GitHub-asset updater were removed. Authenticode is deferred. Version SSOT is `package.json`; `tauri.conf.json` reads that path; `Cargo.toml` is synced by `scripts/set-version.ps1`.

## UI foundation

Official shadcn CLI with Radix. Product strings live in `src/lib/i18n.ts`.
