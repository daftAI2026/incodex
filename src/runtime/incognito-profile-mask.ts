/**
 * [INPUT]: 依赖 window.__incodexProfileMask、blobatar@2.4.0 与当前 renderer 的 profile DOM
 * [OUTPUT]: 对外提供唯一 profile footer、已打开账号菜单身份行的遮罩、头像解码门禁、重渲染补挂与 health 判断
 * [POS]: Runtime profile 隐私边界；只改无痕窗口身份视觉，不接管账号数据、语义或交互行为
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import { blobatarUri } from "blobatar/uri";

const PROFILE_MASK_ATTR = "data-incodex-profile-mask";
const PROFILE_MASK_NAME_ATTR = "data-incodex-profile-mask-name";
const PROFILE_MASK_AVATAR_ATTR = "data-incodex-profile-mask-avatar";
const PROFILE_FOOTER_SELECTOR = 'button.sidebar-item[type="button"]';
const PROFILE_NAME_SELECTOR = ":scope > span.min-w-0.flex-1.truncate";
const PROFILE_AVATAR_SELECTOR = ":scope > img.rounded-full, :scope > span.rounded-full";
const PROFILE_MENU_SELECTOR = '[role="menu"]';
const PROFILE_MENU_ITEM_SELECTOR = '[role="menuitem"]';
const PROFILE_MENU_NAME_SELECTOR = ":scope > div > span.flex-1.min-w-0.truncate";
const PROFILE_MENU_AVATAR_SELECTOR =
  ":scope > div > span > img.icon-sm.rounded-full, :scope > div > span > span.rounded-full";
const PROFILE_NAME_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-name]";
const PROFILE_AVATAR_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-avatar]";
const PROFILE_NAME_MAX_CHARS = 64;
const PROFILE_AVATAR_MAX_DATA_URL_CHARS = 8 * 1024 * 1024;

type ProfileIdentitySelectors = {
  name: string;
  avatar: string;
};

const PROFILE_FOOTER_IDENTITY_SELECTORS: ProfileIdentitySelectors = {
  name: PROFILE_NAME_SELECTOR,
  avatar: PROFILE_AVATAR_SELECTOR,
};
const PROFILE_MENU_IDENTITY_SELECTORS: ProfileIdentitySelectors = {
  name: PROFILE_MENU_NAME_SELECTOR,
  avatar: PROFILE_MENU_AVATAR_SELECTOR,
};

type ProfileMaskAvatar =
  | { kind: "generated"; dataUrl?: never }
  | { dataUrl: string; kind?: never };

type ProfileMaskBootstrap = {
  name: string;
  avatar: ProfileMaskAvatar;
};

type ResolvedProfileMask = {
  name: string;
  avatarDataUrl: string;
};

type ProfileAvatarDecodeState = {
  dataUrl: string;
  probe: HTMLImageElement | null;
  status: "loading" | "ready" | "failed";
};

declare global {
  interface Window {
    __incodexIncognito?: boolean;
    __incodexProfileAvatarDecodeState?: ProfileAvatarDecodeState;
    __incodexProfileMask?: ProfileMaskBootstrap | null;
    __incodexProfileMaskHealth?: boolean;
  }
}

function profileMaskConfigured(): boolean {
  return window.__incodexProfileMask !== null && window.__incodexProfileMask !== undefined;
}

function readProfileMask(): ResolvedProfileMask | null {
  const value = window.__incodexProfileMask;
  if (!value || typeof value.name !== "string" || !value.avatar) return null;
  const name = value.name.trim();
  if (!name || [...name].length > PROFILE_NAME_MAX_CHARS || /\p{Cc}/u.test(name)) {
    return null;
  }

  const avatar = value.avatar;
  if (typeof avatar !== "object" || Array.isArray(avatar)) return null;
  if (avatar.kind === "generated") {
    if (avatar.dataUrl !== undefined) return null;
    return { name, avatarDataUrl: blobatarUri(name, { background: "circle" }) };
  }
  if (typeof avatar.dataUrl !== "string" || avatar.kind !== undefined) return null;
  if (
    avatar.dataUrl.length > PROFILE_AVATAR_MAX_DATA_URL_CHARS ||
    !/^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(avatar.dataUrl)
  ) {
    return null;
  }
  return { name, avatarDataUrl: avatar.dataUrl };
}

function findUniqueCandidate<T>(candidates: T[]): T | null {
  return candidates.length === 1 ? candidates[0] : null;
}

function findUniqueProfileIdentity<T extends HTMLElement>(
  root: ParentNode,
  candidateSelector: string,
  selectors: ProfileIdentitySelectors,
): T | null {
  const candidates = [...root.querySelectorAll<T>(candidateSelector)].filter((element) =>
    Boolean(element.querySelector(selectors.name) && element.querySelector(selectors.avatar)),
  );
  return findUniqueCandidate(candidates);
}

export function findProfileFooter(): HTMLElement | null {
  return findUniqueProfileIdentity(
    document,
    PROFILE_FOOTER_SELECTOR,
    PROFILE_FOOTER_IDENTITY_SELECTORS,
  );
}

function findControlledProfileMenu(profileFooter: HTMLElement): HTMLElement | null {
  const menuId = profileFooter.getAttribute("aria-controls");
  if (!menuId) return null;
  const menu = document.getElementById(menuId);
  if (!menu?.matches(PROFILE_MENU_SELECTOR)) return null;
  return menu;
}

export function findProfileMenuIdentity(profileMenu: HTMLElement): HTMLElement | null {
  return findUniqueProfileIdentity(
    profileMenu,
    PROFILE_MENU_ITEM_SELECTOR,
    PROFILE_MENU_IDENTITY_SELECTORS,
  );
}

function writeProfileAvatar(avatar: HTMLElement, mask: ResolvedProfileMask): boolean {
  if (avatar instanceof HTMLImageElement) {
    avatar.src = mask.avatarDataUrl;
    avatar.style.objectFit = "cover";
    avatar.style.objectPosition = "center";
    return true;
  }
  if (avatar.matches("span.rounded-full")) {
    avatar.textContent = "";
    avatar.style.backgroundImage = `url("${mask.avatarDataUrl}")`;
    avatar.style.backgroundSize = "cover";
    avatar.style.backgroundPosition = "center";
    return true;
  }
  return false;
}

function ensureIdentityMask(
  identity: HTMLElement,
  selectors: ProfileIdentitySelectors,
  mask: ResolvedProfileMask,
): boolean {
  const nameHost =
    identity.querySelector<HTMLElement>(PROFILE_NAME_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(selectors.name);
  const avatar =
    identity.querySelector<HTMLElement>(PROFILE_AVATAR_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(selectors.avatar);
  if (!nameHost || !avatar || !writeProfileAvatar(avatar, mask)) return false;

  nameHost.setAttribute(PROFILE_MASK_NAME_ATTR, "true");
  nameHost.textContent = mask.name;
  avatar.setAttribute(PROFILE_MASK_AVATAR_ATTR, "true");
  identity.setAttribute(PROFILE_MASK_ATTR, "true");
  return true;
}

function ensureProfileMenuMask(profileFooter: HTMLElement, mask: ResolvedProfileMask): void {
  const profileMenu = findControlledProfileMenu(profileFooter);
  if (!profileMenu) return;
  const menuIdentity = findProfileMenuIdentity(profileMenu);
  if (!menuIdentity) return;
  ensureIdentityMask(menuIdentity, PROFILE_MENU_IDENTITY_SELECTORS, mask);
}

export function ensureProfileMask(): void {
  if (!window.__incodexIncognito || !profileMaskConfigured()) return;
  const mask = readProfileMask();
  const profileFooter = mask ? findProfileFooter() : null;
  if (!mask || !profileFooter) return;
  if (!ensureIdentityMask(profileFooter, PROFILE_FOOTER_IDENTITY_SELECTORS, mask)) {
    return;
  }
  ensureProfileMenuMask(profileFooter, mask);
}

function profileAvatarHealth(avatar: HTMLElement, mask: ResolvedProfileMask): boolean {
  if (avatar instanceof HTMLImageElement) {
    return (
      avatar.getAttribute("src") === mask.avatarDataUrl &&
      avatar.style.objectFit === "cover" &&
      avatar.style.objectPosition === "center center"
    );
  }
  return (
    avatar.style.backgroundImage === `url("${mask.avatarDataUrl}")` &&
    avatar.style.backgroundSize === "cover" &&
    avatar.style.backgroundPosition === "center center"
  );
}

function profileAvatarDecoded(dataUrl: string): boolean {
  const current = window.__incodexProfileAvatarDecodeState;
  if (current?.dataUrl === dataUrl) return current.status === "ready";

  const state: ProfileAvatarDecodeState = { dataUrl, probe: null, status: "loading" };
  window.__incodexProfileAvatarDecodeState = state;
  const probe = new Image();
  state.probe = probe;
  const finish = (status: ProfileAvatarDecodeState["status"]): void => {
    if (window.__incodexProfileAvatarDecodeState !== state) return;
    state.status = status;
    state.probe = null;
    window.__incodexProfileMaskHealth = profileMaskHealth();
  };
  probe.addEventListener(
    "load",
    () => finish(probe.naturalWidth > 0 && probe.naturalHeight > 0 ? "ready" : "failed"),
    { once: true },
  );
  probe.addEventListener("error", () => finish("failed"), { once: true });
  probe.src = dataUrl;
  return false;
}

function identityMaskHealth(
  identity: HTMLElement,
  selectors: ProfileIdentitySelectors,
  mask: ResolvedProfileMask,
): boolean {
  const nameHost =
    identity.querySelector<HTMLElement>(PROFILE_NAME_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(selectors.name);
  const avatar =
    identity.querySelector<HTMLElement>(PROFILE_AVATAR_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(selectors.avatar);
  return Boolean(
    nameHost &&
      avatar &&
      identity.getAttribute(PROFILE_MASK_ATTR) === "true" &&
      nameHost.getAttribute(PROFILE_MASK_NAME_ATTR) === "true" &&
      avatar.getAttribute(PROFILE_MASK_AVATAR_ATTR) === "true" &&
      nameHost.textContent === mask.name &&
      profileAvatarHealth(avatar, mask),
  );
}

export function profileMaskHealth(): boolean {
  if (!profileMaskConfigured()) return true;
  const mask = readProfileMask();
  const profileFooter = mask ? findProfileFooter() : null;
  if (!mask || !profileAvatarDecoded(mask.avatarDataUrl) || !profileFooter) return false;
  if (!identityMaskHealth(profileFooter, PROFILE_FOOTER_IDENTITY_SELECTORS, mask)) {
    return false;
  }
  const profileMenu = findControlledProfileMenu(profileFooter);
  if (!profileMenu) return true;
  const menuIdentity = findProfileMenuIdentity(profileMenu);
  if (!menuIdentity) return false;
  return identityMaskHealth(menuIdentity, PROFILE_MENU_IDENTITY_SELECTORS, mask);
}

export function profileMaskNeedsInject(): boolean {
  if (!profileMaskConfigured()) return false;
  return !profileMaskHealth();
}
