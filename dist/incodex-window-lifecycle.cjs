// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.createIncognitoWindowLifecycle = createIncognitoWindowLifecycle;
const WINDOW_CLOSE_SETTLE_MS = 100;
function scheduleCloseProbe(callback, delay) {
    setTimeout(callback, delay);
}
function createIncognitoWindowLifecycle(exit, schedule = scheduleCloseProbe) {
    if (typeof exit !== "function" || typeof schedule !== "function") {
        throw new Error("invalid host window lifecycle");
    }
    const activeWindows = new Set();
    const observedWindows = new WeakSet();
    let exited = false;
    function exitIfEmpty() {
        if (exited || activeWindows.size !== 0)
            return;
        exited = true;
        exit(0);
    }
    function observe(win) {
        if (!win?.on)
            throw new Error("invalid host window lifecycle");
        if (observedWindows.has(win))
            return;
        observedWindows.add(win);
        activeWindows.add(win);
        let active = true;
        let destroyed = false;
        let closeProbeGeneration = 0;
        function activate() {
            if (active || destroyed || exited)
                return;
            active = true;
            activeWindows.add(win);
        }
        function retire() {
            if (!active)
                return;
            active = false;
            activeWindows.delete(win);
            exitIfEmpty();
        }
        win.on("close", (event) => {
            const generation = ++closeProbeGeneration;
            function observeHidden() {
                if (!active || generation !== closeProbeGeneration)
                    return;
                if (win.isDestroyed?.() === true || win.isVisible?.() === false) {
                    retire();
                    return;
                }
                if (event?.defaultPrevented === true)
                    return;
                schedule(observeHidden, WINDOW_CLOSE_SETTLE_MS);
            }
            schedule(observeHidden, WINDOW_CLOSE_SETTLE_MS);
        });
        win.on("closed", () => {
            closeProbeGeneration += 1;
            destroyed = true;
            retire();
        });
        win.on("show", () => {
            closeProbeGeneration += 1;
            activate();
        });
    }
    return { observe };
}
