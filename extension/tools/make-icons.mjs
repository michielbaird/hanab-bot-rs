// Draws the extension's icons (icons/icon-<size>.png) from src/icons/robot.svg with Chromium. Run it when the robot changes:
//   node tools/make-icons.mjs

import { chromium } from "playwright-core";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const robot = await readFile(path.join(root, "src/icons/robot.svg"), "utf8");

const browser = await chromium.launch();
for (const size of [16, 32, 48, 128]) {
  const page = await browser.newPage({ viewport: { width: size, height: size } });
  await page.setContent(`<!doctype html><style>html,body{margin:0;background:transparent}
    .icon{width:${size}px;height:${size}px;border-radius:${size * 0.22}px;background:#1f6f3a;color:#fff;display:flex;align-items:center;justify-content:center}
    svg{width:${size * 0.78}px;height:${size * 0.78}px}</style><div class="icon">${robot}</div>`);
  await page.screenshot({ path: path.join(root, `icons/icon-${size}.png`), omitBackground: true });
  await page.close();
}
await browser.close();
