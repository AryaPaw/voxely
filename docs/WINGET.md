# WinGet (Windows Package Manager)

Do not create a GitHub Release, tag, or `microsoft/winget-pkgs` PR from this document. Those happen only when you explicitly ask.

Package identifier: `AryaPaw.Voxely` (not Tauri `com.voxely.desktop`).

## Compatibility (checked against `v0.2.12`)

| Item                 | Value                                                                                                                       |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Installer file       | `Voxely_X.Y.Z_x64-setup.exe`                                                                                                |
| URL                  | `https://github.com/AryaPaw/voxely/releases/download/vX.Y.Z/Voxely_X.Y.Z_x64-setup.exe`                                     |
| Technology           | NSIS (`NullsoftInst` in the binary). WinGet `InstallerType`: `nullsoft`                                                     |
| Architecture         | App is x64. The NSIS stub PE machine is `0x014C` (i386). Always pass `\|x64\|user` to `wingetcreate update`                 |
| Scope                | `user` (`bundle.windows.nsis.installMode: currentUser`)                                                                     |
| Elevation            | Not required. Installs to `%LOCALAPPDATA%\Voxely`                                                                           |
| Silent               | `/S` (WinGet sets this for `nullsoft`). CI also uses `/NS`                                                                  |
| Silent with progress | Default NSIS `/S` is fully silent. WinGet `silentWithProgress` uses the same Nullsoft behavior                              |
| Uninstall            | `"%LOCALAPPDATA%\Voxely\uninstall.exe"` plus `/S`. Tauri does not write `QuietUninstallString`                              |
| Apps & Features      | HKCU `Uninstall\Voxely`: DisplayName `Voxely`, DisplayVersion = package version                                             |
| Publisher            | Set `bundle.publisher` to `AryaPaw`. Older NSIS builds used `voxely` (second identifier segment)                            |
| Reinstall / upgrade  | Same uninstall key `Voxely`; Tauri NSIS upgrades in place                                                                   |
| WebView2             | `downloadBootstrapper` with `silent: true`. Bootstrapper should not show UI. A machine without WebView2 still needs network |

Do not use `releases/latest/download/...` as `InstallerUrl`.

`AryaPaw.Voxely` is published in the community repository. Version 0.3.0 passed validation and was [merged on 2026-10-08](https://github.com/microsoft/winget-pkgs/pull/449088).

## Expected first-manifest metadata

Use `wingetcreate new <version-specific-installer-url>` so SHA-256 comes from the real file. Fill or confirm:

- PackageIdentifier: `AryaPaw.Voxely`
- PackageName: `Voxely`
- Publisher: `AryaPaw`
- PackageVersion: SSOT from `package.json` (must match installer `ProductVersion` and Apps & Features)
- License: `AGPL-3.0-or-later`
- LicenseUrl: `https://github.com/AryaPaw/voxely/blob/main/LICENSE`
- PackageUrl: `https://github.com/AryaPaw/voxely`
- PublisherUrl: `https://github.com/AryaPaw`
- PublisherSupportUrl: `https://github.com/AryaPaw/voxely/issues`
- ShortDescription: `Windows voice dictation with direct text insertion`
- Architecture: `x64`
- Scope: `user`
- InstallerType: `nullsoft`
- InstallerUrl: versioned GitHub Release asset (not `latest`)
- ReleaseNotesUrl: `https://github.com/AryaPaw/voxely/releases/tag/vX.Y.Z`
- Tags: `dictation`, `speech-to-text`, `voice`, `transcription`, `windows`

Keep the default locale `en-US`. A `ru-RU` locale file can wait until after the first PR; it does not change install behavior.

`AppsAndFeaturesEntries` should be unnecessary if DisplayName `Voxely`, DisplayVersion `X.Y.Z`, and Publisher `AryaPaw` match.

## Local commands

Version always comes from `package.json`:

```powershell
pwsh -File scripts/check-version.ps1
pwsh -File scripts/winget-lib.test.ps1
pwsh -File scripts/winget-prepare.ps1 -Mode inspect
```

`inspect` fails if the GitHub Release for that version is missing.

After the first public Release you intend to submit:

```powershell
# Interactive first manifest. Review every field. Do not pass -Submit.
pwsh -File scripts/winget-prepare.ps1 -Mode new

winget validate --manifest <manifest-directory>
# Once, elevated:
winget settings --enable LocalManifestFiles
winget install --manifest <manifest-directory> --silent --accept-package-agreements
winget list --id AryaPaw.Voxely
# Confirm Apps & Features: Voxely, version X.Y.Z, Publisher AryaPaw
# Confirm the app starts (tray / local title as appropriate)
winget uninstall --id AryaPaw.Voxely --silent
```

Or `pwsh -File scripts/winget-test.ps1 -ManifestDir <dir> [-Install] [-Uninstall]`.

Windows Sandbox: `scripts/run-windows-sandbox.ps1` after a local NSIS build (not a substitute for `winget install --manifest`).

## One-time first submission (only when you say so)

1. Have a non-draft GitHub Release `vX.Y.Z` whose asset is `Voxely_X.Y.Z_x64-setup.exe` (existing `v0.2.12` already matches this pattern).
2. Sign the Microsoft CLA if you have not.
3. Create a **classic** PAT (fine-grained tokens are not supported by winget-create) with `public_repo` and `workflow`. `workflow` allows fork synchronization to bring in upstream `.github/workflows` changes. `repo` also covers `public_repo`, but isn't needed for these public repositories. Don't grant `delete_repo`; this automation doesn't delete forks.
4. On a Windows machine: `wingetcreate new https://github.com/AryaPaw/voxely/releases/download/vX.Y.Z/Voxely_X.Y.Z_x64-setup.exe`
5. Set PackageIdentifier to `AryaPaw.Voxely`. Confirm `nullsoft`, `x64`, `user`. Do not keep a guessed SHA; let the tool hash the download.
6. `winget validate` and a silent local install/uninstall as above. Reinstall once and confirm a single Apps & Features entry.
7. Submit the PR from the wizard (or `wingetcreate submit`) to `microsoft/winget-pkgs`. Do not open an extra issue unless their current docs require it.
8. After the PR merges, set GitHub Actions:
   - Secret `WINGET_CREATE_GITHUB_TOKEN` = that classic PAT
   - Variable `WINGET_AUTO_UPDATE` = `true`

## Later versions (almost no manual work)

After `AryaPaw.Voxely` exists on the community source:

```text
Git tag vX.Y.Z -> existing release.yml -> GitHub Release
  -> optional winget.yml (only if WINGET_AUTO_UPDATE=true)
  -> check published version and existing PR
  -> verify PAT scopes and fast-forward fork master
  -> wingetcreate update AryaPaw.Voxely --urls "<versioned-url>|x64|user" --version X.Y.Z --submit --no-open
```

The token is read from `WINGET_CREATE_GITHUB_TOKEN` in the environment. Never pass `--token` on the command line (it can appear in logs).

`GH_TOKEN` stays the read-only Actions token for release inspection/download. Fork operations use only `WINGET_CREATE_GITHUB_TOKEN`, including identity and scope checks. Workflow inputs enter PowerShell through environment variables; the script validates `X.Y.Z` before using the version.

All WinGet workflow runs share one concurrency group because every version updates the same fork. Before submission:

- If the version is already in `microsoft/winget-pkgs`, compare its installer URL and SHA256 with the downloaded release installer before exiting successfully. Mismatched bytes require a new version.
- If the token owner's exact version PR is open, verify its repository, changed-file scope and installer URL/SHA256 at the immutable PR head SHA, print its URL and exit successfully. Search/API errors stop the job rather than being treated as "no PR".
- Require a classic PAT with `public_repo` (or `repo`) and `workflow` before any write. Expiration, insufficient scopes and missing/wrong forks fail with an actionable error.
- Verify the token owner's `winget-pkgs` is a writable fork of `microsoft/winget-pkgs` with default branch `master`.
- Capture upstream/fork SHAs and refuse synchronization if fork `master` has unique commits. Update only `master`, using GitHub's reference API with `force=false`, then read it back. PR branches aren't modified. Concurrent ref conflicts get at most three attempts; every attempt rechecks ancestry.

The reference API avoids depending on `merge-upstream` for a large stale fork. This also prevents destructive recovery from a divergent branch. GitHub outages, expired/revoked tokens, or a manually diverged fork still need attention; errors remain fatal.

Offline regression fixtures run through `bun run winget:test-lib` and `bun run verify:metadata`. They check idempotency, scopes, credential separation, fast-forward-only writes, divergence, concurrent conflicts and API failures without contacting GitHub.

The helper pins the official `wingetcreate 1.12.13.0` executable and its SHA256 in `Get-VoxelyWingetCreateRelease`. Downloads use a bounded temporary file in the cache directory. A failed transfer or hash mismatch cannot replace the cached executable; a corrupt or older cache is repaired on the next successful download. Update the version, versioned URL and hash together from the [official release](https://github.com/microsoft/winget-create/releases/tag/v1.12.13.0).

### Release identity and existing assets

`release.yml` verifies the exact checked-out SHA against any existing version tag before building and publishing. Runs are serialized across refs. Publication creates a draft tied to that SHA, uploads only missing assets, checks all asset sizes and SHA256 digests, then publishes the complete draft. A matching partial draft can resume; existing differing assets and tags are never replaced. An identical published candidate is a read-only no-op. Rebuilt bytes that differ need a new version, even when the source version string is unchanged.

### Recovering the old fork-sync error

For `The forked repository could not be synced with the upstream commits`, update the existing classic PAT's scopes to include `workflow` at [GitHub token settings](https://github.com/settings/tokens). If you regenerate or replace it, update the existing `WINGET_CREATE_GITHUB_TOKEN` Actions secret too. Never paste it into a log, workflow YAML or chat.

After this automation is on the selected workflow ref, run `winget.yml` again for the published version with `submit=true`. A successful existing submission is a no-op; a new version first synchronizes the fork. Don't rebuild the release, move its tag, delete the fork or use a force push to recover.

References: [GitHub reference updates and permissions](https://docs.github.com/en/rest/git/refs#update-a-reference), [winget-create token setup](https://github.com/microsoft/winget-create/blob/main/doc/token.md), [winget-create fork synchronization implementation](https://github.com/microsoft/winget-create/blob/main/src/WingetCreateCore/Common/GitHub.cs).

Manual retry without rebuilding Voxely:

```text
Actions -> WinGet -> Run workflow -> version X.Y.Z -> submit true
```

```powershell
$env:WINGET_CREATE_GITHUB_TOKEN = "<PAT>"  # session only
pwsh -File scripts/winget-prepare.ps1 -Mode update -Submit
```

A failed WinGet job does not unpublish the GitHub Release.

## Secrets and variables

| Name                         | Kind             | When                                                                                           |
| ---------------------------- | ---------------- | ---------------------------------------------------------------------------------------------- |
| `WINGET_CREATE_GITHUB_TOKEN` | Actions secret   | Classic PAT, `public_repo` and `workflow`; token owner must have a writable `winget-pkgs` fork |
| `WINGET_AUTO_UPDATE`         | Actions variable | Set to `true` only after the first `AryaPaw.Voxely` PR is merged                               |
| `TAURI_SIGNING_PRIVATE_KEY`  | Existing secret  | Unchanged; not used by WinGet                                                                  |

Do not commit tokens. Do not put the PAT in workflow YAML.
