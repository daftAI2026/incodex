export type RendererHooks = {
  start(): void;
  refresh(): void;
  dispose(): void;
  keydown(event: KeyboardEvent): void;
  blur(): void;
  focus(): void;
  dismiss(): void;
  actionResult(ok: boolean): void;
};

type Generation = { id: string; hooks: RendererHooks };
type RendererController = {
  protocol: 1;
  active: Generation | null;
  switching: boolean;
  pending?: () => void;
};
export type RendererGenerationScope = {
  __incodexRendererController?: RendererController;
  __incodexRendererRequest?: { protocol: number; id: string };
  __incodexRendererGeneration?: Readonly<{ protocol: 1; id: string; restartRequired: boolean }>;
  __incodexIncognito?: boolean;
  __incodexStarted?: boolean;
};

// The entry points outlive a generation. They capture the controller, never a
// particular implementation. Session/IPC state belongs to the existing host.
export function installRendererGeneration(
  scope: Window & RendererGenerationScope,
  doc: Document,
  buildId: string,
  create: () => RendererHooks,
): void {
  const request = scope.__incodexRendererRequest ?? { protocol: 1, id: buildId };
  if (request.protocol !== 1 || typeof request.id !== "string" || !request.id) {
    throw new Error("Unsupported Incodex renderer generation");
  }
  let controller = scope.__incodexRendererController;
  if (controller && controller.protocol !== 1) throw new Error("Incompatible Incodex renderer controller");
  if (!controller && scope.__incodexStarted) {
    scope.__incodexRendererGeneration = Object.freeze({ protocol: 1, id: "legacy", restartRequired: true });
    return;
  }
  if (!controller) {
    controller = { protocol: 1, active: null, switching: false };
    scope.__incodexRendererController = controller;
    const stable = controller;
    scope.addEventListener("keydown", event => stable.active?.hooks.keydown(event), true);
    scope.addEventListener("blur", () => stable.active?.hooks.blur());
    scope.addEventListener("focus", () => stable.active?.hooks.focus());
    scope.addEventListener("codex:dismiss-tooltips", () => stable.active?.hooks.dismiss());
    doc.addEventListener("DOMContentLoaded", () => {
      const pending = stable.pending;
      stable.pending = undefined;
      pending?.();
    }, { once: true });
  }
  if (doc.readyState === "loading") {
    const owner = controller, id = request.id;
    controller.pending = () => activateGeneration(scope, owner, id, create);
    return;
  }
  activateGeneration(scope, controller, request.id, create);
}

function activateGeneration(
  scope: RendererGenerationScope,
  controller: RendererController,
  id: string,
  create: () => RendererHooks,
): void {
  if (controller.switching) throw new Error("Incodex renderer switch already in progress");
  const previous = controller.active;
  if (previous?.id === id) {
    previous.hooks.refresh();
    return;
  }
  // A private window keeps its original privacy implementation until normal
  // close. Live privacy migration needs its own real-window acceptance.
  if (previous && scope.__incodexIncognito) {
    scope.__incodexRendererGeneration = Object.freeze({ protocol: 1, id: previous.id, restartRequired: true });
    return;
  }
  const candidate = { id, hooks: create() };
  controller.switching = true;
  try {
    previous?.hooks.dispose();
    controller.active = candidate;
    candidate.hooks.start();
    scope.__incodexRendererGeneration = Object.freeze({ protocol: 1, id: candidate.id, restartRequired: false });
  } catch (error) {
    try { candidate.hooks.dispose(); } finally {
      controller.active = previous;
      previous?.hooks.start();
    }
    throw error;
  } finally {
    controller.switching = false;
  }
}

export function settleRendererAction(scope: RendererGenerationScope, ok: boolean): void {
  scope.__incodexRendererController?.active?.hooks.actionResult(ok);
}
