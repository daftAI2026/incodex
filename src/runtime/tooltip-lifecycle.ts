export type TooltipLifecycle = {
  pointerEnter: () => void;
  pointerLeave: () => void;
  focus: () => void;
  blur: () => void;
  windowBlur: () => void;
  windowFocus: () => void;
  dismiss: () => void;
  trigger: () => void;
  dispose: () => void;
};

type TooltipLifecycleDeps = {
  delayMs: number;
  resolveDelay?: (fallbackMs: number) => number;
  schedule: (callback: () => void, delayMs: number) => number;
  cancel: (id: number) => void;
  canShow: () => boolean;
  onOpen?: (close: () => void) => void;
  onClose?: () => void;
  show: () => void;
  hide: () => void;
};

export function createTooltipLifecycle(deps: TooltipLifecycleDeps): TooltipLifecycle {
  let hovering = false;
  let focused = false;
  let open = false;
  let pending: number | null = null;
  let triggerBlocked = false;
  let windowFocused = true;
  let restoredFocusBlocked = false;

  function cancelPending(): void {
    if (pending === null) return;
    deps.cancel(pending);
    pending = null;
  }

  function hide(): void {
    cancelPending();
    if (open) {
      open = false;
      deps.onClose?.();
    }
    deps.hide();
  }

  function scheduleShow(): void {
    cancelPending();
    if (triggerBlocked) return;
    pending = deps.schedule(() => {
      pending = null;
      if (triggerBlocked || !(hovering || focused) || !deps.canShow()) return;
      open = true;
      deps.onOpen?.(hide);
      if (!open) return;
      deps.show();
    }, deps.resolveDelay?.(deps.delayMs) ?? deps.delayMs);
  }

  return {
    pointerEnter() {
      hovering = true;
      restoredFocusBlocked = false;
      scheduleShow();
    },
    pointerLeave() {
      hovering = false;
      hide();
      if (!focused) triggerBlocked = false;
    },
    focus() {
      focused = true;
      if (restoredFocusBlocked) return;
      scheduleShow();
    },
    blur() {
      focused = false;
      hide();
      if (windowFocused) {
        restoredFocusBlocked = false;
        if (!hovering) triggerBlocked = false;
      }
    },
    windowBlur() {
      const focusWasActive = focused;
      windowFocused = false;
      focused = false;
      restoredFocusBlocked = focusWasActive;
      hide();
    },
    windowFocus() {
      windowFocused = true;
    },
    dismiss: hide,
    trigger() {
      triggerBlocked = true;
      hide();
    },
    dispose() {
      hovering = false;
      focused = false;
      triggerBlocked = false;
      windowFocused = true;
      restoredFocusBlocked = false;
      hide();
    },
  };
}
