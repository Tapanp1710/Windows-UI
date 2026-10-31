# PROGRESS

**Read this file first when resuming.**

## Current phase
STEP 0, PHASE 1 (Dock), PHASE 2 (Top bar) and PHASE 6 (Settings / startup / packaging)
are complete and verified. PHASES 5, 4 and 3 are next, in that order.

## Status summary

| Phase | State |
|---|---|
| STEP 0 — environment & feasibility | **DONE** |
| PHASE 1 — Dock | **Core done**, some items outstanding (below) |
| PHASE 2 — Top bar | **Done**, verified |
| PHASE 6 — Settings / startup / packaging | **DONE**, verified |
| PHASE 5 — Desktop widgets | **Not started** — next |
| PHASE 4 — Window switcher | **Not started** |
| PHASE 3 — Launcher | **Not started** — last, by design (riskiest) |

---

## Environment (verified 2026-10-31)

| Tool | Status | Version |
|---|---|---|
| Node / npm | OK | v24.15.0 / 11.12.1 |
| Git | OK | 2.54.0.windows.1 |
| WebView2 Runtime | OK | 156.0.4314.50 |
| Rust / Cargo | OK | 1.99.1 (`stable-x86_64-pc-windows-gnu`, installed per-user) |
| MinGW-w64 GCC | OK | 16.2.0 UCRT (portable, `%LOCALAPPDATA%\macui-toolchain`) |
| MSVC build tools | **absent** | cannot install without an admin/UAC prompt |
| Windows SDK | **absent** | same |

### Revised targets (agreed 2026-10-31)
- **Memory: under 250 MB private bytes for the whole process tree**, WebView2 included,
  replacing the original 150 MB. Always measured across all processes.
- **The single-window merge is not being done.** It stays recorded in REPORT.md as a
  future option.
- **Build order is 6 → 5 → 4 → 3**, not 3 → 4 → 5 → 6.

### Deviations from the brief
The brief described an ASUS X515EA (i5-1135G7, 12 GB, Windows 11 24H2 / build 26100).
The actual machine is:

- **Windows 11 Pro build 22631** (23H2)
- **7.9 GB RAM**
- **Intel UHD 620** graphics (not Iris Xe)
- **1920×1080 at 150% scaling**, so the logical desktop is **1280×720**
- Dark mode on; Windows animations on; **taskbar already set to auto-hide**

None of this blocked the work, but the 150% scaling matters for every layout number,
and the pre-existing auto-hide setting is the reason `restore-windows.ps1` restores from
a recorded baseline instead of forcing "always visible".

---

## What works (verified by test, not by assumption)

### Toolchain / build
- Tauri v2 builds and runs on the **`x86_64-pc-windows-gnu`** target — the MSVC-free
  path works end to end, including `wry` and `webview2-com`.
- Release `.exe` runs; **App Control for Business does not block it**.
- Build with `scripts/build.sh` (or `npm run tauri build`).

### Dock
- Floating glass dock, bottom-centre, 16px radius, 1px light border, soft shadow,
  nothing touching a screen edge.
- **Real app icons at 256px**, pulled through `IShellItemImageFactory` (the same path
  Explorer uses), premultiplied alpha undone, cached in Rust.
- **Imports the real Windows taskbar pins** on first run (read from the Quick Launch
  `User Pinned\TaskBar` folder).
- Running apps shown with a dot; multiple windows of one app show a wider pill and
  open a chooser listing the window titles.
- Pinned + running de-duplicate onto one button (pins are matched to processes by
  resolving each `.lnk` to its target exe).
- Divider, then Recycle Bin (with its real icon, and a "not empty" state).
- Hover magnification 1.15× with 4px lift, neighbours scaled less; tooltips.
- Right-click menu: launch / pin / unpin / close window(s). Middle-click = new instance.
- Drag to reorder pinned icons, persisted to `%APPDATA%\macui\settings.json`.
- Auto-hide: reveals on touching the bottom screen edge, conceals 400 ms after leaving.
- Light/dark follow Windows; Windows' "Animation effects" setting is respected.

### Top bar (PHASE 2)
- Three floating glass pills 6px from the top: focused app, date and time in the user's
  own Windows format, and a status pill (network, Bluetooth, volume, battery).
- Clicking the status icons opens **Windows' own Quick Settings**; clicking the clock
  opens **Windows' own calendar and notification centre**. Not replicas.
- Scrolling on the volume icon changes the volume, 2% per notch.
- The window spans the screen but its **clickable area is only the three pills**, so
  clicks between them reach the app underneath.
- 63px is reserved at the top, so maximised windows stop below the bar.

### Settings, startup and packaging (PHASE 6)
- **A real settings window**, opened from the dock's right-click menu, from a
  right-click on any top-bar pill, or from the tray. Built on demand and destroyed on
  close, so it costs nothing while shut (D-012).
- Live, no restart: theme, icon shape, dock on/off, always-visible, magnification,
  icon size, top bar on/off, top bar auto-hide, animation speed.
- Two of those settings — **always-visible** and **icon size** — were stored but never
  read before this phase. They work now.
- **Startup entry**: a shortcut in your own Startup folder, toggled from settings. No
  registry key (D-013).
- **Wallpaper**: the project's own warm abstract artwork, offered in settings and never
  applied unless you ask. Your previous wallpaper is recorded first and can be put back
  from settings or from `restore-windows.ps1` (D-015).
- **"Return to normal Windows"** — taskbar, work area and wallpaper restored, then quit.
  Same code path as the crash restore and as `restore-windows.ps1`.
- **NSIS installer**, `macui_0.1.0_x64-setup.exe`, 7.1 MB, per-user, **no UAC prompt**.
  Installing and uninstalling were both verified; the uninstaller leaves nothing behind
  but this app's own log file. The loose `.exe` still runs without installing anything.

### Safety
- Taskbar hidden while running, restored on clean exit.
- **Watchdog process restores the taskbar after a hard kill** (verified repeatedly:
  taskbar returns, the user's original auto-hide setting is preserved, no orphans).
- `restore-windows.ps1` recovers the taskbar even when the whole process tree is killed.

### Performance (measured over 30 s idle, dock + top bar, settings closed)
- **Idle CPU: 0.104%** across all 9 processes (8 cores) — target near-0%. **Met.**
- **RAM: 200.4 MB private / 453.7 MB working set.** Revised target under 250 MB. **Met.**

Working set overstates it: Chromium's shared binaries are counted once per process.
Private bytes is the fair number. PHASE 6 added no idle cost — with the settings window
open it is 210.6 MB, and closing it returns to 200.4 MB.

The original 150 MB target was not reachable with Tauri + WebView2 for a multi-window
shell; REPORT.md §3.1 has the numbers and the one change that would cut it further.

---

## What is NOT done in Phase 6
- The **tray icon** exists and is wired, but cannot be clicked while the app is running:
  the notification area is part of the taskbar we hide (D-016). The same menu is
  reachable from the dock and the top bar.
- **Top bar auto-hide** (`topbarAutoHide`) has a switch now, and correctly gives the
  reserved strip back, but the bar itself does not yet slide away on a timer — it simply
  stays put. Treat the switch as "reserve screen space: yes/no" until PHASE 5.

## What is NOT done in Phase 1

- **Window thumbnails** in the grouped-window chooser. It currently lists window
  *titles*. Live previews need the DWM thumbnail API, which is Phase 4 work.
- **Real jump lists**. The right-click menu offers launch / pin / unpin / close, not the
  app's own jump-list entries. Reading another app's jump list means parsing the
  undocumented `automaticDestinations-ms` streams; deferred, see DECISIONS D-005.
- **Progress indicators** are implemented in the UI layer (the thin bar, including
  error/paused/indeterminate states) but nothing feeds them yet: reading another app's
  taskbar progress requires implementing the `ITaskbarList3` server side, which a
  normal app cannot do. See DECISIONS D-006.
- **Attention/flash state** likewise renders but has no feed yet (needs a shell hook).
- **Pin order** is imported alphabetically, not in the user's Windows taskbar order
  (that order lives in an undocumented registry blob). See DECISIONS D-004.
- Multi-monitor is untested — this machine has one display.

---

## Bugs found and fixed along the way
These cost real time and are worth not rediscovering:

1. **Blank release window.** Tauri computes `dev = !custom_protocol`. A bare
   `cargo build --release` therefore produces a *dev* binary that loads the Vite dev
   server and shows nothing. Fixed with a `custom-protocol` feature + `scripts/build.sh`.
2. **No `capabilities/` directory**, so `listen()` was denied by Tauri v2's permission
   system while `invoke()` of app commands still worked — auto-hide events never
   arrived and the dock stayed visible forever.
3. **Initial-state desync.** The frontend started "revealed" while the Rust sampler
   started "concealed", and the sampler only emits on a *change* — so a freshly started
   dock never hid until a full reveal-then-leave cycle. Both sides now start concealed.
4. **Auto-hide thread wedging.** The sampler called `outer_position()`/`outer_size()`
   from its own thread, which dispatches onto the main event loop; once the dock began
   doing per-second IPC this blocked and auto-hide silently stopped. The window rect is
   now published by the main thread and read as plain data.
5. **Popups clipped.** A transparent window clips anything outside its rect, so
   tooltips and menus drawn above the dock were invisible. The window now grows upward
   only while a popup needs the space (and gives it straight back, so it does not
   swallow clicks).
6. **`html/body` had no height**, so `.slider { height: 100% }` collapsed and the dock
   drifted off the bottom edge whenever the window was resized for a popup.
7. **Screenshot tool was DPI-unaware**, silently capturing 1280×720 instead of the real
   1920×1080 — it was hiding exactly the 1px borders the visual check exists to verify.
8. Rust structs returned to JS need `#[serde(rename_all = "camelCase")]`; without it
   `iconShape` was `undefined` and the icon-shape setting silently did nothing.
9. **Creating a window from a synchronous Tauri command half-builds it.** The window
   appears, its WebView2 child is 0x0 and hidden, and nothing ever paints — with no
   error anywhere. A sync command runs on the main thread inside the WebView2 callback
   that delivered the IPC message. Window creation must be in an `async` command.
10. **Chromium ignores `::-webkit-scrollbar` entirely once `scrollbar-width` is set**,
   so a half-styled scrollbar silently falls back to the browser default.
11. The three pages now report JS errors into `macui.log` via `cmd_log`. The old
   `document.title` diagnostics channel they carried is unreadable from outside,
   because wry does not mirror `document.title` onto the native window title.

---

## Next step
**PHASE 5 — desktop widgets.** Then PHASE 4 (window switcher), then PHASE 3 (launcher).

For PHASE 4 and PHASE 3, the keyboard-hook rules agreed for this run:
- If the hook fails, is not installed, or an elevated window has focus, Windows' own
  behaviour must work. Test it explicitly, **including killing the app mid-Alt+Tab with
  Alt still held down**.
- For the Win key, test every combination listed in the brief and record pass/fail in
  TESTS.md. If any combination breaks, keep Windows Start on the Win key and log it.

## How to run / stop
```
scripts/build.sh                                  # release build (exe only)
npm run tauri build                               # release build + NSIS installer
src-tauri/target/release/macui.exe                # run
powershell -ExecutionPolicy Bypass -File restore-windows.ps1   # back to normal Windows
```
Settings: right-click the dock background, or any top-bar pill.
Installer: `src-tauri/target/release/bundle/nsis/macui_0.1.0_x64-setup.exe` (per-user,
no UAC).
Log: `%LOCALAPPDATA%\macui\macui.log`. Settings: `%APPDATA%\macui\settings.json`.
