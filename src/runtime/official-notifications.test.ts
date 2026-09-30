import { describe, expect, test } from "bun:test";
import {
  createOfficialNotifications,
  discoverOfficialBannerComponent,
  findOfficialToaster,
  initializeOfficialBanner,
} from "./official-notifications.ts";

function fixture() {
  const calls: Array<{ title: string; options: Record<string, any> }> = [];
  let closed = 0;
  const toaster = {
    warning(title: string, options: Record<string, any>) {
      calls.push({ title, options });
      return { close() { closed += 1; options.onRemove(); } };
    },
    registerViewport() {},
    custom() {},
  };
  const area = { isConnected: true, __reactFiber$fixture: { memoizedProps: {}, return: { memoizedProps: { toaster } } } };
  const hosts: any[] = [];
  const doc = {
    querySelectorAll: (selector: string) => selector === ".codex-toast-area" ? [area] : [],
    createElement: () => {
      const host = { isConnected: false, attrs: {} as Record<string, string>, setAttribute(k: string, v: string) { this.attrs[k] = v; }, remove() { this.isConnected = false; } };
      hosts.push(host);
      return host;
    },
  } as unknown as Document;
  const slot = { isConnected: true, insertBefore(host: any) { host.isConnected = true; host.parentElement = this; }, firstChild: null } as unknown as HTMLElement;
  const renders: any[] = [];
  let unmounted = 0;
  function Banner() {}
  const modules = {
    Banner,
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => ({ render(element: unknown) { renders.push(element); }, unmount() { unmounted += 1; } }),
  };
  const manager = createOfficialNotifications(doc, async () => modules);
  return { manager, calls, toaster, area, slot, hosts, renders, Banner, closed: () => closed, unmounted: () => unmounted };
}
const error = { title: "Unable to open", body: "Multiline explanation", retryLabel: "Try again", onRetry() {} };
const landing = { title: "Incognito", body: "Isolated chats", closeLabel: "Dismiss", icon: "<svg viewBox=\"0 0 16 16\"><path d=\"M1 1\"/></svg>", onClose() {} };

describe("official notifications", () => {
  test("finds the mounted official toaster without a home banner or Search", () => {
    const f = fixture();
    expect(findOfficialToaster(f.manager.document)).toEqual({ host: f.area, toaster: f.toaster });
  });
  test("rejects ambiguous official toast viewports", () => {
    const f = fixture();
    const doc = { querySelectorAll: () => [f.area, { ...f.area }] } as unknown as Document;
    expect(findOfficialToaster(doc)).toBeNull();
  });
  test("uses the original warning API with inline alignment and persistent own copy", () => {
    const f = fixture();
    f.manager.showError(error);
    f.manager.ensure(null, null);
    expect(f.calls).toHaveLength(1);
    expect(f.calls[0]!.title).toBe(error.title);
    expect(f.calls[0]!.options).toMatchObject({ description: error.body, actionPlacement: "inline", duration: 0, primaryAction: { label: error.retryLabel } });
    expect(f.hosts).toHaveLength(0);
  });
  test("deduplicates refresh and ignores stale removal while replacing an error", () => {
    const f = fixture();
    f.manager.showError(error);
    f.manager.ensure(null, null);
    const stale = f.calls[0]!.options.onRemove;
    f.manager.showError({ ...error, title: "New failure" });
    stale();
    f.manager.ensure(null, null);
    expect(f.calls).toHaveLength(2);
    expect(f.manager.errorPending()).toBe(true);
  });
  test("Retry clears the current warning once and runs the action", () => {
    const f = fixture(); let retry = 0;
    f.manager.showError({ ...error, onRetry() { retry += 1; } });
    f.calls[0]!.options.primaryAction.onClick();
    f.manager.ensure(null, null);
    expect(retry).toBe(1); expect(f.manager.errorPending()).toBe(false);
    expect(f.closed()).toBe(1); expect(f.calls).toHaveLength(1);
  });
  test("native dismissal stays dismissed across refresh", () => {
    const f = fixture(); f.manager.showError(error);
    f.calls[0]!.options.onRemove(); f.manager.ensure(null, null);
    expect(f.manager.errorPending()).toBe(false); expect(f.calls).toHaveLength(1);
  });
  test("keeps the error pending through host teardown and rebinds the replacement viewport", () => {
    const f = fixture(); f.manager.showError(error);
    f.area.isConnected = false; f.calls[0]!.options.onRemove();
    expect(f.manager.errorPending()).toBe(true);
    f.area.isConnected = true; f.manager.ensure(null, null);
    expect(f.calls).toHaveLength(2);
  });
  test("renders privacy content through the official Banner and its defaults", async () => {
    const f = fixture(); await f.manager.ensure(f.slot, landing);
    const rendered = f.renders.at(-1);
    expect(rendered.type).toBe(f.Banner);
    expect(rendered.props.title.props.children).toBe(landing.title);
    expect(rendered.props.description.props.children).toBe(landing.body);
    expect(rendered.props.dismissAction.ariaLabel).toBe(landing.closeLabel);
    expect(rendered.props).not.toHaveProperty("className");
    expect(rendered.props).not.toHaveProperty("actionsPlacement");
    expect(rendered.props).not.toHaveProperty("density");
    const count = f.renders.length; await f.manager.ensure(f.slot, landing);
    expect(f.renders).toHaveLength(count);
    await f.manager.ensure(null, null);
    expect(f.unmounted()).toBe(1); expect(f.hosts[0].isConnected).toBe(false);
  });
  test("does not resurrect privacy content when async preparation resolves after dismissal", async () => {
    const f = fixture(); let resolve!: (m: any) => void;
    const pending = new Promise<any>((r) => { resolve = r; });
    const manager = createOfficialNotifications(f.manager.document, () => pending);
    const first = manager.ensure(f.slot, landing);
    await manager.ensure(null, null);
    resolve({ Banner: f.Banner, createElement: () => ({}), createRoot: () => { throw Error("stale mount"); } });
    await first;
  });
});

describe("current official Banner discovery", () => {
  test("discovers renamed exports by component capabilities, never build names", () => {
    function Renamed(props: any) { const { actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title } = props; return [actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title]; }
    expect(discoverOfficialBannerComponent({ arbitrary: Renamed, unrelated: () => null })).toBe(Renamed);
    expect(() => discoverOfficialBannerComponent({ a: Renamed, b: function Also(props: any) { const { actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title } = props; return [actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title]; } })).toThrow();
  });
  test("initializes the current component's JSX receiver via its own exported factory", () => {
    let currentJsx: any; let count = 0;
    function Banner(props: any) { return currentJsx.jsx("aside", props); }
    function Factory() { count += 1; currentJsx = { jsx: () => null }; }
    initializeOfficialBanner({ arbitraryFactory: Factory, component: Banner }, Banner);
    expect(count).toBe(1);
  });
});
