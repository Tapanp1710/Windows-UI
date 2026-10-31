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
N/A — not started. Will verify Alt+Tab, Alt+Shift+Tab, Esc, Win+Tab, virtual desktops.

## PHASE 5 — Desktop widgets
N/A — not started.

## PHASE 6 — Settings, startup, packaging
Partial — settings *storage* works and is used by the dock; no settings UI, no startup
entry, no installer yet.
