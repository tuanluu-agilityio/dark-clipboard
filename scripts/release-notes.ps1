# Writes the GitHub release notes: the CHANGELOG section for the version (a pre-release without its own
# section falls back to the section of X.Y.Z), then the fixed unsigned/platform paragraphs.
#   scripts/release-notes.ps1 -Version 0.1.0-rc.1 -CoreVersion 0.1.0 -Prerelease -Out notes.md [-DryRun]
# A missing section is an error, except with -DryRun: then a warning and a placeholder marked DRY RUN.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$CoreVersion,
    [switch]$Prerelease,
    [switch]$DryRun,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$ChangelogPath
)
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'changelog-section.ps1'
$extra = @{}
if ($ChangelogPath) { $extra.Path = $ChangelogPath }

$section = & $script -Version $Version @extra 2>$null
if ($LASTEXITCODE -ne 0) { $section = $null }
if (-not $section -and $Version -ne $CoreVersion) {
    $section = & $script -Version $CoreVersion @extra 2>$null
    if ($LASTEXITCODE -ne 0) { $section = $null }
}
if (-not $section) {
    if (-not $DryRun) { throw "CHANGELOG.md has no section for version $Version" }
    Write-Host "::warning::CHANGELOG.md has no section for version $Version (dry run: placeholder notes written)"
    $section = "DRY RUN: CHANGELOG.md has no section for version $Version."
}

$parts = New-Object System.Collections.Generic.List[string]
if ($Prerelease) { $parts.Add("Release candidate for $CoreVersion."); $parts.Add('') }
$parts.Add(($section -join "`n"))
$parts.Add('')
$parts.Add('---')
$parts.Add('')
$parts.Add('**Unsigned Windows build.** The binary has no Authenticode signature, so Windows SmartScreen may warn. Verify the download with `SHA256SUMS.txt` (PowerShell: `Get-FileHash <file> -Algorithm SHA256`, then compare).')
$parts.Add('')
$parts.Add('**Platforms:** Windows x64 only, tested by the developer only. macOS and Linux builds are not available yet.')
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path -LiteralPath $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
[System.IO.File]::WriteAllText($Out, ($parts -join "`n") + "`n", (New-Object System.Text.UTF8Encoding($false)))
Write-Host "wrote $Out"
# A missing section in a dry run left $LASTEXITCODE = 1 from changelog-section.ps1, and the
# Actions pwsh step returns $LASTEXITCODE. The notes were written, so succeed explicitly.
exit 0
