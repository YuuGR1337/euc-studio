// One engine object for the whole app whose source can change: the live simulator, or a recording
// played back. Components keep importing `engine`; a swap re-renders everything that listens to
// it, so panels pick up the new rings and bodies. Every member not handled here is forwarded to
// the current engine, so members added to EngineClient later need no change in this file.

import type { Tone } from "./buzzerAudio.ts";
import type { EngineClient } from "./types.ts";

export class EngineSwitch {
  /** The object components use. */
  readonly client: EngineClient;
  private current: EngineClient;
  private detach: () => void;
  private version = 0;
  private dataVersion = 0;
  private listeners = new Set<() => void>();
  private dataListeners = new Set<() => void>();
  private toneListeners = new Set<(tones: Tone[]) => void>();

  constructor(initial: EngineClient) {
    this.current = initial;
    this.detach = this.attach(initial);
    const own: Partial<Record<keyof EngineClient, unknown>> = {
      subscribe: this.subscribe,
      getVersion: this.getVersion,
      subscribeData: this.subscribeData,
      getDataVersion: this.getDataVersion,
      subscribeTones: this.subscribeTones,
    };
    this.client = new Proxy({} as EngineClient, {
      get: (_, prop) => {
        if (typeof prop === "string" && prop in own) return own[prop as keyof EngineClient];
        const value: unknown = Reflect.get(this.current, prop, this.current);
        return typeof value === "function" ? value.bind(this.current) : value;
      },
      set: (_, prop, value) => Reflect.set(this.current, prop, value, this.current),
      has: (_, prop) => prop in own || prop in this.current,
    });
  }

  /** The engine that runs now. */
  get engine(): EngineClient {
    return this.current;
  }

  /** Makes `next` the engine everything talks to, and tells every listener. */
  swap(next: EngineClient): void {
    if (next === this.current) return;
    this.detach();
    this.current = next;
    this.detach = this.attach(next);
    this.emit();
  }

  private attach(engine: EngineClient): () => void {
    const offs = [
      engine.subscribe(() => this.emit()),
      engine.subscribeData(() => this.emitData()),
      engine.subscribeTones((tones) => {
        for (const fn of this.toneListeners) fn(tones);
      }),
    ];
    return () => {
      for (const off of offs) off();
    };
  }

  private emit(): void {
    this.version++;
    for (const fn of this.listeners) fn();
    this.emitData();
  }

  private emitData(): void {
    this.dataVersion++;
    for (const fn of this.dataListeners) fn();
  }

  subscribe = (fn: () => void): (() => void) => {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  };

  getVersion = (): number => this.version;

  subscribeData = (fn: () => void): (() => void) => {
    this.dataListeners.add(fn);
    return () => this.dataListeners.delete(fn);
  };

  getDataVersion = (): number => this.dataVersion;

  subscribeTones = (fn: (tones: Tone[]) => void): (() => void) => {
    this.toneListeners.add(fn);
    return () => this.toneListeners.delete(fn);
  };
}
