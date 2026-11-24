# Quality gate before merging any branch. Prints "ALL CHECKS PASSED" and exits 0 when everything holds.
# Mirrors Darkshot's scripts/check.ps1 approach: cargo gates + static privacy/architecture rules.
# Bash twin for macOS/Linux: scripts/check.sh (keep the two in sync).
$ErrorActionPreference = 'Continue'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location (Split-Path $PSScriptRoot -Parent)
$root = (Get-Location).Path

function Fail([string]$msg) { Write-Host "CHECK FAILED: $msg"; exit 1 }
function Step([string]$name, [scriptblock]$cmd) {
    Write-Host "== $name"
    & $cmd
    if ($LASTEXITCODE -ne 0) { Fail $name }
}

$hasWorkspace = Test-Path -LiteralPath 'Cargo.toml'

if ($hasWorkspace) {
    Step 'cargo fmt --check' { cargo fmt --all --check }
    Step 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings }
    Step 'cargo test' { cargo test --workspace }

    Write-Host '== portable crates must not depend on OS/UI/async crates (GC-01)'
    $portable = @('darkclip-core', 'darkclip-platform', 'darkclip-storage', 'darkclip-app')
    $bannedPattern = '^(windows|windows-sys|objc2[\w-]*|core-foundation[\w-]*|x11rb|wayland-[\w-]+|gtk[\w-]*|slint[\w-]*|tauri[\w-]*|tokio) v'
    foreach ($crate in $portable) {
        $manifest = "crates/$crate/Cargo.toml"
        if (-not (Test-Path -LiteralPath $manifest)) { continue }
        $tree = cargo tree -p $crate -e normal --prefix none
        if ($LASTEXITCODE -ne 0) { Fail "cargo tree -p $crate" }
        $banned = $tree | Where-Object { $_ -match $bannedPattern } | Sort-Object -Unique
        if ($banned) {
            Write-Host "CHECK FAILED: $crate depends on a banned crate:"
            $banned | ForEach-Object { Write-Host "  $_" }
            exit 1
        }
    }
}

if ($hasWorkspace) {
    # GC-08 as amended by D-057: the only network code is the opt-in update check in apps/desktop
    # (src/update.rs). Nothing under crates/ may depend on an HTTP or TLS client, on any target.
    Write-Host '== crates/ stay network-free (GC-08, D-057)'
    $netPattern = '^(ureq[\w-]*|reqwest|hyper[\w-]*|isahc|surf|attohttpc|curl[\w-]*|minreq|h2|rustls[\w-]*|tokio-rustls|native-tls|openssl[\w-]*|webpki[\w-]*) v'
    foreach ($manifest in @(Get-ChildItem -Path 'crates' -Directory | ForEach-Object { Join-Path $_.FullName 'Cargo.toml' } | Where-Object { Test-Path -LiteralPath $_ })) {
        $tree = cargo tree --manifest-path $manifest -e normal,build,dev --target all --prefix none
        if ($LASTEXITCODE -ne 0) { Fail "cargo tree --manifest-path $manifest" }
        $net = $tree | Where-Object { $_ -match $netPattern } | Sort-Object -Unique
        if ($net) {
            Write-Host "CHECK FAILED: $manifest depends on a network crate:"
            $net | ForEach-Object { Write-Host "  $_" }
            exit 1
        }
    }
}

# Static rules over Rust sources (no-ops until the files exist).
$rs = @()
if (Test-Path -LiteralPath 'crates') { $rs += @(Get-ChildItem -Path 'crates' -Recurse -File -Filter '*.rs' | Where-Object { $_.FullName -notmatch '\\target\\' }) }
if (Test-Path -LiteralPath 'apps') { $rs += @(Get-ChildItem -Path 'apps' -Recurse -File -Filter '*.rs' | Where-Object { $_.FullName -notmatch '\\target\\' }) }
function Rel($f) { $f.FullName.Substring($root.Length + 1).Replace('\', '/') }

Write-Host '== no logging in privacy-critical modules (GC-06)'
$logPattern = '\b(e?println|e?print|dbg)!|\b(log|tracing)::(trace|debug|info|warn|error)!|\b(trace|debug|info|warn|error)!\s*\('
# Also the platform adapters and the app's image decoder (Task 110).
$quiet = '^(crates/darkclip-app/src/capture/.*\.rs|crates/darkclip-app/src/image\.rs|crates/darkclip-core/src/policy\.rs|crates/darkclip-core/src/sensitive/.*\.rs|crates/darkclip-(windows|macos|linux)/src/.*\.rs)$'
$bad = $false
foreach ($f in $rs) {
    $rel = Rel $f
    if ($rel -notmatch $quiet) { continue }
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern $logPattern)) {
        Write-Host "CHECK FAILED: logging macro in ${rel}:$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== headless binary: log/print lines carry no content-like words (S-10)'
# In apps/*/src/**, a log or print macro line must not mention preview, content, bytes, text, query,
# or Debug formatting. Line based: keep such macro calls on one line. Bash twin: scripts/check.sh.
$s10Words = 'preview|content|bytes|text|query|\{[^}]*:#?\?\}'
# Log lines: the macros of the GC-06 rule, plus calls of the binary's own logger (`.event(`, `Val::`).
$s10Lines = $logPattern + '|\.event\s*\(|\bVal::'
foreach ($f in $rs) {
    $rel = Rel $f
    if ($rel -notmatch '^apps/[^/]+/src/.*\.rs$') { continue }
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern $s10Lines)) {
        if ($h.Line -match $s10Words) { Write-Host "CHECK FAILED: content-like word in log/print line ${rel}:$($h.LineNumber)"; $bad = $true }
    }
}
if ($bad) { exit 1 }

Write-Host '== clipboard read/write only from the capture actor (GC-03)'
# Calls like `.read(&plan)` / `.write(&reps)` on ClipboardAccess are allowed only under capture/, the adapters
# (which implement them) and the platform crate's fake. Heuristic: method call on a variable named access/acc.
$accessCall = '\b(access|acc|clipboard_access)\s*\.\s*(read|write)\s*\('
$allowed = '^(crates/darkclip-app/src/capture/.*|crates/darkclip-platform/src/.*|crates/darkclip-(windows|macos|linux)/src/.*)$'
foreach ($f in $rs) {
    $rel = Rel $f
    if ($rel -match $allowed) { continue }
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern $accessCall)) {
        Write-Host "CHECK FAILED: clipboard access outside capture actor in ${rel}:$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }
# Hardening (Task 010 review): any use of the ClipboardAccess type or a bundle's `.access` field outside the
# allowed files fails, whatever the variable name. Comment lines are ignored.
$typeUse = '\bClipboardAccess\b|\.access\b|\baccess\s*:'
foreach ($f in $rs) {
    $rel = Rel $f
    if ($rel -match $allowed) { continue }
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern $typeUse)) {
        if ($h.Line -match '^\s*//') { continue }
        Write-Host "CHECK FAILED: ClipboardAccess used outside capture actor in ${rel}:$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== no renamed network dependency or Windows networking feature in any Cargo.toml (GC-08, D-057)'
$tomls = @()
foreach ($d in @('crates', 'apps', 'fuzz')) {
    if (Test-Path -LiteralPath $d) { $tomls += @(Get-ChildItem -Path $d -Recurse -File -Filter 'Cargo.toml' | Where-Object { $_.FullName -notmatch '\target\' }) }
}
foreach ($f in $tomls) {
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern 'package\s*=\s*"(ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)"|Win32_Networking|WinHttp|WinINet')) {
        Write-Host "CHECK FAILED: network dependency or feature in $(Rel $f):$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== network APIs only in apps/desktop/src/update.rs (GC-08, D-057)'
foreach ($f in $rs) {
    if ((Rel $f) -eq 'apps/desktop/src/update.rs') { continue }
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern '\b(ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)::|\b(use|extern crate) (ureq|reqwest|hyper|h2|attohttpc|isahc|minreq|curl|socket2|surf)\b|\b(mio|tokio)::net\b|\bstd::net\b|\b(TcpStream|TcpListener|UdpSocket)\b|\bWinHttp|\bWinINet|\bWin32::Networking')) {
        if ($h.Line -match '^\s*//') { continue }
        Write-Host "CHECK FAILED: network API outside update.rs in $(Rel $f):$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== no window-title capture (SR-11)'
foreach ($f in $rs) {
    foreach ($h in @(Select-String -LiteralPath $f.FullName -Pattern '\b(GetWindowText(W|A|Length\w*)?|InternalGetWindowText)\b|\bWM_GETTEXT(LENGTH)?\b|_NET_WM_NAME|\bkCGWindowName\b')) {
        Write-Host "CHECK FAILED: window title API in $(Rel $f):$($h.LineNumber)"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== SQLite connections keep temporary data in memory (D-073, D-077)'
# Every Connection::open*( in non-test code calls keep_temp_in_memory( within the next 25 lines of the same
# function, or carries "// temp-store: <reason>" on its line or the line before. Test code (tests.rs, tests/,
# #[cfg(test)] modules: an inline one up to its closing brace, `mod x;` just its line) is skipped. Bash twin: scripts/check.sh + scripts/temp-store-check.awk.
function Get-TempStoreMisses([string]$path, [string]$label) {
    $lines = @(Get-Content -LiteralPath $path)
    $skipl = New-Object bool[] $lines.Count
    for ($i = 0; $i -lt $lines.Count - 1; $i++) {
        if ($lines[$i] -notmatch '^#\[cfg\(test\)\]\s*$' -or $lines[$i + 1] -notmatch '^(pub(\([a-z]+\))? )?mod\s') { continue }
        $skipl[$i] = $true; $skipl[$i + 1] = $true
        if ($lines[$i + 1] -match ';\s*$') { continue }
        $depth = 0; $seen = $false
        for ($j = $i + 1; $j -lt $lines.Count; $j++) {
            $skipl[$j] = $true
            $opens = ([regex]::Matches($lines[$j], '\{')).Count; $closes = ([regex]::Matches($lines[$j], '\}')).Count
            $depth += $opens - $closes
            if ($opens -gt 0) { $seen = $true }
            if ($seen -and $depth -le 0) { break }
        }
    }
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($skipl[$i]) { continue }
        $l = $lines[$i]
        if ($l -match '^\s*//') { continue }
        if ($l -notmatch 'Connection::open\w*\(' -or $l -match 'Connection::open_in_memory') { continue }
        if ($l -match '// temp-store: ' -or ($i -gt 0 -and $lines[$i - 1] -match '// temp-store: ')) { continue }
        $ok = $false
        for ($j = $i; $j -le $i + 25 -and $j -lt $lines.Count; $j++) {
            if ($j -gt $i -and $lines[$j] -match '^\s*(pub(\([a-z]+\))? )?(async )?(unsafe )?fn\s') { break }
            if ($lines[$j] -match 'keep_temp_in_memory\(') { $ok = $true; break }
        }
        if (-not $ok) { "${label}:$($i + 1)" }
    }
}
$tsBad = @(Get-TempStoreMisses 'scripts/fixtures/temp-store-bad.rs' 'scripts/fixtures/temp-store-bad.rs')
$tsGood = @(Get-TempStoreMisses 'scripts/fixtures/temp-store-good.rs' 'scripts/fixtures/temp-store-good.rs')
if ($tsBad.Count -ne 1 -or $tsBad[0] -ne 'scripts/fixtures/temp-store-bad.rs:3') { Fail "temp-store self-test: bad fixture not flagged exactly once (got: $($tsBad -join ' '))" }
if ($tsGood.Count -ne 0) { Fail "temp-store self-test: good fixture flagged ($($tsGood -join ' '))" }
foreach ($tsName in @('after-inline', 'after-external')) {
    $tsOut = @(Get-TempStoreMisses "scripts/fixtures/temp-store-bad-$tsName.rs" "scripts/fixtures/temp-store-bad-$tsName.rs")
    if ($tsOut.Count -ne 1) { Fail "temp-store self-test: bad-$tsName fixture not flagged exactly once (got: $($tsOut -join ' '))" }
}
Write-Host "self-test ok: bad fixture flagged ($($tsBad[0])), good fixture passed"
foreach ($f in $rs) {
    $rel = Rel $f
    if ($rel -match '(^|/)tests\.rs$|/tests/') { continue }
    foreach ($miss in @(Get-TempStoreMisses $f.FullName $rel)) {
        Write-Host "CHECK FAILED: SQLite connection without keep_temp_in_memory (or '// temp-store: <reason>') at $miss"; $bad = $true
    }
}
if ($bad) { exit 1 }

Write-Host '== mandatory markers present (GC-07)'
$markersFile = 'crates/darkclip-core/src/markers.rs'
if (Test-Path -LiteralPath $markersFile) {
    $m = Get-Content -Raw -LiteralPath $markersFile
    foreach ($name in @('ExcludeClipboardContentFromMonitorProcessing', 'CanIncludeInClipboardHistory', 'org.nspasteboard.ConcealedType', 'x-kde-passwordManagerHint')) {
        if ($m -notmatch [regex]::Escape($name)) { Fail "mandatory marker '$name' missing from $markersFile" }
    }
}

Write-Host '== JSON schemas and examples'
if (Get-Command node -ErrorAction SilentlyContinue) {
    if (-not (Test-Path -LiteralPath 'tools/schema-check/node_modules')) {
        Push-Location tools/schema-check; npm ci --silent; $code = $LASTEXITCODE; Pop-Location
        if ($code -ne 0) { Fail 'npm ci in tools/schema-check' }
    }
    Step 'schema validation' { node tools/schema-check/validate.mjs }
    Step 'workflow YAML parses' { node tools/schema-check/check-workflows.mjs }
} else {
    Fail 'node not found (required for schema validation)'
}

Write-Host '== required Phase 0 documents'
$required = @(
    'docs/product/product-vision.md', 'docs/product/feature-matrix.md', 'docs/product/mvp-scope.md', 'docs/product/user-journeys.md',
    'docs/architecture/system-overview.md', 'docs/architecture/platform-compatibility.md', 'docs/architecture/data-model.md',
    'docs/architecture/ai-integration.md', 'docs/security/threat-model.md', 'docs/security/privacy-design.md',
    'docs/plans/roadmap.md', 'docs/plans/global-constraints.md', 'memory-bank/activeContext.md', 'memory-bank/progress.md',
    'CLAUDE.md', 'AGENTS.md'
)
foreach ($p in $required) { if (-not (Test-Path -LiteralPath $p)) { Fail "missing $p" } }
if (@(Get-ChildItem 'docs/adr' -Filter '0*.md').Count -lt 6) { Fail 'expected at least 6 ADRs in docs/adr' }

Write-Host '== relative links in docs resolve'
$broken = $false
$mdFiles = @(Get-ChildItem -Path 'docs', 'memory-bank', '.claude' -Recurse -File -Filter '*.md') + @(Get-Item 'README.md', 'CLAUDE.md', 'AGENTS.md')
foreach ($md in $mdFiles) {
    $text = Get-Content -Raw -LiteralPath $md.FullName
    foreach ($mt in [regex]::Matches($text, '\]\(([^)#\s]+)(#[^)]*)?\)')) {
        $target = $mt.Groups[1].Value
        if ($target -match '^(https?|mailto):') { continue }
        $resolved = Join-Path $md.DirectoryName $target
        if (-not (Test-Path -LiteralPath $resolved)) { Write-Host "CHECK FAILED: broken link in $(Rel $md): $target"; $broken = $true }
    }
}
if ($broken) { exit 1 }

Write-Host '== release script self-tests (Task 408)'
Step 'changelog-section self-test' { & (Get-Process -Id $PID).Path -NoProfile -File scripts/changelog-section.ps1 -SelfTest }
Step 'check-release-tag self-test' { & (Get-Process -Id $PID).Path -NoProfile -File scripts/check-release-tag.ps1 -SelfTest }
if (Get-Command cargo-about -ErrorAction SilentlyContinue) {
    Step 'THIRD-PARTY-NOTICES generator runs' { & (Get-Process -Id $PID).Path -NoProfile -File scripts/release-notices.ps1 -Out (Join-Path $env:TEMP 'dc-notices-check/THIRD-PARTY-NOTICES.txt') }
} else {
    Write-Host 'cargo-about not installed: notices generator not exercised here (the release workflow runs it)'
}

Write-Host 'ALL CHECKS PASSED'
exit 0
