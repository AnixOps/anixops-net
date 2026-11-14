import { describe, expect, it } from "vitest";
import { defaultShare, inspectorSize, parseDock, parseShare, shareFor, splitLimits } from "./split-pane";

describe("inspectorSize", () => {
  it("gives the inspector its share of the room", () => {
    expect(inspectorSize("right", 0.4, 1200)).toBe(480);
    expect(inspectorSize("bottom", 0.5, 700)).toBe(350);
  });

  it("keeps both sides at least their minimum", () => {
    expect(inspectorSize("right", 0.05, 1200)).toBe(splitLimits.right.inspector);
    expect(inspectorSize("right", 0.95, 1200)).toBe(1200 - splitLimits.right.table);
    expect(inspectorSize("bottom", 0.99, 700)).toBe(700 - splitLimits.bottom.table);
  });

  it("favours the table when there is no room for both minimums", () => {
    expect(inspectorSize("right", 0.5, 600)).toBe(220);
    expect(inspectorSize("right", 0.5, 300)).toBe(0);
  });
});

describe("shareFor", () => {
  it("turns a dragged position into a share within the limits", () => {
    expect(shareFor("right", 480, 1200)).toBe(0.4);
    expect(shareFor("right", 10, 1200)).toBe(0.283);
    expect(shareFor("right", 480, 0)).toBe(defaultShare.right);
  });

  it("round-trips with inspectorSize", () => {
    for (const pixels of [340, 400, 555, 700]) {
      expect(Math.abs(inspectorSize("right", shareFor("right", pixels, 1280), 1280) - pixels)).toBeLessThanOrEqual(1);
    }
  });
});

describe("stored layout", () => {
  it("reads the dock and falls back to the right", () => {
    expect(parseDock("bottom")).toBe("bottom");
    expect(parseDock("right")).toBe("right");
    expect(parseDock(null)).toBe("right");
    expect(parseDock("left")).toBe("right");
  });

  it("reads a share and ignores nonsense", () => {
    expect(parseShare("right", "0.35")).toBe(0.35);
    expect(parseShare("right", null)).toBe(defaultShare.right);
    expect(parseShare("bottom", "abc")).toBe(defaultShare.bottom);
    expect(parseShare("bottom", "0.99")).toBe(defaultShare.bottom);
    expect(parseShare("right", "")).toBe(defaultShare.right);
  });
});
