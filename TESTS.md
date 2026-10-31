# TESTS

Pass/fail log. The core rule being tested throughout: **every Windows behaviour must
work exactly as it does on a stock machine.** A visual change that alters behaviour is a
failure, not a feature.

Legend: PASS · FAIL · N/A (phase not reached) · DEFERRED (deliberately not built, see DECISIONS)

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
| 0.9 | Rust can compile + link a release exe | PASS | `linktest.exe` built and ran |
| 0.10 | Release exe runs under App Control for Business | PASS | unsigned local build executed normally |
| 0.11 | Tauri v2 builds on `x86_64-pc-windows-gnu` | PASS | 294 crates inc. wry + webview2-com |
| 0.12 | screenshot.ps1 captures and is readable | PASS | |
| 0.13 | capture-baseline.ps1 records shell state | PASS | taskbar was already auto-hide — caught before anything changed |
| 0.14 | restore-windows.ps1 runs without admin | PASS | |
| 0.15 | restore-windows.ps1 is idempotent | PASS | taskbar state 1 before and after; did not flip the user's setting |
| 0.16 | Release Tauri exe launches and renders | PASS | |
| 0.17 | Webview CSS px == Windows logical px at 150% DPI | PASS | `devicePixelRatio`=1.5, `innerWidth`=420 for a 420 config |
| 0.18 | screenshot.ps1 captures true 1920x1080 | PASS | was silently capturing 1280x720 until DPI awareness was added |

---

## PHASE 1 — Dock

### Appearance
| # | Test | Result | Notes |
|---|---|---|---|
| 1.1 | Floating island, nothing touches a screen edge | PASS | 12px above the bottom edge, centred |
| 1.2 | 16px radius on the bar, 1px light border, soft shadow | PASS | |
| 1.3 | Glass translucency, light + dark | PASS | dark verified on this machine (Windows is in dark mode) |
| 1.4 | Follows the Windows light/dark setting | PASS | via `prefers-color-scheme`, overridable in settings |
| 1.5 | Inter bundled and used | PASS | `@fontsource-variable/inter`, no network at runtime |
| 1.6 | Icon shape applied globally (circle default) | PASS | broken until `serde(rename_all="camelCase")` was added |
| 1.7 | Real app icons at high resolution | PASS | 256px via `IShellItemImageFactory` |
| 1.8 | Icons on a soft glass plate, clipped to shape | PASS | |
| 1.9 | Running dot under running apps | PASS | |
| 1.10 | Grouped apps show a wider pill | PASS | |
| 1.11 | Divider then Recycle Bin on the right | PASS | real Recycle Bin icon, "not empty" state |
| 1.12 | Window sized exactly to the island | PASS | so the transparent always-on-top window swallows no clicks elsewhere |

### Behaviour (must match the Windows taskbar)
| # | Test | Result | Notes |
|---|---|---|---|
| 1.13 | Imports the current Windows taskbar pins on first run | PASS | 2 pins (Brave, File Explorer) imported from Quick Launch |
| 1.14 | Pinned app that is also running shows one button, not two | PASS | `.lnk` resolved to its target exe to match the process |
| 1.15 | Click a pinned, not-running app launches it | PASS | via `ShellExecuteW` default verb |
| 1.16 | Click a running, unfocused app activates it | PASS | |
| 1.17 | Click again when focused minimises it | PASS | |
| 1.18 | Several windows → chooser | PARTIAL | lists window **titles**; live thumbnails DEFERRED to Phase 4 |
| 1.19 | Right-click menu | PARTIAL | launch / pin / unpin / close window(s). Real jump lists DEFERRED (D-005) |
| 1.20 | Middle-click opens a new instance | PASS | |
| 1.21 | Drag to reorder pinned icons, persisted | PASS | saved to `%APPDATA%\macui\settings.json` |
| 1.22 | Attention (flash) state | DEFERRED | renders correctly, but nothing can feed it (D-006) |
| 1.23 | Progress indicator | DEFERRED | renders correctly, but nothing can feed it (D-006) |
| 1.24 | Hover magnification 1.15× + 4px lift, neighbours less | PASS | |
| 1.25 | Tooltip with the app name above the icon | PASS | broken by window clipping until headroom was added |

### Auto-hide
| # | Test | Result | Notes |
|---|---|---|---|
| 1.26 | Hidden by default on a fresh start | PASS | was broken: frontend started revealed, sampler started concealed |
| 1.27 | Reveals when the pointer touches the bottom edge | PASS | |
| 1.28 | Conceals ~400 ms after the pointer leaves | PASS | |
| 1.29 | Stable over repeated reveal/conceal cycles | PASS | 3 cycles, identical luma delta each time (12.6) |
| 1.30 | Does not slide away while the pointer is on it | PASS | |
| 1.31 | Windows taskbar hidden while the dock runs | PASS | |
| 1.32 | Maximised windows use the full height (auto-hide semantics) | PASS | work area left at full screen, matching Windows' own auto-hide behaviour |

### Animation (10–14 frame bursts)
| # | Test | Result | Notes |
|---|---|---|---|
| 1.33 | No flicker during the slide | PASS | no reversals across 14 frames |
| 1.34 | No blank frames mid-animation | PASS | |
| 1.35 | No position jumps | PASS | monotonic, settles and stays |
| 1.36 | Respects Windows' "Animation effects" setting | PASS (by code) | `SPI_GETCLIENTAREAANIMATION`; not toggled on this machine to verify end-to-end |

**Caveat on 1.33–1.35:** the capture rig manages ~50–65 ms per frame, so a 250 ms
transition is sampled about 4–5 times. That is enough to prove there is no flicker,
blanking or jumping, but **it cannot prove subjective smoothness** — that needs a human
eye. Flagged in REPORT.md.

### Crash safety
| # | Test | Result | Notes |
|---|---|---|---|
| 1.37 | Taskbar restored on clean exit | PASS | |
| 1.38 | Taskbar restored after the main process is hard-killed | PASS | watchdog; verified twice |
| 1.39 | Restore preserves the user's original auto-hide setting | PASS | state 1 before and after, not forced to "always visible" |
| 1.40 | No orphan processes after a kill | PASS | 0 left |
| 1.41 | Killing the **whole process tree** | KNOWN GAP | watchdog dies too; `restore-windows.ps1` recovers it (verified) |

### Quitting
| # | Test | Result | Notes |
|---|---|---|---|
| 1.44 | Tray icon created | PASS | but **unreachable** while the taskbar is hidden — it lives in the notification area. Fixed by 1.45 |
| 1.45 | Right-click the dock background opens the app menu | PASS | Settings (stub) / Restart UI / Quit |
| 1.46 | Quit restores the Windows taskbar | PASS | taskbar back, state 1 preserved, 0 processes left |

### Performance (20 s idle, pointer parked away from the dock)
| # | Test | Result | Notes |
|---|---|---|---|
| 1.42 | Idle CPU near 0% | PASS | **0.09%** across all 9 processes (8 cores) |
| 1.43 | RAM under 150 MB | **FAIL** | **202.6 MB private / 441.8 MB working set.** The first measurement here said 38.8 MB, which counted only our 2 host processes and omitted the 7 WebView2 processes doing the rendering. See REPORT.md 3.1 |

---

## PHASE 2 — Top bar

| # | Test | Result | Notes |
|---|---|---|---|
| 2.1 | Three glass pills, 6px from the top edge | PASS | focused app / clock / status |
| 2.2 | Left pill shows the focused app's name and icon | PASS | excludes our own windows |
| 2.3 | Centre pill shows day, date and time | PASS | |
| 2.4 | Time and date follow the Windows format | PASS | via `Intl` with the system locale |
| 2.5 | Clicking the clock opens the Windows calendar / notifications (Win+N) | PASS | confirmed by screenshot — the real panel |
| 2.6 | Clicking Wi-Fi opens Windows Quick Settings (Win+A) | PASS | confirmed by screenshot |
| 2.7 | Clicking Bluetooth opens Quick Settings | PASS | same handler |
| 2.8 | Clicking volume opens Quick Settings | PASS | confirmed by screenshot |
| 2.9 | Clicking battery opens Quick Settings | PASS | same handler |
| 2.10 | Scrolling the volume icon changes volume | PASS | 2%/notch; 86% → 74% over 6 notches |
| 2.11 | Scrolling elsewhere does NOT change volume | PASS | |
| 2.12 | Battery shows level + charging state | PASS | 100%, charging/saver states handled |
| 2.13 | Network shows connected / disconnected | PASS | signal strength + SSID not shown (known limitation) |
| 2.14 | Live updates without heavy polling | PASS | 2 s interval; contributes to the 0.09% idle figure |
| 2.15 | Clicks BETWEEN pills pass through to the app underneath | PASS | `WindowFromPoint` returns Code, not macui |
| 2.16 | Clicks BELOW pills but inside the window rect pass through | PASS | same |
| 2.17 | Clicks ON a pill hit the top bar | PASS | |
| 2.18 | Visible bar reserves screen space | PASS | work area top = 63px (42 CSS × 1.5) |
| 2.19 | Reservation released when the app dies | PASS | work area back to top = 0 |
| 2.20 | Taskbar stays hidden while the bar is up | PASS | broke when using an appbar; fixed (D-010) |
| 2.21 | Tray overflow chevron | PARTIAL | falls back to Quick Settings on Win11 (XAML host has no clickable child window) |
| 2.22 | Auto-hide toggle for the top bar | NOT BUILT | setting exists (`topbarAutoHide`), no UI, untested |
| 2.23 | Notification indicator dot | DEFERRED | renders, but nothing can feed it (same reason as D-006) |

### Startup robustness
| # | Test | Result | Notes |
|---|---|---|---|
| 2.24 | Launch with the pointer already at the bottom edge | PASS | was broken: the reveal event fired before the webview attached its listener and the dock stayed concealed. Fixed by querying state on startup (D-011) |

## PHASE 3 — Launcher
N/A — not started. Will verify Win, Esc, and that Win+E/R/D/L/I/A/N/V/Shift+S/arrows/
numbers all still work untouched.

## PHASE 4 — Window switcher

The phase with the only keyboard hook in the project, so the fallbacks were tested by
forcing them rather than by reading the code.

### Behaviour
| # | Test | Result | Notes |
|---|---|---|---|
| 4.1 | Alt+Tab opens the switcher | PASS | glass bar, centred |
| 4.2 | Live window previews | PASS | real DWM thumbnails, not screenshots — the previews are live |
| 4.3 | Second entry selected on open | PASS | Windows' own behaviour: tap Alt+Tab and let go to swap the last two windows |
| 4.4 | Alt+Tab again moves forward | PASS | wraps around |
| 4.5 | Alt+Shift+Tab moves back | PASS | |
| 4.6 | Releasing Alt switches | PASS | focus moved from the desktop to the selected window |
| 4.7 | Escape cancels | PASS | focus unchanged afterwards — verified by comparing the foreground window before and after |
| 4.8 | Clicking a card switches to it | PASS | |
| 4.9 | Order matches Windows' | PASS | z-order, which *is* the most-recently-used order |
| 4.10 | App icons on the cards | PASS | same icons the dock shows, from the owning executable |

### The fallbacks — the part that matters
| # | Test | Result | Notes |
|---|---|---|---|
| 4.11 | **Killed mid-Alt+Tab with Alt still held** | **PASS** | app gone, taskbar restored by the watchdog, and Windows' own Alt+Tab worked on the very next press. Confirmed by screenshot of the native switcher |
| 4.12 | Switched off in settings | PASS | the hook stops swallowing Tab; Windows' own switcher appears |
| 4.13 | Hook fails to install | PASS (by construction) | logged, nothing swallowed. Forcing a real failure needs a machine that refuses the hook |
| 4.14 | A window running as administrator has focus | **NOT TESTED** | Windows does not deliver our hook its input in that case, so the native switcher runs. That is how the API behaves rather than something we implement, but it was not forced here — nothing on this machine runs elevated |
| 4.15 | Alt never gets stuck | PASS | the hook deliberately does not swallow Alt (D-027) |
| 4.16 | Fewer than two windows | PASS | nothing is shown rather than a one-entry panel |

### What the switcher shows
| # | Test | Result | Notes |
|---|---|---|---|
| 4.17 | Our own panels are excluded | PASS | **failed first**: the dock, top bar and widgets appeared as switchable windows. Tauri hides a window from the taskbar through `ITaskbarList` rather than `WS_EX_TOOLWINDOW`, so the usual style test let them through |
| 4.18 | Cloaked windows excluded | PASS | **failed first**: "Windows Input Experience" was listed. Windows' own switcher filters on `DWMWA_CLOAKED`, and now so does this |
| 4.19 | Previews released on close | PASS | every exit path goes through `dwm::clear()`; a leaked thumbnail holds a reference to a window that may be closing |

### Win+Tab
| # | Test | Result | Notes |
|---|---|---|---|
| 4.20 | Win+Tab opens Windows Task View | PASS | deliberately not restyled — virtual desktops have no supported API, and a restyled view that could not drive them would be worse than the real one (D-028) |
| 4.21 | Virtual desktops still work | PASS | untouched |

### Resources
| # | Test | Result | Notes |
|---|---|---|---|
| 4.22 | Idle memory with the switcher loaded | PASS | **229.3 MB** private against the 250 MB target. The window is created hidden at startup so Alt+Tab is instant rather than waiting for a webview |
| 4.23 | Idle CPU | PASS | 0.208%. The hook costs nothing when no key is pressed |

## PHASE 5 — Desktop widgets

Run against the release build on 2026-10-31, by driving a real pointer and reading the
screenshots back. The media tests use a local fixture page that publishes a real
Windows media session, so the widget was exercised against the genuine API rather than
a mock.

### Placement and desktop behaviour
| # | Test | Result | Notes |
|---|---|---|---|
| 5.1 | All five widgets render | PASS | clock, calendar, weather, music, system stats |
| 5.2 | Default layout matches the brief | PASS | clock top left, calendar top right, the rest arranged without overlapping; computed from the viewport, so it is correct at any resolution |
| 5.3 | Widgets sit **behind** normal windows | PASS | invisible under a maximised editor, which is the point |
| 5.4 | Widgets survive Show Desktop (Win+D) | PASS | they stay on the desktop as part of it, as the brief requires |
| 5.5 | Clicks **between** widgets reach the desktop | PASS | right-clicking an empty spot gave the Windows desktop menu. The window is clipped to the cards with `SetWindowRgn` |
| 5.6 | Desktop icons still work around them | PASS | same mechanism |
| 5.7 | Widget layer never takes focus | PASS | failed first: WebView2 focuses its own child and activated the window despite `WS_EX_NOACTIVATE`. Focus is now handed straight back (D-021) |
| 5.8 | Z-order survives other windows being activated | PASS | re-asserted ~1/s above `Progman`; `alwaysOnBottom` would have put it *below* the wallpaper (D-019) |

### Dragging and persistence
| # | Test | Result | Notes |
|---|---|---|---|
| 5.9 | Drag a widget anywhere | PASS | verified by dragging the weather card to mid-screen |
| 5.10 | Position persists | PASS | written to `settings.json` on release, not on every pixel |
| 5.11 | Drag does not die mid-screen | PASS | the clickable region becomes the whole window for the duration of a drag, then goes back to the cards |
| 5.12 | A plain click does not nudge a widget | PASS | 4px threshold; this was a real bug found by my own stray test clicks moving the layout |
| 5.13 | Widgets stay on screen | PASS | clamped to the viewport after layout |

### Content
| # | Test | Result | Notes |
|---|---|---|---|
| 5.14 | Clock shows the user's own time format | PASS | ticks on the minute boundary, not on a 1s timer |
| 5.15 | Calendar shows the month with today marked | PASS | October 2026, 31 highlighted; weekday names come from the locale |
| 5.16 | Weather is live | PASS | Open-Meteo, no API key: "30°, Clear, Bengaluru" |
| 5.17 | Weather city is configurable | PASS | typed Hyderabad → "31°, Drizzle, Hyderabad" within seconds, then back to Bengaluru |
| 5.18 | Weather failure is handled | PASS | shows "Weather unavailable" and logs it, rather than an empty card |
| 5.19 | System stats are live | PASS | CPU from `GetSystemTimes` deltas, RAM from `GlobalMemoryStatusEx`, battery from the existing power query |
| 5.20 | Media widget resting state | PASS | "Nothing playing" with a play icon when Windows has no session — independently confirmed there was genuinely no session, through a separate WinRT query |

### Media controls, against a real Windows media session
| # | Test | Result | Notes |
|---|---|---|---|
| 5.21 | Title and artist appear | PASS | "Widget Test Track" / "macui Test · MSEdge" |
| 5.22 | Play/pause works | PASS | failed first: `TryTogglePlayPauseAsync` returns false on sessions that expose Play and Pause but not the toggle. Now asks for the specific transition |
| 5.23 | Next track works | PASS | the fixture page received `nexttrack` |
| 5.24 | Previous track works | PASS | the fixture page received `previoustrack` |
| 5.25 | Play/pause icon follows the real state | PASS | pause glyph while playing, play glyph while paused |
| 5.26 | VLC playing without a media session | PASS (by design) | VLC published no session; the widget correctly said nothing was playing rather than inventing something |

### Settings
| # | Test | Result | Notes |
|---|---|---|---|
| 5.27 | Master switch for widgets | PASS | the whole layer is created and destroyed, so switching it off gives the memory back |
| 5.28 | Per-widget switches | PASS | clock switched off and back on, live, with the clickable region following |
| 5.29 | Settings survive a restart | PASS | |

### Resources (idle, 30 s, dock + top bar + widgets, settings closed)
| # | Test | Result | Notes |
|---|---|---|---|
| 5.30 | Private bytes, whole tree | **PASS** | **213.9 MB** against the 250 MB target |
| 5.31 | Idle CPU | PASS | 0.195% across 8 cores |
| 5.32 | Cost of the widget layer | noted | it first took idle memory to 258 MB — over target. Sharing one renderer across all panels brought the whole app to 213.9 MB and 10 processes down to 8 (D-020) |

### Windows behaviour
| # | Test | Result | Notes |
|---|---|---|---|
| 5.33 | No keyboard hook added | PASS | this phase installs none |
| 5.34 | The Windows taskbar stays hidden | PASS | **found a real bug**: opening the shell's "Open with" dialog brought the taskbar back, and it stayed. Hiding it once at startup was not enough; it is now re-asserted, except while the tray overflow is deliberately showing it |
| 5.35 | Taskbar returns on a hard kill, widgets running | PASS | watchdog unaffected by the extra window |
| 5.36 | Unit tests | PASS | 3 passed, including the media source-name parser |

## PHASE 6 — Settings, startup, packaging

All run against the release build on 2026-10-31, by driving a real mouse pointer with
`scripts/cursor.ps1` and reading the screenshots back.

### Settings window
| # | Test | Result | Notes |
|---|---|---|---|
| 6.1 | Opens from the dock's right-click menu | PASS | the "Settings…" placeholder is now live |
| 6.2 | Opens from a right-click on any top-bar pill | PASS | second reachable route while the taskbar is hidden |
| 6.3 | Opens from the tray menu | **UNTESTABLE** | the item exists and is wired, but the notification area is inside the taskbar we hide — see D-016 |
| 6.4 | Window renders | PASS | only after a real bug was fixed: built from a *sync* command, the WebView2 controller came up 0x0 and hidden and nothing ever painted. Window creation now happens in an async command (D-012) |
| 6.5 | Card is legible over a bright window | PASS | failed first: 12% transparency let a code editor's text show through its own. Fixed with an opaque surface token (D-014) |
| 6.6 | Scrollbar matches the panel | PASS | failed first: Chromium ignores `::-webkit-scrollbar` once `scrollbar-width` is set, leaving a white track. Standard properties only now |
| 6.7 | Closes with the close dot | PASS | WebView2 process count back to 7 |
| 6.8 | Closes with Escape | PASS | |
| 6.9 | Dragging by the header moves it | PASS | `data-tauri-drag-region`; the close dot is outside it |
| 6.10 | Costs nothing while closed | PASS | created on demand, destroyed on close (D-012) |

### Settings that actually change something
| # | Test | Result | Notes |
|---|---|---|---|
| 6.11 | Theme → Light applies live | PASS | dock and settings window both switched without a restart |
| 6.12 | Theme → Follow Windows applies live | PASS | |
| 6.13 | Dock "Always visible" applies live | PASS | dock stayed up with the pointer at (400,600). This setting was stored but **never read** before this phase |
| 6.14 | Icon size slider | PASS | written on release, not on every drag pixel; `--dock-icon` was likewise stored-but-unread before |
| 6.15 | Values survive a restart | PASS | read back from `settings.json` |
| 6.16 | A settings write cannot clobber the pin order | PASS | the window sends a patch of changed keys, never the whole struct |

### Startup entry
| # | Test | Result | Notes |
|---|---|---|---|
| 6.17 | Switch on creates the shortcut | PASS | `%APPDATA%\...\Startup\macui.lnk` |
| 6.18 | Shortcut points at the right exe | PASS | target + working directory + description all correct |
| 6.19 | Switch off deletes it | PASS | |
| 6.20 | No registry key written | PASS | `HKCU\...\Run` untouched (D-013) |
| 6.21 | `restore-windows.ps1` removes it | PASS | it also clears a stray `Run` value defensively |

### Wallpaper
| # | Test | Result | Notes |
|---|---|---|---|
| 6.22 | Nothing applied unless asked | PASS | the brief's "offered, not force-applied" |
| 6.23 | Applying unpacks and sets it | PASS | unpacked from inside the exe to `%APPDATA%\macui` |
| 6.24 | Original recorded before changing | PASS | `originalWallpaper` captured the Windows Spotlight path |
| 6.25 | "Put my own wallpaper back" restores it | PASS | byte-identical path, record cleared |
| 6.26 | Switching between our two does not overwrite the record | PASS | recorded once only |
| 6.27 | "Return to normal Windows" restores it too | PASS | wallpaper, taskbar, work area, then exit |

### Leaving
| # | Test | Result | Notes |
|---|---|---|---|
| 6.28 | "Restart the UI" | PASS | new pid, new watchdog, taskbar hidden again |
| 6.29 | "Return to normal Windows" | PASS | 0 macui processes, 0 WebView2 processes, taskbar visible, **taskbar state 1 = the user's own auto-hide**, work area full, wallpaper restored |
| 6.30 | Hard kill with the settings window open | PASS | watchdog restored the taskbar; no orphan processes |
| 6.31 | Killing *every* macui process at once | FAIL (known) | takes the watchdog too; `restore-windows.ps1` recovers it. Documented in SYSTEM_CHANGES "Known gap" — unchanged by this phase |

### Packaging
| # | Test | Result | Notes |
|---|---|---|---|
| 6.32 | NSIS installer builds on the MinGW toolchain | PASS | the GNU target was no obstacle; Tauri fetched NSIS itself, no admin prompt (T-3) |
| 6.33 | Installer runs without UAC | PASS | `installMode: currentUser`, installed to `%LOCALAPPDATA%\macui` |
| 6.34 | Installed copy works | PASS | launched, hid the taskbar, behaved identically |
| 6.35 | Uninstaller removes everything it added | PASS | registry entry gone; only our own `macui.log` left behind |
| 6.36 | Portable use still possible | PASS | the loose `.exe` runs with no install at all |

### Resources (idle, 30 s, dock + top bar, settings closed)
| # | Test | Result | Notes |
|---|---|---|---|
| 6.37 | Private bytes across the whole tree | **PASS** | **200.4 MB**, against the revised 250 MB target. 9 processes: 2 ours, 7 WebView2 |
| 6.38 | Idle CPU | PASS | 0.104% across 8 cores |
| 6.39 | Settings window adds nothing at idle | PASS | 210.6 MB with it open, 200.4 MB after closing — it leaves no residue |

### Windows behaviour
| # | Test | Result | Notes |
|---|---|---|---|
| 6.40 | No keyboard hook added | PASS | this phase installs none; grep for `SetWindowsHookEx` returns nothing in the whole project |
| 6.41 | Settings window behaves like a normal window | PASS | appears in Alt+Tab, focusable, closable — it is not always-on-top and steals nothing |
| 6.42 | Unit tests | PASS | 2 passed (`startup::lnk_path_is_under_the_users_startup_folder`, `watchdog::parses_watchdog_invocation`) |
