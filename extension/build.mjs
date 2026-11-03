// Builds the extension into dist/chrome and dist/firefox.
//
//   node build.mjs                      the extension for https://hanab.live
//   EXT_MATCHES="http://localhost:8099/*" node build.mjs     for another site (the tests use this)
//
// The variant and suit definitions that the bot needs are the site's own: they are downloaded from the
// hanabi-live repository (the ref is HANABI_LIVE_REF, a pinned commit by default) into build/data and bundled into the worker.

import { build } from "esbuild";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
// A commit of the hanabi-live repository, so that a build is the same every time. Bump it when the site gets new variants.
const ref = process.env["HANABI_LIVE_REF"] ?? "918779de02ae384545c4fd0904c93544bd6ded1b";
const matches = (process.env["EXT_MATCHES"] ?? "https://hanab.live/*").split(",");
const dataDir = path.join(root, "build", "data");

async function downloadData() {
  await mkdir(dataDir, { recursive: true });
  for (const file of ["variants.json", "suits.json"]) {
    const target = path.join(dataDir, file);
    if (existsSync(target) && !process.env["REFRESH_DATA"]) {
      continue;
    }
    const url = `https://raw.githubusercontent.com/Hanabi-Live/hanabi-live/${ref}/packages/game/src/json/${file}`;
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`Failed to download ${url}: ${response.status}`);
    }
    await writeFile(target, await response.text());
    console.log(`Downloaded ${file} (${ref}).`);
  }
}

const base = JSON.parse(await readFile(path.join(root, "manifest.base.json"), "utf8"));

function manifest(browser) {
  const result = {
    ...base,
    content_scripts: [
      { matches, js: ["content.js"], run_at: "document_idle" },
      { matches, js: ["page.js"], run_at: "document_start", world: "MAIN" },
    ],
    web_accessible_resources: [
      { resources: ["worker.js", "hanab_bot_wasm_bg.wasm"], matches },
    ],
  };
  if (browser === "firefox") {
    result.browser_specific_settings = {
      gecko: { id: "hanab-analysis@michielbaird.github.io", strict_min_version: "128.0" },
    };
  }
  return result;
}

await downloadData();

for (const browser of ["chrome", "firefox"]) {
  const out = path.join(root, "dist", browser);
  await rm(out, { recursive: true, force: true });
  await mkdir(out, { recursive: true });

  const common = { bundle: true, minify: true, target: "es2022", logLevel: "warning" };
  await build({ ...common, entryPoints: [path.join(root, "src/content.ts")], outfile: path.join(out, "content.js"), format: "iife", loader: { ".css": "text" } });
  await build({ ...common, entryPoints: [path.join(root, "src/page.ts")], outfile: path.join(out, "page.js"), format: "iife" });
  await build({ ...common, entryPoints: [path.join(root, "src/worker.ts")], outfile: path.join(out, "worker.js"), format: "esm" });
  await copyFile(path.join(root, "node_modules/hanab-bot-wasm/hanab_bot_wasm_bg.wasm"), path.join(out, "hanab_bot_wasm_bg.wasm"));
  await writeFile(path.join(out, "manifest.json"), `${JSON.stringify(manifest(browser), null, 2)}\n`);
  console.log(`Built dist/${browser}`);
}
