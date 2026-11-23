import { describe, expect, it } from "vitest";
import { maskPassword, parsePort, setupUrl } from "./pages/PhoneCaptureCards";

describe("phone debugging", () => {
  it("accepts unprivileged ports only", () => {
    expect(parsePort("8899")).toBe(8899);
    expect(parsePort(" 9090 ")).toBe(9090);
    expect(parsePort("80")).toBeNull();
    expect(parsePort("65536")).toBeNull();
    expect(parsePort("88a")).toBeNull();
    expect(parsePort("")).toBeNull();
  });

  it("builds the setup page address the QR code holds", () => {
    expect(setupUrl("192.168.1.5", 8899)).toBe("http://192.168.1.5:8899/");
  });

  it("hides the share password behind dots of the same length", () => {
    expect(maskPassword("abcdefghjkmnpqrstuvw")).toBe("\u2022".repeat(20));
    expect(maskPassword("abc")).not.toContain("a");
    expect(maskPassword("")).toBe("");
  });
});
