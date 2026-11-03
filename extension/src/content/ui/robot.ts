import { getStops } from "../analysis";
import { getButtonPositions } from "../layout";
import { isVisible, type ContentState } from "../state";
import type { Store } from "../store";
import type { Controller } from "../controller";
import { el, place } from "./dom";

const TITLE =
  "Bot analysis: the bot goes through this game in your browser and marks every move it would have played differently. " +
  "Unofficial, and often wrong.";

/** The robot that opens the analysis box, and the buttons that jump between the bot's comments. */
export function mountButtons(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  const robot = el("button", "robot", "🤖");
  robot.title = TITLE;
  robot.addEventListener("click", controller.togglePanel);

  const previous = el("button", "nav", "◀!");
  previous.title = "The previous move the bot comments on";
  previous.addEventListener("click", () => {
    controller.jump(-1);
  });

  const next = el("button", "nav", "!▶");
  next.title =
    "The next move the bot comments on (the last stop is the summary)";
  next.addEventListener("click", () => {
    controller.jump(1);
  });

  root.append(robot, previous, next);

  const render = (state: ContentState) => {
    const { info, geometry } = state.page;
    const visible = isVisible(state) && info !== null && geometry !== null;
    robot.hidden = !visible;
    const analysis = state.analysis?.result;
    const jumps =
      visible && analysis !== undefined && analysis.comments.length > 0;
    previous.hidden = !jumps;
    next.hidden = !jumps;
    if (jumps) {
      const stops = getStops(analysis, info.finalSegment);
      previous.disabled = !stops.some((stop) => stop < info.segment);
      next.disabled = !stops.some((stop) => stop > info.segment);
    }
    if (!visible) {
      return;
    }

    const positions = getButtonPositions(geometry);
    place(robot, positions.robot);
    place(next, positions.next);
    place(previous, positions.previous);
  };
  store.subscribe(render);
  render(store.get());
}
