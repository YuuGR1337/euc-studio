import type { EngineClient } from "./types.ts";

// The wheel's buzzer through the speakers. The engine streams every change of the PWM the
// firmware drives (frequency, duty, simulated time of the change); this schedules them with Web
// Audio against simulated time. A square wave, as the pin drives it, through a band-pass around
// a small piezo's resonance; starts and stops ramp over about 2.5 ms so they do not click.

/** A buzzer change: from `t_s` (simulated) on, `hz` at `duty` of the period high; 0 Hz is silent. */
export interface Tone {
  t_s: number;
  hz: number;
  duty: number;
}

export type FirmwareAudioRoute = Pick<EngineClient, "audio" | "setSound">;

/** Simulation state is independent of the selected audio device. */
export function firmwareAudioHold(source: Pick<EngineClient, "status" | "playing">) {
  if (["halted", "error", "unavailable", "completed"].includes(source.status.state))
    return { label: "Stopped", detail: "The simulation stopped. Firmware audio is silent." };
  if (!source.playing)
    return { label: "Paused", detail: "The simulation is paused. Firmware audio is silent." };
  if (source.status.state === "starting")
    return { label: "Starting", detail: "The simulator starts the firmware." };
  return null;
}

/** Pending capability and every native state keep the renderer audio path closed. */
export const usesRendererAudio = (source: FirmwareAudioRoute): boolean =>
  source.audio?.backend === "unsupported";

/** A physical gesture enables the selected route. Native errors never fall back to Web Audio. */
export async function prepareFirmwareAudio(
  source: FirmwareAudioRoute,
  prepareWeb: () => Promise<boolean> = prepareBuzzerAudio,
): Promise<boolean> {
  if (!source.audio) return false;
  if (source.audio.backend === "native") {
    if (source.audio.enabled && source.audio.state === "ready") return true;
    if (!source.setSound) return false;
    try {
      await source.setSound(true);
      return true;
    } catch {
      return false;
    }
  }
  return prepareWeb();
}

/** Audio scheduled ahead of the newest simulated time, s: covers the stream's batching. */
const LEAD_S = 0.12;
/** A jump in the time mapping larger than this resynchronises it instead of following it. */
const RESYNC_S = 0.08;
/** Time constant of the gain ramps (about 2.5 ms to 95 %). */
const RAMP_TAU_S = 0.0008;
/** A typical 12 mm piezo disc resonates near 2.7 kHz. */
const PIEZO_HZ = 2700;
const PIEZO_Q = 2.2;
const MAX_GAIN = 0.22;

// A UI action can unlock Web Audio before React runs the sound effect. Keep that context for
// the player instead of creating a second, suspended context after the gesture has ended.
let preparedContext: AudioContext | null = null;

export function prepareBuzzerAudio(): Promise<boolean> {
  try {
    if (!preparedContext || preparedContext.state === "closed") {
      preparedContext = new AudioContext();
    }
    const ctx = preparedContext;
    return new Promise((resolve) => {
      // Browsers can leave resume pending until a later gesture. Do not lock the UI.
      const timer = window.setTimeout(() => resolve(false), 1500);
      void ctx.resume().then(
        () => {
          window.clearTimeout(timer);
          resolve(ctx.state === "running");
        },
        () => {
          window.clearTimeout(timer);
          resolve(false);
        },
      );
    });
  } catch {
    return Promise.resolve(false);
  }
}

/** Loudness from the duty: the fundamental of a pulse wave scales with sin(pi * duty). */
export const toneGain = (duty: number) =>
  MAX_GAIN * Math.sin(Math.PI * Math.min(1, Math.max(0, duty)));

/** Audio-clock time of simulated time `t_s`, given the mapping `offset` (audio = sim + offset). */
export function nextOffset(offset: number | null, audioNow: number, simNow: number): number {
  const target = audioNow + LEAD_S - simNow;
  if (offset === null || Math.abs(target - offset) > RESYNC_S) return target;
  // Follow slow drift between the two clocks without jitter from batch arrival.
  return offset + (target - offset) * 0.02;
}

export class BuzzerAudio {
  private ctx: AudioContext | null = null;
  private osc: OscillatorNode | null = null;
  private gain: GainNode | null = null;
  private offset: number | null = null;

  get running(): boolean {
    return this.ctx !== null;
  }

  get audible(): boolean {
    return this.ctx?.state === "running";
  }

  subscribeState(listener: () => void): () => void {
    const ctx = this.ctx;
    ctx?.addEventListener("statechange", listener);
    return () => ctx?.removeEventListener("statechange", listener);
  }

  /** Opens the audio output. Call from a user gesture where possible (autoplay rules). */
  start(): void {
    if (this.ctx) {
      void this.ctx.resume().catch(() => {});
      return;
    }
    const ctx =
      preparedContext?.state !== "closed" && preparedContext ? preparedContext : new AudioContext();
    preparedContext = ctx;
    const osc = ctx.createOscillator();
    osc.type = "square";
    osc.frequency.value = PIEZO_HZ;
    const gain = ctx.createGain();
    gain.gain.value = 0;
    const piezo = ctx.createBiquadFilter();
    piezo.type = "bandpass";
    piezo.frequency.value = PIEZO_HZ;
    piezo.Q.value = PIEZO_Q;
    osc.connect(gain).connect(piezo).connect(ctx.destination);
    osc.start();
    Object.assign(this, { ctx, osc, gain, offset: null });
    void ctx.resume().catch(() => {});
  }

  /** Resumes a context the browser started suspended (no gesture yet). */
  resume(): void {
    if (this.audible) return;
    // A suspended clock can retain automation from before the interruption.
    this.silence();
    void this.ctx?.resume().catch(() => {});
  }

  stop(): void {
    if (preparedContext === this.ctx) preparedContext = null;
    void this.ctx?.close().catch(() => {});
    Object.assign(this, { ctx: null, osc: null, gain: null, offset: null });
  }

  /** Schedules tone changes; `simNow` is the newest simulated time the client has. */
  play(tones: Tone[], simNow: number): void {
    const { ctx, osc, gain } = this;
    // Do not queue old alarms against a frozen audio clock while autoplay blocks the context.
    if (!ctx || ctx.state !== "running" || !osc || !gain || tones.length === 0) return;
    const offset = nextOffset(this.offset, ctx.currentTime, simNow);
    if (this.offset !== null && Math.abs(offset - this.offset) > RESYNC_S) this.silence();
    this.offset = offset;
    for (const t of tones) {
      const at = Math.max(ctx.currentTime, t.t_s + this.offset);
      if (t.hz > 0) osc.frequency.setValueAtTime(t.hz, at);
      gain.gain.setTargetAtTime(t.hz > 0 ? toneGain(t.duty) : 0, at, RAMP_TAU_S);
    }
  }

  /** Silences now (pause, reset, audio interruption) and forgets the time mapping. */
  silence(): void {
    const { ctx, gain, osc } = this;
    if (ctx && gain) {
      osc?.frequency.cancelScheduledValues(ctx.currentTime);
      gain.gain.cancelScheduledValues(ctx.currentTime);
      gain.gain.setTargetAtTime(0, ctx.currentTime, RAMP_TAU_S);
    }
    this.offset = null;
  }
}
