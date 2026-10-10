"use strict";

// 稳定桥只保留 native open 能力；共享业务 factory 不持有原生 owner 或回执队列。
function createWindowsActionController(nativeOpen) {
  let active = null;
  let staged = null;
  const prepared = new WeakSet();
  const responses = new WeakMap();
  async function launch(payload, release) {
    const response = await nativeOpen(payload, release);
    responses.set(payload, response);
    return { ok: response.ok === true, reason: response.ok ? undefined : response.code };
  }
  const deps = Object.freeze({
    isIncognito: () => false,
    launchIncognito: async payload => {
      return launch(payload);
    },
    configureDockMenu: () => false,
    configureStatusMenu: () => false,
    quit: () => { throw new Error("installed Windows bridge accepts only open"); },
  });
  return Object.freeze({
    prepare(factory, id, release) {
      if (typeof factory !== "function" || typeof id !== "string" || !/^[a-f0-9]{64}$/.test(id)) {
        throw new Error("invalid Windows action generation");
      }
      const actions = factory(Object.freeze({ ...deps, launchIncognito: payload => launch(payload, release) }));
      if (actions?.protocol !== 1 || typeof actions.handle !== "function" || typeof actions.open !== "function") {
        throw new Error("unsupported Windows action factory protocol");
      }
      const generation = Object.freeze({ id, actions });
      prepared.add(generation);
      return generation;
    },
    commit(generation) {
      if (!prepared.has(generation)) throw new Error("Windows action generation was not prepared by this controller");
      active = generation;
    },
    stage(generation) {
      if (!prepared.has(generation)) throw new Error("Windows action generation was not prepared by this controller");
      staged = generation;
    },
    stagedGeneration() {
      return staged ? { protocol: 1, id: staged.id } : null;
    },
    commitStaged(id) {
      if (!staged || staged.id !== id) throw new Error("Windows staged action generation changed");
      active = staged;
      staged = null;
      return { protocol: 1, id: active.id };
    },
    generation() {
      return active ? { protocol: 1, id: active.id } : null;
    },
    async request(payload) {
      if (payload?.action !== "open" || typeof payload.requestId !== "string") {
        return { ok: false, code: "UNKNOWN_ACTION" };
      }
      // 捕获请求开始时的一代，热换不会改写进行中的 Promise 或 native 请求。
      const generation = active;
      if (!generation) return nativeOpen(payload);
      try {
        const result = await generation.actions.handle(payload.action, payload, payload);
        const response = responses.get(payload);
        return { ...result, ...(response ? { reason: response.reason } : {}), requestId: payload.requestId };
      } finally {
        responses.delete(payload);
      }
    },
  });
}

module.exports = { createWindowsActionController };
