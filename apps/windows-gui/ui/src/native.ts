// Browser behaviour that gives a desktop window away as a web page: the page context menu (Back, Reload, Inspect),
// reload and print shortcuts, browser zoom and dragging links or images out of the window. Text fields keep their
// context menu so copy and paste work as usual.

const editable = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.closest("input, textarea, [contenteditable='true'], .selectable") !== null);

/** Whether a key press is a browser shortcut that a native window would not have. */
export function isBrowserShortcut(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "shiftKey">): boolean {
  const key = event.key.toLowerCase();
  if (key === "f5" || key === "f7" || key === "browserback" || key === "browserforward") return true;
  const command = event.ctrlKey || event.metaKey;
  if (!command) return false;
  if (key === "r" || key === "p" || key === "u" || key === "s" || key === "g" || key === "j") return true;
  if (key === "=" || key === "+" || key === "-" || key === "0") return true;
  return false;
}

export function installNativeWindowBehaviour(): void {
  window.addEventListener("contextmenu", (event) => {
    if (!editable(event.target)) event.preventDefault();
  });
  window.addEventListener("keydown", (event) => {
    if (isBrowserShortcut(event)) event.preventDefault();
  });
  window.addEventListener("wheel", (event) => {
    if (event.ctrlKey) event.preventDefault();
  }, { passive: false });
  window.addEventListener("dragstart", (event) => {
    if (!editable(event.target)) event.preventDefault();
  });
}
