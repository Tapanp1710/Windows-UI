# SYSTEM CHANGES

Every change made outside the project folder, with exact undo steps.
`restore-windows.ps1` automates all of the reversible ones.

**Nothing here required administrator rights. Nothing here has been deleted or
overwritten outside the project folder.**

**The app itself writes no registry keys.** The only registry entry anywhere in this
project is the uninstall record Windows expects, and that is written by the NSIS
installer *if you choose to run it* - see section 3.

---

## 1. Toolchain installs (STEP 0) — additive, no admin

| # | What | Where | Undo |
|---|---|---|---|
| T-1 | Rust toolchain `stable-x86_64-pc-windows-gnu` 1.99.1, installed per-user by `rustup-init.exe` | `%USERPROFILE%\.rustup`, `%USERPROFILE%\.cargo` | `rustup self uninstall -y`, or delete both folders |
| T-2 | Portable WinLibs MinGW-w64 GCC 16.2.0 (UCRT), extracted from a zip — no installer, no registry | `%LOCALAPPDATA%\macui-toolchain` | `Remove-Item -Recurse -Force "$env:LOCALAPPDATA\macui-toolchain"` |
| T-3 | NSIS 3.13 + `nsis_tauri_utils.dll`, downloaded automatically by the Tauri CLI the first time an installer is built (7.9 MB, no installer, no registry) | `%LOCALAPPDATA%\tauri` | `Remove-Item -Recurse -Force "$env:LOCALAPPDATA\tauri"` |

`rustup-init` was run with `--no-modify-path`, so **the user's PATH environment variable
was not touched**. Build commands put the toolchain on PATH for that one process only.

No registry keys were written. No system-wide software was installed. No MSIs ran.

### A profile path with a space in it (2026-11-01)

The same T-1 (Rust 1.99.1) and T-3 (NSIS 3.13) installs, plus:

| # | What | Where | Undo |
|---|---|---|---|
| T-2a | MinGW (T-2) moved out of `%LOCALAPPDATA%` when the profile path has a space (`C:\Users\First Last`): gcc's spec adds `default-manifest.o` through `%:if-exists`, which splits its own install path at spaces, so every link fails. A `subst` drive does not help (gcc resolves its real path). `scripts\env.ps1` falls back to this folder when the default one is absent | `C:\macui-toolchain` | `Remove-Item -Recurse -Force C:\macui-toolchain` (also done by `restore-windows.ps1 -RemoveToolchain`) |
| T-4 | Cargo build output. The project folder has a space too, and `windres` passes the build's `OUT_DIR` as an unquoted `-I` path, so the build was run with `$env:CARGO_TARGET_DIR = 'C:\macui-toolchain\target'`. The exe and installer are therefore at `C:\macui-toolchain\target\release\macui.exe` and `...\bundle\nsis\macui_0.1.0_x64-setup.exe`, not under `src-tauri\target` | `C:\macui-toolchain\target` | deleted with T-2a |

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

### Opt-in changes (PHASE 6) — nothing here happens unless you press a button

| # | What | Changed how | Restored how |
|---|---|---|---|
| U-4 | Startup entry: "Open macui when I sign in" | A shortcut written to `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\macui.lnk`. **No registry key.** | Turn the switch off, delete that one file, disable it in Task Manager > Startup, or run `restore-windows.ps1` |
| U-5 | Desktop wallpaper | `SystemParametersInfo(SPI_SETDESKWALLPAPER)`. This is the one change Windows *persists*, which is why it is opt-in. The previous path is recorded in `settings.json` as `originalWallpaper` **before** anything changes | "Put my own wallpaper back" in settings, "Return to normal Windows", or `restore-windows.ps1`. All three read the recorded path and then clear it |

The image itself is unpacked from inside the executable to
`%APPDATA%\macui\macui-warm-*.png`, because Windows needs a real file path to point at.
Deleting that folder after restoring your own wallpaper removes every trace.

Nothing else is touched. In particular: no theme setting is changed, and **no shortcut
or key binding is added, removed or remapped anywhere** — this app installs no keyboard
hook of any kind.

Neither U-4 nor U-5 is on by default, and neither has been left on: both were switched
on, verified, and switched off again during testing.

### The one keyboard hook (PHASE 4)
To put its own switcher on Alt+Tab, the app installs a single `WH_KEYBOARD_LL` hook
while it runs. It is the only hook in the project and it **swallows exactly two
keystrokes**: `Tab` while Alt is physically held, and `Esc` while the switcher is
showing. Everything else passes through untouched.

The hook lives in this process and **dies with it** - there is nothing to undo and
nothing left behind. Switching "Use this app's window switcher" off in settings stops
it swallowing anything, and Windows' own Alt+Tab is back immediately. So does quitting
the app, killing it, or a window running as administrator taking focus.

### A note on the two shortcuts the UI sends
The Start button sends **Ctrl+Esc**, and the top bar sends **Win+A** / **Win+N**, through
`SendInput`. These *send* a keystroke the way any automation tool does; they do **not**
install a hook, intercept a key, or remap anything. Every Windows shortcut continues to
work exactly as before.

### A note on the widget layer (PHASE 5)
The desktop widgets are an ordinary window of ours, parked in the z-order just above
the desktop. **Nothing about the desktop itself is modified**: no `SetParent` into
`Progman`, no wallpaper hook, no shell extension. It is removed by the app exiting,
like every other window.

### Known gap
Killing the **entire process tree** at once (e.g. "End process tree" in Task Manager)
kills the watchdog along with the app, so U-1..U-3 stay in effect until you run
`restore-windows.ps1`. Every other exit path — clean quit, crash, single-process kill,
power loss on next boot — is covered.

---

## 3. If you run the installer (optional)

`src-tauri/target/release/bundle/nsis/macui_0.1.0_x64-setup.exe` installs **per user**,
with no UAC prompt. Verified by installing and uninstalling it:

| What it does | Where | Undo |
|---|---|---|
| Copies `macui.exe`, `WebView2Loader.dll` and `uninstall.exe` | `%LOCALAPPDATA%\macui` | `%LOCALAPPDATA%\macui\uninstall.exe` (add `/S` for silent) |
| Writes the Add/Remove Programs entry Windows expects | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\macui` | removed by the same uninstaller |

**Installed on this machine (2026-11-01)**, at the user's request that it keep
running without VS Code: the installer above ran with `/S`, and
`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\macui.lnk` points at
`%LOCALAPPDATA%\macui\macui.exe` so it starts at sign-in. To undo it, use "Turn off"
in the quick panel (passcode), which deletes the shortcut and restores Windows. You can
also delete the shortcut yourself and run the uninstaller.

Checked after uninstalling: the registry entry was gone and the folder held nothing but
`macui.log`, this app's own runtime log, which is yours to delete. You do not have to
run the installer at all — the `.exe` in `src-tauri/target/release` runs as it stands.

---

### The three-finger touchpad swipe (2026-11-01, D-061)

| What | Where | Undo |
|---|---|---|
| Windows' own three-finger swipe set to "Nothing" while macui's carousel swipe is on | `HKCU\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad\ThreeFingerSlideEnabled` (was not set on this machine) | Automatic on quit and Turn off; `restore-windows.ps1`; or Settings > Bluetooth & devices > Touchpad > Three-finger gestures > Swipes |

---

## 4. Files created outside the project folder

- `%LOCALAPPDATA%\macui-toolchain\dl\mingw.zip` — downloaded installer archive, safe to
  delete after the toolchain is extracted.
- `%LOCALAPPDATA%\macui-toolchain\dl\rustup-init.exe` — same.

- `%APPDATA%\macui\settings.json` — every setting the app has. Deleting it resets the
  app to defaults and affects nothing else on the machine.
- `%APPDATA%\macui\macui-warm-*.png` — only if you accepted one of the bundled
  wallpapers (U-5).
- `%LOCALAPPDATA%\macui\macui.log` — runtime log, truncated at 1 MB.

## 5. System state the quick-settings panel can change

These are things you change **by using the panel**, exactly as you would from the
Windows taskbar. Nothing here happens on its own, nothing persists in a form the app
owns, and `restore-windows.ps1` does not need to undo any of it — the Windows control
for each one is the undo.

| What | How | Undo |
|---|---|---|
| Wi-Fi radio on/off | `Windows.Devices.Radios`, the same call the Windows Wi-Fi tile makes | the Wi-Fi tile, in our panel or Windows' |
| Bluetooth radio on/off | same | same |
| Airplane mode tile | turns every toggleable radio off (or back on) | the tile again, or Windows' own switch |
| Screen brightness | `WmiMonitorBrightnessMethods::WmiSetBrightness`, the same method Windows' brightness slider drives | any brightness control; Windows' slider reads the same value |
| Output volume | the default audio endpoint, unchanged from before | the volume slider |

The panel's other tiles — Battery saver, Night light, Accessibility, Nearby sharing,
Cast, VPN, Mobile hotspot, Project — **change nothing themselves**. They open the real
Windows page or flyout for that setting (see D-036).
