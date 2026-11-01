# Dot-source this to put the no-admin toolchain on PATH for the current shell only.
#   . .\scripts\env.ps1
# It never writes to the persistent user PATH (see SYSTEM_CHANGES.md T-1/T-2).
# MinGW must live at a path with no spaces: gcc's spec adds default-manifest.o via
# %:if-exists, which splits its install path at spaces. On a profile like
# "C:\Users\First Last" the default location fails to link, so use C:\macui-toolchain.
$toolchain = "$env:LOCALAPPDATA\macui-toolchain"
if (-not (Test-Path "$toolchain\mingw64\bin") -and (Test-Path 'C:\macui-toolchain\mingw64\bin')) { $toolchain = 'C:\macui-toolchain' }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$toolchain\mingw64\bin;$env:PATH"
Write-Host "toolchain on PATH:" -ForegroundColor Cyan
rustc --version
cargo --version
gcc --version | Select-Object -First 1
