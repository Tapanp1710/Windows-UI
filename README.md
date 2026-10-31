# macui

A macOS-shaped visual layer for Windows 11: a dock, a top bar with a Dynamic Island
and shelf, a launcher on the Win key, Alt+Tab with live window previews, desktop
widgets, and a settings window.

**It changes how Windows looks, not how it works.** Every Windows shortcut, click and
flyout still does what it always did. Where something could not be restyled without
changing behaviour, it was left as standard Windows and written down in
[DECISIONS.md](DECISIONS.md).

Your taskbar always comes back — on quit, on crash, and on a hard kill (a watchdog
process restores it). If you want out entirely, run
[`restore-windows.ps1`](restore-windows.ps1).

---

## Installing it on another machine

### The easy way: the installer

Build the installer once (see below) and copy it over, or attach it to a GitHub
Release. On the target machine, run:

```
macui_0.1.0_x64-setup.exe
```

It installs **per user**. No administrator prompt, nothing written to `HKLM`, nothing
installed system-wide.

The target machine needs nothing else except the **WebView2 runtime**, which Windows
11 already ships. On a Windows 10 machine without it, the installer fetches Microsoft's
bootstrapper at install time, so it needs an internet connection that once.

### The other way: build from source

```powershell
git clone <your repo>
cd ui
npm install
. .\scripts\env.ps1      # puts the toolchain on PATH for this shell only
npm run tauri build
```

That produces:

- `src-tauri\target\release\macui.exe` — run it directly, no install
- `src-tauri\target\release\bundle\nsis\macui_0.1.0_x64-setup.exe` — the installer

**Build prerequisites**, neither of which needs administrator rights:

| What | Where it is expected | Notes |
|---|---|---|
| Rust, **`x86_64-pc-windows-gnu`** toolchain | `%USERPROFILE%\.cargo` | via `rustup`; the GNU target was chosen because the MSVC one needs the Visual Studio Build Tools, which need an admin prompt |
| MinGW-w64 GCC (portable) | `%LOCALAPPDATA%\macui-toolchain\mingw64` | a WinLibs UCRT zip, extracted — no installer, no registry. `scripts\env.ps1` expects exactly this path |
| Node.js 18+ | wherever | for Vite and the Tauri CLI |

If you put MinGW somewhere else, edit the one line in `scripts\env.ps1`.

### First run on a new machine

```powershell
powershell -ExecutionPolicy Bypass -File scripts\capture-baseline.ps1
```

This records that machine's **original** taskbar state, work area, theme and wallpaper
into `baseline-windows.json`, so `restore-windows.ps1` can put them back exactly.

`baseline-windows.json` is deliberately **not** in the repo and is stamped with the
machine name. It describes one computer's "before", and a baseline from somewhere else
is worse than none — it would set your taskbar to whatever someone else's happened to
be and call that a faithful restore. `restore-windows.ps1` refuses a foreign one.

---

---

## Setting this up on a new laptop

Clone the repo, open the folder in Claude Code, and paste the prompt below. It does
the whole setup: toolchain, dependencies, build, baseline capture, first run.

```
Set this project up on this machine and run it.

It is a macOS-style desktop UI layer for Windows 11, built with Tauri v2 (Rust
backend, TypeScript frontend). Read README.md, REPORT.md and DECISIONS.md first —
DECISIONS.md explains why things are the way they are, and several of those reasons
are the result of bugs that cost real time, so do not undo them casually.

Do all of this without ever running anything as administrator. If a step would need
an admin prompt, skip it, write down why in SYSTEM_CHANGES.md, and carry on with the
rest.

1. Check what is already installed: Node 18+, Rust with the x86_64-pc-windows-gnu
   target, and MinGW-w64 GCC at %LOCALAPPDATA%\macui-toolchain\mingw64.
2. Install whatever is missing, per-user only:
   - rustup from https://win.rustup.rs, default host x86_64-pc-windows-gnu. The
     MSVC target is NOT usable here: its linker needs the Visual Studio Build
     Tools, which need an admin prompt.
   - A portable WinLibs MinGW-w64 UCRT build, extracted (not installed) to
     %LOCALAPPDATA%\macui-toolchain. scripts\env.ps1 expects exactly that path;
     if you put it elsewhere, edit that one line.
   - Node from nodejs.org if it is missing.
   Record every one of these in SYSTEM_CHANGES.md with its exact undo step.
3. Run: powershell -ExecutionPolicy Bypass -File scripts\capture-baseline.ps1
   This records THIS machine's original taskbar state, work area, theme and
   wallpaper so restore-windows.ps1 can put them back. It is per-machine and is
   deliberately not in the repo — do not copy one from anywhere else.
4. npm install
5. . .\scripts\env.ps1    (puts the toolchain on PATH for this shell only)
6. npm run tauri build
7. Start src-tauri\target\release\macui.exe and confirm, with screenshots, that:
elease\macui.exe and confirm, with screenshots, that:
   the dock appears at the bottom edge on hover; its tray shows network, Bluetooth,
   sound, battery and the clock; clicking the tray opens the quick panel upward;
   the magnifier expands the bar into a search field that finds apps; and Alt+Tab
   shows the switcher.
8. Verify the safety path before you call it done: hard-kill the main macui
   process and confirm the Windows taskbar comes back with its original auto-hide
   setting, and that no orphan processes are left.
9. Tell me what you installed, what you changed on this machine, and anything that
   did not work.

Two things to know going in, both the hard way:
- A synchronous Tauri command runs on the main thread. It may compute, but it must
  never wait — no COM round trips, no process spawns, no joining a thread. That
  mistake made Windows close the app as hung, four separate times.
- Never swallow a modifier key's release in the keyboard hook. Passing the keydown
  through tells Windows the modifier is down; swallowing the keyup means it is
  never told otherwise, and the whole keyboard latches.
```

### If you just want to run it, not build it

Grab `macui_0.1.0_x64-setup.exe` from the repo's Releases page and run it. It
installs per user, needs no admin prompt, and the only other thing the machine needs
is the WebView2 runtime, which Windows 11 already ships.

Still run `scripts\capture-baseline.ps1` once afterwards, so this machine's "before"
is recorded.

## Getting out

```powershell
powershell -ExecutionPolicy Bypass -File restore-windows.ps1
```

Quits the app, restores the taskbar to the recorded baseline, resets the work area,
removes the startup shortcut and puts your wallpaper back. Pass `-RemoveToolchain` to
delete Rust and MinGW too.

Every system change the app can make is listed, with its undo, in
[SYSTEM_CHANGES.md](SYSTEM_CHANGES.md).

---

## What to expect on different hardware

| | |
|---|---|
| **Resolution and DPI** | Adapts. Window sizes and the widget layer are computed from the real screen at startup, and saved widget positions are clamped back on-screen if the new display is smaller. |
| **Multiple monitors** | **Not supported yet.** Everything targets the primary monitor; the bars will not appear on a second display. |
| **Windows 10** | Untested. Nothing used is 11-only, but the acrylic and rounded-corner calls behave differently on 10, and the taskbar is a different window. Treat it as unverified. |
| **Windows 11** | Tested on 22631 (23H2). |
| **Laptop vs desktop** | The brightness slider appears only where Windows reports a software-dimmable panel. Desktops with external monitors get the rest of the panel without it. |
| **The Win key** | The launcher opens on it. The matrix of `Win+<key>` combinations has **not been verified by test** — see below. |

---

## Known gaps

1. **`scripts\winkey-matrix.ps1` has not been run to completion.** It sends every
   `Win+<key>` combination and reports what Windows did. Until it has been run, the
   claim that Win+E, Win+R, Win+L and the rest are untouched rests on how the keyboard
   hook is built ([D-029](DECISIONS.md)), not on evidence. It takes the keyboard over
   for about a minute, so run it when the machine is free. If anything breaks, turn
   "Use this app's launcher" off in settings and the Win key goes straight back to
   Windows Start.
2. **Multi-monitor**, as above.
3. **Parking a window on the island's shelf** — files, folders and images work;
   windows do not yet.
4. **Dropping a file onto the island from Explorer** is implemented but untested.
   Dragging back *out* is tested and works.

---

## The documents

| | |
|---|---|
| [REPORT.md](REPORT.md) | What is built, what is measured, what is left |
| [DECISIONS.md](DECISIONS.md) | Every non-obvious decision and why, D-001 onward |
| [TESTS.md](TESTS.md) | What was tested and what the result was, including the failures |
| [SYSTEM_CHANGES.md](SYSTEM_CHANGES.md) | Every system change, with its undo |
| [PROGRESS.md](PROGRESS.md) | Phase-by-phase state |
