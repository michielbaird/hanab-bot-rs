import { CONVENTIONS } from "../../conventions";
import { getReportUrl, PRIVACY_PAGE } from "../../links";
import { DISCLAIMER } from "../../text";
import type { Controller } from "../controller";
import { getButtonPositions } from "../layout";
import { getSupport, type ContentState } from "../state";
import type { Store } from "../store";
import { el, place } from "./dom";

const PANEL_WIDTH = 300;
const ROBOT_SIZE = 28;

interface Panel {
  element: HTMLElement;
  render: (state: ContentState) => void;
}

function createPanel(
  store: Store<ContentState>,
  controller: Controller,
): Panel {
  const status = el("div", "status");
  const picker = el("select", "picker");
  for (const [value, label] of CONVENTIONS) {
    picker.append(new Option(label, value));
  }
  picker.value = store.get().settings.convention;
  picker.setAttribute("aria-label", "Convention");

  const acknowledge = el("input", "acknowledge");
  acknowledge.type = "checkbox";
  const acknowledgement = el("label", "acknowledgement");
  acknowledgement.append(
    acknowledge,
    " I understand that this is unofficial and that the bot is often wrong.",
  );

  const start = el("button", "start", "Analyze");
  start.addEventListener("click", () => {
    const label = picker.selectedOptions[0]?.textContent ?? picker.value;
    controller.startAnalysis(picker.value, label);
  });

  const links = el("div", "small");
  const privacy = el("a", "link", "Privacy");
  privacy.href = PRIVACY_PAGE;
  const report = el("a", "link", "Report a problem");
  for (const link of [privacy, report]) {
    link.target = "_blank";
    link.rel = "noopener noreferrer";
  }
  links.append(privacy, " · ", report);

  const element = el("div", "panel");
  element.setAttribute("role", "dialog");
  element.setAttribute("aria-label", "Bot analysis");
  element.append(
    el("div", "title", "Bot analysis"),
    status,
    picker,
    acknowledgement,
    start,
    el("div", "small", DISCLAIMER),
    links,
  );

  const render = (current: ContentState) => {
    const supported = getSupport(current);
    status.hidden = supported === true;
    status.textContent =
      supported === undefined
        ? "Checking whether the bot can play this game..."
        : "The bot can't play this variant or these table options.";

    acknowledgement.hidden = current.settings.acknowledged;
    start.disabled =
      supported !== true ||
      !(current.settings.acknowledged || acknowledge.checked);

    const { info } = current.page;
    report.href = getReportUrl({
      gameID: info?.databaseID ?? null,
      variant: info?.variant ?? null,
      convention: picker.value,
    });
  };
  acknowledge.addEventListener("change", () => {
    render(store.get());
  });
  picker.addEventListener("change", () => {
    render(store.get());
  });

  render(store.get());
  return { element, render };
}

/** The box next to the robot where the convention is chosen. */
export function mountPanel(
  root: ShadowRoot,
  store: Store<ContentState>,
  controller: Controller,
) {
  let panel: Panel | null = null;

  const closeOnEscape = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      controller.closePanel();
    }
  };

  store.subscribe((state) => {
    if (!state.panelOpen) {
      if (panel !== null) {
        panel.element.remove();
        panel = null;
        document.removeEventListener("keydown", closeOnEscape);
      }
      return;
    }

    if (panel !== null) {
      panel.render(state);
      return;
    }

    panel = createPanel(store, controller);
    root.append(panel.element);
    document.addEventListener("keydown", closeOnEscape);

    const { geometry } = state.page;
    if (geometry !== null) {
      const robot = getButtonPositions(geometry).robot;
      place(panel.element, {
        left: Math.max(robot.left + ROBOT_SIZE - PANEL_WIDTH, 4),
        top: robot.top + ROBOT_SIZE + 6,
      });
    }
    panel.element.querySelector<HTMLElement>(".picker")?.focus();
  });
}
