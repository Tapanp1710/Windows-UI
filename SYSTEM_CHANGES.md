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

**None yet.** The taskbar has not been hidden, no `Shell_TrayWnd` state has been
changed, no work area has been altered, no startup entry has been created, and no
registry value has been written.

When PHASE 1 begins, every entry will be added here *before* the change is made, in this
form:

| # | Key / setting | Old value | New value | Undo |
|---|---|---|---|---|

Planned entries, for reference (not yet applied):

- Taskbar auto-hide / visibility — changed at runtime via `SHAppBarMessage` only, which
  is process-local and reverts the instant the app exits or is killed.
- Work area — `SystemParametersInfo(SPI_SETWORKAREA)`, restored on exit and by the
  watchdog.
- Startup entry — a `.lnk` in `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
  (per-user, no admin, no registry).

---

## 3. Files created outside the project folder

- `%LOCALAPPDATA%\macui-toolchain\dl\mingw.zip` — downloaded installer archive, safe to
  delete after the toolchain is extracted.
- `%LOCALAPPDATA%\macui-toolchain\dl\rustup-init.exe` — same.

Future runtime settings will live in `%APPDATA%\macui\settings.json`; deleting that
folder resets the app to defaults and affects nothing else.
