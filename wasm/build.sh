#!/usr/bin/env bash
# Builds the browser module into wasm/pkg (ES module for a Web Worker: `import init, { analyze } from "./hanab_bot_wasm.js"`).
# Needs: rustup target add wasm32-unknown-unknown, and wasm-bindgen-cli of the same version as the `wasm-bindgen` crate
# in Cargo.lock (cargo install wasm-bindgen-cli --version <that version> --locked).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/hanab_bot_wasm.wasm
ls -l pkg/*.wasm
