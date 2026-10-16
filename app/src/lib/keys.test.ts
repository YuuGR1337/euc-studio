import { describe, expect, it } from "vitest";
import { comboFromEvent } from "./keys.ts";

const ev = (
  key: string,
  mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {},
) => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("comboFromEvent", () => {
  it("maps ⌘ to mod on macOS and Ctrl elsewhere", () => {
    expect(comboFromEvent(ev("k", { metaKey: true }), true)).toBe("mod+k");
    expect(comboFromEvent(ev("k", { ctrlKey: true }), true)).toBe("k");
    expect(comboFromEvent(ev("k", { ctrlKey: true }), false)).toBe("mod+k");
  });

  it("normalizes space, digits and case", () => {
    expect(comboFromEvent(ev(" "), true)).toBe("space");
    expect(comboFromEvent(ev("1", { metaKey: true }), true)).toBe("mod+1");
    expect(comboFromEvent(ev("B", { metaKey: true, shiftKey: true }), true)).toBe("mod+shift+b");
  });

  it("does not add shift to shifted symbols", () => {
    expect(comboFromEvent(ev("?", { shiftKey: true }), true)).toBe("?");
    expect(comboFromEvent(ev("]"), true)).toBe("]");
  });
});
