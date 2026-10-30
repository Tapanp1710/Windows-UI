# PROGRESS

**Read this file first when resuming.**

## Current phase
STEP 0 — environment and feasibility check.

## Status: IN PROGRESS

### Environment findings (2026-10-31)

| Tool | Status | Version |
|---|---|---|
| Node | OK | v24.15.0 |
| npm | OK | 11.12.1 |
| Git | OK | 2.54.0.windows.1 |
| Python | OK | 3.14.5 (helper scripts only) |
| WebView2 Runtime | OK | 156.0.4314.50 |
| winget | present | user-scope only |
| **Rust / Cargo** | **MISSING** | installing per-user (no admin) |
| **MSVC build tools** | **MISSING** | cannot install — needs admin/UAC |
| **Windows SDK** | **MISSING** | cannot install — needs admin/UAC |
| .NET SDK | MISSING | not needed |

### Hardware/OS deviations from the brief
The brief described Windows 11 24H2 (build 26100) and 12 GB RAM. The actual machine is:

- **Windows 11 Pro build 22631** (23H2), not 24H2/26100
- **7.9 GB RAM**, not 12 GB
- Not running as administrator (correct, per the safety rules)

Build 22631 still supports Mica/Acrylic backdrops (22621+), so the glass design is
unaffected. The lower RAM makes the "under 150 MB" target more important, not less.

### Critical blocker and the workaround
MSVC build tools and the Windows SDK are absent, and installing either triggers a UAC
admin prompt, which the safety rules forbid. Rather than stop, STEP 0 is proceeding down
a **fully no-admin toolchain path**:

- `rustup-init.exe` installs per-user into `%USERPROFILE%\.rustup` + `.cargo` — no admin.
- Portable **WinLibs MinGW-w64 GCC** (UCRT) extracted into
  `%LOCALAPPDATA%\macui-toolchain` — a zip extraction, no installer, no admin.
- Rust target `x86_64-pc-windows-gnu` instead of `-msvc`.

This is a real risk: Tauri v2 officially supports the MSVC target only, and the
`windows-gnu` target is best-effort. STEP 0 exists to prove or disprove it before any
UI work starts. See DECISIONS.md D-001..D-003.

**If the GNU path fails**, the honest outcome is: this project cannot be built on this
machine without one admin-elevated install. The exact thing to install would be
*Visual Studio 2022 Build Tools* with the "Desktop development with C++" workload
(includes MSVC v143 + Windows 11 SDK), from
<https://visualstudio.microsoft.com/visual-cpp-build-tools/>, then
`rustup default stable-x86_64-pc-windows-msvc`.

## What works
Nothing yet — toolchain bring-up in progress.

## What's broken
Nothing built yet.

## Next step
1. Finish MinGW + rustup install, verify `cargo` and `gcc` resolve.
2. Scaffold a minimal Tauri v2 app, make a **release** build.
3. Run the release `.exe` and confirm App Control for Business does not block it.
4. If all three pass → PHASE 1 (dock). If not → log and stop with exact install steps.

## Performance baseline
Not yet measured.
