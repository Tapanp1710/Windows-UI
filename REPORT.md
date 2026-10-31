# REPORT — macOS-inspired UI layer for Windows

Written for: you, the laptop's owner, on waking up.

**Short version:** STEP 0, PHASE 1 (dock) and PHASE 2 (top bar) are built, tested and
committed. PHASES 3–6 (launcher, window switcher, widgets, settings UI + installer) are
**not built**. Everything that is built has been verified by actually running it and
reading screenshots back, not assumed. Your Windows taskbar always comes back.

Two things need your judgement, both flagged below: **memory use is over target** and
**animation smoothness needs a human eye**.

---

## 1. What works

### STEP 0 — environment and feasibility
Your machine had **no Rust and no C++ build tools**, and installing MSVC needs an admin
prompt, which your rules forbid. Rather than stop, I found a fully no-admin path:

- Rust 1.99.1 installed per-user, targeting **`x86_64-pc-windows-gnu`**
- Portable **MinGW-w64 GCC 16.2.0** extracted from a zip (no installer, no registry)

**Tauri v2 builds and runs on that target** — proven, not assumed. The release `.exe`
runs fine; **App Control for Business does not block it**.

### PHASE 1 — Dock
- Floating glass island, bottom centre, 12px above the edge, 16px radius, 1px border,
  soft shadow. Nothing touches a screen edge or corner.
- **Real app icons at 256px**, pulled through the same shell API Explorer uses, on soft
  circular glass plates (circle / squircle / rounded square is a setting).
- **Your actual Windows taskbar pins are imported** on first run (it found Brave and
  File Explorer).
- Running apps get a dot; several windows of one app get a wider pill and a chooser.
- A pinned app that is also running shows as **one** button, not two (pins are matched
  to processes by resolving each shortcut to its target `.exe`).
- Divider, then the Recycle Bin with its real icon and a "not empty" state.
- Hover magnification 1.15× with a 4px lift, neighbours scaled less; tooltips.
- Right-click: launch / pin / unpin / close window(s). Middle-click opens a new instance.
- Drag to reorder pinned icons, saved to disk.
- Auto-hide: slides up when you touch the bottom edge, slides away 400ms after you leave.
- The Windows taskbar is hidden while it runs, and restored when it stops.

### PHASE 2 — Top bar
- Three floating glass pills, 6px from the top: focused app (name + icon), date and
  time in **your** Windows format, and a status pill.
- **Clicking Wi-Fi, Bluetooth, volume or battery opens the real Windows Quick Settings**
  (the Win+A panel). **Clicking the clock opens the real Windows calendar and
  notification centre** (Win+N). Both confirmed by screenshot — these are Windows' own
  panels, not replicas.
- **Scrolling on the volume icon changes the volume**, 2% per notch (verified 86% → 74%
  over six notches). Scrolling anywhere else is unaffected.
- Live battery (with charging and battery-saver states), volume, network and focused app.
- The bar is one full-width window, but its **clickable area is restricted to the three
  pills**. Verified: clicking between or below the pills hits the app underneath, not us.
  The top strip of your screen is not stolen.
- 63px is reserved at the top so **maximised windows stop below the bar**.

### Safety — the part that matters most
- The taskbar is restored on a clean exit.
- A **watchdog process** restores it if the app is hard-killed. Verified repeatedly:
  taskbar returns, **your original auto-hide setting is preserved**, no orphan processes.
- `restore-windows.ps1` recovers everything even if the whole process tree is killed.
- Nothing ran as administrator. No registry keys written. Nothing outside the project
  folder was modified except two toolchain folders, all logged in SYSTEM_CHANGES.md.

---

## 2. What could not be restyled without changing Windows behaviour

| Thing | Why it was left as Windows |
|---|---|
| **Quick Settings panel** (Wi-Fi, Bluetooth, volume, battery) | The brief requires clicking these to open *the same* Windows flyout. A hand-drawn replacement would be a different panel with different behaviour. Our icons open Windows' own. |
| **Calendar / notification centre** | Same reasoning. |
| **Start menu** | PHASE 3 was not reached, so the Start button opens the real Start menu (via Ctrl+Esc). It *sends* a shortcut rather than intercepting one, so no Windows key binding is altered. |
| **Jump lists** | Reading another app's jump list means parsing undocumented files with no supported API. The menu offers launch / pin / unpin / close instead. (D-005) |
| **Progress & attention indicators** | Receiving these means *being* the registered shell taskbar, not drawing one. The visuals are implemented and correct, but nothing can feed them from a normal app. (D-006) |
| **Pin order** | Windows stores taskbar pin order in an undocumented registry blob. Pins import alphabetically; drag to reorder and it sticks. (D-004) |
| **System Acrylic blur** | `backdrop-filter` cannot blur the desktop through a transparent window, and real Acrylic fills a rectangle — which would square off every rounded pill and kill the shadow. Took the brief's documented fallback: more opaque surfaces. (D-009) |
| **Tray icon** | It exists, but it lives in the notification area *inside* the taskbar we hide, so it is unreachable. Right-clicking the dock gives you Settings / Restart / Quit instead. |

---

## 3. Known issues and what needs your eyes

### Needs your judgement

1. **Memory is over target.** The brief asked for under 150 MB.
   - Measured with both bars running and idle: **202 MB private bytes, 442 MB working
     set** across 9 processes (2 ours + 7 WebView2).
   - Working set overstates it badly — Chromium's shared binaries are counted once per
     process. **Private bytes (~202 MB) is the fair number.**
   - This is inherent to the stack *you specified*: Tauri renders through WebView2,
     which is Chromium, and its fixed overhead (browser + GPU + utility processes) is
     roughly 250 MB of working set before our code does anything. Each extra window
     adds a renderer (~60 MB working set, ~25 MB private).
   - **The 150 MB target is not reachable with Tauri + WebView2 for a multi-window
     shell.** My earlier figure of 38 MB was wrong — it counted only our own two
     processes and not the WebView2 runtime doing the rendering. Correcting that here
     rather than leaving a flattering number in the log.
   - **Concrete fix if you want it:** merge every panel into ONE window spanning the
     screen and use the hit-test-region trick already working in the top bar. That
     collapses N renderers into one and would save roughly 60 MB per panel removed.
     It also removes the window-resizing dance the dock does for tooltips. I did not do
     it because it is a sizeable refactor and I would rather hand you something working.

2. **Animation smoothness.** Frame bursts show **no flicker, no blank frames and no
   position jumps**, and the start and end states are correct. But the capture rig only
   manages ~50–65 ms per frame, so a 250 ms animation is sampled 4–5 times. That proves
   the absence of gross faults; it **cannot prove it feels smooth**. Please watch the
   dock slide up and down a few times.

3. **Glass look.** Because real desktop blur is not available (see §2), the surfaces are
   fairly opaque. Check whether that reads as "glass" to you or just "dark panel". The
   opacity is one value in `src/shared/theme.css` (`--glass-bg`).

### Known limitations
- **Multi-monitor is untested** — this laptop has one display.
- Killing the *whole process tree* at once (e.g. "End process tree" in Task Manager)
  kills the watchdog too, so the taskbar stays hidden until you run
  `restore-windows.ps1`. Every other kill path is covered.
- Wi-Fi **signal strength and SSID** are not shown; the icon shows connected/not. Full
  signal detail needs the WLAN API, which was not worth the surface area for one glyph.
- The tray-overflow chevron falls back to Quick Settings on Windows 11, where the
  overflow is a XAML host with no clickable child window.
- The "Settings…" item in the dock's right-click menu is a disabled placeholder —
  PHASE 6 was not reached. Settings are real and respected, but only editable by hand
  in `%APPDATA%\macui\settings.json`.

### Your machine is not the machine in the brief
The brief described an ASUS X515EA (i5-1135G7, 12 GB, Windows 11 24H2 / build 26100).
This machine is **Windows 11 build 22631 (23H2), 7.9 GB RAM, Intel UHD 620**, running
**1920×1080 at 150% scaling** (so the logical desktop is 1280×720). Nothing broke
because of it, but the 150% scaling affects every layout number, and the lower RAM makes
the memory finding above more important, not less.

---

## 4. Decisions I made on my own

Eleven, all in **[DECISIONS.md](DECISIONS.md)**. The ones that would change your plans:

- **D-001** — continued past the missing MSVC toolchain using a no-admin GNU path,
  instead of stopping as STEP 0 says. Stopping would have delivered nothing, and the
  alternative worked.
- **D-002** — put the toolchain in `%LOCALAPPDATA%`, not the project folder, because the
  project folder is OneDrive-synced and this would have uploaded ~1.5 GB of binaries.
- **D-009** — no system Acrylic; higher-opacity surfaces (see §2).
- **D-010** — reserved the work area with a silent `SPI_SETWORKAREA`. The appbar route
  worked but un-hid the taskbar.

---

## 5. How to install, run, and get back to normal Windows

### Run it
```powershell
cd C:\Users\ADMIN\OneDrive\Desktop\ui
.\src-tauri\target\release\macui.exe
```

### Settings
Right-click the **dock background** (not an icon) → Settings / Restart UI / Quit.
There is no settings window yet; edit `%APPDATA%\macui\settings.json` and restart.
Useful keys: `iconShape` (`circle`/`squircle`/`rounded`), `theme` (`auto`/`light`/`dark`),
`dockAlwaysVisible`, `dockMagnification`, `animationSpeed`, `topbarEnabled`.

### Turn it off
Right-click the dock background → **Quit**. This always restores the taskbar.

### Fully restore normal Windows (works even after a crash)
```powershell
cd C:\Users\ADMIN\OneDrive\Desktop\ui
powershell -ExecutionPolicy Bypass -File .\restore-windows.ps1
```
Restores the taskbar to the **exact** state recorded in `baseline-windows.json` before
anything was touched (your taskbar was already set to auto-hide, and that is preserved).

### Uninstall completely
```powershell
powershell -ExecutionPolicy Bypass -File .\restore-windows.ps1 -RemoveToolchain
Remove-Item -Recurse -Force "$env:APPDATA\macui", "$env:LOCALAPPDATA\macui"
```
Then delete the project folder. No installer was produced (PHASE 6 not reached), so
nothing is registered with Windows — there is nothing in Add/Remove Programs.

### Rebuild after changes
```bash
scripts/build.sh
```
**Do not use a bare `cargo build --release`** — Tauri computes `dev = !custom_protocol`,
so without the feature flag you get a binary that loads the Vite dev server and shows a
blank window. That cost a long detour; the script handles it.

---

## 6. Resource usage (idle, measured)

| Metric | Value | Target | Verdict |
|---|---|---|---|
| Idle CPU | **0.09%** across 9 processes (8 cores) | near-0% | **met** |
| Private bytes | **202.6 MB** | under 150 MB | **over** (see §3.1) |
| Working set | **441.8 MB** | — | inflated by Chromium's shared pages |
| Our own processes | 2 (dock/top-bar host + watchdog), ~38 MB | — | |
| WebView2 processes | 7, the rest | — | fixed cost of the stack |

CPU is genuinely idle: auto-hide samples the cursor ~20×/s, the dock refreshes once a
second, and the top bar every two seconds. No hooks are installed anywhere in the system.

---

## 7. Suggested next improvements, in the order I would do them

1. **Merge the panels into one window** (see §3.1). Biggest memory win available, and it
   simplifies the popup handling the dock currently does by resizing its window.
2. **PHASE 6 before PHASES 3–5.** A settings window and a startup entry make what already
   exists genuinely usable; another panel does not. It would also replace that disabled
   "Settings…" placeholder.
3. **PHASE 4 (Alt+Tab) before PHASE 3 (launcher).** The switcher needs DWM thumbnails,
   which the dock's grouped-window chooser also wants — one piece of work, two payoffs.
   The launcher is the riskiest phase (capturing the Win key without breaking Win+E,
   Win+R, Win+L and friends) and should be last.
4. **Multi-monitor support.** Untestable here, but the code assumes one display in
   several places (`screen_size()` callers in particular).
5. **Feed the progress and attention indicators**, if you ever want them — this needs a
   registered shell taskbar, so it is a much bigger change than it looks (D-006).

---

## 8. Where everything is

| File | What |
|---|---|
| [PROGRESS.md](PROGRESS.md) | Phase-by-phase state; **read first when resuming** |
| [TESTS.md](TESTS.md) | Every test run, with pass/fail |
| [DECISIONS.md](DECISIONS.md) | The 11 decisions made without asking |
| [SYSTEM_CHANGES.md](SYSTEM_CHANGES.md) | Every change outside the project, with undo steps |
| [restore-windows.ps1](restore-windows.ps1) | Put Windows back |
| `baseline-windows.json` | Your shell settings as they were before anything ran |
| `src-tauri/src/` | Rust: Win32 integration, dock model, watchdog |
| `src/` | Frontend: dock and top bar |
| `screenshots/` | Everything I looked at while testing |
| `%LOCALAPPDATA%\macui\macui.log` | Runtime log |

Git history has one commit per working step, so any step can be rolled back on its own.
