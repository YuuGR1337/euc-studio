import { describe, expect, it } from "vitest";
import { matchCommand } from "./match.ts";

describe("matchCommand", () => {
  it("needs every word to start a word of the title or keywords", () => {
    expect(matchCommand("accent green", "Accent color: Green")).toBeGreaterThan(0);
    expect(matchCommand("accent green", "Open component gallery", ["design", "tokens"])).toBe(0);
    expect(matchCommand("gre", "Accent color: Green")).toBeGreaterThan(0);
    expect(matchCommand("reen", "Accent color: Green")).toBe(0);
  });

  it("ranks title hits above keyword hits", () => {
    const title = matchCommand("pause", "Pause simulation", ["play"]);
    const keyword = matchCommand("pause", "Run simulation", ["play", "pause"]);
    expect(title).toBeGreaterThan(keyword);
    expect(keyword).toBeGreaterThan(0);
  });

  it("matches everything for an empty search", () => {
    expect(matchCommand("  ", "Anything")).toBe(1);
  });
});
