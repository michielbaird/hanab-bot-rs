import { getCommentBox } from "../layout";
import { getShownAnalysis, type ContentState } from "../state";
import type { Store } from "../store";
import { el, place } from "./dom";

/** The bot's comment on the turn being shown, under the replay controls; at the end of the game, the summary. */
export function mountComment(root: ShadowRoot, store: Store<ContentState>) {
  const comment = el("div", "comment");
  comment.hidden = true;
  comment.setAttribute("role", "status");
  root.append(comment);

  store.subscribe((state) => {
    const { info, geometry } = state.page;
    const analysis = getShownAnalysis(state);
    const box =
      info !== null &&
      geometry !== null &&
      analysis !== null &&
      state.analysis !== null
        ? getCommentBox(geometry, info, analysis, state.analysis.convention)
        : null;

    comment.hidden = box === null;
    if (box === null) {
      return;
    }

    comment.textContent = box.text;
    comment.classList.toggle("mistake", box.mistake);
    comment.style.width = `${box.width}px`;
    place(comment, box);
  });
}
