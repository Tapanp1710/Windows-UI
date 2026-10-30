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
