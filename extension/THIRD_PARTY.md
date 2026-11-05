# Third-party material

The extension is under the GNU General Public License, version 3 or later (see `LICENSE`, which is also in the
packages). It contains:

## The bot

`hanab-bot-wasm`, the game analysis of [hanab-bot-rs](https://github.com/michielbaird/hanab-bot-rs) compiled to
WebAssembly (GPL-3.0-or-later, the same project; a port of [scala-bot](https://github.com/will-hanabi-bot/scala-bot),
which is GPL-3.0). It is built from the Rust source in this repository's `wasm/` directory and published as a release
package; the exact package and its checksum are in `package-lock.json`.

Rust crates compiled into it (all under permissive licenses):

- bumpalo 3.20.3 — MIT OR Apache-2.0
- cfg-if 1.0.5 — MIT OR Apache-2.0
- futures-core 0.3.34 — MIT OR Apache-2.0
- futures-task 0.3.34 — MIT OR Apache-2.0
- futures-util 0.3.34 — MIT OR Apache-2.0
- itoa 1.0.18 — MIT OR Apache-2.0
- js-sys 0.3.107 — MIT OR Apache-2.0
- memchr 2.8.3 — Unlicense OR MIT
- pin-project-lite 0.2.17 — Apache-2.0 OR MIT
- proc-macro2 1.0.107 — MIT OR Apache-2.0
- quote 1.0.47 — MIT OR Apache-2.0
- rustversion 1.0.23 — MIT OR Apache-2.0
- serde 1.0.229 — MIT OR Apache-2.0
- serde_core 1.0.229 — MIT OR Apache-2.0
- serde_derive 1.0.229 — MIT OR Apache-2.0
- serde_json 1.0.154 — MIT OR Apache-2.0
- slab 0.4.12 — MIT
- syn 3.0.9 — MIT OR Apache-2.0
- unicode-ident 1.0.27 — (MIT OR Apache-2.0) AND Unicode-3.0
- wasm-bindgen 0.2.130 — MIT OR Apache-2.0
- wasm-bindgen-macro 0.2.130 — MIT OR Apache-2.0
- wasm-bindgen-macro-support 0.2.130 — MIT OR Apache-2.0
- wasm-bindgen-shared 0.2.130 — MIT OR Apache-2.0
- web-time 1.1.0 — MIT OR Apache-2.0
- zmij 1.0.23 — MIT

## The variant and suit definitions

`variants.json` and `suits.json` are the definitions that hanab.live itself uses, from the
[hanabi-live](https://github.com/Hanabi-Live/hanabi-live) repository (GPL-3.0), at the commit named in `build.mjs`. They
are bundled into the extension so that the bot knows the variants.

## Not included

The extension is not made or endorsed by hanab.live or the H-Group, and uses none of their logos or artwork. The robot
icon was drawn for this project.
