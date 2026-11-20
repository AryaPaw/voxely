param(
    [Parameter(Mandatory)][ValidateSet('preflight', 'fingerprint', 'publish')][string]$Mode,
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$CandidateSha,
    [string]$Repo = $env:GITHUB_REPOSITORY,
    [string]$ExpectedBundleFingerprint
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-lib.ps1')
$actualSha = git rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actualSha.Trim() -ne $CandidateSha) { throw 'Checkout differs from the verified candidate SHA.' }
if ($Mode -eq 'preflight') {
    Assert-VoxelyReleaseBuildAllowed -Repo $Repo -Version $Version -CandidateSha $CandidateSha
    Write-Host "PASS: release identity $CandidateSha / v$Version"
    exit 0
}
$directory = Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri/target/release/bundle/nsis'
$installer = Join-Path $directory "Voxely_${Version}_x64-setup.exe"
$artifactPaths = @($installer, "$installer.sig", (Join-Path $directory 'latest.json'))
if ($Mode -eq 'fingerprint') {
    $files = Get-VoxelyReleaseFiles -Version $Version -ArtifactPaths $artifactPaths
    "fingerprint=$(Get-VoxelyReleaseBundleFingerprint -Files $files)" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
    exit 0
}
$status = Publish-VoxelyRelease -Repo $Repo -Version $Version -CandidateSha $CandidateSha -ArtifactPaths $artifactPaths -ExpectedBundleFingerprint $ExpectedBundleFingerprint
Write-Host "Release v${Version}: $status (candidate $CandidateSha)"
