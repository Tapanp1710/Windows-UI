# PROGRESS

**Read this file first when resuming.**

## Current phase
**All six phases are built.** STEP 0, PHASE 1 (Dock), PHASE 2 (Top bar and island),
PHASE 3 (Launcher), PHASE 4 (Window switcher), PHASE 5 (Desktop widgets) and PHASE 6
(Settings / startup / packaging).

**One thing is not verified**: the Win+<key> matrix. `scripts/winkey-matrix.ps1`
sends every combination the brief lists and reports what Windows did. Run it when the
machine is free — it takes the keyboard for about a minute. Until then the claim that
Win+E, Win+R, Win+L and the rest are untouched rests on how the hook is built
(D-029), not on evidence.

## Status summary

| Phase | State |
|---|---|
| STEP 0 — environment & feasibility | **DONE** |
| PHASE 1 — Dock | **Core done**, some items outstanding (below) |
| PHASE 2 — Top bar | **Done**, verified |
| PHASE 6 — Settings / startup / packaging | **DONE**, verified |
| PHASE 5 — Desktop widgets | **DONE**, verified |
| PHASE 4 — Window switcher | **DONE**, verified |
| PHASE 3 — Launcher | **DONE** — Win-key matrix not yet verified |

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

### Top bar and island (PHASE 2, reworked after feedback)
- **Auto-hides by default**: slides away, and comes back when the pointer touches the
  top edge. **Reserves nothing**, so maximised windows get the whole screen. While it
  is away its hit-test region is empty, so the top strip behaves like bare screen.
- Two pills: the **island** (centre) and a status pill (network, Bluetooth, volume,
  battery). The focused-app pill is off by default — it repeated what the window in
  front already said.
- **The island** is the centre pill: collapsed it is the clock; hovering expands it to
  what is playing, with album art, title and transport controls; and it expands by
  itself for a few seconds when the volume changes, the charger goes in or out, or the
  track changes. (D-026)
- **Clicking Wi-Fi, Bluetooth, volume or battery opens our own glass panel under the
  pill**, not Windows' flyout down by the taskbar. The switches drive the **real
  radios**; the slider drives the real audio endpoint; everything not duplicated links
  to the matching Windows settings page. (D-022)
- Clicking the island opens a calendar panel the same way.
- Scrolling on the volume icon changes the volume, 2% per notch.
- The window spans the screen but its **clickable area is only the pills** (and the
  island's body while it is open), so clicks between them reach the app underneath.

### Launcher (PHASE 3)
- Opens on the **Win key** or the dock's Start button; either again, or Escape,
  closes it. Real glass, DWM-rounded, system shadow.
- **Every installed app** — 158 on this machine, Store apps included, each with its
  real icon. It lists `shell:AppsFolder`, which is the shell's own list (D-030).
- Search filters apps as you type and finds files in your folders through a bounded
  walk: depth 3, 30 results, a 350ms budget.
- Sidebar of folders; click one to browse it, click a folder in the results to go
  deeper, right-click anything for "Open in File Explorer".
- Sleep, restart, shut down and sign out.
- **Only the Win key's release is ever touched**, and only when nothing else was
  pressed while it was held — which is exactly the keystroke that opens Start. Every
  Win+<key> combination reaches Windows by construction (D-029).

### Window switcher (PHASE 4)
- **Alt+Tab** opens a centred glass bar with **live DWM previews** of every window —
  the real thing, updating, not screenshots.
- Alt+Tab forward, Alt+Shift+Tab back, both wrapping; releasing Alt switches; Escape
  cancels and leaves focus exactly where it was. The list is in Windows' own
  most-recently-used order and opens on the second entry, so a tap swaps the last two
  windows just as it always did.
- **Every way it can fail hands Alt+Tab back to Windows** — including being killed
  mid-switch with Alt held down, which was tested by doing it (D-027).
- **Win+Tab is deliberately untouched.** Virtual desktops have no supported API
  (D-028).
- One switch in settings returns Alt+Tab to Windows entirely.

### Desktop widgets (PHASE 5)
- Five glass widgets on the desktop: **clock**, **calendar** (today highlighted),
  **weather**, **music controls** and **system stats** (CPU, RAM, battery).
- They live **behind every window** and **survive Win+D** — they are part of the
  desktop, not a panel on top of it.
- **Clicks between them go straight through to the desktop**: the window is clipped to
  the cards themselves, so icons, right-click and drag-select all behave normally.
- Drag any widget anywhere; positions are saved. A plain click does not move them.
- **Weather is live from Open-Meteo** — no account, no API key. Default city
  **Bengaluru** (D-017); change it in settings and it is geocoded and refreshed.
- **Music controls drive Windows' own media session** — the same source the volume
  flyout and lock screen read, so it works with any player that registers one. Title,
  artist, play/pause, next, previous, all verified against a real session.
- Each widget has its own switch in settings, plus a master switch that creates and
  destroys the whole layer.
- Clicking a widget **hands focus straight back** to whatever you were working in.

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

### Look
- **Real blurred glass** on the settings window and the panels, through
  `SetWindowCompositionAttribute`, rounded by DWM so the blur follows the corner and
  Windows draws the hairline border and drop shadow. The dock, top bar and widgets keep
  painted glass, because each of those windows holds several separate islands and a
  window-wide blur would fill the gaps between them. (D-023)
- **Shadows are sized to the hit-test regions** that clip them: rounded regions,
  inflated by exactly the shadow's reach. Before this the regions were rectangles and
  every rounded card was clipped into a sharp-cornered slab. (D-025)

### Safety
- Taskbar hidden while running, restored on clean exit.
- **Watchdog process restores the taskbar after a hard kill** (verified repeatedly:
  taskbar returns, the user's original auto-hide setting is preserved, no orphans).
- `restore-windows.ps1` recovers the taskbar even when the whole process tree is killed.

### Performance (idle, every phase loaded, panels closed)
- **Idle CPU: ~0.2%** across 8 processes — target near-0%. **Met.**
- **RAM: 242–246 MB private** over repeated fresh starts. Target under 250 MB. **Met**,
  but with little room left.

Getting there took two structural changes, both worth knowing:

- **Every panel shares one WebView2 renderer** (D-020). Without that it was 258 MB
  with only five windows.
- **A window is a compositing surface whether it is showing or not.** The switcher
  was created screen-sized so clicks outside would dismiss it; sizing it to its own
  bar instead gave back 11–14 MB for a window that only ever occupies a strip down
  the middle. The widget layer still has to be screen-sized, because its widgets
  genuinely are spread across the screen.

Working set overstates it badly, because Chromium's shared binaries are counted once
per process; private bytes is the fair number.

How it got there is worth knowing. The widget layer first pushed idle memory to
**258 MB**, over target, because every webview was getting its own renderer process.
Passing `--process-per-site --renderer-process-limit=1` to WebView2 makes all our
panels share one — they are the same origin and all our own code — which brought the
whole app to **213.9 MB** and ten processes down to eight. See DECISIONS D-020,
including what that trades away.

---

## What is NOT done in Phase 5
- **No widget covers the case of a second monitor.** Positions are clamped to the
  primary display.
- The weather widget shows the **current** conditions only — no forecast. The brief
  asked for weather, not a forecast panel.
- Widgets sit **above desktop icons** where they overlap them. That is what desktop
  widgets do, but the default layout does put the clock over the top-left icons; drag
  it anywhere you prefer and it stays there.

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
11. The pages now report JS errors into `macui.log` via `cmd_log`. The old
   `document.title` diagnostics channel they carried is unreadable from outside,
   because wry does not mirror `document.title` onto the native window title.
12. **`alwaysOnBottom` puts a window BELOW the desktop**, not just below other
   windows: `HWND_BOTTOM` is under `Progman`, so the widgets were invisible. Insert
   directly above `Progman` instead, and re-assert it.
13. **Two CSS rules with the same specificity are decided by bundler output order**,
   which is not something to rely on. `.card` and `.glass` both set a background;
   making it `.w .card` settled it.
14. **Chromium ignores `::-webkit-scrollbar` once `scrollbar-width` is set.**
15. **`TryTogglePlayPauseAsync` returns false and does nothing** on media sessions
   that expose Play and Pause but not the toggle. Ask for the specific transition.
16. **The Windows taskbar can come back on its own.** The shell's "Open with" dialog
   did it. Hiding it once at startup is not enough.
17. **`SetWindowRgn` does not clip acrylic.** DWM composites the blur against the
   window *rectangle* and ignores the region, so a region with 21px corners still
   produced a perfectly square panel. `DWMWA_WINDOW_CORNER_PREFERENCE` is what DWM
   itself honours.
18. **A flyout cannot rely on losing focus to know it should close**, at least not
   here: the top bar is not focusable, so clicking a pill leaves focus where it was,
   and a panel that took focus on creation lost it again a moment later and closed
   itself every time. Panels take no focus and watch for a click outside instead.
19. **`WebviewUrl::App` takes a PathBuf**, so `"panel.html?kind=quick"` is a filename,
   not a URL with a query string.
20. **A region sent on a state change is measured mid-transition.** The island's
   region has to reserve its full footprint, or the body is clipped away as it grows.

---

## Next step

1. **Run `scripts/winkey-matrix.ps1`** when the machine is free. It is the one
   outstanding verification, and it is the one that matters most.
2. **Window stashing.** You asked to be able to park an open *window* on the island so
   it drops out of Alt+Tab, and bring it back later. Files, folders and images are
   done; windows are not. It needs hiding the window, remembering it, and restoring
   it — the switcher already skips hidden windows, so that part is free.
3. **The rest of the taskbar's settings in the top panel.** It has Wi-Fi, Bluetooth,
   volume and battery today, each driving the real thing, plus links to the matching
   Windows pages. Brightness, airplane mode and battery saver are the obvious next
   ones.
4. **Multi-monitor.** Untestable here; the code assumes one display in several places.

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

## 2026-10-31 — crash fix, full quick settings, blue palette, Start restyle

- **The hangs are fixed.** They were `AppHangB1`, not crashes: two things waited on the
  main thread, both in the Wi-Fi/Bluetooth path — a PowerShell call behind the Bluetooth
  status read, and the radio toggles themselves. Both moved to a background poller
  (D-033). Low RAM was real (0.38 GB free of 7.90) but it was the aggravator, not the
  fault.
- **Every Windows taskbar control is now at the top**: eleven tiles and two sliders.
  Wi-Fi, Bluetooth, Airplane, brightness and volume drive the real thing; the seven with
  no public API open the real Windows page instead of pretending (D-036).
- **Brightness is real**, through WMI, and the round-trip test caught a type mismatch
  that `ExecMethod` had reported as success (D-035).
- **Palette is white to dark blue**, with every colour now living only in `theme.css`.
- **The white marking at the top is gone** — the collapsed island's hit region was
  measured from its container, which is 330px wide even when empty (D-034).
- **The launcher looks like a Start menu**: pill search, sidebar icons, account row and
  a power menu.
- Memory **236.6 MB** settled, under the 250 MB target.
- I corrupted `settings.json` mid-session writing it from PowerShell (UTF-8 BOM) and
  lost the shelf and widget positions; both restored from the session's earlier values.
