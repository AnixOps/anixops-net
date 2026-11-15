import { describe, expect, it } from "vitest";
import { containerIds, copyText, initiallyCollapsed, jsonRows, nodeCount, parseJson, prettyJson, prettyTokens } from "./json-view";

describe("parseJson", () => {
  it("keeps the digits of numbers JSON.parse would round", () => {
    const node = parseJson('{"id": 9007199254740993, "price": 1.10, "big": 1e400}');
    expect(node).toEqual({
      kind: "object",
      entries: [
        ["id", { kind: "number", raw: "9007199254740993" }],
        ["price", { kind: "number", raw: "1.10" }],
        ["big", { kind: "number", raw: "1e400" }],
      ],
    });
  });

  it("keeps key order and repeated keys", () => {
    const node = parseJson('{"b":1,"a":2,"b":3}');
    expect(node?.kind === "object" && node.entries.map(([name]) => name)).toEqual(["b", "a", "b"]);
  });

  it("reads escapes, surrogate pairs and a byte order mark", () => {
    expect(parseJson('﻿"a\\n\\u4e2d\\ud83d\\ude00\\/"')).toEqual({ kind: "string", value: "a\n中😀/" });
  });

  it("reads scalars, empty containers and surrounding whitespace", () => {
    expect(parseJson(" true ")).toEqual({ kind: "boolean", value: true });
    expect(parseJson("null")).toEqual({ kind: "null" });
    expect(parseJson("[ ]")).toEqual({ kind: "array", items: [] });
    expect(parseJson("{ }")).toEqual({ kind: "object", entries: [] });
    expect(parseJson("-0.5e-3")).toEqual({ kind: "number", raw: "-0.5e-3" });
  });

  it("refuses what is not one complete JSON value", () => {
    for (const text of ["", "{", '{"a":1', '{"a":1,}', "[1,]", "{'a':1}", '{"a" 1}', "01", "1.", "tru", '"a', '"\\x"', '"\n"', '{"a":1}{"b":2}', "[1] x", "NaN", "undefined"]) {
      expect(parseJson(text), text).toBeNull();
    }
  });

  it("refuses nesting too deep to show instead of overflowing the stack", () => {
    expect(parseJson("[".repeat(5000) + "]".repeat(5000))).toBeNull();
    expect(parseJson("[".repeat(100) + "]".repeat(100))).not.toBeNull();
  });
});

describe("jsonRows", () => {
  const root = parseJson('{"items":[{"id":1,"odd key":"x"},{"id":2}],"total":2,"empty":{}}')!;

  it("lists every value with its depth, name and JSONPath", () => {
    const rows = jsonRows(root, new Set());
    expect(rows.map((row) => [row.depth, row.name ?? row.index ?? null, row.path])).toEqual([
      [0, null, "$"],
      [1, "items", "$.items"],
      [2, 0, "$.items[0]"],
      [3, "id", "$.items[0].id"],
      [3, "odd key", '$.items[0]["odd key"]'],
      [2, 1, "$.items[1]"],
      [3, "id", "$.items[1].id"],
      [1, "total", "$.total"],
      [1, "empty", "$.empty"],
    ]);
    expect(rows[0].open).toBe(true);
    // An empty container has nothing to fold; a scalar has no fold state either.
    expect(rows[8].open).toBeUndefined();
    expect(rows[7].open).toBeUndefined();
  });

  it("hides what is under a folded container", () => {
    const items = jsonRows(root, new Set()).find((row) => row.path === "$.items")!;
    const rows = jsonRows(root, new Set([items.id]));
    expect(rows.map((row) => row.path)).toEqual(["$", "$.items", "$.total", "$.empty"]);
    expect(rows[1].open).toBe(false);
  });

  it("gives a repeated key its own row id and names its parent", () => {
    const rows = jsonRows(parseJson('{"a":1,"a":2}')!, new Set());
    expect(new Set(rows.map((row) => row.id)).size).toBe(3);
    expect(rows[1].parent).toBe(rows[0].id);
    expect(rows[0].parent).toBeNull();
  });
});

describe("folding", () => {
  it("lists containers from a depth, leaving out empty ones", () => {
    const root = parseJson('{"a":{"b":{"c":[1]}},"d":[],"e":[{}]}')!;
    expect(containerIds(root, 0)).toHaveLength(5);
    expect(containerIds(root, 1)).toHaveLength(4);
    expect(containerIds(root, 2)).toHaveLength(2);
  });

  it("opens a small document fully and a large one as an outline", () => {
    const small = parseJson('{"a":{"b":[1,2,3]}}')!;
    expect(initiallyCollapsed(small).size).toBe(0);
    const large = parseJson(JSON.stringify({ rows: Array.from({ length: 300 }, (_, id) => ({ id, tags: ["x"] })) }))!;
    expect(nodeCount(large)).toBeGreaterThan(400);
    const collapsed = initiallyCollapsed(large);
    const rows = jsonRows(large, collapsed);
    // The root, "rows" and its 300 items, each folded.
    expect(rows).toHaveLength(302);
    expect(rows[2].open).toBe(false);
  });

  it("stops counting at the limit", () => {
    const root = parseJson(JSON.stringify(Array.from({ length: 1000 }, (_, id) => id)))!;
    expect(nodeCount(root, 10)).toBe(10);
  });
});

describe("pretty printing", () => {
  it("indents by two spaces and writes numbers as received", () => {
    expect(prettyJson(parseJson('{"id":9007199254740993,"tags":["a","b"],"none":null,"ok":true,"e":{},"l":[]}')!)).toBe(
      '{\n  "id": 9007199254740993,\n  "tags": [\n    "a",\n    "b"\n  ],\n  "none": null,\n  "ok": true,\n  "e": {},\n  "l": []\n}',
    );
  });

  it("round-trips through JSON.parse", () => {
    const value = { text: 'quote " and \\ and \n newline', nested: [{ a: 1.5 }, [], {}], unicode: "中文 😀" };
    expect(JSON.parse(prettyJson(parseJson(JSON.stringify(value))!))).toEqual(value);
  });

  it("marks keys apart from string values", () => {
    const kinds = prettyTokens(parseJson('{"a":"b","n":1,"t":false}')!).filter((token) => token.kind !== "space" && token.kind !== "punctuation");
    expect(kinds).toEqual([
      { kind: "key", text: '"a"' }, { kind: "string", text: '"b"' },
      { kind: "key", text: '"n"' }, { kind: "number", text: "1" },
      { kind: "key", text: '"t"' }, { kind: "literal", text: "false" },
    ]);
  });

  it("copies a string as its text and anything else as JSON", () => {
    expect(copyText({ kind: "string", value: "a\nb" })).toBe("a\nb");
    expect(copyText(parseJson("[1,2]")!)).toBe("[\n  1,\n  2\n]");
    expect(copyText({ kind: "number", raw: "1.10" })).toBe("1.10");
  });
});
