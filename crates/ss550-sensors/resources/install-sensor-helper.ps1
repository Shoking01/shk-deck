# Installs the sensor helper and registers it as a logon task with highest
# privileges, so it can open PawnIO's driver while Sh-Deck itself stays
# unprivileged.
#
# One command, run elevated, and it does the whole job:
#
#   Start-Process powershell -Verb RunAs -ArgumentList "-ExecutionPolicy Bypass -File <this file>"
#
# It stops the helper that is running (the file is locked while it serves), copies
# the binary beside the controller, registers the task again and starts it. The
# helper has no window: it is built for the Windows GUI subsystem, so the logon
# task produces no console, no tray icon and nothing to close.
param(
    [string]$Source = ""
)

$ErrorActionPreference = "Stop"
$log = Join-Path $env:TEMP "sensor-helper-install.log"
try {
    if (-not $Source) {
        # Beside the script, or a build of this repository (resources -> crate ->
        # crates -> repository root).
        $here = $PSScriptRoot
        $repo = Join-Path $here "..\..\.."
        $candidates = @(
            (Join-Path $here "ss550-sensor-helper.exe"),
            (Join-Path $repo "target\release\ss550-sensor-helper.exe"),
            (Join-Path $repo "target\debug\ss550-sensor-helper.exe")
        )
        $Source = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $Source -or -not (Test-Path $Source)) {
        "missing: no ss550-sensor-helper.exe to install (looked beside the script and in target\release, target\debug)" | Set-Content $log
        exit 1
    }

    # The running helper holds its own file open, so it goes first.
    Get-Process -Name "ss550-sensor-helper" -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 800

    $folder = Join-Path $env:LOCALAPPDATA "Programs\ss550"
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    $exe = Join-Path $folder "ss550-sensor-helper.exe"
    Copy-Item (Resolve-Path $Source) $exe -Force

    # No trigger: the task is started by Sh-Deck when it needs a temperature and
    # ends on its own a minute after the window is gone (or immediately, if the
    # window asks it to). A privileged process that starts at logon and stays is a
    # process nobody remembers to close.
    $action = New-ScheduledTaskAction -Execute $exe
    $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest

    Register-ScheduledTask -TaskName "Sh-Deck sensors" -Action $action -Principal $principal -Force | Out-Null

    # Prove the registration works once, then leave nothing running.
    Start-ScheduledTask -TaskName "Sh-Deck sensors"
    Start-Sleep -Seconds 2
    $up = Get-Process -Name "ss550-sensor-helper" -ErrorAction SilentlyContinue
    # Windows PowerShell cannot pipe an `if`, so the text is built first.
    $report = if ($up) {
        "installed $Source`nregistered and started: $exe`nthe helper answered: pid $($up.Id), no window"
    } else {
        "installed $Source`nregistered: $exe (did not start; run it from Sh-Deck)"
    }
    $report | Set-Content $log
} catch {
    "failed: $($_.Exception.Message)" | Set-Content $log
    exit 1
}
