$ErrorActionPreference = "Stop"

$Repo = if ($env:CODEX_IMAGE_REPO) { $env:CODEX_IMAGE_REPO } else { "tksuns12/codex-image" }
$InstallDir = if ($env:CODEX_IMAGE_INSTALL_DIR) { $env:CODEX_IMAGE_INSTALL_DIR } else { Join-Path $HOME "bin" }
$ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"

function Assert-GetFileHashAvailable {
    if (-not (Get-Command Get-FileHash -ErrorAction SilentlyContinue)) {
        throw "codex-image installer requires Get-FileHash to verify SHA256SUMS"
    }
}

function Get-ExpectedSha256 {
    param(
        [string]$ChecksumPath,
        [string]$Asset
    )

    $Entries = @()
    foreach ($Line in Get-Content -LiteralPath $ChecksumPath) {
        $Trimmed = $Line.Trim()
        if (-not $Trimmed) {
            continue
        }

        $Parts = $Trimmed -split '\s+'
        if ($Parts.Count -ne 2 -or $Parts[0] -notmatch '^[A-Fa-f0-9]{64}$') {
            throw "malformed SHA256SUMS entry for $Asset"
        }

        $Filename = $Parts[1]
        if ($Filename.StartsWith('*')) {
            $Filename = $Filename.Substring(1)
        }
        if (-not $Filename) {
            throw "malformed SHA256SUMS entry for $Asset"
        }

        if ($Filename -eq $Asset) {
            $Entries += $Parts[0]
        }
    }

    if ($Entries.Count -eq 0) {
        throw "SHA256SUMS does not contain checksum for $Asset"
    }

    if ($Entries.Count -ne 1) {
        throw "SHA256SUMS contains duplicate checksum entries for $Asset"
    }

    return $Entries[0]
}

function Assert-ArchiveChecksum {
    param(
        [string]$ZipPath,
        [string]$ChecksumPath,
        [string]$Asset
    )

    Assert-GetFileHashAvailable
    $ExpectedHash = (Get-ExpectedSha256 -ChecksumPath $ChecksumPath -Asset $Asset).ToLowerInvariant()
    $ActualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ZipPath).Hash.ToLowerInvariant()

    if ($ActualHash -ne $ExpectedHash) {
        throw "checksum mismatch for $Asset"
    }
}

$Release = Invoke-RestMethod $ApiUrl
$Version = $Release.tag_name
if (-not $Version) {
    throw "could not resolve latest codex-image release from $ApiUrl"
}

$Target = "x86_64-pc-windows-msvc"
$Asset = "codex-image-$Version-$Target.zip"
$ArchiveRoot = "codex-image-$Version-$Target"
$TempDir = Join-Path $env:TEMP "codex-image-install-$([System.Guid]::NewGuid().ToString('N'))"
$ZipPath = Join-Path $TempDir $Asset
$ChecksumPath = Join-Path $TempDir "SHA256SUMS"

New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
try {
    Invoke-WebRequest "https://github.com/$Repo/releases/download/$Version/SHA256SUMS" -OutFile $ChecksumPath
    Invoke-WebRequest "https://github.com/$Repo/releases/download/$Version/$Asset" -OutFile $ZipPath
    Assert-ArchiveChecksum -ZipPath $ZipPath -ChecksumPath $ChecksumPath -Asset $Asset
    Expand-Archive -Path $ZipPath -DestinationPath $TempDir -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $BinaryPath = Join-Path $InstallDir "codex-image.exe"
    Copy-Item (Join-Path $TempDir "$ArchiveRoot\codex-image.exe") $BinaryPath -Force

    Write-Host "installed codex-image $Version to $BinaryPath"
    Write-Host "make sure $InstallDir is on your PATH"
    & $BinaryPath --help | Out-Null
}
finally {
    if (Test-Path $TempDir) {
        Remove-Item -Recurse -Force $TempDir
    }
}
