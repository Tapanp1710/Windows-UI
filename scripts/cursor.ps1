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
    [switch]$Report
)

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Cur {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
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
