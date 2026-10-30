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
