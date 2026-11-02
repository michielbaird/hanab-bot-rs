#!/usr/bin/env bash
# Builds the browser module (see build.sh) and packs it as an npm package, wasm/dist/hanab-bot-wasm-<version>.tgz, with its
# SHA-256 next to it. <version> is the one in wasm/Cargo.toml. The package holds hanab_bot_wasm.js, hanab_bot_wasm.d.ts and
# hanab_bot_wasm_bg.wasm, so a web project can depend on it like on any other package:
#
#   "hanab-bot-wasm": "https://github.com/<owner>/hanab-bot-rs/releases/download/wasm-v<version>/hanab-bot-wasm-<version>.tgz"
#
# and npm records its checksum in the lock file. The archive is the same for the same build (no timestamps or owners in
# it). The release workflow (.github/workflows/wasm-release.yml) publishes it.
set -euo pipefail
cd "$(dirname "$0")"

version=$(awk -F'"' '/^version/ { print $2; exit }' Cargo.toml)

./build.sh

rm -rf dist
mkdir -p dist/package
cp pkg/hanab_bot_wasm.js pkg/hanab_bot_wasm.d.ts pkg/hanab_bot_wasm_bg.wasm dist/package/
cp ../LICENSE dist/package/LICENSE
cat > dist/package/package.json <<JSON
{
  "name": "hanab-bot-wasm",
  "version": "$version",
  "description": "The game analysis of hanab-bot-rs (Hanabi, H-Group, Referential Sieve and Reactor conventions) for the browser",
  "license": "GPL-3.0-or-later",
  "repository": "github:michielbaird/hanab-bot-rs",
  "type": "module",
  "main": "hanab_bot_wasm.js",
  "types": "hanab_bot_wasm.d.ts",
  "sideEffects": false,
  "files": [
    "hanab_bot_wasm.js",
    "hanab_bot_wasm.d.ts",
    "hanab_bot_wasm_bg.wasm"
  ]
}
JSON

archive="hanab-bot-wasm-$version.tgz"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner -C dist \
  -cf - package | gzip -n > "dist/$archive"
rm -rf dist/package
(cd dist && sha256sum "$archive" > "$archive.sha256")
ls -l dist
cat "dist/$archive.sha256"
