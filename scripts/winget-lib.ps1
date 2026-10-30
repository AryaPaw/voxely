# Shared WinGet helpers. Version SSOT is package.json.

Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "winget-github.ps1")

function Get-VoxelyRepoRoot {
    param([string]$Start = $PSScriptRoot)
    return (Resolve-Path (Join-Path $Start "..")).Path
}

function Get-VoxelyPackageVersion {
    param([string]$Root = (Get-VoxelyRepoRoot))
    $package = Get-Content (Join-Path $Root "package.json") -Raw | ConvertFrom-Json
    if ($package.version -notmatch '^\d+\.\d+\.\d+$') {
        throw "package.json version must be X.Y.Z (got $($package.version))"
    }
    return [string]$package.version
}

function Get-VoxelyWingetPackageId {
    return "AryaPaw.Voxely"
}

function Get-VoxelyNsisInstallerName {
    param([Parameter(Mandatory = $true)][string]$Version)
    return "Voxely_${Version}_x64-setup.exe"
}

function Get-VoxelyVersionedInstallerUrl {
    param([Parameter(Mandatory = $true)][string]$Version)
    $name = Get-VoxelyNsisInstallerName -Version $Version
    return "https://github.com/AryaPaw/voxely/releases/download/v$Version/$name"
}

function Get-VoxelyWingetCreateUrlArg {
    param([Parameter(Mandatory = $true)][string]$Version)
    # NSIS stub is i386; payload and filename are x64 current-user.
    return "$(Get-VoxelyVersionedInstallerUrl -Version $Version)|x64|user"
}

function Get-VoxelyWingetCreatePath {
    $dir = Join-Path $env:LOCALAPPDATA "VoxelyTools"
    return (Join-Path $dir "wingetcreate.exe")
}

function Assert-VoxelyGithubRelease {
    param(
        [Parameter(Mandatory = $true)][string]$Version,
        [string]$Repo = "AryaPaw/voxely"
    )
    $tag = "v$Version"
    $release = gh release view $tag --repo $Repo --json tagName,assets,url 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $release) {
        throw "GitHub Release $tag does not exist on $Repo. Do not invent a latest URL."
    }
    $data = $release | ConvertFrom-Json
    $name = Get-VoxelyNsisInstallerName -Version $Version
    $asset = @($data.assets) | Where-Object { $_.name -eq $name } | Select-Object -First 1
    if (-not $asset) {
        $names = (@($data.assets) | ForEach-Object { $_.name }) -join ", "
        throw "Release $tag has no $name (assets: $names)"
    }
    return [pscustomobject]@{
        Tag          = $data.tagName
        HtmlUrl      = $data.url
        InstallerUrl = $asset.url
        AssetName    = $asset.name
    }
}

function Install-VoxelyWingetCreate {
    $path = Get-VoxelyWingetCreatePath
    $release = Get-VoxelyWingetCreateRelease
    $dir = Split-Path $path -Parent
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir | Out-Null
    }
    if (Test-VoxelyToolHash -Path $path -Sha256 $release.Sha256) { return $path }
    $temporary = Join-Path $dir "wingetcreate-$([guid]::NewGuid().ToString('N')).tmp"
    try {
        Write-Host "Downloading wingetcreate $($release.Version)"
        Invoke-VoxelyToolDownload -Url $release.Url -Path $temporary
        if (-not (Test-VoxelyToolHash -Path $temporary -Sha256 $release.Sha256)) {
            throw "wingetcreate download failed SHA256 validation. The existing cache was not replaced."
        }
        # The temporary file and destination are on the same volume.
        [IO.File]::Move($temporary, $path, $true)
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary -Force }
    }
    return $path
}

function Get-VoxelyWingetCreateRelease {
    # Official microsoft/winget-create v1.12.13.0 asset digest, verified 2026-10-26.
    return @{
        Version = "1.12.13.0"
        Url = "https://github.com/microsoft/winget-create/releases/download/v1.12.13.0/wingetcreate.exe"
        Sha256 = "24042bd37915805615e6cf969ac57c6439124c3fe85823327f5f3fb24bd9ffea"
    }
}

function Test-VoxelyToolHash {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Sha256)
    return (Test-Path -LiteralPath $Path -PathType Leaf) -and
        ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $Sha256)
}

function Invoke-VoxelyToolDownload {
    param([Parameter(Mandatory)][string]$Url, [Parameter(Mandatory)][string]$Path)
    & curl.exe --location --fail --silent --show-error --connect-timeout 15 --max-time 120 --retry 2 --retry-max-time 180 --output $Path $Url
    if ($LASTEXITCODE -ne 0) { throw "wingetcreate download failed (curl exit $LASTEXITCODE)." }
}

function Get-VoxelyPeMachine {
    param([Parameter(Mandatory = $true)][string]$Path)
    $bytes = [IO.File]::ReadAllBytes($Path)
    $pe = [BitConverter]::ToInt32($bytes, 0x3C)
    return [BitConverter]::ToUInt16($bytes, $pe + 4)
}

function Test-VoxelyNsisInstaller {
    param([Parameter(Mandatory = $true)][string]$Path)
    $bytes = [IO.File]::ReadAllBytes($Path)
    $ascii = [Text.Encoding]::ASCII.GetString($bytes)
    return $ascii.Contains("NullsoftInst")
}
