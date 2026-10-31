# DECISIONS

Autonomous decisions made without asking, per the master prompt. Newest last.

---

## D-001 — Continue past the missing MSVC toolchain instead of stopping
**Date:** 2026-10-31 · **Phase:** STEP 0

STEP 0 says to stop if something critical is missing. MSVC build tools and the Windows
SDK are both missing, and installing them needs an admin/UAC prompt, which the safety
rules forbid.

Stopping outright would deliver nothing. A fully no-admin alternative exists: Rust's
`x86_64-pc-windows-gnu` target with a portable MinGW-w64 GCC. Both the Rust toolchain
and the GCC zip install per-user with no elevation.

**Decision:** attempt the GNU path. Treat it as the thing STEP 0 is testing. If a
release Tauri build cannot be produced with it, then stop as instructed and report the
one admin install needed.

**Reversible:** yes — delete two folders (see SYSTEM_CHANGES.md).

---

## D-002 — Install the toolchain outside the project folder
**Date:** 2026-10-31 · **Phase:** STEP 0

The brief says keep everything inside the project folder. But the project folder is
`OneDrive\Desktop\ui`, which OneDrive actively syncs. An extracted MinGW toolchain is
roughly 1.5 GB and `target/` build output is comparable; putting either there would
upload gigabytes of binaries to the user's OneDrive and could hit their storage quota.

**Decision:** install the toolchain to `%LOCALAPPDATA%\macui-toolchain` and let rustup
use its default `%USERPROFILE%\.cargo` / `.rustup`. All of it is recorded in
SYSTEM_CHANGES.md with exact undo steps. Project source, docs and config stay in the
project folder as instructed.

Also: `target/`, `node_modules/` and `screenshots/` are gitignored so OneDrive still
syncs the repo but git stays clean.

---

## D-003 — GCC 16.2.0 UCRT (WinLibs), posix threads, no LLVM
**Date:** 2026-10-31 · **Phase:** STEP 0

Picked the plain GCC build (276 MB) over the LLVM-bundled one (424 MB) — Rust supplies
its own LLVM, so the bundled copy is dead weight. UCRT over msvcrt because Windows 11
ships UCRT natively and it matches what Rust's gnu target expects.

---

## D-004 — Taskbar pins imported alphabetically, not in Windows' order
**Date:** 2026-10-31 · **Phase:** 1

Windows stores the taskbar pin *order* in an undocumented binary registry blob
(`...\Taskband\FavoritesResolve`) whose layout changes between Windows versions. The
pins themselves are plain `.lnk` files in a folder, which is a documented, stable
place to read.

**Decision:** import the shortcuts and sort them alphabetically rather than reverse
engineer the blob. The dock supports drag-to-reorder and persists the result, so the
user fixes the order once and it sticks. Recorded as a known limitation.

---

## D-005 — No real jump lists in the dock's right-click menu
**Date:** 2026-10-31 · **Phase:** 1

The brief asks for "the app's Windows jump list items where possible". Reading another
application's jump list means parsing the undocumented `automaticDestinations-ms`
OLE compound streams in the user's Recent folder, per-app, with no supported API.

**Decision:** ship the parts of the menu that can be done correctly — launch, pin,
unpin, close window(s) — and leave real jump lists out rather than ship a fragile
half-working parser. "Where possible" is doing the work here. Logged in the report.

---

## D-006 — Progress and attention indicators are rendered but not yet fed
**Date:** 2026-10-31 · **Phase:** 1

An app reports download progress and attention state by calling `ITaskbarList3` on the
shell's taskbar. Receiving those notifications means *being* the shell's taskbar — a
registered taskbar replacement — not merely drawing one. A normal user-level app
cannot subscribe to them.

**Decision:** implement the visuals (thin progress bar with normal/indeterminate/
error/paused states, gentle attention glow + bounce) so the design is complete and the
hook-up is a small change later, but leave them unfed for now. Doing it properly needs
a shell hook that is well beyond "only the visuals change".

---

## D-007 — Popup headroom instead of separate popup windows
**Date:** 2026-10-31 · **Phase:** 1

A transparent window clips anything drawn outside its rect, so tooltips and context
menus above the dock were invisible. Two options: give each popup its own always-on-top
window (correct, more machinery), or grow the dock window upward only while a popup is
open.

**Decision:** grow-on-demand. The dock window is always-on-top, so every reserved pixel
is a pixel that stops clicks reaching what is underneath — which would be a real change
to Windows behaviour. Granting the space only while a popup needs it, and giving it back
immediately, keeps that window of interference to the moments the user is already
pointing at the dock. If later phases need richer popups, promoting them to real windows
is the upgrade path.

---

## D-008 — Start button opens the real Windows Start menu for now
**Date:** 2026-10-31 · **Phase:** 1

PHASE 3 owns the launcher, but the Start button had to do something correct in the
meantime. It synthesises Ctrl+Esc via `SendInput`, which is the documented way to ask
the shell to open Start.

This *sends* a keystroke, it does not intercept or remap one, so no existing Windows
shortcut is affected — which is the distinction the project's core rule turns on.

---

## D-009 — No system Acrylic; higher-opacity surfaces instead
**Date:** 2026-10-31 · **Phase:** 2

The brief asks for Acrylic/Mica "where supported", falling back to semi-transparent
surfaces. Measured behaviour: `backdrop-filter: blur()` on a *transparent* Win32 window
blurs only what the page itself draws behind the element, not the desktop — so the
first build showed sharp VS Code rectangles straight through the top bar.

Real Acrylic (`DwmExtendFrameIntoClientArea` / `SetWindowCompositionAttribute`) fills
the whole **window rectangle**. Our surfaces are rounded islands inside a larger
transparent window, so acrylic would paint a hard rectangle around every pill and the
dock. Clipping it with `SetWindowRgn` is possible but costs the drop shadow and gives
aliased corners, because a region has no antialiasing.

**Decision:** take the documented fallback and raise surface opacity (dark 0.90, light
0.88) so the islands read cleanly on any background. This keeps the exact geometry the
brief specifies — 16px radius, 1px light border, soft shadow, antialiased corners —
which it is far more specific about than the blur. Noted for the user's visual check.

---

## D-010 — Work area reserved by a silent SPI_SETWORKAREA, not an appbar
**Date:** 2026-10-31 · **Phase:** 2

The top bar is permanently visible, so it must reserve screen space or maximised
windows slide underneath it. Three approaches were tried:

1. `SPI_SETWORKAREA` **with** `SPIF_SENDCHANGE` — did not stick. Broadcasting
   `WM_SETTINGCHANGE` makes the shell recompute the work area from its registered
   appbars and discard ours. Measured: reverted within a second.
2. Registering a real **appbar** (`SHAppBarMessage`) — this reserved the strip
   correctly, but it also made the shell take charge of its own bars again and
   **un-hide the Windows taskbar**, which defeats the entire point. It additionally
   moved our window below its own reserved strip.
3. `SPI_SETWORKAREA` **without** the broadcast — sticks, and the taskbar stays hidden.
   Maximised windows still honour it because they query the work area as they maximise.

**Decision:** option 3. Because a silent write has no notification to hook, the existing
auto-hide sampler re-asserts it roughly every 2.5 s (a single cheap query) in case a
display change or shell event resets it. `src/win/appbar.rs` is left in the tree,
unused, as a working reference if a future phase needs genuine appbar semantics.

---

## D-011 — Frontend asks for auto-hide state at startup
**Date:** 2026-10-31 · **Phase:** 2

The sampler emits `dock:reveal` / `dock:conceal` only on a *change*. Anything emitted
before the webview has attached its listener is lost permanently. If the pointer
happened to be at the bottom edge when the app launched, the one reveal was emitted
too early and the dock stayed concealed until the user moved the pointer away and back.

**Decision:** keep the events for liveness, but have the dock call `cmd_dock_revealed`
once after attaching its listeners and sync to the real state. Events are a
notification channel, not a source of truth.

---

## D-012 — The settings window is created on demand, not declared in the config
**Date:** 2026-10-31 · **Phase:** 6

Every live window is a separate WebView2 renderer process: measured, roughly 25 MB of
private bytes and 60 MB of working set each. Declaring the settings window in
`tauri.conf.json` alongside the dock and the top bar would have made that cost
permanent, for a window that is open for a minute a month.

**Decision:** build it at runtime in `open_settings()` and let closing it destroy it.
Idle memory is unchanged by this phase (measured: 200.4 MB private, against 210.6 MB
before it existed — i.e. inside the noise). The capability file already listed
`settings` in its window list, so no permission change was needed.

**Consequence worth knowing:** window creation must happen in an **async** command.
A synchronous command runs on the main thread inside the WebView2 callback that
delivered the IPC message, and creating a second webview from there leaves the new
controller half-built — the window exists, its WebView2 child is 0x0 and hidden, and
nothing ever paints, with no error anywhere. That cost an hour; it is now a comment on
`cmd_open_settings`.

---

## D-013 — Startup entry is a Startup-folder shortcut, not a registry Run value
**Date:** 2026-10-31 · **Phase:** 6

Both are per-user and need no admin rights. `HKCU\...\Run` is one API call; the
shortcut needs `IShellLinkW` + `IPersistFile`, which is about twenty more lines.

**Decision:** the shortcut, for three reasons. It keeps the claim in
SYSTEM_CHANGES.md that **this app writes no registry keys at all**, which is worth more
than twenty lines. It is visible in Explorer and removable by deleting one file, so the
user is not dependent on our settings window to undo it. And the presence of the file
is the single source of truth, so disabling the entry in Task Manager or deleting it by
hand is reflected in the settings window rather than contradicted by it.

---

## D-014 — The settings card is opaque, breaking the glass look on purpose
**Date:** 2026-10-31 · **Phase:** 6

`backdrop-filter` cannot blur the desktop through a transparent window (D-009), so
every surface in this project is translucent over an *unblurred* background. At the
dock's size that reads as glass. At a settings window's size it reads as a page of
text with another application's text showing through it — verified by screenshot
against a code editor, and plainly unusable.

**Decision:** a new `--surface-solid` token, opaque, used by the settings card only.
The border, radius, shadow and spacing are unchanged, so it still belongs to the same
family. The brief's "glass surfaces" is a look for chrome you glance at, not a reason
to make a window you have to read 12% transparent.

---

## D-015 — "Return to normal Windows" restores the wallpaper; "Quit" does not
**Date:** 2026-10-31 · **Phase:** 6

The wallpaper is the only persistent Windows setting this project changes, and only
when the user presses a button asking for it.

**Decision:** Quit is "stop the UI layer" and leaves a wallpaper the user deliberately
chose. "Return to normal Windows" is "undo everything", and puts the recorded original
back first. `restore-windows.ps1` does the same, reading the recorded path from
`settings.json` and clearing it afterwards so a second run cannot fight a wallpaper the
user has since chosen for themselves.

---

## D-016 — The tray icon is kept, and is not the primary route to settings
**Date:** 2026-10-31 · **Phase:** 6

The brief asks for a tray icon offering Settings / Restart UI / Quit. It exists and it
works — but the notification area is part of the Windows taskbar this app hides, so
while the UI layer is running the icon cannot be clicked. Making it reachable would
mean implementing our own notification area, i.e. becoming the registered shell
taskbar, which is the same wall as D-006.

**Decision:** build the tray icon as asked (it is reachable if the app ever runs with
the taskbar visible, e.g. when the watchdog fails to start and the app refuses to hide
it), and give the identical menu two reachable homes: right-click the dock background,
and right-click any top-bar pill. Nothing is lost and nothing is faked.
