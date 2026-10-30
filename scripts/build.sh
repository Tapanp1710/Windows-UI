#!/usr/bin/env bash
# Release build. Use this rather than bare `cargo build --release`:
# without --features custom-protocol Tauri builds a *dev* binary that loads the Vite
# dev server and shows a blank window. See Cargo.toml [features].
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$LOCALAPPDATA/macui-toolchain/mingw64/bin:$PATH"
npm run build
cd src-tauri
cargo build --release --features custom-protocol "$@"
echo "built: src-tauri/target/release/macui.exe"
