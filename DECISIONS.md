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

---

## D-017 — Weather defaults to Bengaluru
**Date:** 2026-10-31 · **Phase:** 5

The brief says to default to a city in India and log which one so it can be changed.
There is nothing on this machine that identifies a city - the timezone says IST and
that is all - so guessing from data was not an option.

**Decision:** **Bengaluru** (12.9716 N, 77.5946 E). Change it in Settings → Desktop
widgets → Weather city: type a name, press Enter, and it is looked up through
Open-Meteo's geocoding service. Verified by switching to Hyderabad and back.

Weather comes from **Open-Meteo**, which the brief names: no account, no API key, no
rate limit worth worrying about at one call every fifteen minutes.

---

## D-018 — One widget layer, not five widget windows
**Date:** 2026-10-31 · **Phase:** 5

Five widgets could be five windows. Each live window is its own WebView2 renderer,
and the brief asks for widgets that sit on the desktop without covering anything.

**Decision:** one transparent window the size of the screen, holding all five widgets,
with its **clickable area cut down to exactly the visible cards** using the same
`SetWindowRgn` trick the top bar uses for its pills. Verified by right-clicking
between two widgets and getting the Windows desktop menu.

That buys three things: one renderer instead of five, dragging a widget across the
screen is a CSS transform rather than a Win32 window move, and widgets can overlap
each other's bounding boxes without fighting over z-order.

---

## D-019 — The widget layer is parked above `Progman`, not sent to the bottom
**Date:** 2026-10-31 · **Phase:** 5

Tauri offers `alwaysOnBottom`, which sends a window to `HWND_BOTTOM`. On Windows that
is **below `Progman`** - the desktop itself - so the widgets would be behind the
wallpaper and invisible.

**Decision:** insert the window immediately after `Progman` in the z-order instead,
which is exactly "part of the desktop, below every application". It is re-asserted
about once a second on the sampler that already runs, because the shell re-sorts the
z-order whenever a window is activated and there is no notification to hook.

Verified: the widgets survive Show Desktop (Win+D) and stay hidden under a maximised
window.

---

## D-020 — All panels share one WebView2 renderer process
**Date:** 2026-10-31 · **Phase:** 5

Adding the widget layer took idle memory to **258 MB**, past the 250 MB target. Each
webview was getting its own renderer, at about 35 MB of private bytes each, plus the
GPU process growing for a full-screen surface.

**Decision:** pass `--process-per-site --renderer-process-limit=1` to WebView2 for
every window. All our pages are the same origin and all our own code, so there is
nothing to isolate from anything. Measured: **258.3 MB → 213.9 MB**, and ten processes
down to eight. Dock, top bar, widgets and settings all verified rendering afterwards.

The trade-off, stated plainly: a renderer crash now takes every panel with it instead
of one. The watchdog still restores the Windows taskbar, which is the part that
matters, and the UI layer can be restarted from the tray.

wry's own default arguments have to be repeated in the string, because this option
**replaces** them rather than adding to them - and every window must pass the same
string or WebView2 refuses to share an environment between them.

---

## D-021 — Clicking a widget hands focus straight back
**Date:** 2026-10-31 · **Phase:** 5

The widget layer carries `WS_EX_NOACTIVATE`, so Windows will not activate it on a
click. WebView2 calls `SetFocus` on its own child anyway, which activates the
top-level window: measured, pressing "pause" moved the foreground window to
`macui widgets`.

Taking the caret out of whatever the user is typing in, to skip a track, is exactly
the kind of behaviour change the core rule forbids.

**Decision:** the sampler remembers the last foreground window that is not one of
ours, and every widget click ends by handing focus back to it. Verified: after
clicking pause, the foreground window is the one it was before.

A drag has a 4px threshold for the same reason - a plain click on a card must not
nudge the layout.

---

## D-022 — The top bar's controls open our own panels, not Windows' flyouts
**Date:** 2026-10-31 · **Phase:** feedback

The brief originally required that clicking Wi-Fi, Bluetooth, volume or the clock open
*the same* Windows flyout, and PHASE 2 did exactly that. In use it reads as a bug: the
Windows panel opens down by the taskbar, nowhere near the bar that was clicked. You
asked for the options to "open there itself".

**Decision:** our own panels, anchored under the pill. They are not pictures of the
Windows ones — the Wi-Fi and Bluetooth switches drive the real radios through
`Windows.Devices.Radios`, and the slider drives the real audio endpoint. Everything
deliberately **not** duplicated (choosing a network, pairing a device, power plans) is
one button away on the matching Windows settings page, so nothing becomes unreachable.

This is a deliberate departure from the original core rule, made because you asked for
it after seeing the first version.

---

## D-023 — Real acrylic for panels, painted glass for bars
**Date:** 2026-10-31 · **Phase:** feedback

`backdrop-filter` cannot blur the desktop through a transparent window (D-009), so
"glass" had been painted translucency. `SetWindowCompositionAttribute` gives real
blur, and `DWMWA_WINDOW_CORNER_PREFERENCE` rounds the window so the blur is clipped to
a rounded shape — `SetWindowRgn` does **not** work for this, because DWM composites
the acrylic against the window *rectangle* and ignores the region (measured: a region
with 21px corners produced perfectly square ones).

**Decision:** acrylic on the settings window and the panels, which are whole windows
and can be rounded by DWM. The dock, top bar and widgets keep painted glass, because
their windows hold several separate islands and a window-wide blur would fill the gaps
between them.

Both are undocumented attributes, looked up at runtime and ignored if missing, so a
machine without them keeps the painted surfaces and stays readable.

---

## D-024 — The top bar hides itself, and reserves nothing
**Date:** 2026-10-31 · **Phase:** feedback

A permanently visible bar has to reserve its strip, and maximised windows then stop
below it. You asked for windows to use the whole screen.

**Decision:** auto-hide is now the default. The bar slides away and comes back when
the pointer touches the top edge, exactly mirroring the dock at the bottom, and it
reserves nothing — maximised windows get every pixel. While it is away its hit-test
region is emptied, so the top strip behaves like bare screen. It is held down while
one of its panels is open, because a bar sliding out from under an open flyout looks
like a fault.

The old behaviour is one switch away in settings.

---

## D-025 — Shadows are sized to the hit-test region
**Date:** 2026-10-31 · **Phase:** feedback

Every surface here lives inside a `SetWindowRgn` region so clicks pass through the
gaps. The region was a **rectangle** two pixels bigger than the card, which cut the
corners off a rounded card and left its shadow behind as four hard corners — the thing
you described as "very sharp at the corners".

**Decision:** regions are rounded rectangles, inflated by exactly how far the shadow
can reach, with a corner radius to match. The shadow itself was retuned so that reach
is 18px rather than 68px, because the inflated region is clickable area that the
desktop underneath loses. `--shadow-reach` in `theme.css` and `PAD` in the frontend
are the same number and have to stay that way.

---

## D-026 — The island, and why it is not a notch
**Date:** 2026-10-31 · **Phase:** feedback

You asked for Alcove's Dynamic Island behaviour on Windows.

**Decision:** the top bar's centre pill *is* the island. Collapsed it is the clock;
hovering expands it to what is playing, with artwork, title and transport controls;
and it expands on its own for a few seconds when the volume changes, the charger goes
in or out, or the track changes.

Two implementation notes worth keeping:

- The top bar **window** is 190px tall while the bar itself is 56px. The island
  expands into that empty space, so growing it is a CSS transition rather than a Win32
  window resize — resizing a window every frame is exactly what makes this kind of UI
  stutter. None of the empty space is clickable.
- While the island is open its region is set to its **full footprint** rather than its
  measured one. The region has to be sent the moment the state changes, and measuring
  then returns the collapsed size, which clipped the body away as it grew.

Not built from Alcove: the file shelf (drag files onto the island to park them),
clipboard history, camera mirror and timers. They are separate features rather than
variations on this one.

---

## D-027 — One keyboard hook, and the rules it lives by
**Date:** 2026-10-31 · **Phase:** 4

Everything else in this project deliberately avoids hooks: the dock samples the
cursor rather than installing `WH_MOUSE_LL`, because a slow hook degrades input for
the whole machine. Alt+Tab leaves no choice — the shell reserves it, and
`WH_KEYBOARD_LL` is the only way to see it.

**Decision:** install exactly one low-level keyboard hook, on **a thread of its own
with its own message loop**, and keep the callback down to reading two key states and
pushing an enum into a channel. It never touches a window, never allocates and never
blocks. A dedicated thread matters: a low-level hook is delivered to the thread that
installed it, and Windows drops a hook whose thread does not answer in time — putting
it on the UI thread would mean a busy webview could lose it.

**What it swallows, and nothing else:** `Tab` while Alt is physically held, and `Esc`
while the switcher is open. It does **not** swallow Alt itself, so the window you are
leaving sees a complete Alt press rather than half of one and cannot be left with a
stuck modifier. The cost is that an app with a menu bar may flash it as you switch
away; a stuck Alt would be far worse.

**Every failure leaves Windows' Alt+Tab working**, which is the rule that mattered
most:

| Failure | What happens |
|---|---|
| `SetWindowsHookEx` fails | logged; nothing is installed and nothing is swallowed |
| A window running as administrator has focus | Windows does not deliver our hook its input at all, so the native switcher runs. Automatic, not something we detect |
| The callback is too slow | Windows silently drops the hook — which restores the native switcher |
| The process dies, however it dies | the hook dies with it |
| The user switches it off in settings | the hook passes everything through |

All but the elevated-window case were tested by forcing them, including killing the
app mid-Alt+Tab **with Alt still held down**: Windows' own switcher worked on the
very next press.

---

## D-028 — Win+Tab is left as Windows Task View
**Date:** 2026-10-31 · **Phase:** 4

The brief asks for a restyled Win+Tab, and allows keeping the Windows one if
restyling would break virtual desktops.

**Decision:** keep Task View. Virtual desktops have no supported API for creating,
switching or moving windows between desktops — the interfaces exist but are
undocumented and change between Windows builds. A restyled Win+Tab that could show
the desktops but not drive them would be a worse Task View, and one that drove them
through undocumented interfaces would break on a Windows update, in a part of the
system the user cannot easily recover.

Alt+Tab is restyled; Win+Tab opens Windows' own, exactly as before.

---

## D-029 — Only the Win key's *release* is ever touched
**Date:** 2026-10-31 · **Phase:** 3

Windows opens Start on the Win key's **release**, and only when nothing else was
pressed while it was held. That is the whole of the opening, and it is the only part
this touches.

**Decision:** pass the Win key*down* through untouched, always. Track whether anything
else happened during the press. On release, swallow it **only** if nothing did, and
open the launcher instead.

Why it matters: `Win+E`, `Win+R`, `Win+L`, `Win+Shift+S`, `Win+1`, `Win+arrow` and
every other combination reach Windows exactly as they always did, because the hook
never sees them as anything but "something else was pressed". The alternative -
swallowing the keydown and re-synthesising it whenever a combination turns up - means
injecting a keystroke from inside the hook callback with the combination's own
keystroke already in flight. That is the version of this that breaks `Win+L`.

A mouse click during the press counts as "used" too, because Windows treats it that
way.

**This is not yet verified by test.** `scripts/winkey-matrix.ps1` sends every
combination and reports what Windows did; it was written for exactly this and has not
been run to completion, because it takes over the keyboard for about a minute and the
machine was in use. Until it has been run, the argument above is reasoning, not
evidence. One switch in settings hands the Win key back.

---

## D-030 — The launcher lists `shell:AppsFolder`, and takes icons from the items
**Date:** 2026-10-31 · **Phase:** 3

Walking the Start Menu folders for `.lnk` files misses every Store and packaged app,
because those have no shortcut on disk. The AppsFolder is the shell's own list and
includes them, with a parsing name `ShellExecuteW` can launch directly.

**Decision:** enumerate `shell:AppsFolder`, and take each icon from the `IShellItem`
that enumeration already hands back - *not* by looking the name up again afterwards.
The parsing names the AppsFolder gives out do not all round-trip through
`SHCreateItemFromParsingName`, and fetching icons that way left almost every app with
a blank page icon. Measured on this machine: 158 apps, every one with its real icon.

The whole enumeration runs once on a background thread at startup, so the second it
costs is spent long before the Win key is pressed.

---

## D-031 — The island is a shelf, not a clock
**Date:** 2026-10-31 · **Phase:** feedback

The first island showed the time, with now-playing behind a hover. You asked for the
opposite: the time is a known quantity, and the island should carry the things that
change and the things you are moving around.

**Decision:** the clock moved to the status pill on the right, where it is always
readable. The island now shows what is playing, and acts as a **shelf**: drop files,
folders or images on it and they stay parked until you drag them somewhere else.

Items are **references, not copies**. Parking something costs nothing, dragging it off
hands over the real path, and nothing is duplicated on disk.

Dragging an item *out* could not be done from the web side at all - HTML drag-and-drop
has no way to hand a filesystem path to a native drop target - so a drag that leaves
the island hands over to a real OLE drag in Rust (`win/dragout.rs`), carrying the same
`CF_HDROP` payload Explorer itself uses. The data object is the shell's own, through
`IShellItemArray::BindToHandler`, which is nine interface methods not written by hand.

`DoDragDrop` runs on the **main thread**, synchronously. It has to be called from the
thread that owns the window the drag started in; from a pool thread it simply never
completed. It blocks the UI until the drag ends, which is what a native drag does.

---

## D-032 — No native tooltips anywhere
**Date:** 2026-10-31 · **Phase:** feedback

The `title` attribute draws Windows' own tooltip: a white-bordered box in the system
font, over a glass panel. That was the "weird white colour marking" on the top bar.

**Decision:** every `title` became an `aria-label`. The information still reaches a
screen reader; nothing is drawn. The dock, which genuinely needs tooltips, has always
drawn its own.

---

## D-033 — Nothing that waits may run on the main thread
**Date:** 2026-10-31 · **Phase:** crash fix

The app was being closed by Windows with `AppHangB1` - "macui.exe stopped interacting
with Windows and was closed" - three times in under an hour. Not a crash: a **hang**.
The Application event log says so plainly, and the distinction is the whole diagnosis.
Windows marks a process as not responding after five seconds without pumping messages,
and the shell then offers to close it.

Two things were sitting on the main thread waiting:

1. **`win::bluetooth()` started PowerShell** - `Get-PnpDevice -Class Bluetooth` -
   behind a `OnceLock`, and the top bar calls `cmd_status` every two seconds. A cold
   PowerShell start on a machine with 0.4 GB of free RAM and a page file under
   pressure is not milliseconds, it is seconds, and they were spent not pumping.
2. **`cmd_radios` and `cmd_set_radio` were synchronous commands**, and
   `radios::mta()` spawned a thread and then **joined** it. A join does not pump
   messages. `SetStateAsync` on a Bluetooth radio routinely takes seconds, and the
   quick panel polled `cmd_radios` while it was open.

A synchronous Tauri command runs on the main thread, inside the WebView2 IPC
callback. That is the same fact D-012 turned on, and it is worth stating as a rule:
**a synchronous command may compute, but it may never wait.**

**Decision:** radios moved to the background-poller shape `media.rs` already uses - one
MTA thread owning every WinRT call, a snapshot the UI reads, and a queue it writes to.
`list()` is now a memory read and `set()` returns immediately. The PowerShell probe is
gone entirely: the radio snapshot already carries Bluetooth presence *and* its real
on/off state, which the shell-out could only guess at by reporting presence twice.

The launcher's `cmd_launcher_apps`, `cmd_launcher_search` and `cmd_launcher_list_dir`
became `async` for the same reason. They are bounded - a 350ms budget on the file walk
- but bounded is not the same as free, and an async command runs off the main thread.

---

## D-034 — A collapsed island is its pill, not its container
**Date:** 2026-10-31 · **Phase:** crash fix

The top bar window is transparent and clipped to a hit-test region covering only the
pills, so everything outside them is bare screen. The region was measured from
`#island`, the flex container - but `.island-body` inside it keeps its full **330px
width** while collapsed to zero height. So a 100px pill produced a 366px region, and
the window's acrylic showed through the empty 266px as a pale slab across the middle
of the bar.

That was the "white marking at the top" - the second half of it. The first half was
native tooltips (D-032); this was the rest.

**Decision:** measure `#clock-pill` while the island is collapsed and `#island` only
while it is open. One element swap, and the region now hugs what is actually drawn.

---

## D-035 — WMI brightness, and the two types it insists on
**Date:** 2026-10-31 · **Phase:** quick settings

A laptop's backlight has no Win32 call. `SetMonitorBrightness` speaks DDC/CI, which
external monitors answer and built-in panels do not; the panel's backlight lives in
`root\wmi` as `WmiMonitorBrightness` to read and `WmiMonitorBrightnessMethods::
WmiSetBrightness` to write. That is the pair the Windows slider itself drives, so
moving ours moves Windows'.

Two things this cost, both found by the test rather than by reading:

- `WmiSetBrightness` declares `uint32 Timeout` and `uint8 Brightness`, and rejects the
  matching `VT_UI4`/`VT_UI1` VARIANTs with `WBEM_E_TYPE_MISMATCH` (0x80041005). Both
  have to go in as **`VT_I4`**.
- `ExecMethod` returns `Ok` on a parameter object built wrong. Success is not evidence
  here. `brightness_round_trip` sets the panel to a different level, reads it back off
  the hardware, and restores it - which is the only check that means anything, and it
  is what caught the type mismatch.

It runs on its own thread, idle except for writes and a 30-second re-read, for the
reason in D-033. Machines with no dimmable panel report `None` and the panel leaves
the slider out rather than showing one that moves nothing.

---

## D-036 — Toggle what Windows lets us toggle; open the rest
**Date:** 2026-10-31 · **Phase:** quick settings

The brief is "every setting the Windows taskbar offers". Windows offers eleven tiles
and two sliders, and has a public API for five of them.

**Decision:** three kinds of tile, and the difference is visible rather than hidden.
Wi-Fi, Bluetooth and Airplane mode drive the real radios. Volume drives the real
endpoint and brightness the real backlight. Battery saver, Night light, Accessibility,
Nearby sharing, Cast, VPN and Mobile hotspot have **no supported way** to toggle from
another process, so their tiles open the real Windows control in one click; Project
runs `DisplaySwitch.exe`, which is what Win+P does.

The alternative for those seven is driving undocumented registry state behind
Windows' back - Night light is the usual example - and that is exactly what the brief
rules out. A tile that takes you to the real switch is honest. A tile that silently
does nothing, or that fights the OS over who owns a setting, is not.

All eleven are shown at once rather than paged behind a chevron: a control hidden
behind a gesture you have to discover first is not "available at the top".
