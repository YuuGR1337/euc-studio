import { describe, expect, it } from "vitest";
import { DemoEngine } from "../sim/synthetic.ts";
import { EngineSwitch } from "./switch.ts";

describe("EngineSwitch", () => {
  it("forwards to the current engine and tells listeners when it swaps", () => {
    const a = new DemoEngine(1);
    const b = new DemoEngine(2);
    b.setPlaying(false);
    const sw = new EngineSwitch(a);
    const engine = sw.client;
    let heard = 0;
    const off = engine.subscribe(() => heard++);
    const subscribe = engine.subscribe;
    expect(engine.subscribe).toBe(subscribe);
    expect(engine.playing).toBe(true);
    expect(engine.time).toBe(a.time);

    engine.setPlaying(false);
    expect(a.playing).toBe(false);
    expect(heard).toBeGreaterThan(0);

    const before = engine.getVersion();
    heard = 0;
    sw.swap(b);
    expect(sw.engine).toBe(b);
    expect(heard).toBe(1);
    expect(engine.getVersion()).toBeGreaterThan(before);
    expect(engine.time).toBe(b.time);
    expect(engine.channels.speed).toBe(b.channels.speed);

    heard = 0;
    a.setPlaying(true);
    expect(heard).toBe(0);
    b.setPlaying(true);
    expect(heard).toBeGreaterThan(0);
    off();
  });

  it("forwards members an engine adds, and leaves out the ones it lacks", () => {
    const a = new DemoEngine(1) as DemoEngine & { extra?: () => string };
    a.extra = function (this: DemoEngine) {
      return this === a ? "bound" : "unbound";
    };
    const sw = new EngineSwitch(a);
    const engine = sw.client as typeof a;
    expect(engine.extra?.()).toBe("bound");
    sw.swap(new DemoEngine(3));
    expect((sw.client as typeof a).extra).toBeUndefined();
  });
});
