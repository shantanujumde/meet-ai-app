<#
TUR-97 gate on Windows (TUR-50): does a recording survive `taskkill /F` of
the installed app mid-meeting?

The same idea as gate.sh (macOS): start a real recording in the installed
app, let it run, end it with `taskkill /F` (no chance to run any code),
relaunch it, then check the files a user would open: both WAV headers,
segments.json, drift-check, and that the app starts again with no ERROR or
panic in its log.

  pwsh scripts/gates/tur97-kill/gate.ps1                    # 60 s, taskkill /F
  pwsh scripts/gates/tur97-kill/gate.ps1 -Seconds 3600      # the 1-hour variant
  pwsh scripts/gates/tur97-kill/gate.ps1 -App C:\path\to\meet-ai.exe

What differs from gate.sh:
- The recording starts with `meet-ai.exe --toggle-recording` (TUR-58), which
  reaches the running app through its single-instance guard. No keystroke.
- There is no meet-stt off macOS, so check 7 (system.wav transcribes) is
  skipped. -Play FILE plays a WAV through the default output while
  recording, so system.wav is not silent.
- The relaunched app is ended with `taskkill` without /F (a close request);
  how long that takes is reported, not graded.

Needs: Python 3 (`py -3` or `python`), cargo (drift-check is built from this
checkout), Windows PowerShell 5.1 or PowerShell 7. See README.md next to
this file for what every check proves.
#>
[CmdletBinding()]
param(
    # Where the NSIS installer puts the program for the current user.
    # Measure it on a real install; pass -App for another one.
    [string]$App = (Join-Path $env:LOCALAPPDATA 'meet-ai\meet-ai.exe'),
    [int]$Seconds = 60,
    [string]$Out = '',
    [switch]$Keep,
    [string]$Play = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Here = $PSScriptRoot
$Root = (Resolve-Path (Join-Path $Here '..\..\..')).Path
$Check = Join-Path $Here 'check.py'

if (-not (Test-Path -LiteralPath $App -PathType Leaf)) {
    Write-Error "no meet-ai program at $App (pass -App)"
}
$App = (Resolve-Path -LiteralPath $App).Path

# Python 3: the launcher first, then python on PATH.
$Python = @()
if (Get-Command py -ErrorAction SilentlyContinue) { $Python = @('py', '-3') }
elseif (Get-Command python -ErrorAction SilentlyContinue) { $Python = @('python') }
else { Write-Error 'Python 3 is required (py -3 or python)' }

if (-not $Out) {
    $Out = Join-Path $Root ("target\tur97-kill-gate\{0}-windows-taskkill-{1}s" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), $Seconds)
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$Summary = Join-Path $Out 'summary.txt'
Set-Content -LiteralPath $Summary -Value $null
$RunLog = Join-Path $Out 'run.log'

function Say-Line([string]$Text) {
    Write-Host $Text
    Add-Content -LiteralPath $Summary -Value $Text
}
function Note([string]$Text) {
    $line = '   {0} {1}' -f (Get-Date -Format 'HH:mm:ss'), $Text
    Write-Host $line
    Add-Content -LiteralPath $RunLog -Value $line
}

$script:Passed = 0
$script:Total = 0
$script:FailedAny = $false
function Report([string]$Status, [string]$Name, [string]$Reason) {
    switch ($Status) {
        'ok' { $script:Passed++; $script:Total++ }
        'FAIL' { $script:Total++; $script:FailedAny = $true }
    }
    Say-Line ('{0,-4}  {1,-34} {2}' -f $Status, $Name, $Reason)
}

# Only processes running the program under test count as "the app": another
# meet-ai build on this machine is never killed.
function App-Pids {
    @(Get-Process -Name 'meet-ai' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and ($_.Path -ieq $App) } |
        ForEach-Object { $_.Id })
}
function App-Running { (App-Pids).Count -gt 0 }
function Wait-For([int]$Secs, [scriptblock]$Test) {
    $deadline = (Get-Date).AddSeconds($Secs)
    while ((Get-Date) -lt $deadline) {
        if (& $Test) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return [bool](& $Test)
}
# Native commands run with 'Continue': Windows PowerShell 5.1 turns any
# stderr line of a native command into a terminating error under 'Stop'.
function Run-Python([string[]]$Arguments) {
    $ErrorActionPreference = 'Continue'
    $exe = $Python[0]
    $rest = @($Python | Select-Object -Skip 1) + $Arguments
    $text = & $exe @rest 2>$null | Out-String
    if (-not $text.Trim()) { return [pscustomobject]@{ ok = $false; reason = 'check.py printed nothing' } }
    return ($text | ConvertFrom-Json)
}
function Field($Object, [string]$Name) {
    $prop = $Object.PSObject.Properties[$Name]
    if ($null -eq $prop) { return $null }
    return $prop.Value
}
function List-Meetings([string]$Dir) {
    @(Get-ChildItem -LiteralPath $Dir -Directory -Filter '*-meeting*' -ErrorAction SilentlyContinue |
        ForEach-Object { $_.FullName } | Sort-Object)
}
function File-Size([string]$Path) {
    if (Test-Path -LiteralPath $Path) { return (Get-Item -LiteralPath $Path).Length }
    return 0
}
function End-App([int]$Secs) {
    $ErrorActionPreference = 'Continue'
    foreach ($id in App-Pids) { & cmd /c "taskkill /PID $id >nul 2>&1" }
    if (Wait-For $Secs { -not (App-Running) }) { return $true }
    foreach ($id in App-Pids) { & cmd /c "taskkill /F /T /PID $id >nul 2>&1" }
    Wait-For 5 { -not (App-Running) } | Out-Null
    return $false
}

# --------------------------------------------------------------------------
# preflight

$already = @(Get-Process -Name 'meet-ai' -ErrorAction SilentlyContinue)
if ($already.Count -gt 0) {
    Write-Error ("meet-ai is already running (pid {0}). Quit it first: the gate has to own the only running copy to know which process to kill." -f (($already | ForEach-Object { $_.Id }) -join ' '))
}

$TempRoot = Join-Path $Out 'meetings-root'
$LogFile = Join-Path $TempRoot '.app\logs\meet-ai.log'
$Player = $null
$Meeting = $null

Say-Line ('TUR-97 kill gate (Windows), {0}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'))
Say-Line "app: $App"
Say-Line "mode: record ${Seconds}s, end=taskkill /F, meetings root: $TempRoot"
Say-Line ''

Write-Host '==> building drift-check (cargo build -p audio --bin drift-check)'
Push-Location $Root
try {
    $ErrorActionPreference = 'Continue'
    & cargo build -q -p audio --bin drift-check *> (Join-Path $Out 'drift-check-build.log')
    if ($LASTEXITCODE -ne 0) { Note 'drift-check did not build; check 6 will fail (see drift-check-build.log)' }
} finally { $ErrorActionPreference = 'Stop'; Pop-Location }

# The onboarding flag lives inside the meetings root, so a fresh temp root
# would open the setup screen. Mark setup as done for the temp root only.
New-Item -ItemType Directory -Force -Path (Join-Path $TempRoot '.app') | Out-Null
$stamp = (Get-Date).ToString('yyyy-MM-ddTHH:mm:sszzz')
Set-Content -LiteralPath (Join-Path $TempRoot '.app\onboarding.json') -Value "{`n  `"completedAt`": `"$stamp`"`n}"
$before = List-Meetings $TempRoot
# Inherited by every meet-ai this script starts. Restored at the end.
$oldRoot = $env:MEET_AI_MEETINGS_ROOT
$env:MEET_AI_MEETINGS_ROOT = $TempRoot

try {
    # ----------------------------------------------------------------------
    # record, taskkill /F

    Note "launching $App"
    Start-Process -FilePath $App -RedirectStandardOutput (Join-Path $Out 'app-stdout.log') `
        -RedirectStandardError (Join-Path $Out 'app-stderr.log') | Out-Null
    if (-not (Wait-For 15 { App-Running })) {
        Report 'FAIL' '1 meeting folder' 'meet-ai did not start within 15 s'
        exit 1
    }
    $AppPid = (App-Pids)[0]
    Note "meet-ai up (pid $AppPid); waiting 4 s before the toggle"
    Start-Sleep -Seconds 4

    Note 'meet-ai --toggle-recording'
    Start-Process -FilePath $App -ArgumentList '--toggle-recording' | Out-Null

    $found = Wait-For 20 {
        $new = @(List-Meetings $TempRoot | Where-Object { $before -notcontains $_ })
        if ($new.Count -gt 0) { $script:Meeting = $new[-1]; return $true }
        return $false
    }
    if (-not $found) {
        Report 'FAIL' '1 meeting folder' 'no new *-meeting folder within 20 s of --toggle-recording'
        exit 1
    }
    $T0 = Get-Date
    Note "recording started: $Meeting"

    if ($Play) {
        Note "playing $Play"
        $Player = New-Object System.Media.SoundPlayer $Play
        $Player.Play()
    }

    while (((Get-Date) - $T0).TotalSeconds -lt $Seconds) {
        if (-not (App-Running)) { Note 'meet-ai exited on its own during the recording'; break }
        Start-Sleep -Seconds 1
    }
    $Recorded = [int][math]::Floor(((Get-Date) - $T0).TotalSeconds)
    Note "taskkill /F /PID $AppPid after ${Recorded}s"
    & cmd /c "taskkill /F /PID $AppPid >nul 2>&1"
    Wait-For 5 { -not (App-Running) } | Out-Null
    if ($Player) { $Player.Stop(); $Player = $null }

    # What the kill left on disk, before the relaunch can repair it.
    $audio = Join-Path $Meeting 'audio'
    foreach ($ch in 'mic', 'system') {
        $wav = Join-Path $audio "$ch.wav"
        if (Test-Path -LiteralPath $wav) {
            Run-Python @($Check, 'wav', $wav, '--expect-seconds', "$Recorded") |
                ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $Out "after-end-$ch-wav.json")
        }
    }
    $segments = Join-Path $audio 'segments.json'
    if (Test-Path -LiteralPath $segments) { Copy-Item -LiteralPath $segments (Join-Path $Out 'after-end-segments.json') }
    Get-ChildItem -LiteralPath $Meeting -Recurse -ErrorAction SilentlyContinue |
        Format-Table Length, LastWriteTime, FullName -AutoSize | Out-String -Width 400 |
        Set-Content -LiteralPath (Join-Path $Out 'after-end-ls.txt')

    # ----------------------------------------------------------------------
    # check 8: the app starts again with a clean log

    $off = File-Size $LogFile
    Note "relaunching $App"
    $t0 = Get-Date
    Start-Process -FilePath $App -RedirectStandardOutput (Join-Path $Out 'relaunch-app-stdout.log') `
        -RedirectStandardError (Join-Path $Out 'relaunch-app-stderr.log') | Out-Null
    if (-not (Wait-For 10 { App-Running })) {
        Report 'FAIL' '8 relaunch' 'meet-ai did not come up within 10 s'
    } else {
        $up = [int]((Get-Date) - $t0).TotalSeconds
        Start-Sleep -Seconds 8
        $why = "up in ${up}s"
        $ok = $true
        if (-not (App-Running)) { $why += ', exited again within 8 s'; $ok = $false }
        $q0 = Get-Date
        if (End-App 20) { Note ('taskkill ended the relaunch in {0}s' -f [int]((Get-Date) - $q0).TotalSeconds) }
        else { Note 'taskkill without /F did not end the relaunch within 20 s; used /F' }
        $scan = Run-Python @($Check, 'logscan', $LogFile, "$off")
        $scan | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $Out 'relaunch-logscan.json')
        if (-not (Field $scan 'ok')) { $ok = $false }
        $why += ', ' + (Field $scan 'reason')
        if ($ok) { Report 'ok' '8 relaunch' $why } else { Report 'FAIL' '8 relaunch' $why }
    }

    # ----------------------------------------------------------------------
    # checks 1-7 on the meeting as the relaunch left it

    if (Test-Path -LiteralPath $audio -PathType Container) { Report 'ok' '1 meeting folder' $Meeting }
    else { Report 'FAIL' '1 meeting folder' "no audio\ folder in $Meeting" }

    $hdrOk = $true; $durOk = $true; $tailOk = $true
    $hdrWhy = @(); $durWhy = @(); $tailWhy = @()
    foreach ($ch in 'mic', 'system') {
        $wav = Join-Path $audio "$ch.wav"
        if (-not (Test-Path -LiteralPath $wav)) {
            $hdrOk = $false; $durOk = $false; $tailOk = $false
            $hdrWhy += "$ch.wav missing"
            continue
        }
        $j = Run-Python @($Check, 'wav', $wav, '--expect-seconds', "$Recorded")
        $j | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $Out "final-$ch-wav.json")
        $reason = Field $j 'reason'
        if (-not (Field $j 'header_ok')) { $hdrOk = $false }
        $hdrWhy += "${ch}: " + $(if (Field $j 'header_reason') { Field $j 'header_reason' } else { $reason })
        if ((Field $j 'duration_ok') -eq $false) { $durOk = $false }
        $durWhy += "${ch}: " + $(if (Field $j 'duration_reason') { Field $j 'duration_reason' } else { $reason })
        if (-not (Field $j 'tail_ok')) { $tailOk = $false }
        $tailWhy += "${ch}: " + $(if (Field $j 'tail_reason') { Field $j 'tail_reason' } else { $reason })
    }
    Report $(if ($hdrOk) { 'ok' } else { 'FAIL' }) '2 WAV headers parse + consistent' ($hdrWhy -join '; ')
    Report $(if ($durOk) { 'ok' } else { 'FAIL' }) '3 declared duration' ($durWhy -join '; ')
    Report $(if ($tailOk) { 'ok' } else { 'FAIL' }) '4 PCM beyond header < 6 s' ($tailWhy -join '; ')

    $j = Run-Python @($Check, 'segments', $segments)
    $j | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $Out 'final-segments-check.json')
    Report $(if (Field $j 'ok') { 'ok' } else { 'FAIL' }) '5 segments.json' (Field $j 'reason')

    # Same acceptance as gate.sh: exit 0, or exit 2 "no checkpoint anchors"
    # for a recording shorter than 10 s.
    $dOut = Join-Path $Out 'drift-check.out'
    $dErr = Join-Path $Out 'drift-check.err'
    Push-Location $Root
    try {
        $ErrorActionPreference = 'Continue'
        & cargo run -q -p audio --bin drift-check -- $audio 1> $dOut 2> $dErr
        $drc = $LASTEXITCODE
    } finally { $ErrorActionPreference = 'Stop'; Pop-Location }
    $errText = if (Test-Path -LiteralPath $dErr) { Get-Content -LiteralPath $dErr -Raw } else { '' }
    if (-not $errText) { $errText = '' }
    $outText = if (Test-Path -LiteralPath $dOut) { Get-Content -LiteralPath $dOut -Raw } else { '' }
    if (-not $outText) { $outText = '' }
    if ($errText -match 'invariant violation') {
        Report 'FAIL' '6 drift-check' "exit $drc, invariant violation (see drift-check.err)"
    } elseif ($drc -eq 0) {
        $pass = ($outText -split "`n" | Where-Object { $_ -match 'PASS' } | Select-Object -First 1)
        Report 'ok' '6 drift-check' $(if ($pass) { $pass.Trim() } else { 'exit 0' })
    } elseif ($drc -eq 2 -and $Recorded -lt 10 -and $errText -match 'no checkpoint anchors') {
        Report 'ok' '6 drift-check' 'exit 2 accepted: recording shorter than 10 s has no checkpoint yet'
    } else {
        $last = ($errText.Trim() -split "`n" | Select-Object -Last 1)
        Report 'FAIL' '6 drift-check' "exit ${drc}: $last"
    }

    Report 'skip' '7 system.wav transcribes' 'meet-stt is macOS only'

    if (-not $script:FailedAny -and -not $Keep) {
        Remove-Item -LiteralPath $Meeting -Recurse -Force
        Note 'all checks passed; deleted the test recording (use -Keep to keep it)'
    } else {
        Note "test recording kept at $Meeting"
    }
} finally {
    if ($Player) { $Player.Stop() }
    if (App-Running) { Note 'cleanup: meet-ai still running, ending it'; End-App 10 | Out-Null }
    $env:MEET_AI_MEETINGS_ROOT = $oldRoot
}

Say-Line ''
Say-Line ("{0}/{1} passed" -f $script:Passed, $script:Total)
Say-Line "evidence: $Out"
if ($script:FailedAny) { exit 1 }
exit 0
