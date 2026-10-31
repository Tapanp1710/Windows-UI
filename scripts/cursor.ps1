<#
    cursor.ps1 - move the mouse pointer, for testing hover and edge-trigger behaviour.

        powershell -File scripts\cursor.ps1 -X 960 -Y 1079      # bottom edge, centre
        powershell -File scripts\cursor.ps1 -BottomEdge          # same, computed
        powershell -File scripts\cursor.ps1 -X 960 -Y 400        # away from the dock

    Coordinates are PHYSICAL pixels (1920x1080 on this machine), matching what the
    Rust side sees from GetCursorPos.
#>
[CmdletBinding()]
param(
    [int]$X = -1,
    [int]$Y = -1,
    [switch]$BottomEdge,
    [switch]$Report,
    # Synthesise a real mouse click at the destination, for behaviour tests.
    [ValidateSet('none','left','middle','right')]
    [string]$Click = 'none',
    # Wheel notches: positive scrolls up, negative down.
    [int]$Scroll = 0
)

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Cur {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, IntPtr e);
    public const uint LEFTDOWN=0x0002, LEFTUP=0x0004, RIGHTDOWN=0x0008, RIGHTUP=0x0010, MIDDLEDOWN=0x0020, MIDDLEUP=0x0040, WHEEL=0x0800;
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static void Dpi() { try { SetProcessDpiAwarenessContext(new IntPtr(-4)); } catch {} }
}
'@
[Cur]::Dpi()

$sw = [Cur]::GetSystemMetrics(0)
$sh = [Cur]::GetSystemMetrics(1)

if ($BottomEdge) { $X = [int]($sw / 2); $Y = $sh - 1 }

if ($Report -or $X -lt 0) {
    $p = New-Object Cur+POINT
    [void][Cur]::GetCursorPos([ref]$p)
    Write-Output "screen ${sw}x${sh}  cursor $($p.X),$($p.Y)"
    if ($X -lt 0) { exit 0 }
}

[void][Cur]::SetCursorPos($X, $Y)
$p = New-Object Cur+POINT
[void][Cur]::GetCursorPos([ref]$p)
Write-Output "cursor -> $($p.X),$($p.Y)  (screen ${sw}x${sh})"

if ($Click -ne 'none') {
    Start-Sleep -Milliseconds 120
    switch ($Click) {
        'left'   { [Cur]::mouse_event([Cur]::LEFTDOWN,0,0,0,[IntPtr]::Zero);   Start-Sleep -Milliseconds 40; [Cur]::mouse_event([Cur]::LEFTUP,0,0,0,[IntPtr]::Zero) }
        'middle' { [Cur]::mouse_event([Cur]::MIDDLEDOWN,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 40; [Cur]::mouse_event([Cur]::MIDDLEUP,0,0,0,[IntPtr]::Zero) }
        'right'  { [Cur]::mouse_event([Cur]::RIGHTDOWN,0,0,0,[IntPtr]::Zero);  Start-Sleep -Milliseconds 40; [Cur]::mouse_event([Cur]::RIGHTUP,0,0,0,[IntPtr]::Zero) }
    }
    Write-Output "clicked: $Click"
}

if ($Scroll -ne 0) {
    Start-Sleep -Milliseconds 150
    for ($i = 0; $i -lt [Math]::Abs($Scroll); $i++) {
        # WHEEL_DELTA is signed but the parameter is a DWORD, so a negative notch
        # must be passed as its unchecked two's-complement value. Note PowerShell 5.1
        # parses 0xFFFFFFFF as Int32 -1, so masking with it is a silent no-op - add
        # 2^32 instead.
        $n = [Math]::Sign($Scroll) * 120
        $delta = if ($n -lt 0) { [uint32](4294967296 + $n) } else { [uint32]$n }
        [Cur]::mouse_event([Cur]::WHEEL, 0, 0, $delta, [IntPtr]::Zero)
        Start-Sleep -Milliseconds 80
    }
    Write-Output "scrolled: $Scroll notches"
}
