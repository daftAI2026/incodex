import { expect, test } from "bun:test";
import { officialStyleAttributes, syncOfficialButtonAppearance } from "./official-style-attributes.ts";

function button(className: string, attributes: Record<string, string>): HTMLElement {
  const values = new Map(Object.entries(attributes));
  return {
    className,
    getAttribute: (name: string) => values.get(name) ?? null,
    setAttribute: (name: string, value: string) => { values.set(name, value); },
    removeAttribute: (name: string) => { values.delete(name); },
  } as unknown as HTMLElement;
}

test("syncs live Search style changes without copying interaction state", () => {
  const search = button("new-theme", { "data-color": "accent", "data-state": "open" });
  const hat = button("old-theme", { "data-color": "secondary", "data-size": "small", "data-incodex-privacy-toggle": "true" });
  syncOfficialButtonAppearance(search, hat, new Set(["data-color", "data-size"]));
  expect(hat.className).toBe("new-theme");
  expect(hat.getAttribute("data-color")).toBe("accent");
  expect(hat.getAttribute("data-size")).toBeNull();
  expect(hat.getAttribute("data-state")).toBeNull();
  expect(hat.getAttribute("data-incodex-privacy-toggle")).toBe("true");
});

function documentWith(rules: unknown[]): Pick<Document, "styleSheets"> {
  return { styleSheets: [{ cssRules: rules }] } as unknown as Pick<Document, "styleSheets">;
}

test("discovers current and future style attribute names from loaded official rules", () => {
  const names = officialStyleAttributes(documentWith([
    { cssRules: [{ selectorText: ".generated:where([data-icon-size=sm])[data-future-metric]" }] },
    { selectorText: ".generated svg:where(:not([data-no-autosize]))" },
    { styleSheet: { cssRules: [{ selectorText: ".generated[data-next-shape]" }] } },
  ]));
  expect([...names].sort()).toEqual(["data-future-metric", "data-icon-size", "data-next-shape", "data-no-autosize"]);
});

test("excludes official interaction, identity and test state despite CSS references", () => {
  const names = officialStyleAttributes(documentWith([{
    selectorText: "[data-size][data-state][data-testid][data-loading][data-selected][data-disabled][data-incodex-privacy-toggle][data-tracking-id][data-slot][data-component][data-part]",
  }]));
  expect([...names]).toEqual(["data-size"]);
});

test("does not fetch inaccessible stylesheets and still reads available ones", () => {
  const inaccessible = { get cssRules(): never { throw new Error("SecurityError"); } };
  const document = { styleSheets: [inaccessible, { cssRules: [{ selectorText: "[data-icon-size]" }] }] };
  expect([...officialStyleAttributes(document as unknown as Pick<Document, "styleSheets">)]).toEqual(["data-icon-size"]);
});

test("handles recursive imported rules without repeated traversal", () => {
  const sheet: { cssRules: unknown[] } = { cssRules: [] };
  sheet.cssRules.push({ selectorText: "[data-shape]", styleSheet: sheet });
  expect([...officialStyleAttributes({ styleSheets: [sheet] } as unknown as Pick<Document, "styleSheets">)]).toEqual(["data-shape"]);
});

test("discovers style keys from already-adopted constructed stylesheets", () => {
  const document = { styleSheets: [], adoptedStyleSheets: [{ cssRules: [{ selectorText: "[data-future-size]" }] }] };
  expect([...officialStyleAttributes(document as unknown as Document)]).toEqual(["data-future-size"]);
});
