import { describe, expect, it } from "vitest";
import { nodeFormProblem } from "./node-form";

describe("nodeFormProblem", () => {
  it("accepts a host and a port in range", () => {
    expect(nodeFormProblem(" hk.example ", " 8443 ")).toBeNull();
    expect(nodeFormProblem("2001:db8::1", "1")).toBeNull();
  });

  it("refuses what the iOS form refuses", () => {
    expect(nodeFormProblem(" ", "443")).not.toBeNull();
    expect(nodeFormProblem("a b", "443")).not.toBeNull();
    expect(nodeFormProblem("a/b", "443")).not.toBeNull();
    expect(nodeFormProblem("a", "0")).not.toBeNull();
    expect(nodeFormProblem("a", "65536")).not.toBeNull();
    expect(nodeFormProblem("a", "44x")).not.toBeNull();
    expect(nodeFormProblem("a", "")).not.toBeNull();
  });
});
