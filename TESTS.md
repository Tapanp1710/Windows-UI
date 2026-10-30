# TESTS

Pass/fail log. The core rule being tested throughout: **every Windows behaviour must
work exactly as it does on a stock machine.** A visual change that alters behaviour is a
failure, not a feature.

Legend: PASS · FAIL · N/A (phase not reached) · BLOCKED (cannot test here)

---

## STEP 0 — environment and feasibility

| # | Test | Result | Notes |
|---|---|---|---|
| 0.1 | Node present | PASS | v24.15.0 |
| 0.2 | npm present | PASS | 11.12.1 |
| 0.3 | Git present | PASS | 2.54.0.windows.1 |
| 0.4 | WebView2 runtime present | PASS | 156.0.4314.50 — required by Tauri |
| 0.5 | Rust + Cargo present | PASS | 1.99.1, installed per-user during this step |
| 0.6 | MSVC build tools present | **FAIL** | absent; installing needs UAC, which the safety rules forbid |
| 0.7 | Windows SDK present | **FAIL** | same |
| 0.8 | A no-admin linker exists | PASS | portable WinLibs MinGW-w64 GCC 16.2.0 UCRT |
| 0.9 | Rust can compile + link a release exe | PASS | `linktest.exe` built and ran, printed `link ok` |
| 0.10 | Release exe runs under App Control for Business | PASS | see note below |
| 0.11 | Tauri v2 builds on `x86_64-pc-windows-gnu` | **PASS** | 294 crates inc. wry + webview2-com; 4.2 MB release exe |
| 0.16 | Release Tauri exe launches and renders | PASS | WebView2 window drew; 21-24 MB RAM for one window |
| 0.17 | Webview CSS px == Windows logical px at 150% DPI | PASS | `devicePixelRatio`=1.5, `innerWidth`=420 for a 420 config |
| 0.18 | screenshot.ps1 captures true 1920x1080 | PASS | was silently capturing 1280x720 until DPI awareness was added |
| 0.12 | screenshot.ps1 captures and is readable | PASS | `20261031-023908_step0-baseline.png` |
| 0.13 | capture-baseline.ps1 records shell state | PASS | taskbar was already auto-hide — caught before anything changed |
| 0.14 | restore-windows.ps1 runs without admin | PASS | 2 actions, no errors |
| 0.15 | restore-windows.ps1 is idempotent | PASS | taskbar state 1 before and after; did not flip the user's setting |

**On 0.10 (App Control for Business):** a freshly compiled, unsigned release binary
(`linktest.exe`) executed and produced output. The policy on this machine is therefore
not blocking locally built unsigned user-mode executables, so the project is viable on
that front. This will be re-confirmed for each real release build, since a larger binary
with more imports is a different test.

---

## PHASE 1 — Dock

Not reached.

Shortcuts and behaviours to verify when it is: click to launch, click to focus,
click-again to minimise, grouped-window previews, right-click jump list, middle-click new
instance, attention flash, drag-reorder persistence, maximised-window geometry with
auto-hide, and taskbar return on kill.

## PHASE 2 — Top bar
Not reached. Will verify Win+A, Win+N, scroll-to-volume, tray overflow.

## PHASE 3 — Launcher
Not reached. Will verify Win, Esc, and that Win+E/R/D/L/I/A/N/V/Shift+S/arrows/numbers
all still work untouched.

## PHASE 4 — Window switcher
Not reached. Will verify Alt+Tab, Alt+Shift+Tab, Esc, Win+Tab, virtual desktops.

## PHASE 5 — Widgets
Not reached. Will verify Win+D behaviour and that widgets never cover apps.

## PHASE 6 — Settings, startup, packaging
Not reached.
