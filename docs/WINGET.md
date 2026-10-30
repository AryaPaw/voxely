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
3. Create a **classic** PAT with `public_repo` only. Leave `repo`, `workflow` and `delete_repo` unchecked: this automation doesn't update workflows, access private repositories or delete forks.
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
  -> wingetcreate update (generation and validation only)
  -> isolated version branch containing exactly three manifests
  -> verify immutable diff and open or reuse the version PR
```

The token is read from `WINGET_CREATE_GITHUB_TOKEN` in the environment. Never pass `--token` on the command line (it can appear in logs).

`GH_TOKEN` stays the read-only Actions token for release inspection/download. Fork operations use only `WINGET_CREATE_GITHUB_TOKEN`, including identity and scope checks. Workflow inputs enter PowerShell through environment variables; the script validates `X.Y.Z` before using the version.

WinGet runs share one concurrency group to prevent duplicate submissions. The dedicated classic PAT needs only `public_repo`. Neither `repo` nor `workflow` is required; don't expand access to private repositories. GitHub's token UI may select `repo` automatically when `workflow` is selected, so leave `workflow` unchecked.

- An already published version is checked against the release installer URL and SHA256 before returning successfully.
- An existing open version PR is checked for the correct fork, base and exactly three added manifest paths. Its installer URL and SHA256 are verified at its immutable head SHA. A closed version PR requires inspection instead of creating a duplicate. Search/API errors remain fatal.
- New submissions require a public writable fork of `microsoft/winget-pkgs`. The script captures upstream and fork master SHAs and selects their common ancestor. Stale or diverged master branches are preserved, including user commits.
- Official `wingetcreate` generates and validates the manifests; its `--submit` path isn't used because it synchronizes fork master.
- `--release-date` is derived from the GitHub Release's `publishedAt` in UTC, and `--release-notes-url` uses that release's URL. The current local day and the app's build day aren't release metadata sources. Missing, duplicate or mismatched `ReleaseDate` is rejected before submission, including when checking an existing PR.
- Only the three regular YAML files for the requested version can enter a new tree. Their package ID, version and release installer hash/URL are checked. File size is bounded and reparse points are rejected.
- GitHub Git Data APIs create one commit and a version branch named `codex/winget-AryaPaw.Voxely-X.Y.Z`. No existing ref, master branch or workflow file is updated. A compare against the captured upstream SHA must contain exactly the three additions with the expected Git blob hashes before a PR can be opened.
- Retries reuse a matching branch and PR. Ref creation races and duplicate-PR responses are resolved by reading and validating existing state. An existing branch with unrelated changes or different bytes is never overwritten. No force push, fork deletion or token replacement is used.
- Public PR creation is verified again at the expected head SHA. If another writer changes that branch, the job fails for inspection rather than claiming success.

Offline API fixtures run through `bun run winget:test-lib` and `bun run verify:metadata`. They execute the production request and publisher functions using a token fixture with only `public_repo`, including fresh creation, stale/diverged forks, retries, collisions, concurrent creation, API errors, credential isolation and exact manifest-only diffs.

The helper pins the official `wingetcreate 1.12.13.0` executable and its SHA256 in `Get-VoxelyWingetCreateRelease`. Downloads use a bounded temporary file in the cache directory. A failed transfer or hash mismatch cannot replace the cached executable; a corrupt or older cache is repaired on the next successful download. Update the version, versioned URL and hash together from the [official release](https://github.com/microsoft/winget-create/releases/tag/v1.12.13.0).

### Release identity and existing assets

`release.yml` verifies the exact checked-out SHA against any existing version tag before building and publishing. Runs are serialized across refs. Publication creates a draft tied to that SHA, uploads only missing assets, checks all asset sizes and SHA256 digests, then publishes the complete draft. A matching partial draft can resume; existing differing assets and tags are never replaced. An identical published candidate is a read-only no-op. Rebuilt bytes that differ need a new version, even when the source version string is unchanged.

### Recovering the old fork-sync error

Deploy this automation to the workflow ref and rerun `winget.yml` for the already published version with `submit=true`. Keep the existing `public_repo` token. Don't add `workflow`, grant private-repository access, synchronize/reset master, regenerate the token or delete the fork. If an older release tag still contains the old synchronization code, dispatch the standalone WinGet workflow from the updated main branch instead of rerunning that old tag's code.

An existing matching PR is a read-only no-op; a fresh version uses a separate branch from shared history. This avoids the permission problem even when upstream workflows changed after the fork was created. GitHub outages, revoked/expired tokens and manually conflicting version branches can still fail safely and require attention.

References: [GitHub reference creation](https://docs.github.com/en/rest/git/refs#create-a-reference), [Git trees](https://docs.github.com/en/rest/git/trees#create-a-tree), [Git commits](https://docs.github.com/en/rest/git/commits#create-a-commit), [winget-create token setup](https://github.com/microsoft/winget-create/blob/main/doc/token.md).

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

| Name                         | Kind             | When                                                                                        |
| ---------------------------- | ---------------- | ------------------------------------------------------------------------------------------- |
| `WINGET_CREATE_GITHUB_TOKEN` | Actions secret   | Classic PAT, `public_repo` only; token owner must have a public writable `winget-pkgs` fork |
| `WINGET_AUTO_UPDATE`         | Actions variable | Set to `true` only after the first `AryaPaw.Voxely` PR is merged                            |
| `TAURI_SIGNING_PRIVATE_KEY`  | Existing secret  | Unchanged; not used by WinGet                                                               |

Do not commit tokens. Do not put the PAT in workflow YAML.
