# Packages the Windows x64 release: dark-clipboard-<version>-windows-x64.zip, the bare exe and
# SHA256SUMS.txt, all in -OutDir. Used by CI (release.yml) and by the owner locally.
#   scripts/package-windows.ps1 -OutDir $env:TEMP\dc-pkg [-Version 0.1.0] [-NoticesPath file]
# Run `cargo build --release --locked -p dark-clipboard` first. Never writes into the repo.
param(
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Version,
    [string]$ExePath,
    [string]$NoticesPath
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

if (-not $Version) {
    $m = [regex]::Match([System.IO.File]::ReadAllText((Join-Path $repo 'Cargo.toml')), '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $m.Success) { throw 'workspace version not found in Cargo.toml' }
    $Version = $m.Groups[1].Value
}
if (-not $ExePath) { $ExePath = Join-Path $repo 'target/release/dark-clipboard.exe' }
if (-not (Test-Path -LiteralPath $ExePath)) { throw "release exe not found: $ExePath (build with cargo build --release --locked -p dark-clipboard)" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path
if (-not $NoticesPath) {
    $NoticesPath = Join-Path $OutDir 'THIRD-PARTY-NOTICES.txt'
    & (Join-Path $PSScriptRoot 'release-notices.ps1') -Out $NoticesPath
}
if (-not (Test-Path -LiteralPath $NoticesPath)) { throw "notices file not found: $NoticesPath" }

$stage = Join-Path $OutDir 'stage'
if (Test-Path -LiteralPath $stage) { Remove-Item -Recurse -Force -LiteralPath $stage }
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -LiteralPath $ExePath -Destination (Join-Path $stage 'dark-clipboard.exe')
foreach ($f in @('LICENSE', 'README.md', 'CHANGELOG.md')) { Copy-Item -LiteralPath (Join-Path $repo $f) -Destination (Join-Path $stage $f) }
Copy-Item -LiteralPath $NoticesPath -Destination (Join-Path $stage 'THIRD-PARTY-NOTICES.txt')

$zipName = "dark-clipboard-$Version-windows-x64.zip"
$zip = Join-Path $OutDir $zipName
if (Test-Path -LiteralPath $zip) { Remove-Item -Force -LiteralPath $zip }
$files = @('dark-clipboard.exe', 'LICENSE', 'README.md', 'CHANGELOG.md', 'THIRD-PARTY-NOTICES.txt') | ForEach-Object { Join-Path $stage $_ }
Compress-Archive -LiteralPath $files -DestinationPath $zip -CompressionLevel Optimal

$bareExe = Join-Path $OutDir 'dark-clipboard.exe'
Copy-Item -LiteralPath $ExePath -Destination $bareExe -Force
Remove-Item -Recurse -Force -LiteralPath $stage

$sb = New-Object System.Text.StringBuilder
foreach ($f in @($zip, $bareExe)) {
    $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $f).Hash.ToLowerInvariant()
    [void]$sb.Append("$h  $(Split-Path -Leaf $f)`n")
}
[System.IO.File]::WriteAllText((Join-Path $OutDir 'SHA256SUMS.txt'), $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "packaged $zipName in $OutDir"
