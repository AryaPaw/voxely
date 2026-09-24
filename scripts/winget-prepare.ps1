# Prepare or update a WinGet manifest from the GitHub Release installer.
# Does not submit a PR unless -Submit is passed.

param(
    [ValidateSet("inspect", "new", "update")]
    [string]$Mode = "inspect",
    [string]$Version = "",
    [switch]$Submit,
    [string]$OutDir = ""
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "winget-lib.ps1")

$root = Get-VoxelyRepoRoot
& (Join-Path $PSScriptRoot "check-version.ps1")
$ssot = Get-VoxelyPackageVersion -Root $root
if (-not $Version) { $Version = $ssot }
if ($Version -ne $ssot) {
    Write-Warning "Requested $Version but package.json is $ssot"
}

$id = Get-VoxelyWingetPackageId
$release = Assert-VoxelyGithubRelease -Version $Version
Write-Host "Release $($release.HtmlUrl)"
Write-Host "Installer $($release.InstallerUrl)"

$work = Join-Path $env:TEMP "voxely-winget-$Version"
New-Item -ItemType Directory -Force -Path $work | Out-Null
$local = Join-Path $work (Get-VoxelyNsisInstallerName -Version $Version)
Write-Host "Downloading installer for inspect"
gh release download "v$Version" --repo AryaPaw/voxely --pattern (Get-VoxelyNsisInstallerName -Version $Version) --dir $work --clobber
$vi = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($local)
$hash = (Get-FileHash $local -Algorithm SHA256).Hash
$machine = Get-VoxelyPeMachine -Path $local
$nsis = Test-VoxelyNsisInstaller -Path $local
Write-Host "ProductVersion=$($vi.ProductVersion) ProductName=$($vi.ProductName) CompanyName=$($vi.CompanyName)"
Write-Host "PE Machine=0x$($machine.ToString('X4')) (NSIS stub is often i386 / 0x014C)"
Write-Host "NullsoftInst=$nsis SHA256=$hash Size=$((Get-Item $local).Length)"
if ($vi.ProductVersion -and $vi.ProductVersion -ne $Version) {
    throw "Installer ProductVersion $($vi.ProductVersion) does not match $Version"
}
if (-not $nsis) { throw "Installer is not Nullsoft/NSIS" }

$urlArg = Get-VoxelyWingetCreateUrlArg -Version $Version
Write-Host "wingetcreate URL arg: $urlArg"

if ($Mode -eq "inspect") {
    Write-Host "Inspect only. First submission: wingetcreate new $($release.InstallerUrl)"
    Write-Host "Later updates: pwsh -File scripts/winget-prepare.ps1 -Mode update [-Submit]"
    exit 0
}

if ($Submit -and -not $env:WINGET_CREATE_GITHUB_TOKEN) {
    throw "Submit requires env WINGET_CREATE_GITHUB_TOKEN (do not pass --token on the CLI)"
}

$create = Install-VoxelyWingetCreate
if (-not $OutDir) {
    $OutDir = Join-Path $work "manifests"
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

if ($Mode -eq "new") {
    Write-Host "wingetcreate new is interactive. It will not run --submit unless you confirm in the wizard."
    if ($Submit) {
        throw "Refusing -Submit with -Mode new. Finish the first PR by reviewing the wizard locally."
    }
    & $create new $release.InstallerUrl --out $OutDir --no-open
    exit $LASTEXITCODE
}

# update: package must already exist on winget-pkgs
$args = @(
    "update", $id,
    "--urls", $urlArg,
    "--version", $Version,
    "--out", $OutDir,
    "--no-open"
)
if ($Submit) {
    $args += "--submit"
    Write-Host "Submitting PR to microsoft/winget-pkgs (token from env only)"
}
& $create @args
if ($LASTEXITCODE -ne 0) { throw "wingetcreate failed with $LASTEXITCODE" }
Write-Host "Manifest output: $OutDir"
