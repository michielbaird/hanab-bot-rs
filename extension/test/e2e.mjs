// End-to-end test in a real Chromium: loads the built extension (dist/chrome, built for the fake site, see below), opens
// the fake replay page and goes through an analysis.
//
//   EXT_MATCHES="http://localhost:8099/*" node build.mjs && node test/e2e.mjs
//
// It needs a Chromium that Playwright can find (and its system libraries).

import assert from "node:assert/strict";
import { mkdtemp } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { GAME_ID, startFakeSite } from "./fake-site.mjs";

const PORT = 8099;
// SCREENSHOTS=<directory> saves pictures of the steps.
const shots = process.env["SCREENSHOTS"];
const extension = path.join(path.dirname(fileURLToPath(import.meta.url)), "../dist/chrome");
const server = await startFakeSite(PORT);
const userDataDir = await mkdtemp(path.join(os.tmpdir(), "hanab-ext-"));

const context = await chromium.launchPersistentContext(userDataDir, {
  channel: "chromium",
  headless: true,
  args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
});

let failed = false;
try {
  const page = await context.newPage();
  page.on("console", (message) => {
    if (message.type() === "error") console.log("  [page error]", message.text());
  });
  // Real clicks (so that a marker covered by something else would fail the test). Chromium needs some fonts installed.
  const click = (locator) => locator.click();
  const segment = () => page.evaluate(() => window.globals.store.getState().replay.segment);

  // An unsupported variant: no robot.
  await page.goto(`http://localhost:${PORT}/replay/${GAME_ID}?variant=${encodeURIComponent("Blind Fours (5 Suits)")}`);
  await page.waitForTimeout(3000);
  assert.equal(await page.locator(".robot").isVisible(), false, "the robot must not be shown for an unsupported variant");
  console.log("ok   no robot for an unsupported variant");

  // A supported one.
  await page.goto(`http://localhost:${PORT}/replay/${GAME_ID}`);
  const robot = page.locator(".robot");
  await robot.waitFor({ state: "visible", timeout: 30_000 });
  console.log("ok   the robot is shown for a supported variant");

  await click(robot);
  const panel = page.locator(".panel");
  await panel.waitFor({ state: "visible" });
  assert.match(await panel.innerText(), /Unofficial/, "the panel carries the disclaimer");
  assert.equal(await panel.locator("option").count(), 14, "12 H-Group levels, Reactor and RefSieve");
  await panel.locator("select").selectOption("HGroup3");
  await click(panel.getByText("Analyze"));
  console.log("ok   the panel opens and starts an analysis");

  const shot = async (name) => {
    if (shots) await page.screenshot({ path: path.join(shots, `${name}.png`) });
  };
  await shot("1-panel");

  const progress = page.locator(".progress");
  await progress.waitFor({ state: "visible" });
  await progress.getByText(/move \d+ of \d+/).waitFor({ timeout: 60_000 });
  console.log("ok   the progress bar is shown:", (await progress.innerText()).split("\n")[0]);

  await shot("2-progress");
  await progress.waitFor({ state: "hidden", timeout: 180_000 });
  console.log("ok   the progress bar goes away when the analysis is done");

  const ticks = page.locator(".tick");
  assert.equal(await ticks.count(), 13, "12 comments and the summary");
  console.log("ok   12 markers and the summary marker");

  // The jump buttons: the first comment is on turn 4, which is segment 3.
  const next = page.locator(".nav", { hasText: "▶" });
  const previous = page.locator(".nav", { hasText: "◀" });
  assert.equal(await previous.isDisabled(), true, "nothing before the start");
  await click(next);
  await page.waitForFunction(() => window.globals.store.getState().replay.segment === 3);
  const comment = page.locator(".comment");
  await comment.waitFor({ state: "visible", timeout: 5000 });
  assert.match(await comment.innerText(), /suggest/, "the comment for the turn");
  await shot("3-comment");
  console.log("ok   the next button moves the replay and the comment is shown:", (await comment.innerText()).replaceAll("\n", " | "));

  // Walk to the end: the last stop is the summary.
  for (let i = 0; i < 20 && (await segment()) < 72; i++) {
    if (await next.isDisabled()) break;
    await click(next);
    await page.waitForTimeout(450);
  }
  assert.equal(await segment(), 72, "the last stop is the end of the game");
  await comment.waitFor({ state: "visible" });
  const summary = await comment.innerText();
  assert.match(summary, /12 moves differ/);
  assert.match(summary, /8 clear mistakes/);
  assert.match(summary, /Unofficial/);
  console.log("ok   the summary at the end of the game:", summary.replaceAll("\n", " | "));
  assert.equal(await next.isDisabled(), true, "nothing after the end");
  await shot("4-summary");

  // Clicking a marker moves the replay.
  await click(ticks.first());
  await page.waitForTimeout(500);
  assert.notEqual(await segment(), 72, "a marker moves the replay");
  console.log("ok   a marker moves the replay to segment", await segment());
} catch (error) {
  failed = true;
  console.error("FAILED:", error);
} finally {
  await context.close();
  server.close();
}
process.exit(failed ? 1 : 0);
