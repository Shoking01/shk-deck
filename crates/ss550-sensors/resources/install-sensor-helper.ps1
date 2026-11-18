# Registers the sensor helper as a logon task with highest privileges, so it can
# open PawnIO's driver while Sh-Deck itself stays unprivileged.
$log = Join-Path $env:TEMP "sensor-helper-install.log"
try {
    Get-Process -Name "ss550-sensor-helper" -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 500

    $exe = Join-Path $env:LOCALAPPDATA "Programs\ss550\ss550-sensor-helper.exe"
    if (-not (Test-Path $exe)) {
        "missing: $exe" | Set-Content $log
        exit 1
    }

    $action = New-ScheduledTaskAction -Execute $exe
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" -LogonType Interactive -RunLevel Highest

    Register-ScheduledTask -TaskName "Sh-Deck sensors" -Action $action -Trigger $trigger -Principal $principal -Force | Out-Null
    Start-ScheduledTask -TaskName "Sh-Deck sensors"
    "registered and started: $exe" | Set-Content $log
} catch {
    "failed: $($_.Exception.Message)" | Set-Content $log
}
