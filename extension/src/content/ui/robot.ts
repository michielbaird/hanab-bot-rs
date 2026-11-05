import robotSvg from "../../icons/robot.svg";
import { getStops } from "../analysis";
import type { Controller } from "../controller";
import { getButtonPositions } from "../layout";
import { getShownAnalysis, isVisible, type ContentState } from "../state";
import type { Store } from "../store";
import { el, place } from "./dom";

const TITLE =
  "Bot analysis: the bot goes through this game in your browser and marks every move it would have played differently. " +
  "Unofficial, and often wrong.";

function drawRobot(): Node {
  const markup = new DOMParser().parseFromString(robotSvg, "image/svg+xml");
  return document.importNode(markup.documentElement, true);
}

/** The robot that opens the analysis box, and the buttons that jump between the bot's comments. */
export function mountButtons(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  const robot = el("button", "robot");
  robot.append(drawRobot());
  robot.title = TITLE;
  robot.setAttribute("aria-label", "Bot analysis");
  robot.setAttribute("aria-haspopup", "dialog");
  robot.addEventListener("click", controller.togglePanel);

  const previous = el("button", "nav", "◀!");
  previous.title = "The previous move the bot comments on";
  previous.setAttribute("aria-label", previous.title);
  previous.addEventListener("click", () => {
    controller.jump(-1);
  });

  const next = el("button", "nav", "!▶");
  next.title =
    "The next move the bot comments on (the last stop is the summary)";
  next.setAttribute("aria-label", next.title);
  next.addEventListener("click", () => {
    controller.jump(1);
  });

  root.append(robot, previous, next);

  const render = (state: ContentState) => {
    const { info, geometry } = state.page;
    const visible = isVisible(state) && info !== null && geometry !== null;
    robot.hidden = !visible;
    robot.setAttribute("aria-expanded", String(state.panelOpen));

    const analysis = getShownAnalysis(state);
    const jumps = visible && analysis !== null && analysis.comments.length > 0;
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
