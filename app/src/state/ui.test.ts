import { afterEach, describe, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe("startup sound preference", () => {
  it.each([
    [null, true],
    ["true", true],
    ["false", false],
    ["null", true],
    ["invalid JSON", true],
  ])("loads stored preference %s as %s", async (stored, expected) => {
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => (key === "euc-studio.buzzerSound" ? stored : null),
      setItem: vi.fn(),
    });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().buzzerSound).toBe(expected);
    useUi.getState().setBuzzerSound(false);
    expect(localStorage.setItem).toHaveBeenCalledWith("euc-studio.buzzerSound", "false");
    expect(useUi.getState().buzzerSound).toBe(false);
  });

  it("uses enabled sound when storage is blocked", async () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
    });
    const { useUi } = await import("./ui.ts");
    expect(useUi.getState().buzzerSound).toBe(true);
    expect(() => useUi.getState().setBuzzerSound(false)).not.toThrow();
    expect(useUi.getState().buzzerSound).toBe(false);
  });
});
