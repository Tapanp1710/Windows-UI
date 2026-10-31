# SYSTEM CHANGES

Every change made outside the project folder, with exact undo steps.
`restore-windows.ps1` automates all of the reversible ones.

**Nothing here required administrator rights. Nothing here has been deleted or
overwritten outside the project folder.**

---

## 1. Toolchain installs (STEP 0) — additive, no admin

| # | What | Where | Undo |
|---|---|---|---|
| T-1 | Rust toolchain `stable-x86_64-pc-windows-gnu` 1.99.1, installed per-user by `rustup-init.exe` | `%USERPROFILE%\.rustup`, `%USERPROFILE%\.cargo` | `rustup self uninstall -y`, or delete both folders |
| T-2 | Portable WinLibs MinGW-w64 GCC 16.2.0 (UCRT), extracted from a zip — no installer, no registry | `%LOCALAPPDATA%\macui-toolchain` | `Remove-Item -Recurse -Force "$env:LOCALAPPDATA\macui-toolchain"` |

`rustup-init` was run with `--no-modify-path`, so **the user's PATH environment variable
was not touched**. Build commands put the toolchain on PATH for that one process only.

No registry keys were written. No system-wide software was installed. No MSIs ran.

---

## 2. Windows UI state

All of these are **runtime only**: they exist while `macui.exe` is running and are undone
when it stops, when it is killed (by the watchdog), or by `restore-windows.ps1`.
**No registry value is written for any of them.**

| # | What | Changed how | Restored how |
|---|---|---|---|
| U-1 | Windows taskbar hidden | `ShowWindow(Shell_TrayWnd, SW_HIDE)` — and `Shell_SecondaryTrayWnd` on multi-monitor | `SW_SHOW` on clean exit, by the watchdog on a hard kill, or by `restore-windows.ps1` |
| U-2 | Taskbar auto-hide state | `SHAppBarMessage(ABM_SETSTATE)` | Restored to the **recorded original** from `baseline-windows.json`. On this machine that is auto-hide ON, which is what you already had — it is not forced to "always visible" |
| U-3 | Desktop work area | `SystemParametersInfo(SPI_SETWORKAREA)` **without** `SPIF_SENDCHANGE`, reserving 63px at the top for the top bar | Reset to the full screen *with* the broadcast, so every app re-lays-out |

Nothing else is touched. In particular: no startup entry has been created (PHASE 6 was
not reached), no wallpaper was changed, no theme setting was changed, and no shortcut
or key binding was added, removed or remapped anywhere.

### A note on the two shortcuts the UI sends
The Start button sends **Ctrl+Esc**, and the top bar sends **Win+A** / **Win+N**, through
`SendInput`. These *send* a keystroke the way any automation tool does; they do **not**
install a hook, intercept a key, or remap anything. Every Windows shortcut continues to
work exactly as before.

### Known gap
Killing the **entire process tree** at once (e.g. "End process tree" in Task Manager)
kills the watchdog along with the app, so U-1..U-3 stay in effect until you run
`restore-windows.ps1`. Every other exit path — clean quit, crash, single-process kill,
power loss on next boot — is covered.

---

## 3. Files created outside the project folder

- `%LOCALAPPDATA%\macui-toolchain\dl\mingw.zip` — downloaded installer archive, safe to
  delete after the toolchain is extracted.
- `%LOCALAPPDATA%\macui-toolchain\dl\rustup-init.exe` — same.

Future runtime settings will live in `%APPDATA%\macui\settings.json`; deleting that
folder resets the app to defaults and affects nothing else.
