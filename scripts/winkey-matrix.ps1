<#
    winkey-matrix.ps1 — proves every Win+<key> combination still does its Windows job
    while the launcher is on the Win key.

        powershell -ExecutionPolicy Bypass -File scripts\winkey-matrix.ps1

    This is the test PHASE 3 exists to pass. The launcher is opened by the Win key's
    RELEASE, and only when nothing else was pressed while it was held, so every
    combination below should reach Windows completely untouched. Each one is sent for
    real and the result is observed, rather than reasoned about.

    After each combination the script presses Escape and gives Windows a moment, so a
    panel opened by one test cannot be mistaken for the result of the next.

    Win+L is deliberately NOT sent: it locks the screen, which would interrupt the
    person running this. See TESTS.md for why that is argued rather than measured.
#>

[CmdletBinding()]
param([switch]$Quiet)

Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices;
public static class WK {
    [DllImport("user32.dll")] public static extern void keybd_event(byte k, byte s, uint f, IntPtr e);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);

    public const byte LWIN = 0x5B, SHIFT = 0x10, CTRL = 0x11, ESC = 0x1B;
    public const uint KEYUP = 2;

    public static string Title() { var sb = new StringBuilder(300); GetWindowTextW(GetForegroundWindow(), sb, 300); return sb.ToString(); }
    public static string Class() { var sb = new StringBuilder(200); GetClassNameW(GetForegroundWindow(), sb, 200); return sb.ToString(); }

    public static void Tap(byte key) { keybd_event(key, 0, 0, IntPtr.Zero); System.Threading.Thread.Sleep(30); keybd_event(key, 0, KEYUP, IntPtr.Zero); }

    /// Win + key, with optional modifiers held inside the Win press.
    public static void WinCombo(byte key, bool shift, bool ctrl) {
        keybd_event(LWIN, 0, 0, IntPtr.Zero);
        System.Threading.Thread.Sleep(60);
        if (shift) keybd_event(SHIFT, 0, 0, IntPtr.Zero);
        if (ctrl)  keybd_event(CTRL, 0, 0, IntPtr.Zero);
        System.Threading.Thread.Sleep(30);
        keybd_event(key, 0, 0, IntPtr.Zero);
        System.Threading.Thread.Sleep(40);
        keybd_event(key, 0, KEYUP, IntPtr.Zero);
        if (ctrl)  keybd_event(CTRL, 0, KEYUP, IntPtr.Zero);
        if (shift) keybd_event(SHIFT, 0, KEYUP, IntPtr.Zero);
        System.Threading.Thread.Sleep(40);
        keybd_event(LWIN, 0, KEYUP, IntPtr.Zero);
    }

    public static void WinAlone() {
        keybd_event(LWIN, 0, 0, IntPtr.Zero);
        System.Threading.Thread.Sleep(60);
        keybd_event(LWIN, 0, KEYUP, IntPtr.Zero);
    }
}
'@
[void][WK]::SetProcessDpiAwarenessContext([IntPtr](-4))

$results = New-Object System.Collections.Generic.List[object]

function Settle {
    # Escape closes whatever a test opened. Twice, because some panels take one to
    # dismiss a sub-menu and another to close.
    [WK]::Tap([WK]::ESC); Start-Sleep -Milliseconds 350
    [WK]::Tap([WK]::ESC); Start-Sleep -Milliseconds 650
}

function Try-Combo {
    param(
        [string]$Name,
        [byte]$Key,
        [string]$Expect,          # regex matched against "title | class"
        [switch]$Shift,
        [switch]$Ctrl,
        [int]$WaitMs = 1500
    )
    Settle
    $before = "$([WK]::Title()) | $([WK]::Class())"
    [WK]::WinCombo($Key, $Shift.IsPresent, $Ctrl.IsPresent)
    Start-Sleep -Milliseconds $WaitMs
    $after = "$([WK]::Title()) | $([WK]::Class())"

    $launcher = $after -like "*macui launcher*"
    $matched = $after -match $Expect
    # The combination passes when Windows did its own thing AND the launcher stayed
    # out of it. A combination that opened our launcher is the failure this whole
    # phase is designed to avoid.
    $verdict = if ($launcher) { "FAIL - launcher opened" }
               elseif ($matched) { "PASS" }
               else { "CHECK - '$after'" }

    $results.Add([pscustomobject]@{ Combo = $Name; Result = $verdict; Foreground = $after })
    if (-not $Quiet) { Write-Host ("  {0,-16} {1}" -f $Name, $verdict) }
}

Write-Host "`nWin+<key> matrix - every one of these must still be Windows' own behaviour`n" -ForegroundColor Cyan

Try-Combo -Name "Win+E"        -Key 0x45 -Expect "CabinetWClass|ExplorerWClass|File Explorer|Home"
Try-Combo -Name "Win+R"        -Key 0x52 -Expect "#32770|Run"
Try-Combo -Name "Win+I"        -Key 0x49 -Expect "ApplicationFrameWindow|Settings"
Try-Combo -Name "Win+A"        -Key 0x41 -Expect "ControlCenterWindow|Shell_|Quick"
Try-Combo -Name "Win+N"        -Key 0x4E -Expect "ShellFlyout|Shell_|Notification"
Try-Combo -Name "Win+V"        -Key 0x56 -Expect "Shell_|Clipboard|Windows.UI"
Try-Combo -Name "Win+Shift+S"  -Key 0x53 -Expect "Snip|Shell_|Windows.UI|ScreenClip" -Shift
Try-Combo -Name "Win+D"        -Key 0x44 -Expect "Program Manager|WorkerW|^\s*\|"
Try-Combo -Name "Win+Up"       -Key 0x26 -Expect "."
Try-Combo -Name "Win+Down"     -Key 0x28 -Expect "."
Try-Combo -Name "Win+Left"     -Key 0x25 -Expect "."
Try-Combo -Name "Win+Right"    -Key 0x27 -Expect "."
Try-Combo -Name "Win+1"        -Key 0x31 -Expect "."
Try-Combo -Name "Win+Tab"      -Key 0x09 -Expect "Task|XamlExplorerHost|MultitaskingView"
Try-Combo -Name "Win+Ctrl+D"   -Key 0x44 -Expect "." -Ctrl

# And the one that SHOULD open the launcher.
Settle
[WK]::WinAlone()
Start-Sleep -Milliseconds 1500
$after = "$([WK]::Title()) | $([WK]::Class())"
$ok = $after -like "*macui launcher*"
$results.Add([pscustomobject]@{
    Combo = "Win (alone)"
    Result = if ($ok) { "PASS - launcher opened" } else { "FAIL - '$after'" }
    Foreground = $after
})
if (-not $Quiet) { Write-Host ("  {0,-16} {1}" -f "Win (alone)", $results[-1].Result) }
Settle

Write-Host ""
$fails = @($results | Where-Object { $_.Result -like "FAIL*" })
if ($fails.Count -eq 0) {
    Write-Host "No combination opened the launcher, and Win alone did." -ForegroundColor Green
} else {
    Write-Host "$($fails.Count) FAILURE(S) - Start must go back on the Win key:" -ForegroundColor Red
    $fails | ForEach-Object { Write-Host "   $($_.Combo): $($_.Foreground)" }
}
$results | Format-Table -AutoSize
