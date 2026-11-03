import { CONVENTIONS } from "../conventions";
import { getButtonPositions } from "../layout";
import type { ContentState } from "../state";
import type { Store } from "../store";
import type { Controller } from "../controller";
import { el, place } from "./dom";

const DISCLAIMER =
  "Unofficial. Not made or endorsed by hanab.live or the H-Group. The bot is often wrong, " +
  "especially at higher H-Group levels: treat what it says as a second opinion, not as advice.";
const CONVENTION_KEY = "hanab-analysis-convention";
const PANEL_WIDTH = 300;

/** The box next to the robot where the convention is chosen. */
export function mountPanel(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  let panel: HTMLElement | null = null;

  const open = (state: ContentState) => {
    const picker = el("select", "picker");
    for (const [value, label] of CONVENTIONS) {
      picker.append(new Option(label, value));
    }
    picker.value = localStorage.getItem(CONVENTION_KEY) ?? "HGroup3";

    const start = el("button", "start", "Analyze");
    start.addEventListener("click", () => {
      const label = picker.selectedOptions[0]?.textContent ?? picker.value;
      localStorage.setItem(CONVENTION_KEY, picker.value);
      controller.startAnalysis(picker.value, label);
    });

    panel = el("div", "panel");
    panel.append(
      el("div", "title", "Bot analysis"),
      picker,
      start,
      el("div", "small", DISCLAIMER),
    );
    root.append(panel);

    const { geometry } = state.page;
    if (geometry !== null) {
      const robot = getButtonPositions(geometry).robot;
      place(panel, {
        left: Math.max(robot.left + 28 - PANEL_WIDTH, 4),
        top: robot.top + 28 + 6,
      });
    }
  };

  store.subscribe((state) => {
    if (state.panelOpen && panel === null) {
      open(state);
    } else if (!state.panelOpen && panel !== null) {
      panel.remove();
      panel = null;
    }
  });
}
