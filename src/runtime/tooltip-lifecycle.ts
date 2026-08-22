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
  schedule: (callback: () => void, delayMs: number) => number;
  cancel: (id: number) => void;
  canShow: () => boolean;
  show: () => void;
  hide: () => void;
};

export function createTooltipLifecycle(deps: TooltipLifecycleDeps): TooltipLifecycle {
  let hovering = false;
  let focused = false;
  let pending: number | null = null;

  const cancelPending = (): void => {
    if (pending === null) return;
    deps.cancel(pending);
    pending = null;
  };

  const hide = (): void => {
    cancelPending();
    deps.hide();
  };

  const scheduleShow = (): void => {
    cancelPending();
    pending = deps.schedule(() => {
      pending = null;
      if (!(hovering || focused) || !deps.canShow()) return;
      deps.show();
    }, deps.delayMs);
  };

  return {
    pointerEnter() {
      hovering = true;
      scheduleShow();
    },
    pointerLeave() {
      hovering = false;
      if (!focused) hide();
    },
    focus() {
      focused = true;
      scheduleShow();
    },
    blur() {
      focused = false;
      if (!hovering) hide();
    },
    dismiss: hide,
    dispose() {
      hovering = false;
      focused = false;
      hide();
    },
  };
}
