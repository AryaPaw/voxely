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

`AryaPaw.Voxely` was not present in `microsoft/winget-pkgs` and `winget search voxely` returned nothing at prep time.

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
- ShortDescription: `Windows voice dictation that inserts speech into the window you started in`
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
3. Create a **classic** PAT (fine-grained tokens are not supported by winget-create): scope `public_repo`. Optional: `delete_repo` so a failed fork can be deleted.
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
  -> wingetcreate update AryaPaw.Voxely --urls "<versioned-url>|x64|user" --version X.Y.Z --submit --no-open
```

The token is read from `WINGET_CREATE_GITHUB_TOKEN` in the environment. Never pass `--token` on the command line (it can appear in logs).

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

| Name                         | Kind             | When                                                             |
| ---------------------------- | ---------------- | ---------------------------------------------------------------- |
| `WINGET_CREATE_GITHUB_TOKEN` | Actions secret   | Before enabling auto-update. Classic PAT, `public_repo`          |
| `WINGET_AUTO_UPDATE`         | Actions variable | Set to `true` only after the first `AryaPaw.Voxely` PR is merged |
| `TAURI_SIGNING_PRIVATE_KEY`  | Existing secret  | Unchanged; not used by WinGet                                    |

Do not commit tokens. Do not put the PAT in workflow YAML.
