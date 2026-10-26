import { describe, expect, it } from "vitest";
import { formatBytes, updateIntervalChoices, updateIntervalLabel, usedFraction } from "./subscription-format";

describe("subscription format", () => {
  it("shows traffic in binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(10 * 1024 ** 3)).toBe("10.0 GB");
    expect(formatBytes(200 * 1024 ** 3)).toBe("200 GB");
  });

  it("offers a server interval that is not a preset", () => {
    expect(updateIntervalChoices(24)).not.toContain(5);
    expect(updateIntervalChoices(5)).toContain(5);
    expect(updateIntervalLabel(0)).toBe("手动更新");
    expect(updateIntervalLabel(48)).toBe("每 2 天自动更新");
    expect(updateIntervalLabel(6)).toBe("每 6 小时自动更新");
  });

  it("caps the used share at the total", () => {
    expect(usedFraction(5, 10)).toBe(0.5);
    expect(usedFraction(20, 10)).toBe(1);
    expect(usedFraction(5, 0)).toBeUndefined();
    expect(usedFraction(undefined, 10)).toBeUndefined();
  });
});
