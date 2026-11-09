import { describe, expect, it } from "vitest";
import { isBrowserShortcut } from "./native";

const key = (value: string, modifiers: Partial<{ ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }> = {}) => ({
  key: value,
  ctrlKey: false,
  metaKey: false,
  shiftKey: false,
  ...modifiers,
});

describe("isBrowserShortcut", () => {
  it("blocks reload, print, view source and zoom", () => {
    expect(isBrowserShortcut(key("F5"))).toBe(true);
    expect(isBrowserShortcut(key("r", { ctrlKey: true }))).toBe(true);
    expect(isBrowserShortcut(key("P", { ctrlKey: true }))).toBe(true);
    expect(isBrowserShortcut(key("=", { ctrlKey: true }))).toBe(true);
    expect(isBrowserShortcut(key("0", { metaKey: true }))).toBe(true);
  });

  it("keeps editing shortcuts and the app's own connect shortcut", () => {
    expect(isBrowserShortcut(key("c", { ctrlKey: true }))).toBe(false);
    expect(isBrowserShortcut(key("v", { ctrlKey: true }))).toBe(false);
    expect(isBrowserShortcut(key("a", { ctrlKey: true }))).toBe(false);
    expect(isBrowserShortcut(key("C", { ctrlKey: true, shiftKey: true }))).toBe(false);
    expect(isBrowserShortcut(key("r"))).toBe(false);
  });
});
