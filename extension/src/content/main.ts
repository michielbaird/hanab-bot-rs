// The extension's part on hanab.live: the interface, the `/export/<id>` request and the bot. What it knows about the
// game and the canvas comes from the page bridge (`page.ts`).

import { startBridge } from "./bridge";
import { createController } from "./controller";
import { createEngine } from "./engine";
import { initialState } from "./state";
import { createStore } from "./store";
import { mountComment } from "./ui/comment";
import { mountMarkers } from "./ui/markers";
import { mountPanel } from "./ui/panel";
import { mountProgress } from "./ui/progress";
import { createRoot } from "./ui/root";
import { mountButtons } from "./ui/robot";

const store = createStore(initialState);
const bridge = startBridge((page) => {
  store.update({ page });
});
const controller = createController(store, createEngine(), bridge.goTo);

const root = createRoot();
mountMarkers(root, store, controller);
mountComment(root, store);
mountButtons(root, store, controller);
mountPanel(root, store, controller);
mountProgress(root, store, controller);
