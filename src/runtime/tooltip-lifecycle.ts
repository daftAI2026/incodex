export type TooltipLifecycle = {
  pointerEnter: () => void;
  presentationReady: () => void;
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
  let awaitingPresentation = false;

  function cancelPending(): void {
    if (pending === null) return;
    deps.cancel(pending);
    pending = null;
  }

  function hide(): void {
    awaitingPresentation = false;
    cancelPending();
    if (open) {
      open = false;
      deps.onClose?.();
    }
    deps.hide();
  }

  function scheduleShow(): void {
    awaitingPresentation = false;
    cancelPending();
    if (triggerBlocked) return;
    pending = deps.schedule(() => {
      pending = null;
      if (triggerBlocked || !windowFocused || !(hovering || focused)) return;
      if (!deps.canShow()) {
        awaitingPresentation = true;
        return;
      }
      open = true;
      deps.onOpen?.(hide);
      if (!open) return;
      deps.show();
    }, deps.resolveDelay?.(deps.delayMs) ?? deps.delayMs);
  }

  return {
    presentationReady() {
      // Readiness is not fresh input. All dismissal paths clear this intent;
      // an existing delay also keeps its original deadline.
      if (awaitingPresentation && windowFocused) scheduleShow();
    },
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
      if (windowFocused) restoredFocusBlocked = focused;
      windowFocused = false;
      focused = false;
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
