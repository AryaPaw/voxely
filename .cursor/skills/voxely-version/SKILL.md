---
name: voxely-version
description: Bump Voxely's app version with one script. Use when the user asks to update, bump, or set the version, or when wrapping up a release after they confirm the number.
---

# Voxely version bump

Do not rediscover which files hold the version. Do not edit seven files by hand.

## Confirm first

If the user has not confirmed a number, propose one (patch / minor / major + why) and stop.

If they said "обнови версию" after a proposal, that number is confirmed.

Never tag, never `git push`, never GitHub Release unless that same message asks for it.

## One command

```text
pwsh -File scripts/set-version.ps1 -To X.Y.Z
```

or `-To patch`, `-To minor`, `-To major`.

Then:

```text
pwsh -File scripts/check-version.ps1 -Expected X.Y.Z
```

That is the whole bump. The script writes `package.json` and syncs `Cargo.toml` / the `voxely` row in `Cargo.lock`. `tauri.conf.json` stays `../package.json`.

## Date

Do not write a date into source. About shows the build day from `get_runtime_info.buildDate`, baked in `build.rs` as `VOXELY_BUILD_DATE`.

CI is GitHub Actions (`.github/workflows/*`). Set `VOXELY_BUILD_DATE=yyyy-MM-dd` in the workflow; local production uses git `%cs` or UTC today. Daily driver: `VOXELY_LOCAL_BUILD=1` + `bunx tauri build --no-bundle`.

## After the script

Rebuild the daily driver if the running About screen must show the new number:

```text
bun run rebuild:local-app
```

That builds and starts `src-tauri/target/release/voxely.exe` with `VOXELY_LOCAL_BUILD=1`. It is not a GitHub Release. Do not use `tauri build -d`.

## Release (only when the current message asks)

Version must already be the confirmed `X.Y.Z` in `package.json`.

1. `bun run verify` and wait. Do not tag on a red gate.
2. Commit the product files (not ECC dumps).
3. `git push origin HEAD` then `git tag vX.Y.Z` and `git push origin vX.Y.Z`.
4. Watch `.github/workflows/release.yml` on that tag until success or failure. A failed Prettier/coverage job is your bug.
5. If the tag never published NSIS, fix, push, delete and recreate `vX.Y.Z` on the fix. Do not walk away from a red run.

The workflow publishes the GitHub Release. Do not `gh release create` as a workaround.
