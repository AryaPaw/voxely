param(
    [Parameter(Mandatory)][ValidateSet('preflight', 'publish')][string]$Mode,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$CandidateSha,
    [string]$Repo = $env:GITHUB_REPOSITORY
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-lib.ps1')
$actualSha = git rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actualSha.Trim() -ne $CandidateSha) { throw 'Checkout differs from the verified candidate SHA.' }
$null = Assert-VoxelyReleaseIdentity -Repo $Repo -Version $Version -CandidateSha $CandidateSha -AllowMissing
if ($Mode -eq 'preflight') { Write-Host "PASS: release identity $CandidateSha / v$Version"; exit 0 }
$directory = Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri/target/release/bundle/nsis'
$installer = Join-Path $directory "Voxely_${Version}_x64-setup.exe"
$status = Publish-VoxelyRelease -Repo $Repo -Version $Version -CandidateSha $CandidateSha -ArtifactPaths @($installer, "$installer.sig", (Join-Path $directory 'latest.json'))
Write-Host "Release v${Version}: $status (candidate $CandidateSha)"
