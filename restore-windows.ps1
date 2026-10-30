<#
    restore-windows.ps1

    Undoes every system change this project makes and brings back normal Windows:
    taskbar, Start, Alt+Tab, work area, startup entry.

    Safe to run at any time, as many times as you like. Needs NO administrator rights.
    Run it with:   powershell -ExecutionPolicy Bypass -File .\restore-windows.ps1

    Kept in step with each phase. As of STEP 0 the UI layer does not yet touch any
    Windows state, so the only things to restore are the app process and the taskbar
    (defensively), plus the optional -RemoveToolchain cleanup.
#>

[CmdletBinding()]
param(
    # Also delete the Rust + MinGW toolchains installed during STEP 0.
    [switch]$RemoveToolchain,
    # Don't print the summary.
    [switch]$Quiet,
    # Where capture-baseline.ps1 recorded the pre-existing Windows state.
    [string]$Baseline
)

$ErrorActionPreference = 'Continue'
$actions = New-Object System.Collections.Generic.List[string]

function Note($msg) { $actions.Add($msg); if (-not $Quiet) { Write-Host "  $msg" } }

if (-not $Quiet) { Write-Host "`nRestoring normal Windows..." -ForegroundColor Cyan }

# ---------------------------------------------------------------------------
# 1. Stop our app so it stops managing any Windows state.
# ---------------------------------------------------------------------------
$ourProcs = @('macui', 'macui-shell')
foreach ($name in $ourProcs) {
    $procs = Get-Process -Name $name -ErrorAction SilentlyContinue
    foreach ($p in $procs) {
        try { $p.CloseMainWindow() | Out-Null } catch {}
    }
}
Start-Sleep -Milliseconds 400
foreach ($name in $ourProcs) {
    $procs = Get-Process -Name $name -ErrorAction SilentlyContinue
    foreach ($p in $procs) {
        try { Stop-Process -Id $p.Id -Force -Confirm:$false -ErrorAction Stop; Note "stopped process $name (pid $($p.Id))" } catch {}
    }
}

# ---------------------------------------------------------------------------
# 2. Un-hide the Windows taskbar and clear any auto-hide state we may have set.
#
#    The app only ever sets taskbar auto-hide through SHAppBarMessage, which is
#    process-local and reverts when the process dies. This block is a belt-and-braces
#    reset in case the app was killed at a bad moment: it clears the auto-hide bit,
#    shows Shell_TrayWnd, and restores the work area to the full monitor.
# ---------------------------------------------------------------------------
$sig = @'
using System;
using System.Runtime.InteropServices;
public class MacUiRestore {
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int left, top, right, bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct APPBARDATA {
        public uint cbSize;
        public IntPtr hWnd;
        public uint uCallbackMessage;
        public uint uEdge;
        public RECT rc;
        public int lParam;
    }

    [DllImport("shell32.dll")]
    public static extern IntPtr SHAppBarMessage(uint dwMessage, ref APPBARDATA pData);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool SystemParametersInfo(uint uiAction, uint uiParam, ref RECT pvParam, uint fWinIni);

    [DllImport("user32.dll")]
    public static extern int GetSystemMetrics(int nIndex);

    public const uint ABM_SETSTATE = 0x0000000A;
    public const uint ABM_GETSTATE = 0x00000004;
    public const int  ABS_AUTOHIDE    = 0x0000001;
    public const int  ABS_ALWAYSONTOP = 0x0000002;   // visible, on top, NOT auto-hide
    public const int  SW_SHOW = 5;
    public const uint SPI_SETWORKAREA = 0x002F;
    public const uint SPIF_SENDCHANGE = 0x02;
    public const int  SM_CXSCREEN = 0;
    public const int  SM_CYSCREEN = 1;

    public static int GetTaskbarState() {
        APPBARDATA d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        return (int)SHAppBarMessage(ABM_GETSTATE, ref d);
    }

    // Put the taskbar back to the EXACT state recorded in baseline-windows.json.
    // Forcing "always visible" would be wrong on a machine where the user had
    // auto-hide switched on before any of this ran.
    public static void SetTaskbarState(int state) {
        IntPtr tray = FindWindow("Shell_TrayWnd", null);
        APPBARDATA d = new APPBARDATA();
        d.cbSize = (uint)Marshal.SizeOf(typeof(APPBARDATA));
        d.hWnd = tray;
        d.lParam = state;
        SHAppBarMessage(ABM_SETSTATE, ref d);

        // The tray window itself must exist and be shown either way; auto-hide is
        // the shell sliding it off-screen, not the window being hidden.
        if (tray != IntPtr.Zero) ShowWindow(tray, SW_SHOW);
        IntPtr sec = FindWindow("Shell_SecondaryTrayWnd", null);
        if (sec != IntPtr.Zero) ShowWindow(sec, SW_SHOW);
    }

    // Hand the whole primary monitor back to Windows; the shell immediately
    // re-subtracts the real taskbar, so this ends at the correct default.
    public static void ResetWorkArea() {
        RECT r = new RECT();
        r.left = 0; r.top = 0;
        r.right = GetSystemMetrics(SM_CXSCREEN);
        r.bottom = GetSystemMetrics(SM_CYSCREEN);
        SystemParametersInfo(SPI_SETWORKAREA, 0, ref r, SPIF_SENDCHANGE);
    }
}
'@

try {
    Add-Type -TypeDefinition $sig -ErrorAction Stop

    # Prefer the recorded baseline; fall back to "visible, on top" only if there
    # isn't one, since that is the Windows factory default.
    if (-not $Baseline) { $Baseline = Join-Path $PSScriptRoot 'baseline-windows.json' }
    $wantState = [MacUiRestore]::ABS_ALWAYSONTOP
    $src = 'Windows default (no baseline file found)'
    if (Test-Path $Baseline) {
        try {
            $b = Get-Content $Baseline -Raw | ConvertFrom-Json
            if ($null -ne $b.taskbarStateRaw) {
                $wantState = [int]$b.taskbarStateRaw
                $src = "baseline-windows.json (captured $($b.capturedAt))"
            }
        } catch { Write-Warning "baseline file unreadable, using Windows default: $($_.Exception.Message)" }
    }

    $before = [MacUiRestore]::GetTaskbarState()
    [MacUiRestore]::SetTaskbarState($wantState)
    $autohide = if ($wantState -band [MacUiRestore]::ABS_AUTOHIDE) { 'auto-hide ON' } else { 'always visible' }
    Note "taskbar restored to '$autohide' from $src (was state=$before, now state=$wantState)"

    [MacUiRestore]::ResetWorkArea()
    Note "work area reset to full screen (shell re-reserves the taskbar strip)"
} catch {
    Write-Warning "could not call the Windows APIs directly: $($_.Exception.Message)"
    Write-Warning "fall back to: Settings > Personalisation > Taskbar > Taskbar behaviours > 'Automatically hide the taskbar'"
}

# ---------------------------------------------------------------------------
# 3. Remove the per-user startup entry, if PHASE 6 created one.
# ---------------------------------------------------------------------------
$startupLnk = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\macui.lnk'
if (Test-Path $startupLnk) {
    Remove-Item $startupLnk -Force -Confirm:$false
    Note "removed startup shortcut macui.lnk"
}

# Defensive: the Run key, in case a future phase uses it instead of a .lnk.
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if (Test-Path $runKey) {
    $existing = Get-ItemProperty -Path $runKey -ErrorAction SilentlyContinue
    if ($existing -and $existing.PSObject.Properties.Name -contains 'macui') {
        Remove-ItemProperty -Path $runKey -Name 'macui' -Force -Confirm:$false
        Note "removed HKCU Run value 'macui'"
    }
}

# ---------------------------------------------------------------------------
# 4. Restart Explorer only if the taskbar window is genuinely gone.
#    Explorer restarts itself and this loses nothing, but it reshuffles open
#    File Explorer windows, so it is a last resort rather than routine.
# ---------------------------------------------------------------------------
try {
    $tray = [MacUiRestore]::FindWindow("Shell_TrayWnd", $null)
    if ($tray -eq [IntPtr]::Zero) {
        Note "Shell_TrayWnd missing - restarting explorer.exe to bring the taskbar back"
        Stop-Process -Name explorer -Force -Confirm:$false -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 800
        if (-not (Get-Process -Name explorer -ErrorAction SilentlyContinue)) { Start-Process explorer.exe }
    }
} catch {}

# ---------------------------------------------------------------------------
# 5. Optional: remove the STEP 0 toolchains (see SYSTEM_CHANGES.md T-1, T-2).
# ---------------------------------------------------------------------------
if ($RemoveToolchain) {
    $rustup = "$env:USERPROFILE\.cargo\bin\rustup.exe"
    if (Test-Path $rustup) {
        & $rustup self uninstall -y 2>$null | Out-Null
        Note "uninstalled the Rust toolchain"
    }
    foreach ($d in @("$env:USERPROFILE\.rustup", "$env:USERPROFILE\.cargo", "$env:LOCALAPPDATA\macui-toolchain")) {
        if (Test-Path $d) {
            try { Remove-Item -Recurse -Force -Confirm:$false $d -ErrorAction Stop; Note "deleted $d" }
            catch { Write-Warning "could not delete $d : $($_.Exception.Message)" }
        }
    }
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
if (-not $Quiet) {
    Write-Host ""
    if ($actions.Count -eq 0) {
        Write-Host "Nothing needed changing - Windows was already in its normal state." -ForegroundColor Green
    } else {
        Write-Host "Done. $($actions.Count) action(s) taken." -ForegroundColor Green
    }
    Write-Host "Your taskbar, Start menu and Alt+Tab are standard Windows again."
    if (-not $RemoveToolchain) {
        Write-Host "Toolchain left in place. Pass -RemoveToolchain to delete Rust and MinGW too." -ForegroundColor DarkGray
    }
    Write-Host ""
}
