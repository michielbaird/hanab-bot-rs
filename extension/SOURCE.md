# Building the extension from source

For store reviewers (and anyone else). The submitted files are built from this directory.

Needs Node 22 or newer and network access (to download the packages and the variant data).

```sh
cd extension
npm ci            # esbuild, TypeScript, and the bot (hanab-bot-wasm, pinned with a checksum in package-lock.json)
npm run build     # dist/chrome and dist/firefox
```

- `build.mjs` bundles `src/` with esbuild (no other transformation), copies the icons, the popup and the bot's
  `.wasm` file, and writes the manifest for each browser from `manifest.base.json`.
- The variant and suit definitions are downloaded from the hanabi-live repository at a pinned commit (see `build.mjs`).
- `hanab-bot-wasm` is built from `../wasm` by `.github/workflows/wasm-release.yml` (Rust, `wasm-bindgen`) and
  published as a GitHub release; `npm ci` verifies its checksum. To rebuild it yourself: `../wasm/package.sh`.
- The icons in `icons/` are generated from `src/icons/robot.svg` by `tools/make-icons.mjs`.

Checks: `npm run typecheck`, `npm run test:unit`, and `npm test` (an end-to-end test in Chromium).
