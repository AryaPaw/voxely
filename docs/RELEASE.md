# Release process

Do not bump `package.json`, `src-tauri/Cargo.toml`, or `src-tauri/tauri.conf.json` until the version is confirmed.

Proposed next version after 0.2.2: `0.2.3` (patch) unless a user-visible feature lands.

## Secrets

- `TAURI_SIGNING_PRIVATE_KEY`: minisign private key (never commit)
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: optional if the key has a password
- Losing the private key means installed copies cannot receive signed updates

Generate keys with `bunx tauri signer generate --ci -w <path-outside-repo>`.

## Build

```text
bun run verify
bun run tauri build
./scripts/check-version.ps1
```

## Smoke

- GitHub `windows-2022`: `scripts/ci-installer-smoke.ps1` after a signed NSIS build
- Windows Sandbox: `scripts/run-windows-sandbox.ps1` (fresh install, launch, uninstall). Hosted GitHub is not Sandbox.
- Host dictation only after Sandbox PASS

## Publish

Push a `vX.Y.Z` tag only after manifests already contain `X.Y.Z`. The release workflow builds signed NSIS, runs installer smoke, then publishes one GitHub Release named `vX.Y.Z`. It must not leave a leftover draft.

Rollback: keep the previous NSIS and do not move `latest`. A failed update must leave the running app intact.
