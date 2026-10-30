<#
    screenshot.ps1 — full-screen capture for the visual and animation checks.

    Single frame:
        powershell -ExecutionPolicy Bypass -File scripts\screenshot.ps1 -Label dock-idle

    Animation burst (10 frames, ~40 ms apart, to catch flicker/jumps/blank frames):
        powershell -ExecutionPolicy Bypass -File scripts\screenshot.ps1 -Label dock-slide -Count 10 -IntervalMs 40

    Files land in .\screenshots as  <timestamp>_<label>[_NN].png
    Scaled captures (-Scale) keep file size down so frames are quick to review.
#>

[CmdletBinding()]
param(
    [string]$Label = 'screen',
    [int]$Count = 1,
    [int]$IntervalMs = 40,
    # Delay before the FIRST frame, so you can trigger an animation and catch it.
    [int]$DelayMs = 0,
    # 1.0 = native 1920x1080. 0.5 halves each dimension.
    [double]$Scale = 1.0,
    [string]$OutDir,
    # Animation triggers, fired from inside the capture loop so the timing is
    # deterministic instead of racing a separate process's startup.
    # Moves the pointer to the bottom edge (summons the dock) after frame N.
    [int]$TriggerBottomEdgeAfterFrame = 0,
    # Moves the pointer away from the edge after frame N (dismisses the dock).
    [int]$TriggerLeaveAfterFrame = 0
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

# This display runs at 150% scaling. A DPI-unaware process is handed a virtualised
# 1280x720 desktop and its capture comes back blurry and 1.5x too small, which hides
# exactly the 1px borders and 12px margins the visual check exists to verify.
# Declaring per-monitor-v2 awareness first makes CopyFromScreen return true 1920x1080.
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class DpiAware {
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("shcore.dll")] static extern int SetProcessDpiAwareness(int v);
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    public static string Enable() {
        try { if (SetProcessDpiAwarenessContext(new IntPtr(-4))) return "per-monitor-v2"; } catch {}
        try { if (SetProcessDpiAwareness(2) == 0) return "per-monitor"; } catch {}
        try { if (SetProcessDPIAware()) return "system"; } catch {}
        return "none";
    }
}
'@ -ErrorAction SilentlyContinue
$dpiMode = [DpiAware]::Enable()

if (-not $OutDir) { $OutDir = Join-Path $PSScriptRoot '..\screenshots' }
$OutDir = [IO.Path]::GetFullPath($OutDir)
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

# Virtual screen covers all monitors; for one 1920x1080 panel it is just that.
$bounds = [Windows.Forms.SystemInformation]::VirtualScreen
$stamp  = Get-Date -Format 'yyyyMMdd-HHmmss'

if ($DelayMs -gt 0) { Start-Sleep -Milliseconds $DelayMs }

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Pointer {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
}
'@ -ErrorAction SilentlyContinue

$written = New-Object System.Collections.Generic.List[string]
$sw = [Diagnostics.Stopwatch]::StartNew()

# Capture into memory first and encode afterwards. PNG encoding a 1920x1080 frame
# costs well over 100 ms, which is longer than the 150-250 ms animations we are
# trying to inspect — encoding inside the loop made every burst look like a still.
$frames = New-Object System.Collections.Generic.List[object]
$stamps = New-Object System.Collections.Generic.List[int]

for ($i = 1; $i -le $Count; $i++) {

    $bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bmp.Size)
    $gfx.Dispose()

    $frames.Add($bmp)
    $stamps.Add([int]$sw.Elapsed.TotalMilliseconds)

    if ($TriggerBottomEdgeAfterFrame -gt 0 -and $i -eq $TriggerBottomEdgeAfterFrame) {
        [void][Pointer]::SetCursorPos([int]($bounds.Width / 2), $bounds.Height - 1)
    }
    if ($TriggerLeaveAfterFrame -gt 0 -and $i -eq $TriggerLeaveAfterFrame) {
        [void][Pointer]::SetCursorPos([int]($bounds.Width / 2), [int]($bounds.Height / 3))
    }

    if ($i -lt $Count -and $IntervalMs -gt 0) { Start-Sleep -Milliseconds $IntervalMs }
}

for ($i = 0; $i -lt $frames.Count; $i++) {
    $bmp = $frames[$i]
    $out = $bmp
    if ($Scale -ne 1.0) {
        $w = [int]($bounds.Width * $Scale); $h = [int]($bounds.Height * $Scale)
        $small = New-Object System.Drawing.Bitmap($w, $h)
        $g2 = [System.Drawing.Graphics]::FromImage($small)
        $g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g2.DrawImage($bmp, 0, 0, $w, $h)
        $g2.Dispose()
        $out = $small
    }

    $name = if ($Count -eq 1) { "${stamp}_${Label}.png" }
            else { "${stamp}_${Label}_{0:D2}.png" -f ($i + 1) }
    $path = Join-Path $OutDir $name
    $out.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    if ($out -ne $bmp) { $out.Dispose() }
    $bmp.Dispose()

    $written.Add($path)
    Write-Output ("{0}  (captured at +{1} ms)" -f $path, $stamps[$i])
}

Write-Output ""
Write-Output ("{0} frame(s) at {1}x{2} (dpi awareness: {3}) -> {4}" -f `
    $written.Count, $bounds.Width, $bounds.Height, $dpiMode, $OutDir)
