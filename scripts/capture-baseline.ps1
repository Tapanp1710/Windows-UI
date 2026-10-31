<#
    capture-baseline.ps1

    Records the Windows shell settings this project is allowed to touch, BEFORE it
    touches them, into baseline-windows.json. restore-windows.ps1 reads that file so it
    puts back what was actually there rather than guessing at a default.

    This matters on this machine: the taskbar is ALREADY set to auto-hide. A restore
    that blindly forced "always visible" would leave the user worse off than it found
    them.

    Run once, before the UI layer ever starts. Re-running will NOT overwrite an existing
    baseline unless you pass -Force.
#>

[CmdletBinding()]
param(
    [string]$Path,
    [switch]$Force
)

if (-not $Path) { $Path = Join-Path $PSScriptRoot '..\baseline-windows.json' }
$Path = [IO.Path]::GetFullPath($Path)

if ((Test-Path $Path) -and -not $Force) {
    Write-Host "baseline already exists, keeping it: $Path" -ForegroundColor Yellow
    Write-Host "(pass -Force to overwrite - only do that when Windows is in its normal state)"
    exit 0
}

$sig = @'
using System;
using System.Runtime.InteropServices;
public class MacUiBaseline {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int left, top, right, bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA {
        public uint cbSize; public IntPtr hWnd; public uint uCallbackMessage;
        public uint uEdge; public RECT rc; public int lParam;
    }
    [DllImport("shell32.dll")] public static extern IntPtr SHAppBarMessage(uint m, ref APPBARDATA d);
    [DllImport("user32.dll")]  public static extern IntPtr FindWindow(string c, string w);
    [DllImport("user32.dll")]  public static extern bool SystemParametersInfo(uint a, uint p, ref RECT v, uint f);
    public const uint ABM_GETSTATE = 0x00000004;
    public const uint SPI_GETWORKAREA = 0x0030;

    public static int TaskbarState() {
        APPBARDATA d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        return (int)SHAppBarMessage(ABM_GETSTATE, ref d);
    }
    public static RECT WorkArea() {
        RECT r = new RECT();
        SystemParametersInfo(SPI_GETWORKAREA, 0, ref r, 0);
        return r;
    }
}
'@
Add-Type -TypeDefinition $sig

$state = [MacUiBaseline]::TaskbarState()      # bit 0 = ABS_AUTOHIDE, bit 1 = ABS_ALWAYSONTOP
$wa    = [MacUiBaseline]::WorkArea()

Add-Type -AssemblyName System.Windows.Forms
$scr = [Windows.Forms.Screen]::PrimaryScreen

$themeKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$theme = Get-ItemProperty $themeKey -ErrorAction SilentlyContinue

$baseline = [ordered]@{
    # Which machine this describes. restore-windows.ps1 refuses a baseline captured
    # somewhere else, because "put the taskbar back as it was" means as it was HERE -
    # and this file is per-machine state that must never travel with the repo.
    machine           = $env:COMPUTERNAME
    capturedAt        = (Get-Date).ToString('o')
    taskbarStateRaw   = $state
    taskbarAutoHide   = [bool]($state -band 1)
    taskbarAlwaysOnTop= [bool]($state -band 2)
    workArea          = [ordered]@{ left=$wa.left; top=$wa.top; right=$wa.right; bottom=$wa.bottom }
    logicalScreen     = [ordered]@{ width=$scr.Bounds.Width; height=$scr.Bounds.Height }
    appsUseLightTheme = $(if ($theme) { [int]$theme.AppsUseLightTheme } else { $null })
    systemUsesLight   = $(if ($theme) { [int]$theme.SystemUsesLightTheme } else { $null })
    minAnimate        = (Get-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics' -Name MinAnimate -ErrorAction SilentlyContinue).MinAnimate
    wallpaper         = (Get-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Wallpaper -ErrorAction SilentlyContinue).Wallpaper
}

$baseline | ConvertTo-Json -Depth 5 | Out-File -FilePath $Path -Encoding utf8
Write-Host "baseline written: $Path" -ForegroundColor Green
Get-Content $Path
