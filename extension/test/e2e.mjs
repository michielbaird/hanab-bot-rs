// End-to-end test in a real Chromium: loads the built extension (dist/chrome, built for the fake site, see below), opens
// the fake replay page and goes through an analysis.
//
//   EXT_MATCHES="http://localhost:8099/*" node build.mjs && node test/e2e.mjs
//
// It needs a Chromium that Playwright can find (and its system libraries, and some fonts).

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, realpath } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { GAME_ID, startFakeSite } from "./fake-site.mjs";

const PORT = 8099;
// SCREENSHOTS=<directory> saves pictures of the steps.
const shots = process.env["SCREENSHOTS"];
const extension = await realpath(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "../dist/chrome"),
);
const server = await startFakeSite(PORT);
const userDataDir = await mkdtemp(path.join(os.tmpdir(), "hanab-ext-"));

/** The id of an unpacked extension is made from the path it is loaded from. */
const extensionId = createHash("sha256")
  .update(extension)
  .digest("hex")
  .slice(0, 32)
  .replaceAll(/./g, (c) =>
    String.fromCharCode("a".charCodeAt(0) + Number.parseInt(c, 16)),
  );

const context = await chromium.launchPersistentContext(userDataDir, {
  channel: "chromium",
  headless: true,
  args: [
    `--disable-extensions-except=${extension}`,
    `--load-extension=${extension}`,
  ],
});

const replayURL = (query = "") =>
  `http://localhost:${PORT}/replay/${GAME_ID}${query}`;
const ok = (message) => console.log(`ok   ${message}`);

let failed = false;
try {
  const page = await context.newPage();
  page.on("console", (message) => {
    if (message.type() === "error")
      console.log("  [page error]", message.text());
  });
  // Real clicks (so that a marker covered by something else would fail the test).
  const click = (locator) => locator.click();
  const segment = () =>
    page.evaluate(() => window.globals.store.getState().replay.segment);
  const shot = async (name) => {
    if (shots) await page.screenshot({ path: path.join(shots, `${name}.png`) });
  };

  // The popup holds the settings.
  const popup = await context.newPage();
  const setSetting = async (id, checked) => {
    await popup.goto(`chrome-extension://${extensionId}/popup.html`);
    await popup.locator(`#${id}`).setChecked(checked);
    await popup.waitForTimeout(300);
  };
  await popup.goto(`chrome-extension://${extensionId}/popup.html`);
  assert.equal(
    await popup.locator("#enabled").isChecked(),
    true,
    "the robot is on by default",
  );
  assert.equal(await popup.locator("#convention option").count(), 14);
  ok("the toolbar popup shows the settings, with the robot on by default");

  // An unsupported variant: the robot is shown, and the box says that the bot can't play it.
  await page.goto(
    replayURL(`?variant=${encodeURIComponent("Blind Fours (5 Suits)")}`),
  );
  const robot = page.locator(".robot");
  await robot.waitFor({ state: "visible", timeout: 30_000 });
  assert.equal(await robot.getAttribute("aria-label"), "Bot analysis");
  assert.ok(
    (await robot.locator("svg").count()) === 1,
    "the robot is a drawing, not an emoji",
  );
  await click(robot);
  const panel = page.locator(".panel");
  await panel.waitFor({ state: "visible" });
  await panel
    .locator(".status")
    .getByText(/can't play/)
    .waitFor({ timeout: 30_000 });
  assert.equal(await panel.locator(".start").isDisabled(), true);
  await page.keyboard.press("Escape");
  await panel.waitFor({ state: "hidden" });
  ok(
    "an unsupported variant gets the robot, and the box says why it can't analyze (Escape closes it)",
  );

  // A supported one.
  await page.goto(replayURL());
  await robot.waitFor({ state: "visible", timeout: 30_000 });
  await click(robot);
  await panel.waitFor({ state: "visible" });
  assert.equal(
    await panel.locator("option").count(),
    14,
    "12 H-Group levels, Reactor and RefSieve",
  );
  await panel.locator(".status").waitFor({ state: "hidden", timeout: 30_000 });
  assert.equal(await panel.locator(".start").isDisabled(), false);
  assert.ok(
    (
      await panel
        .locator("a", { hasText: "Report a problem" })
        .getAttribute("href")
    )?.includes(`replay%2F${GAME_ID}`),
  );
  await panel.locator("select").selectOption("HGroup3");
  await shot("1-panel");
  ok("the box checks support and links to a pre-filled report");

  await click(panel.locator(".start"));
  const progress = page.locator(".progress");
  await progress.waitFor({ state: "visible" });
  await progress.getByText(/move \d+ of \d+/).waitFor({ timeout: 60_000 });
  await shot("2-progress");
  await progress.waitFor({ state: "hidden", timeout: 180_000 });
  ok("the analysis runs with a progress bar");

  const ticks = page.locator(".tick");
  assert.equal(await ticks.count(), 13, "12 comments and the summary");
  assert.equal(await ticks.first().getAttribute("role"), "button");
  ok("12 markers and the summary marker, as buttons");

  // The jump buttons: the first comment is on turn 4, which is segment 3.
  const next = page.locator(".nav", { hasText: "▶" });
  const previous = page.locator(".nav", { hasText: "◀" });
  assert.equal(await previous.isDisabled(), true, "nothing before the start");
  await click(next);
  await page.waitForFunction(
    () => window.globals.store.getState().replay.segment === 3,
  );
  const comment = page.locator(".comment");
  await comment.waitFor({ state: "visible", timeout: 5000 });
  assert.match(
    await comment.innerText(),
    /suggest/,
    "the comment for the turn",
  );
  await shot("3-comment");
  ok("the next button moves the replay and the comment is shown");

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
  await shot("4-summary");
  ok("the summary at the end of the game");

  await click(ticks.first());
  await page.waitForTimeout(500);
  assert.notEqual(await segment(), 72, "a marker moves the replay");
  ok("a marker moves the replay");

  // Settings take effect at once.
  await setSetting("mistakesOnly", true);
  await page.waitForFunction(
    () =>
      document
        .querySelector("#hanab-analysis")
        .shadowRoot.querySelectorAll(".tick").length === 9,
  );
  ok("'only show clear mistakes' keeps the 8 mistakes and the summary");
  await setSetting("mistakesOnly", false);
  await page.waitForFunction(
    () =>
      document
        .querySelector("#hanab-analysis")
        .shadowRoot.querySelectorAll(".tick").length === 13,
  );

  // The analysis was kept: the page shows it again without running the bot.
  await page.goto(replayURL());
  await ticks.first().waitFor({ state: "visible", timeout: 10_000 });
  assert.equal(await ticks.count(), 13);
  assert.equal(
    await page.locator(".progress").count(),
    0,
    "no analysis was run",
  );
  ok("a saved analysis comes back when the game is opened again");

  await setSetting("enabled", false);
  await robot.waitFor({ state: "hidden", timeout: 5000 });
  assert.equal(await ticks.count(), 0, "nothing is left on the page");
  ok("turning the extension off removes the robot and the markers");
  await setSetting("enabled", true);
} catch (error) {
  failed = true;
  console.error("FAILED:", error);
} finally {
  await context.close();
  server.close();
}
process.exit(failed ? 1 : 0);
