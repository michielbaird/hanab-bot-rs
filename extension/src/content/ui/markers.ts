import { getMarkers } from "../layout";
import type { ContentState } from "../state";
import type { Store } from "../store";
import type { Controller } from "../controller";
import { el, place } from "./dom";

/** The ticks above the replay bar, one for every move the bot commented on and one for the end of the game. */
export function mountMarkers(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  const container = el("div", "markers");
  root.append(container);

  let drawn = "";
  store.subscribe((state) => {
    const { info, geometry } = state.page;
    const analysis = state.analysis?.result;
    if (info === null || geometry === null || analysis === undefined) {
      container.replaceChildren();
      drawn = "";
      return;
    }

    // The positions only depend on these, not on the segment being shown.
    const key = JSON.stringify([geometry, info.finalSegment, analysis]);
    if (key === drawn) {
      return;
    }
    drawn = key;

    container.replaceChildren(
      ...getMarkers(geometry, info, analysis).map((marker) => {
        const tick = el("div", "tick");
        tick.style.background = marker.color;
        tick.style.width = `${marker.width}px`;
        tick.style.height = `${marker.height}px`;
        tick.title = marker.title;
        place(tick, marker);
        tick.addEventListener("click", () => {
          controller.goTo(marker.segment);
        });
        return tick;
      }),
    );
  });
}
