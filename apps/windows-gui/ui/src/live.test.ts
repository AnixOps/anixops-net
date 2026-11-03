import { describe, expect, it } from "vitest";
import { keepIfUnchanged } from "./live";

describe("keepIfUnchanged", () => {
  it("keeps the previous object when the data is the same", () => {
    const previous = { connection: "Connected", nodes: [{ id: "a" }] };
    const next = { connection: "Connected", nodes: [{ id: "a" }] };
    expect(keepIfUnchanged(previous, next)).toBe(previous);
  });

  it("takes the next object when anything changed", () => {
    const previous = { connection: "Connected" };
    const next = { connection: "Disconnected" };
    expect(keepIfUnchanged(previous, next)).toBe(next);
  });

  it("handles null on either side", () => {
    const next = { connection: "Connected" };
    expect(keepIfUnchanged<typeof next | null>(null, next)).toBe(next);
    expect(keepIfUnchanged<typeof next | null>(next, null)).toBeNull();
  });
});
