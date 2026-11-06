export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  if (text !== undefined) {
    element.textContent = text;
  }
  return element;
}

export function place(
  element: HTMLElement,
  position: { left: number; top: number },
) {
  element.style.left = `${Math.round(position.left)}px`;
  element.style.top = `${Math.round(position.top)}px`;
}
