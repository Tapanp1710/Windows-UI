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
    [string]$OutDir
)

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if (-not $OutDir) { $OutDir = Join-Path $PSScriptRoot '..\screenshots' }
$OutDir = [IO.Path]::GetFullPath($OutDir)
if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

# Virtual screen covers all monitors; for one 1920x1080 panel it is just that.
$bounds = [Windows.Forms.SystemInformation]::VirtualScreen
$stamp  = Get-Date -Format 'yyyyMMdd-HHmmss'

if ($DelayMs -gt 0) { Start-Sleep -Milliseconds $DelayMs }

$written = New-Object System.Collections.Generic.List[string]
$sw = [Diagnostics.Stopwatch]::StartNew()

for ($i = 1; $i -le $Count; $i++) {

    $bmp = New-Object System.Drawing.Bitmap($bounds.Width, $bounds.Height)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CopyFromScreen($bounds.X, $bounds.Y, 0, 0, $bmp.Size)
    $gfx.Dispose()

    $out = $bmp
    if ($Scale -ne 1.0) {
        $w = [int]($bounds.Width * $Scale); $h = [int]($bounds.Height * $Scale)
        $small = New-Object System.Drawing.Bitmap($w, $h)
        $g2 = [System.Drawing.Graphics]::FromImage($small)
        $g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g2.DrawImage($bmp, 0, 0, $w, $h)
        $g2.Dispose(); $bmp.Dispose()
        $out = $small
    }

    $name = if ($Count -eq 1) { "${stamp}_${Label}.png" }
            else { "${stamp}_${Label}_{0:D2}.png" -f $i }
    $path = Join-Path $OutDir $name
    $out.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $out.Dispose()

    $written.Add($path)
    Write-Output ("{0}  (+{1} ms)" -f $path, [int]$sw.Elapsed.TotalMilliseconds)

    if ($i -lt $Count -and $IntervalMs -gt 0) { Start-Sleep -Milliseconds $IntervalMs }
}

Write-Output ""
Write-Output ("{0} frame(s) -> {1}" -f $written.Count, $OutDir)
