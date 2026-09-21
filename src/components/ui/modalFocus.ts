let observers = 0;
let pointerTarget: HTMLElement | null = null;

function rememberPointer(event: Event) {
  pointerTarget =
    event.target instanceof Element
      ? event.target.closest<HTMLElement>("button, a, input, [role=tab]")
      : null;
}

function rememberKeyboard() {
  pointerTarget = null;
}

// Shared across dialogs so a conditionally mounted dialog can still find the
// pointer trigger in WebKit, where clicking buttons does not focus them.
export function observeModalTriggers() {
  if (observers++ === 0) {
    document.addEventListener("pointerdown", rememberPointer, true);
    document.addEventListener("click", rememberPointer, true);
    document.addEventListener("keydown", rememberKeyboard, true);
  }
  return () => {
    if (--observers === 0) {
      document.removeEventListener("pointerdown", rememberPointer, true);
      document.removeEventListener("click", rememberPointer, true);
      document.removeEventListener("keydown", rememberKeyboard, true);
      pointerTarget = null;
    }
  };
}

export function modalTrigger(): HTMLElement | null {
  return pointerTarget?.isConnected ? pointerTarget : null;
}
