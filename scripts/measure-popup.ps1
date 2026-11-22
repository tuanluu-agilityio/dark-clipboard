# NFR-01 measurement (Task 204): time from "show asked" (shortcut press, tray click or --toggle
# received over IPC) to the first frame of the popup, from the app's content-free `perf` log events:
#     <unix_ms> perf what=popup_shown src=<hotkey|ipc|tray> us=<microseconds>
#
# Two ways to use it:
#   1. Drive it over IPC (default). Starts the app on a fresh temp data dir (global shortcut off),
#      sends `--toggle` 2 x Trials times (show, hide), quits it, parses the log, deletes the dir.
#      No key or mouse input is simulated and the clipboard is never written.
#        powershell -File scripts/measure-popup.ps1 [-Exe target\release\dark-clipboard.exe] [-Trials 50] [-SeedClips 10000]
#   2. Parse an existing log (for the owner's real-shortcut row: press the shortcut 50 times, then):
#        powershell -File scripts/measure-popup.ps1 -LogPath <data dir>\logs\dark-clipboard.log -Source hotkey
#
# Prints every value (ms) and p50 / p95 (nearest rank) of the last -Trials events of -Source.
param(
    [string]$Exe = 'target\release\dark-clipboard.exe',
    [int]$Trials = 50,
    [string]$LogPath = '',
    [ValidateSet('ipc', 'hotkey', 'tray')][string]$Source = 'ipc',
    [int]$SeedClips = 0,
    [int]$ShowMs = 400,
    [int]$HideMs = 250
)
$ErrorActionPreference = 'Stop'

function Get-Percentile([double[]]$sorted, [double]$p) {
    $rank = [math]::Ceiling($p / 100.0 * $sorted.Count)
    if ($rank -lt 1) { $rank = 1 }
    return $sorted[$rank - 1]
}

function Read-PerfValues([string]$path, [string]$src) {
    if (-not (Test-Path -LiteralPath $path)) { throw "log not found: $path" }
    $values = @()
    foreach ($line in Get-Content -LiteralPath $path) {
        if ($line -match "^\d+ perf what=popup_shown src=$src us=(\d+)$") { $values += [double]$Matches[1] / 1000.0 }
    }
    return , $values
}

function Invoke-App([string[]]$argv) {
    $p = Start-Process -FilePath $Exe -ArgumentList $argv -Wait -PassThru -WindowStyle Hidden
    return $p.ExitCode
}

$tempDir = $null
$app = $null
try {
    if ($LogPath -eq '') {
        if (-not (Test-Path -LiteralPath $Exe)) { throw "binary not found: $Exe (cargo build --release -p dark-clipboard)" }
        $Exe = (Resolve-Path -LiteralPath $Exe).Path
        $tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("darkclip-measure-" + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tempDir | Out-Null
        if ($SeedClips -gt 0) {
            $code = Invoke-App @('--seed-demo-clips', "$SeedClips", '--data-dir', "`"$tempDir`"")
            if ($code -ne 0) { throw "seeding failed with exit code $code" }
        }
        $LogPath = Join-Path $tempDir 'logs\dark-clipboard.log'
        # Safety net: the app stops by itself even if this script is interrupted.
        $limit = ($Trials * ($ShowMs + $HideMs + 2000)) + 60000
        $app = Start-Process -FilePath $Exe -PassThru -ArgumentList @('--data-dir', "`"$tempDir`"", '--no-global-shortcut', '--exit-after-ms', "$limit")
        $deadline = (Get-Date).AddSeconds(30)
        while (-not ((Test-Path -LiteralPath $LogPath) -and (Select-String -LiteralPath $LogPath -Pattern ' running$' -Quiet))) {
            if ((Get-Date) -gt $deadline) { throw 'the app did not report running within 30 s' }
            if ($app.HasExited) { throw "the app exited early with code $($app.ExitCode)" }
            Start-Sleep -Milliseconds 100
        }
        Start-Sleep -Milliseconds 500
        for ($i = 1; $i -le $Trials; $i++) {
            if ((Invoke-App @('--toggle', '--data-dir', "`"$tempDir`"")) -ne 0) { throw "toggle (show) $i failed" }
            Start-Sleep -Milliseconds $ShowMs
            if ((Invoke-App @('--toggle', '--data-dir', "`"$tempDir`"")) -ne 0) { throw "toggle (hide) $i failed" }
            Start-Sleep -Milliseconds $HideMs
        }
        [void](Invoke-App @('--quit', '--data-dir', "`"$tempDir`""))
        if (-not $app.WaitForExit(10000)) { $app.Kill(); throw 'the app did not quit within 10 s' }
    }

    $values = Read-PerfValues $LogPath $Source
    if ($values.Count -lt $Trials) { throw "only $($values.Count) perf events with src=$Source (wanted $Trials)" }
    $last = @($values | Select-Object -Last $Trials)
    $sorted = [double[]]($last | Sort-Object)
    Write-Host ("raw ms ({0} trials, src={1}): {2}" -f $Trials, $Source, (($last | ForEach-Object { '{0:N1}' -f $_ }) -join ', '))
    Write-Host ('min {0:N1} ms  p50 {1:N1} ms  p95 {2:N1} ms  max {3:N1} ms' -f $sorted[0], (Get-Percentile $sorted 50), (Get-Percentile $sorted 95), $sorted[$sorted.Count - 1])
}
finally {
    if ($app -and -not $app.HasExited) { $app.Kill() }
    if ($tempDir -and (Test-Path -LiteralPath $tempDir)) {
        Start-Sleep -Milliseconds 300
        Remove-Item -LiteralPath $tempDir -Recurse -Force
        if (Test-Path -LiteralPath $tempDir) { Write-Host "WARNING: could not delete $tempDir" } else { Write-Host 'temp data dir deleted' }
    }
}
