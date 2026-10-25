/**
 * [INPUT]: 依赖 renderer 代际、通知、侧栏、Tooltip 与 UI 探针类型。
 * [OUTPUT]: 提供 renderer IPC 合同与 Window 扩展的编译期类型。
 * [POS]: 共享 renderer 桥接类型边界；不生成运行时代码。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { createOfficialNotifications } from "./official-notifications.ts";
import type { createOfficialSidebarEntry } from "./official-sidebar-entry.ts";
import type { sharedTooltipState } from "./official-tooltip-renderer.ts";
import type { deriveUiProbe } from "./incodex-ui-probe.ts";
import type { RendererGenerationScope } from "./renderer-generation.ts";

export type IncognitoAction = "open" | "quit";
export type IncognitoBridgeAction =
  | IncognitoAction
  | "activate-device-key"
  | "configure-dock-menu"
  | "configure-status-menu";
export type IncognitoButtonIcon = "hat-glasses" | "circle-x";

export type IncognitoActionResponse = {
  code?: string;
  ok: boolean;
  reason?: string;
  requestId?: string;
};

declare global {
  interface Window extends RendererGenerationScope {
    __incodexSidebarEntry?: ReturnType<typeof createOfficialSidebarEntry>;
    __incodexSidebarActivate?: () => void;
    __incodexSidebarObserver?: MutationObserver;
    __incodexNotifications?: ReturnType<typeof createOfficialNotifications>;
    __incodexTooltipState?: ReturnType<typeof sharedTooltipState>;
    __incodexStarted?: boolean;
    __incodexIncognito?: boolean;
    __incodexDockMenuConfigured?: boolean;
    __incodexStatusMenuConfigured?: boolean;
    __incodexLocale?: string;
    __incodexPlatform?: string;
    __incodexMutationObserver?: MutationObserver;
    __incodexTooltipPresentationObserver?: MutationObserver;
    __incodexProfileObservationEnabled?: boolean;
    __incodexSearchAppearanceObserver?: MutationObserver;
    __incodexObservedSearch?: HTMLElement;
    __incodexObservedButton?: HTMLElement;
    __incodexProfileMaskHealth?: boolean;
    __incodexRefreshProfileMaskHealth?: () => boolean;
    __incodexUiProbe?: ReturnType<typeof deriveUiProbe>;
    incodex?: {
      requestIncognitoAction?: (payload: {
        action: IncognitoBridgeAction;
        label?: string;
        requestId: string;
      }) => Promise<IncognitoActionResponse>;
    };
  }
}
