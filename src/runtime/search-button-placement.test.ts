import { describe, expect, test } from "bun:test";
import { searchButtonPlacement, searchTooltipOpen } from "./search-button-placement";

type FakeNode = {
  tagName: string;
  parentElement: FakeNode | null;
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
  return {
    tagName,
    parentElement: parent,
    classList: { contains: (name) => classes.includes(name) },
    getAttribute: (name) => attributes[name] ?? null,
    hasAttribute: (name) => Object.hasOwn(attributes, name),
  };
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
