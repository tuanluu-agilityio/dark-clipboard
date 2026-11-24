# Prints the CHANGELOG.md section for one version (the text between its "## [version]" heading
# and the next "## " heading, without the heading). Exits 1 when the section is missing or empty.
#   scripts/changelog-section.ps1 0.1.0
#   scripts/changelog-section.ps1 Unreleased -Path other/CHANGELOG.md
#   scripts/changelog-section.ps1 -SelfTest      (run by check.ps1 against scripts/fixtures)
# Works on pwsh and Windows PowerShell 5.1. Output is the section text on the pipeline (one string).
param(
    [Parameter(Position = 0)][string]$Version,
    [string]$Path,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent

function Get-ChangelogSection([string]$File, [string]$Ver) {
    $lines = [System.IO.File]::ReadAllText($File).Replace("`r`n", "`n").Split("`n")
    $head = '## [' + $Ver + ']'
    $out = New-Object System.Collections.Generic.List[string]
    $inside = $false
    foreach ($l in $lines) {
        if ($l.StartsWith('## ')) {
            if ($inside) { break }
            # exact version only: "## [1.2.0]" followed by end of line or a space (date suffix)
            if ($l -eq $head -or $l.StartsWith($head + ' ')) { $inside = $true }
            continue
        }
        if ($inside) { $out.Add($l) }
    }
    if (-not $inside) { return $null }
    while ($out.Count -gt 0 -and $out[0].Trim() -eq '') { $out.RemoveAt(0) }
    while ($out.Count -gt 0 -and $out[$out.Count - 1].Trim() -eq '') { $out.RemoveAt($out.Count - 1) }
    return ($out -join "`n")
}

if ($SelfTest) {
    $fx = Join-Path $PSScriptRoot 'fixtures/CHANGELOG.fixture.md'
    $fail = 0
    function Assert-Eq($name, $actual, $expected) {
        if ($actual -ceq $expected) { Write-Host "ok   $name" } else {
            Write-Host "FAIL $name`n--- expected`n$expected`n--- actual`n$actual"; $script:fail++
        }
    }
    Assert-Eq 'middle section with date' (Get-ChangelogSection $fx '1.2.0') "### Added`n- Feature A.`n- Feature B with ``code``.`n`n### Fixed`n- Bug C."
    Assert-Eq 'section without date' (Get-ChangelogSection $fx '1.1.0') "### Added`n- Older feature."
    Assert-Eq 'last section' (Get-ChangelogSection $fx '1.0.0') "### Added`n- First release."
    Assert-Eq 'unreleased' (Get-ChangelogSection $fx 'Unreleased') "### Added`n- Something not yet released."
    Assert-Eq 'missing version is null' ($null -eq (Get-ChangelogSection $fx '9.9.9')) $true
    Assert-Eq 'prefix does not match' ($null -eq (Get-ChangelogSection $fx '1.2')) $true
    $ErrorActionPreference = 'Continue'
    $self = (Get-Process -Id $PID).Path
    $null = & $self -NoProfile -File $PSCommandPath 9.9.9 -Path $fx 2>&1
    Assert-Eq 'script exits 1 for a missing version' $LASTEXITCODE 1
    if ($fail -gt 0) { Write-Host "changelog-section self-test: $fail failed"; exit 1 }
    Write-Host 'changelog-section self-test passed'
    exit 0
}

if (-not $Version) { Write-Error 'usage: changelog-section.ps1 <version> [-Path file] | -SelfTest'; exit 2 }
if (-not $Path) { $Path = Join-Path $repo 'CHANGELOG.md' }
$section = Get-ChangelogSection $Path $Version
if ($null -eq $section) { [Console]::Error.WriteLine("no CHANGELOG section for version $Version in $Path"); exit 1 }
if ($section.Trim() -eq '') { [Console]::Error.WriteLine("CHANGELOG section for version $Version is empty"); exit 1 }
Write-Output $section
exit 0
