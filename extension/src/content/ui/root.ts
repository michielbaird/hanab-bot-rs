import styles from "../../content.css";

/** A shadow root on the page, so that the site's styles and ours don't touch each other. */
export function createRoot(): ShadowRoot {
  const host = document.createElement("div");
  host.id = "hanab-analysis";
  const shadow = host.attachShadow({ mode: "open" });
  const style = document.createElement("style");
  style.textContent = styles;
  shadow.append(style);

  const mount = () => {
    document.body.append(host);
  };
  if (document.body === null) {
    document.addEventListener("DOMContentLoaded", mount);
  } else {
    mount();
  }
  return shadow;
}
