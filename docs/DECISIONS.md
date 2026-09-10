# Retry and timeout decisions

## Problem

Spokenly-class tools can wait a minute or more on a hung HTTP request before Retry is possible.

## Decision

A dedicated `RetryScheduler` owns attempts. Defaults:

- Automatic retries on
- Additional retries: 3 (1 initial + 3)
- Connect timeout: 3s
- Request timeout: 12s (scales with audio duration, cap 40s)
- Backoff: 500ms, 1s, 2s, jitter <= 20%, cap 3s
- Total operation timeout: 45s (hard deadline)

Retry-After is honored but never past the remaining deadline.

## Alternatives

Unbounded reqwest default timeouts: rejected.

TCP-level connectivity ping loops: rejected; the STT request is the source of truth.

Idempotency-Key: not documented by OpenRouter for STT, so retries remain bounded and may double-bill if the server succeeded and the response was lost.

## Installer and updates

Official Tauri NSIS + signed updater. Inno Setup and the custom GitHub-asset updater were removed. Authenticode is deferred. Version SSOT is `package.json`, `Cargo.toml`, and `tauri.conf.json`.

## UI foundation

Official shadcn CLI with Radix. Product strings live in `src/lib/i18n.ts`.
