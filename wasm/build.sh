#!/usr/bin/env bash
# Builds the browser module into wasm/pkg (ES module for a Web Worker: `import init, { analyze } from "./hanab_bot_wasm.js"`).
# Needs: rustup target add wasm32-unknown-unknown, and wasm-bindgen-cli of the same version as the `wasm-bindgen` crate
# in Cargo.lock (cargo install wasm-bindgen-cli --version <that version> --locked).
set -euo pipefail
cd "$(dirname "$0")"

# The generated glue only works with the wasm of the same wasm-bindgen version.
locked=$(awk '/^name = "wasm-bindgen"$/ { getline; print $3 }' Cargo.lock | tr -d '"')
installed=$(wasm-bindgen --version | awk '{ print $2 }')
if [[ $locked != "$installed" ]]; then
  echo "wasm-bindgen-cli $installed does not match the wasm-bindgen $locked in Cargo.lock:" >&2
  echo "  cargo install wasm-bindgen-cli --version $locked --locked" >&2
  exit 1
fi

cargo build --release --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir pkg target/wasm32-unknown-unknown/release/hanab_bot_wasm.wasm
ls -l pkg/*.wasm
