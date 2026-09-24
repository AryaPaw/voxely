# Shared WinGet helpers. Version SSOT is package.json.

Set-StrictMode -Version Latest

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
    $dir = Split-Path $path -Parent
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir | Out-Null
    }
    if (-not (Test-Path $path)) {
        Write-Host "Downloading wingetcreate.exe"
        curl.exe -L --fail -o $path "https://aka.ms/wingetcreate/latest"
    }
    if (-not (Test-Path $path) -or ((Get-Item $path).Length -lt 1KB)) {
        throw "Failed to download wingetcreate.exe"
    }
    return $path
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
