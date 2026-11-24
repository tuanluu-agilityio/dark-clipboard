# Validates a release tag against the workspace version in Cargo.toml and prints the release facts.
#   scripts/check-release-tag.ps1 v0.1.0          -> version=0.1.0 prerelease=false
#   scripts/check-release-tag.ps1 v0.1.0-rc.1     -> version=0.1.0-rc.1 prerelease=true
#   scripts/check-release-tag.ps1 -SelfTest
# vX.Y.Z must equal the workspace version; vX.Y.Z-<pre> is allowed when X.Y.Z matches (pre-release).
# When $env:GITHUB_OUTPUT is set, also writes version, core_version and prerelease there.
param([Parameter(Position = 0)][string]$Tag, [switch]$SelfTest)
$ErrorActionPreference = 'Stop'

function Get-ReleaseFacts([string]$Tag, [string]$WorkspaceVersion) {
    $m = [regex]::Match($Tag, '^v(\d+\.\d+\.\d+)(-[0-9A-Za-z][0-9A-Za-z.-]*)?$')
    if (-not $m.Success) { throw "tag '$Tag' is not vX.Y.Z or vX.Y.Z-<pre>" }
    $core = $m.Groups[1].Value
    if ($core -ne $WorkspaceVersion) { throw "tag '$Tag' does not match the workspace version $WorkspaceVersion in Cargo.toml" }
    [pscustomobject]@{ Version = $Tag.Substring(1); Core = $core; Prerelease = $m.Groups[2].Success }
}

if ($SelfTest) {
    $fail = 0
    function Expect([string]$name, [bool]$ok) { if ($ok) { Write-Host "ok   $name" } else { Write-Host "FAIL $name"; $script:fail++ } }
    $a = Get-ReleaseFacts 'v0.1.0' '0.1.0'
    Expect 'plain tag' ($a.Version -eq '0.1.0' -and -not $a.Prerelease)
    $b = Get-ReleaseFacts 'v0.1.0-rc.1' '0.1.0'
    Expect 'rc tag is a pre-release' ($b.Version -eq '0.1.0-rc.1' -and $b.Core -eq '0.1.0' -and $b.Prerelease)
    foreach ($bad in @('v0.1.1', 'v0.1.0.1', '0.1.0', 'v0.1', 'v0.1.0-', 'v0.1.0 rc', 'v1.1.0-rc.1')) {
        $threw = $false
        try { $null = Get-ReleaseFacts $bad '0.1.0' } catch { $threw = $true }
        Expect "rejects $bad" $threw
    }
    if ($fail -gt 0) { exit 1 }
    Write-Host 'check-release-tag self-test passed'
    exit 0
}

$repo = Split-Path $PSScriptRoot -Parent
$cargo = [System.IO.File]::ReadAllText((Join-Path $repo 'Cargo.toml'))
$w = [regex]::Match($cargo, '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
if (-not $w.Success) { throw 'workspace version not found in Cargo.toml' }
$facts = Get-ReleaseFacts $Tag $w.Groups[1].Value
$pre = $facts.Prerelease.ToString().ToLowerInvariant()
Write-Host "version=$($facts.Version) core_version=$($facts.Core) prerelease=$pre"
if ($env:GITHUB_OUTPUT) {
    Add-Content -LiteralPath $env:GITHUB_OUTPUT -Value "version=$($facts.Version)", "core_version=$($facts.Core)", "prerelease=$pre"
}
