export type TooltipLifecycle = {
  pointerEnter: () => void;
  pointerLeave: () => void;
  focus: () => void;
  blur: () => void;
  dismiss: () => void;
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

  const cancelPending = (): void => {
    if (pending === null) return;
    deps.cancel(pending);
    pending = null;
  };

  const hide = (): void => {
    cancelPending();
    if (open) {
      open = false;
      deps.onClose?.();
    }
    deps.hide();
  };

  const scheduleShow = (): void => {
    cancelPending();
    pending = deps.schedule(() => {
      pending = null;
      if (!(hovering || focused) || !deps.canShow()) return;
      open = true;
      deps.onOpen?.(hide);
      if (!open) return;
      deps.show();
    }, deps.resolveDelay?.(deps.delayMs) ?? deps.delayMs);
  };

  return {
    pointerEnter() {
      hovering = true;
      scheduleShow();
    },
    pointerLeave() {
      hovering = false;
      hide();
    },
    focus() {
      focused = true;
      scheduleShow();
    },
    blur() {
      focused = false;
      hide();
    },
    dismiss: hide,
    dispose() {
      hovering = false;
      focused = false;
      hide();
    },
  };
}
