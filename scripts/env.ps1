# Dot-source this to put the no-admin toolchain on PATH for the current shell only.
#   . .\scripts\env.ps1
# It never writes to the persistent user PATH (see SYSTEM_CHANGES.md T-1/T-2).
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\macui-toolchain\mingw64\bin;$env:PATH"
Write-Host "toolchain on PATH:" -ForegroundColor Cyan
rustc --version
cargo --version
gcc --version | Select-Object -First 1
