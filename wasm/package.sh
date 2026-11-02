#!/usr/bin/env bash
# Builds the browser module (see build.sh) and packs it as wasm/dist/hanab-bot-wasm.tar.gz, with its SHA-256 in
# hanab-bot-wasm.tar.gz.sha256. The archive holds hanab_bot_wasm.js, hanab_bot_wasm.d.ts and hanab_bot_wasm_bg.wasm, and
# is the same for the same build (no timestamps or owners in it), so the checksum can be pinned by the program using it.
# The release workflow (.github/workflows/wasm-release.yml) publishes it.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh

rm -rf dist
mkdir dist
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -C pkg \
  -cf - hanab_bot_wasm.js hanab_bot_wasm.d.ts hanab_bot_wasm_bg.wasm | gzip -n > dist/hanab-bot-wasm.tar.gz
(cd dist && sha256sum hanab-bot-wasm.tar.gz > hanab-bot-wasm.tar.gz.sha256)
ls -l dist
cat dist/hanab-bot-wasm.tar.gz.sha256
