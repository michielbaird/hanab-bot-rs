import {
  CONTENT_SOURCE,
  PAGE_SOURCE,
  type GoToMessage,
  type PageMessage,
  type PageState,
} from "../protocol";

/** Listens to the page bridge (`page.ts`) and tells it where to move the replay. */
export function startBridge(onPage: (page: PageState) => void): {
  goTo: (segment: number) => void;
} {
  window.addEventListener("message", (event: MessageEvent<unknown>) => {
    if (event.source !== window) {
      return;
    }
    const message = event.data as Partial<PageMessage> | null;
    if (message?.source !== PAGE_SOURCE || message.type !== "state") {
      return;
    }
    onPage({ info: message.info ?? null, geometry: message.geometry ?? null });
  });

  return {
    goTo(segment) {
      const message: GoToMessage = {
        source: CONTENT_SOURCE,
        type: "goto",
        segment,
      };
      window.postMessage(message, window.location.origin);
    },
  };
}
