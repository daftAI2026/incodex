import { describe, expect, test } from "bun:test";
import { searchButtonPlacement, searchTooltipOpen } from "./search-button-placement.ts";

type FakeNode = {
  tagName: string;
  parentElement: FakeNode | null;
  children: FakeNode[];
  classList: { contains: (name: string) => boolean };
  getAttribute: (name: string) => string | null;
  hasAttribute: (name: string) => boolean;
};

function node({
  tagName = "DIV",
  parent = null,
  classes = [],
  attributes = {},
}: {
  tagName?: string;
  parent?: FakeNode | null;
  classes?: string[];
  attributes?: Record<string, string>;
} = {}): FakeNode {
  const result: FakeNode = {
    tagName,
    parentElement: parent,
    children: [],
    classList: { contains: (name) => classes.includes(name) },
    getAttribute: (name) => attributes[name] ?? null,
    hasAttribute: (name) => Object.hasOwn(attributes, name),
  };
  parent?.children.push(result);
  return result;
}

function groupedTooltipActions(): {
  header: FakeNode;
  actions: FakeNode;
  bellTrigger: FakeNode;
  searchTrigger: FakeNode;
  search: FakeNode;
} {
  const header = node();
  node({ tagName: "H1", parent: header, attributes: { "aria-label": "Workspace title" } });
  const actions = node({ parent: header, classes: ["header-action-group"] });
  const bellTrigger = node({ tagName: "DIV", parent: actions });
  node({
    tagName: "BUTTON",
    parent: bellTrigger,
    attributes: { "aria-label": "Notifications", "data-size": "md", "data-color": "secondary" },
  });
  const searchTrigger = node({
    tagName: "SPAN",
    parent: actions,
    attributes: { "data-state": "closed" },
  });
  const search = node({
    tagName: "BUTTON",
    parent: searchTrigger,
    attributes: { "aria-label": "Search", "data-size": "md", "data-color": "secondary" },
  });
  return { header, actions, bellTrigger, searchTrigger, search };
}

describe("Search button placement", () => {
  test("parks before the official tooltip trigger instead of inside it", () => {
    const header = node();
    const tooltipTrigger = node({
      tagName: "SPAN",
      parent: header,
      classes: ["future-display-contents"],
      attributes: { "data-state": "closed" },
    });
    const search = node({ tagName: "BUTTON", parent: tooltipTrigger });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(header as unknown as HTMLElement);
    expect(placement?.before).toBe(tooltipTrigger as unknown as HTMLElement);
  });

  test("keeps the direct sibling placement when Search has no tooltip trigger wrapper", () => {
    const header = node();
    const search = node({ tagName: "BUTTON", parent: header });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(header as unknown as HTMLElement);
    expect(placement?.before).toBe(search as unknown as HTMLElement);
  });

  test("parks at the left edge of the nearest same-level Search and bell action group", () => {
    const { header, actions, bellTrigger, searchTrigger, search } = groupedTooltipActions();

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(actions as unknown as HTMLElement);
    expect(placement?.before).toBe(bellTrigger as unknown as HTMLElement);
    expect(placement?.before).not.toBe(searchTrigger as unknown as HTMLElement);
    expect(placement?.parent).not.toBe(header as unknown as HTMLElement);
  });

  test("never treats a Search tooltip trigger containing another button as the toolbar group", () => {
    const actions = node();
    node({ tagName: "BUTTON", parent: actions });
    const trigger = node({ tagName: "SPAN", parent: actions, attributes: { "data-state": "closed" } });
    const search = node({ tagName: "BUTTON", parent: trigger, attributes: { "aria-label": "Search" } });
    node({ tagName: "BUTTON", parent: trigger, attributes: { "aria-label": "Tooltip auxiliary action" } });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(actions as unknown as HTMLElement);
    expect(placement?.before).toBe(trigger as unknown as HTMLElement);
  });

  test("does not count an existing injected hat as a first-party toolbar action", () => {
    const { actions, bellTrigger, search } = groupedTooltipActions();
    const hat = node({
      tagName: "BUTTON",
      parent: actions,
      attributes: { "data-incodex-privacy-toggle": "true", "aria-label": "Open private window" },
    });
    actions.children.splice(actions.children.indexOf(hat), 1);
    actions.children.splice(1, 0, hat);

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(actions as unknown as HTMLElement);
    expect(placement?.before).toBe(bellTrigger as unknown as HTMLElement);
  });

  test("keeps the legacy single Search placement without moving it to its header's title edge", () => {
    const header = node();
    node({ tagName: "H1", parent: header });
    const searchTrigger = node({
      tagName: "SPAN",
      parent: header,
      attributes: { "data-state": "closed" },
    });
    const search = node({ tagName: "BUTTON", parent: searchTrigger, attributes: { "aria-label": "Search" } });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(header as unknown as HTMLElement);
    expect(placement?.before).toBe(searchTrigger as unknown as HTMLElement);
  });

  test("keeps Search in its own action group when a different mode control shares the header", () => {
    const header = node();
    node({
      tagName: "BUTTON",
      parent: header,
      attributes: { "data-color": "primary", "data-size": "lg", "aria-label": "Switch mode" },
    });
    const searchActions = node({ parent: header });
    const searchTrigger = node({
      tagName: "SPAN",
      parent: searchActions,
      attributes: { "data-state": "closed" },
    });
    const search = node({
      tagName: "BUTTON",
      parent: searchTrigger,
      attributes: { "data-color": "secondary", "data-size": "md", "aria-label": "Search" },
    });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(searchActions as unknown as HTMLElement);
    expect(placement?.before).toBe(searchTrigger as unknown as HTMLElement);
  });

  test("does not escape an unrelated stateful wrapper", () => {
    const header = node();
    const unrelated = node({
      tagName: "DIV",
      parent: header,
      classes: ["contents"],
      attributes: { "data-state": "closed" },
    });
    const search = node({ tagName: "BUTTON", parent: unrelated });

    const placement = searchButtonPlacement(search as unknown as HTMLElement);
    expect(placement?.parent).toBe(unrelated as unknown as HTMLElement);
    expect(placement?.before).toBe(search as unknown as HTMLElement);
  });

  test("does not promote Search across a title and an unrelated header action", () => {
    const header = node();
    node({ tagName: "H1", parent: header });
    node({ tagName: "BUTTON", parent: header });
    const wrapper = node({ tagName: "SPAN", parent: header, attributes: { "data-state": "closed" } });
    const search = node({ tagName: "BUTTON", parent: wrapper });
    expect(searchButtonPlacement(search as unknown as HTMLElement)?.before).toBe(wrapper as unknown as HTMLElement);
  });

  test("reports an official Search tooltip that remains open through keyboard focus", () => {
    const header = node();
    const tooltipTrigger = node({
      tagName: "SPAN",
      parent: header,
      attributes: { "data-state": "instant-open", "aria-describedby": "_r_tip_" },
    });
    const search = node({ tagName: "BUTTON", parent: tooltipTrigger });

    expect(searchTooltipOpen(search as unknown as HTMLElement)).toBe(true);
  });

  test("does not suppress the injected tooltip after Search closes", () => {
    const header = node();
    const tooltipTrigger = node({
      tagName: "SPAN",
      parent: header,
      attributes: { "data-state": "closed" },
    });
    const search = node({ tagName: "BUTTON", parent: tooltipTrigger });

    expect(searchTooltipOpen(search as unknown as HTMLElement)).toBe(false);
  });
});
