/**
 * [INPUT]: 依赖 window.__incodexProfileMask、blobatar@2.4.0 与当前 renderer 的 profile DOM
 * [OUTPUT]: 对外提供唯一 profile footer、已打开账号菜单身份行的遮罩、重渲染补挂与 health 判断
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
const PROFILE_MENU_NAME_SELECTOR = ":scope > span.flex-1.min-w-0.truncate";
const PROFILE_MENU_AVATAR_SELECTOR =
  ":scope > img.icon-sm.rounded-full, :scope > span.rounded-full";
const PROFILE_NAME_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-name]";
const PROFILE_AVATAR_MARKER_SELECTOR = ":scope > [data-incodex-profile-mask-avatar]";
const PROFILE_NAME_MAX_CHARS = 64;
const PROFILE_AVATAR_MAX_DATA_URL_CHARS = 8 * 1024 * 1024;

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

declare global {
  interface Window {
    __incodexIncognito?: boolean;
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

export function findProfileFooter(): HTMLElement | null {
  const candidates = [...document.querySelectorAll<HTMLElement>(PROFILE_FOOTER_SELECTOR)].filter(
    (element) =>
      element.querySelector(PROFILE_NAME_SELECTOR) && element.querySelector(PROFILE_AVATAR_SELECTOR),
  );
  return candidates.length === 1 ? candidates[0] : null;
}

export function findProfileMenuIdentity(): HTMLElement | null {
  const candidates = [...document.querySelectorAll<HTMLElement>(PROFILE_MENU_SELECTOR)].flatMap(
    (menu) =>
      [...menu.querySelectorAll<HTMLElement>(PROFILE_MENU_ITEM_SELECTOR)].filter(
        (item) =>
          item.querySelector(PROFILE_MENU_NAME_SELECTOR) &&
          item.querySelector(PROFILE_MENU_AVATAR_SELECTOR),
      ),
  );
  return candidates.length === 1 ? candidates[0] : null;
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
  nameSelector: string,
  avatarSelector: string,
  mask: ResolvedProfileMask,
): boolean {
  const nameHost =
    identity.querySelector<HTMLElement>(PROFILE_NAME_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(nameSelector);
  const avatar =
    identity.querySelector<HTMLElement>(PROFILE_AVATAR_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(avatarSelector);
  if (!nameHost || !avatar || !writeProfileAvatar(avatar, mask)) return false;

  nameHost.setAttribute(PROFILE_MASK_NAME_ATTR, "true");
  nameHost.textContent = mask.name;
  avatar.setAttribute(PROFILE_MASK_AVATAR_ATTR, "true");
  identity.setAttribute(PROFILE_MASK_ATTR, "true");
  return true;
}

function ensureProfileMenuMask(mask: ResolvedProfileMask): void {
  const identity = findProfileMenuIdentity();
  if (!identity) return;
  ensureIdentityMask(identity, PROFILE_MENU_NAME_SELECTOR, PROFILE_MENU_AVATAR_SELECTOR, mask);
}

export function ensureProfileMask(): void {
  if (!window.__incodexIncognito || !profileMaskConfigured()) return;
  const mask = readProfileMask();
  const profileFooter = mask ? findProfileFooter() : null;
  if (!mask || !profileFooter) return;
  if (!ensureIdentityMask(profileFooter, PROFILE_NAME_SELECTOR, PROFILE_AVATAR_SELECTOR, mask)) {
    return;
  }
  ensureProfileMenuMask(mask);
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

function identityMaskHealth(
  identity: HTMLElement,
  nameSelector: string,
  avatarSelector: string,
  mask: ResolvedProfileMask,
): boolean {
  const nameHost =
    identity.querySelector<HTMLElement>(PROFILE_NAME_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(nameSelector);
  const avatar =
    identity.querySelector<HTMLElement>(PROFILE_AVATAR_MARKER_SELECTOR) ??
    identity.querySelector<HTMLElement>(avatarSelector);
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
  if (!mask || !profileFooter) return false;
  if (!identityMaskHealth(profileFooter, PROFILE_NAME_SELECTOR, PROFILE_AVATAR_SELECTOR, mask)) {
    return false;
  }
  const menuIdentity = findProfileMenuIdentity();
  return menuIdentity
    ? identityMaskHealth(
        menuIdentity,
        PROFILE_MENU_NAME_SELECTOR,
        PROFILE_MENU_AVATAR_SELECTOR,
        mask,
      )
    : true;
}

export function profileMaskNeedsInject(): boolean {
  if (!profileMaskConfigured()) return false;
  return !profileMaskHealth();
}
