# PROGRESS

**Read this file first when resuming.**

## Current phase
PHASE 1 (Dock) and PHASE 2 (Top bar) complete and verified. PHASES 3–6 not started.

## Status summary

| Phase | State |
|---|---|
| STEP 0 — environment & feasibility | **DONE** |
| PHASE 1 — Dock | **Core done**, some items outstanding (below) |
| PHASE 2 — Top bar | **Done**, verified |
| PHASE 3 — Launcher | **Not started** |
| PHASE 4 — Window switcher | **Not started** |
| PHASE 5 — Desktop widgets | **Not started** |
| PHASE 6 — Settings / startup / packaging | **Partly**: settings *storage* exists, no settings UI, no installer yet |

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

### Safety
- Taskbar hidden while running, restored on clean exit.
- **Watchdog process restores the taskbar after a hard kill** (verified repeatedly:
  taskbar returns, the user's original auto-hide setting is preserved, no orphans).
- `restore-windows.ps1` recovers the taskbar even when the whole process tree is killed.

### Performance (measured over 20 s idle, dock + top bar)
- **Idle CPU: 0.09%** across all 9 processes (8 cores) — target was near-0%. **Met.**
- **RAM: 202.6 MB private / 441.8 MB working set.** Target was under 150 MB. **Over.**

An earlier note in this file said 38.8 MB; that was wrong — it counted only our own two
processes and not the 7 WebView2 processes that do the actual rendering. The 150 MB
target is not reachable with Tauri + WebView2 for a multi-window shell; see REPORT.md
section 3.1 for the numbers and the concrete way to cut it.

---

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

---

## Next step
PHASE 2 (top bar). Before that, the highest-value Phase 1 follow-ups are the DWM
thumbnails for the window chooser and a settings UI to drive the options that already
exist in `settings.json`.

## How to run / stop
```
scripts/build.sh                                  # release build
src-tauri/target/release/macui.exe                # run
powershell -ExecutionPolicy Bypass -File restore-windows.ps1   # back to normal Windows
```
Log: `%LOCALAPPDATA%\macui\macui.log`. Settings: `%APPDATA%\macui\settings.json`.
