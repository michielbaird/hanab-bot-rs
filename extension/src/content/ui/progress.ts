import type { ContentState, Progress } from "../state";
import type { Store } from "../store";
import type { Controller } from "../controller";
import { el } from "./dom";

/** The box in the middle of the screen while the bot goes through the game, or when that failed. */
export function mountProgress(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  let box: HTMLElement | null = null;
  let shown: Progress | null = null;

  const text = el("div", "text");
  const bar = el("progress", "bar");
  const cancel = el("button", "cancel", "Cancel");
  cancel.addEventListener("click", controller.cancelAnalysis);

  store.subscribe(({ progress }) => {
    if (progress === null) {
      box?.remove();
      box = null;
      shown = null;
      return;
    }

    if (box === null || shown?.kind !== progress.kind) {
      box?.remove();
      box = el("div", "progress");
      if (progress.kind === "running") {
        box.append(text, bar, cancel);
      } else {
        box.append(text);
        box.addEventListener("click", controller.dismissProgress);
      }
      root.append(box);
    }
    shown = progress;

    if (progress.kind === "running") {
      text.className = "text";
      text.textContent = progress.text;
      bar.max = progress.total;
      bar.value = progress.done;
    } else {
      text.className = "text error";
      text.textContent = `The analysis failed: ${progress.message}`;
    }
  });
}
