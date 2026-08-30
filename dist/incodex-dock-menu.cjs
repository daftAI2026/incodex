// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createDockMenuController = createDockMenuController;
exports.normalizeDockMenuLabel = normalizeDockMenuLabel;
const OPEN_ITEM_ID = "incodex-open-incognito";
const IDENTITY_ITEM_ID = "incodex-incognito-identity";
const SEPARATOR_ITEM_ID = "incodex-menu-separator";
const MAX_LABEL_LENGTH = 80;
function normalizeDockMenuLabel(value) {
    if (typeof value !== "string")
        return null;
    if (/[\u0000-\u001f\u007f-\u009f]/u.test(value))
        return null;
    const label = value.trim();
    if (!label || [...label].length > MAX_LABEL_LENGTH)
        return null;
    return label;
}
function createDockMenuController(options) {
    const { dock, Menu, MenuItem, isIncognito, onOpen, log } = options;
    const originalSetMenu = typeof dock?.setMenu === "function" ? dock.setMenu.bind(dock) : null;
    let label = null;
    let installed = false;
    function report(error) {
        try {
            log("dock-menu-decoration-failed", { error: String(error) });
        }
        catch {
            /* Logging cannot interfere with the official Dock menu. */
        }
    }
    function ownItemId() {
        return isIncognito ? IDENTITY_ITEM_ID : OPEN_ITEM_ID;
    }
    function findItem(menu, id) {
        if (typeof menu?.getMenuItemById === "function")
            return menu.getMenuItemById(id);
        return menu?.items?.find?.((item) => item?.id === id) ?? null;
    }
    function decorate(menu) {
        if (!menu || !label)
            return menu;
        const existing = findItem(menu, ownItemId());
        if (existing) {
            existing.label = label;
            return menu;
        }
        const hasOfficialItems = Array.isArray(menu.items) && menu.items.length > 0;
        if (hasOfficialItems && !findItem(menu, SEPARATOR_ITEM_ID)) {
            menu.insert(0, new MenuItem({ id: SEPARATOR_ITEM_ID, type: "separator" }));
        }
        menu.insert(0, new MenuItem({
            id: ownItemId(),
            label,
            enabled: !isIncognito,
            click: isIncognito ? undefined : onOpen,
        }));
        return menu;
    }
    function setDecoratedMenu(menu) {
        if (!originalSetMenu)
            return false;
        let decorated = menu;
        let decorationSucceeded = true;
        try {
            decorated = decorate(menu);
        }
        catch (error) {
            decorationSucceeded = false;
            report(error);
        }
        try {
            originalSetMenu(decorated);
            return decorationSucceeded;
        }
        catch (error) {
            report(error);
            return false;
        }
    }
    if (originalSetMenu) {
        try {
            dock.setMenu = function setIncodexDockMenu(menu) {
                setDecoratedMenu(menu);
            };
            installed = true;
        }
        catch (error) {
            report(error);
        }
    }
    function configure(value) {
        const nextLabel = normalizeDockMenuLabel(value);
        if (!installed || !nextLabel || typeof Menu !== "function")
            return false;
        label = nextLabel;
        let menu = null;
        try {
            menu = typeof dock.getMenu === "function" ? dock.getMenu() : null;
            if (!menu)
                menu = new Menu();
            return setDecoratedMenu(menu);
        }
        catch (error) {
            report(error);
            return false;
        }
    }
    return { configure };
}
