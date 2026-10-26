import { describe, expect, it } from "vitest";
import { filterNodes, groupNodes } from "./node-filter";

const nodes = [
  { id: "a", label: "Tokyo Primary", protocol: "Shadowsocks", selected: true },
  { id: "b", label: "Berlin Edge", protocol: "VLESS", selected: false },
];

describe("filterNodes", () => {
  it("combines the text and protocol filters without changing selection", () => {
    expect(filterNodes(nodes, "tokyo", "All")).toEqual([nodes[0]]);
    expect(filterNodes(nodes, "", "VLESS")).toEqual([nodes[1]]);
    expect(filterNodes(nodes, "primary", "VLESS")).toEqual([]);
  });
});

describe("groupNodes", () => {
  it("keeps each subscription's nodes together in the listed order", () => {
    const listed = [
      { id: "a", label: "Tokyo", protocol: "trojan", selected: false, group: "One" },
      { id: "b", label: "Osaka", protocol: "trojan", selected: false, group: "One" },
      { id: "s2.a", label: "Paris", protocol: "vless", selected: true, group: "Two" },
    ];
    expect(groupNodes(listed)).toEqual([
      { group: "One", nodes: [listed[0], listed[1]] },
      { group: "Two", nodes: [listed[2]] },
    ]);
    expect(filterNodes(listed, "two", "All")).toEqual([listed[2]]);
    expect(groupNodes(nodes)).toEqual([{ group: undefined, nodes }]);
    expect(groupNodes([])).toEqual([]);
  });
});
