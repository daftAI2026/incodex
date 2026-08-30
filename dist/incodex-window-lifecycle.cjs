// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.exitAfterLastMainWindowCloses = exitAfterLastMainWindowCloses;
const WINDOW_CLOSE_SETTLE_MS = 100;
function scheduleCloseProbe(callback, delay) {
    setTimeout(callback, delay);
}
function exitAfterLastMainWindowCloses(win, hasAnotherMainWindow, exit, schedule = scheduleCloseProbe) {
    if (!win?.on ||
        typeof hasAnotherMainWindow !== "function" ||
        typeof exit !== "function" ||
        typeof schedule !== "function") {
        throw new Error("invalid host window lifecycle");
    }
    let exited = false;
    let closeProbeGeneration = 0;
    function exitIfLast(requireHidden) {
        if (exited || hasAnotherMainWindow())
            return;
        if (requireHidden && win.isDestroyed?.() !== true && win.isVisible?.() !== false)
            return;
        exited = true;
        exit(0);
    }
    win.on("close", () => {
        const generation = ++closeProbeGeneration;
        function observeHidden() {
            if (exited || generation !== closeProbeGeneration)
                return;
            if (win.isDestroyed?.() === true || win.isVisible?.() === false) {
                exitIfLast(false);
                return;
            }
            schedule(observeHidden, WINDOW_CLOSE_SETTLE_MS);
        }
        schedule(observeHidden, WINDOW_CLOSE_SETTLE_MS);
    });
    win.on("closed", () => {
        closeProbeGeneration += 1;
        exitIfLast(false);
    });
}
