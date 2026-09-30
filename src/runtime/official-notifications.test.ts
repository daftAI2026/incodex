import { describe, expect, test } from "bun:test";
import {
  createOfficialNotifications,
  discoverOfficialBannerComponent,
  discoverOfficialBannerCloseIconImport,
  findOfficialToaster,
  initializeOfficialBanner,
  hasExportedBanner,
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
  function CloseIcon() {}
  const modules = {
    Banner,
    CloseIcon,
    createElement: (type: unknown, props: Record<string, unknown>) => ({ type, props }),
    createRoot: () => ({ render(element: unknown) { renders.push(element); }, unmount() { unmounted += 1; } }),
  };
  const manager = createOfficialNotifications(doc, async () => modules);
  return { manager, calls, toaster, area, slot, hosts, renders, Banner, CloseIcon, closed: () => closed, unmounted: () => unmounted };
}
const error = { title: "Unable to open", body: "Multiline explanation", retryLabel: "Try again", onRetry() {} };
const landing = { title: "Incognito", body: "Isolated chats", closeLabel: "Dismiss", icon: "<svg viewBox=\"0 0 16 16\"><path d=\"M1 1\"/></svg>", onClose() {} };

describe("official notifications", () => {
  test("finds the mounted official toaster without a home banner or Search", () => {
    const f = fixture();
    expect(findOfficialToaster(f.manager.document)?.host).toBe(f.area as unknown as HTMLElement);
    expect(findOfficialToaster(f.manager.document)?.toaster).toBe(f.toaster);
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
  test("native dismissal stays dismissed across refresh", async () => {
    const f = fixture(); f.manager.showError(error);
    f.calls[0]!.options.onRemove(); await Promise.resolve(); f.manager.ensure(null, null);
    expect(f.manager.errorPending()).toBe(false); expect(f.calls).toHaveLength(1);
  });
  test("does not resurrect a dismissed warning when DOM reconciliation runs before removal settles", async () => {
    const f = fixture(); f.manager.showError(error);
    f.calls[0]!.options.onRemove();
    await f.manager.ensure(null, null);
    await Promise.resolve();
    expect(f.manager.errorPending()).toBe(false);
    expect(f.calls).toHaveLength(1);
  });
  test("does not treat synchronous child removal before provider detach as acknowledgement", async () => {
    const f = fixture(); f.manager.showError(error);
    f.calls[0]!.options.onRemove(); f.area.isConnected = false;
    await Promise.resolve();
    expect(f.manager.errorPending()).toBe(true);
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
    expect(rendered.props.dismissAction.icon).toBe(f.CloseIcon);
    expect(rendered.props).not.toHaveProperty("className");
    expect(rendered.props).not.toHaveProperty("actionsPlacement");
    expect(rendered.props).not.toHaveProperty("density");
    const count = f.renders.length; await f.manager.ensure(f.slot, landing);
    expect(f.renders).toHaveLength(count);
    await f.manager.ensure(null, null);
    expect(f.unmounted()).toBe(1); expect(f.hosts[0].isConnected).toBe(false);
  });
  test("requests reconciliation when the home slot disappears even if all other controls stay mounted", async () => {
    const f = fixture(); await f.manager.ensure(f.slot, landing);
    f.hosts[0].isConnected = false;
    expect(f.manager.bannerNeedsReconcile(null)).toBe(true);
    await f.manager.ensure(null, null);
    expect(f.manager.bannerNeedsReconcile(null)).toBe(false);
    expect(f.unmounted()).toBe(1);
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
  test("indexes a large official chunk once instead of rescanning it for each export", () => {
    const names = Array.from({ length: 256 }, (_, index) => `Unrelated${index}`);
    const source = `${"/* unrelated packaged source */".repeat(32768)}${names.map((name) => `function ${name}(p){return p}`).join("")}` +
      "function CurrentBanner(p){const{actionsPlacement,attachedToComposer,description,dismissAction,leadingVisual,title}=p;return title}" +
      `export{${names.join(",")},CurrentBanner as Renamed};`;
    let fullSourceSearches = 0;
    let wholeDeclarationScans = 0;
    const counted = Object.assign(new String(source), {
      indexOf(needle: string, from?: number) {
        fullSourceSearches += 1;
        return source.indexOf(needle, from);
      },
      matchAll(pattern: RegExp) {
        if (pattern.source.includes("function ")) wholeDeclarationScans += 1;
        return source.matchAll(pattern);
      },
    });
    expect(hasExportedBanner(counted as unknown as string)).toBe(true);
    expect(fullSourceSearches).toBeLessThanOrEqual(1);
    expect(wholeDeclarationScans).toBe(0);
    expect(hasExportedBanner(source.replaceAll("dismissAction", "unrelatedAction"))).toBe(false);
  });
  test("preserves first-declaration and exported prop-capability recognition", () => {
    const banner = "function Component(p){const{actionsPlacement,attachedToComposer,description,dismissAction,leadingVisual,title}=p;return title}";
    expect(hasExportedBanner(`${banner}export{Component as Current};`)).toBe(true);
    expect(hasExportedBanner(`${banner}export{Unrelated};`)).toBe(false);
    expect(hasExportedBanner(`function Component(p){return p}${" ".repeat(1200)}${banner}export{Component};`)).toBe(false);
    expect(hasExportedBanner(`${banner.replace("dismissAction", "unrelated")}export{Component};`)).toBe(false);
  });
  test("discovers renamed exports by component capabilities, never build names", () => {
    function Renamed(props: any) { const { actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title } = props; return [actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title]; }
    expect(discoverOfficialBannerComponent({ arbitrary: Renamed, unrelated: () => null })).toBe(Renamed);
    expect(() => discoverOfficialBannerComponent({ a: Renamed, b: function Also(props: any) { const { actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title } = props; return [actionsPlacement, attachedToComposer, description, dismissAction, leadingVisual, title]; } })).toThrow();
  });
  test("reads the native dismiss glyph import from the current Banner action component", () => {
    const component = "function Renamed(p){return jsx(Action,{action:p.dismissAction,kind:`dismiss`})}";
    const source = "import {originalGlyph as ChangedIcon} from './arbitrary-generation.js';function Action(p){return jsx(ChangedIcon,{className:styles.desktopDismiss})}function Other(){}";
    expect(discoverOfficialBannerCloseIconImport(component, source)).toEqual({ specifier: "./arbitrary-generation.js", imported: "originalGlyph" });
  });
  test("uses the source export map after a lazy initializer has replaced its exported function", () => {
    const currentJsx: any = {};
    function Banner(props: any) { return currentJsx.jsx("aside", props); }
    const source = "function factory(){return(factory=lazy((()=>{currentJsx=getJsx()})))()}export{factory as changed};";
    let calls = 0;
    initializeOfficialBanner({ changed: () => { calls += 1; } }, Banner, source);
    expect(calls).toBe(1);
  });
  test("initializes the current component's JSX receiver via its own exported factory", () => {
    let currentJsx: any; let count = 0;
    function Banner(props: any) { return currentJsx.jsx("aside", props); }
    let Factory: () => void = () => { return (Factory = (() => { count += 1; currentJsx = { jsx: () => null }; return () => {}; })())(); };
    initializeOfficialBanner({ arbitraryFactory: Factory, component: Banner }, Banner);
    expect(count).toBe(1);
  });
});
