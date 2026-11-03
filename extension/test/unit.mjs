// Runs the unit tests (src/**/*.test.ts) with node's test runner, after bundling them with esbuild.

import { build } from "esbuild";
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const src = path.join(root, "src");
const outdir = path.join(root, "build", "test");

const tests = readdirSync(src, { recursive: true, withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".test.ts"))
  .map((entry) => path.join(entry.parentPath, entry.name));

await build({
  entryPoints: tests,
  outdir,
  outbase: src,
  bundle: true,
  platform: "node",
  format: "cjs",
  logLevel: "warning",
});

const built = tests.map((file) =>
  path.join(outdir, path.relative(src, file).replace(/\.ts$/, ".js")),
);
const result = spawnSync(process.execPath, ["--test", ...built], { stdio: "inherit" });
process.exit(result.status ?? 1);
