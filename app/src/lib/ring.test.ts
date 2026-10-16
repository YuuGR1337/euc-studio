import { describe, expect, it } from "vitest";
import { RingBuffer } from "./ring.ts";

describe("RingBuffer", () => {
  it("rejects a non-positive capacity", () => {
    expect(() => new RingBuffer(0)).toThrow(RangeError);
    expect(() => new RingBuffer(2.5)).toThrow(RangeError);
  });

  it("keeps insertion order before wrapping", () => {
    const r = new RingBuffer(4);
    r.push(1);
    r.push(2);
    r.push(3);
    expect(r.length).toBe(3);
    expect(Array.from(r.toArray())).toEqual([1, 2, 3]);
    expect(r.at(0)).toBe(1);
    expect(r.at(-1)).toBe(3);
    expect(r.last()).toBe(3);
  });

  it("drops the oldest samples after wrapping", () => {
    const r = new RingBuffer(3);
    for (let i = 1; i <= 7; i++) r.push(i);
    expect(r.length).toBe(3);
    expect(Array.from(r.toArray())).toEqual([5, 6, 7]);
    expect(r.at(0)).toBe(5);
    expect(r.at(3)).toBeUndefined();
  });

  it("copies the newest window across the wrap point", () => {
    const r = new RingBuffer(5);
    for (let i = 0; i < 8; i++) r.push(i); // holds 3..7, head in the middle
    const out = new Float64Array(5);
    expect(r.copyTo(out, 4)).toBe(4);
    expect(Array.from(out.subarray(0, 4))).toEqual([4, 5, 6, 7]);
    expect(r.copyTo(out)).toBe(5);
    expect(Array.from(out)).toEqual([3, 4, 5, 6, 7]);
  });

  it("limits copies to the output length", () => {
    const r = new RingBuffer(8);
    r.pushMany([1, 2, 3, 4, 5]);
    const out = new Float32Array(2);
    expect(r.copyTo(out)).toBe(2);
    expect(Array.from(out)).toEqual([4, 5]);
  });

  it("pushMany keeps only what fits", () => {
    const r = new RingBuffer(3);
    r.pushMany([1, 2, 3, 4, 5, 6]);
    expect(Array.from(r.toArray())).toEqual([4, 5, 6]);
  });

  it("returns the fallback when empty and clears", () => {
    const r = new RingBuffer(2);
    expect(r.last(-1)).toBe(-1);
    r.push(9);
    r.clear();
    expect(r.length).toBe(0);
    expect(r.copyTo(new Float64Array(2))).toBe(0);
  });
});
