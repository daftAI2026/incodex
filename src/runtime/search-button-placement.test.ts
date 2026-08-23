import { describe, expect, test } from "bun:test";
import { searchButtonPlacement } from "./search-button-placement";

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
      classes: ["contents"],
      attributes: { "data-state": "closed" },
    });
    const search = node({ tagName: "BUTTON", parent: tooltipTrigger });

    expect(searchButtonPlacement(search as unknown as HTMLElement)).toEqual({
      parent: header,
      before: tooltipTrigger,
    });
  });

  test("keeps the direct sibling placement when Search has no tooltip trigger wrapper", () => {
    const header = node();
    const search = node({ tagName: "BUTTON", parent: header });

    expect(searchButtonPlacement(search as unknown as HTMLElement)).toEqual({
      parent: header,
      before: search,
    });
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

    expect(searchButtonPlacement(search as unknown as HTMLElement)).toEqual({
      parent: unrelated,
      before: search,
    });
  });
});
