# Generates THIRD-PARTY-NOTICES.txt for the Windows x64 dependency graph of dark-clipboard (F407-07).
# The file is generated at release time and not committed (about 300 KB, changes with every Cargo.lock update).
#   scripts/release-notices.ps1 [-Out target/release-notices/THIRD-PARTY-NOTICES.txt]
# Needs: cargo install cargo-about --locked --version 0.8.4   (CI installs the same pinned version)
param([string]$Out)
$ErrorActionPreference = 'Stop'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo
$AboutVersion = '0.8.4'

if (-not $Out) { $Out = Join-Path $repo 'target/release-notices/THIRD-PARTY-NOTICES.txt' }
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

$ver = (& cargo about --version) 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $ver -notmatch [regex]::Escape($AboutVersion)) {
    throw "cargo-about $AboutVersion is required (found: $($ver.Trim())). Run: cargo install cargo-about --locked --version $AboutVersion"
}
& cargo about generate --frozen -c about.toml about.hbs -o $Out -m apps/desktop/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'cargo about generate failed' }

$text = [System.IO.File]::ReadAllText($Out)
foreach ($needle in @('webpki-roots', 'CDLA-Permissive-2.0', 'GPL-3.0-only')) {
    if ($text -notmatch [regex]::Escape($needle)) { throw "notices file is missing expected entry: $needle" }
}
Write-Host "wrote $Out ($([int]((Get-Item -LiteralPath $Out).Length / 1024)) KB)"
