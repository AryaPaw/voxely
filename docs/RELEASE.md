# Release process

Version number SSOT is `package.json`. `src-tauri/tauri.conf.json` points at that file. `Cargo.toml` is synced by the setter, not edited by hand.

```text
pwsh -File scripts/set-version.ps1 -To 0.2.9
pwsh -File scripts/set-version.ps1 -To patch
```

Do not bump until the user confirms the number. Do not write a calendar date into source: About uses the compile-time build day from `VOXELY_BUILD_DATE`, `SOURCE_DATE_EPOCH`, git `%cs`, or UTC today.

Do not create tags or GitHub Releases unless asked.

## Secrets

- `TAURI_SIGNING_PRIVATE_KEY`: minisign private key (never commit)
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: optional if the key has a password
- Losing the private key means installed copies cannot receive signed updates
- WinGet (after the first community PR merges): secret `WINGET_CREATE_GITHUB_TOKEN` and variable `WINGET_AUTO_UPDATE=true`. See [WINGET.md](WINGET.md). Do not enable auto-update before `AryaPaw.Voxely` exists.

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

Rollback: keep the previous NSIS on its GitHub tag. A failed update must leave the running app intact. To roll the Tauri updater back, replace the published `latest.json` with the previous signed manifest; do not rely on GitHub `latest` alone.

WinGet uses the same versioned NSIS URL as the updater platform entry, never `releases/latest/download`. A failed WinGet job does not roll back the GitHub Release. First submission is manual; later versions can call `.github/workflows/winget.yml`.
