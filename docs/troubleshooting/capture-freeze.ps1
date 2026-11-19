# Captures the whole picture WHILE the mouse is frozen. Run it at the moment of
# the freeze, with nothing else running: it answers "is input blocked?", "which
# thread of ss550-gui is stuck?", and "is something else eating the machine?".
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File capture-freeze.ps1
#
# It writes nothing to the deck and changes nothing; it only reads.

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class FreezeProbe {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetCursorPos(out POINT p);
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)]
    public struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public MOUSEINPUT mi; }
    [DllImport("user32.dll", SetLastError = true)]
    public static extern uint SendInput(uint n, INPUT[] p, int size);
    public struct POINT { public int X; public int Y; }
}
"@ -ErrorAction Stop

$stamp = Get-Date
"=== capture at $($stamp.ToString('yyyy-MM-dd HH:mm:ss.fff')) ==="

# 1. Two different questions, and they must not be confused:
#    - SetCursorPos moves the cursor directly, bypassing the input queue. It
#      answers "will the system let the cursor be placed?".
#    - SendInput(MOVE) goes through the input queue, exactly like the physical
#      mouse. It answers "are mouse events being delivered?".
#    A frozen pointer with the first working and the second swallowed is an
#    interception: a low-level hook whose thread is stuck, or a raw-input grab.
$here = New-Object FreezeProbe+POINT
$null = [FreezeProbe]::GetCursorPos([ref]$here)
$placed = [FreezeProbe]::SetCursorPos($here.X + 60, $here.Y)
Start-Sleep -Milliseconds 80
$moved = New-Object FreezeProbe+POINT
$null = [FreezeProbe]::GetCursorPos([ref]$moved)
$null = [FreezeProbe]::SetCursorPos($here.X, $here.Y)

$injected = New-Object 'FreezeProbe+INPUT[]' 1
$injected[0].type = 0
$injected[0].mi.dx = 80
$injected[0].mi.dy = 0
$injected[0].mi.dwFlags = 0x0001 # MOUSEEVENTF_MOVE
$start = New-Object FreezeProbe+POINT
$null = [FreezeProbe]::GetCursorPos([ref]$start)
$sent = [FreezeProbe]::SendInput(1, $injected, [System.Runtime.InteropServices.Marshal]::SizeOf([type]('FreezeProbe+INPUT')))
Start-Sleep -Milliseconds 150
$end = New-Object FreezeProbe+POINT
$null = [FreezeProbe]::GetCursorPos([ref]$end)

"Placed by SetCursorPos     : $placed  (cursor $($here.X),$($here.Y) -> $($moved.X),$($moved.Y))"
"Delivered via SendInput    : $sent event(s)  (cursor $($start.X),$($start.Y) -> $($end.X),$($end.Y))"
if (-not $placed) {
    "VERDICT: input is blocked process-wide (BlockInput). Something called it."
}
elseif ($end.X -ne $start.X -and $moved.X -ne $here.X) {
    "VERDICT: Windows is delivering mouse events. If the pointer still feels stuck, the problem is above the input stack."
}
elseif ($end.X -eq $start.X) {
    "VERDICT: mouse events are being SWALLOWED. The queue accepted the event and nothing moved - a low-level hook (stuck or filtering) or a raw-input grab is holding the pointer. It is not this app if it has no window and no CPU use; check the processes below."
}
else {
    "VERDICT: inconclusive - the two probes disagreed in an unexpected way."
}

# 2. The controller, per thread: which one is spinning or blocked.
"--- ss550-gui ---"
$apps = Get-Process ss550-gui -ErrorAction SilentlyContinue
if (-not $apps) { "not running" }
foreach ($app in $apps) {
    "pid=$($app.Id) responding=$($app.Responding) hwnd=$($app.MainWindowHandle) ws=$([math]::Round([double]$app.WorkingSet64/1MB))MB"
    $cpu = if ($null -eq $app.CPU) { 'n/a' } else { [math]::Round([double]$app.CPU, 2) }
    "  total cpu=$($cpu)s"
    $app.Threads | Sort-Object TotalProcessorTime -Descending | Select-Object -First 6 | ForEach-Object {
        $t = [math]::Round([double]$_.TotalProcessorTime.TotalSeconds, 2)
        "  tid=$($_.Id) state=$($_.ThreadState) wait=$($_.WaitReason) cpu=$($t)s"
    }
}

# 3. Everything else that could be eating the machine instead: a process using
#    most of a core while this app sits idle is the one to suspect.
"--- cpu used in the last three seconds ---"
$before = @{}
Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
    if ($null -ne $_.CPU) { $before[$_.Id] = [double]$_.CPU }
}
Start-Sleep -Seconds 3
Get-Process -ErrorAction SilentlyContinue | ForEach-Object {
    $id = $_.Id
    if ($null -ne $_.CPU -and $before.ContainsKey($id)) {
        [pscustomobject]@{
            name = $_.ProcessName
            pid  = $id
            used = [math]::Round(([double]$_.CPU - $before[$id]), 2)
        }
    }
} | Sort-Object used -Descending | Select-Object -First 10 | ForEach-Object {
    "  $($_.name.PadRight(16)) pid=$($_.pid) used=$($_.used)s of 3s"
}

# 4. What the controller itself last said, and when.
$log = Join-Path $env:APPDATA "ss550\ss550.log"
"--- last lines of $log ---"
if (Test-Path $log) {
    (Get-Item $log) | ForEach-Object { "  size=$([math]::Round($_.Length/1KB))KB modified=$($_.LastWriteTime.ToString('HH:mm:ss'))" }
    Get-Content $log -Tail 25 | ForEach-Object { "  $_" }
} else {
    "  no log at that path"
}
