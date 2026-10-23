// First-ride hints, each cleared by doing it: turn, brake, then the other keys once. A skill shown
// before its hint is due retires the hint silently; seen hints are remembered across sessions.
// The pattern and its timing come from EUC Thrills (src/ui/onboarding.ts): the first hint after
// 0.8 s, 1.6 s between hints, 0.45 s of turning and 0.3 s of braking above 1.5 m/s.

export type HintId = "turn" | "brake" | "keys";

export const HINT_ORDER: readonly HintId[] = ["turn", "brake", "keys"];

export const HINT_TIMING = {
  /** Riding time before the first hint, s. */
  firstS: 0.8,
  /** Quiet time after a hint is done, s. */
  gapS: 1.6,
  /** Turning (half a steer or more) that clears the turn hint, s. */
  turnS: 0.45,
  /** Braking (half a back lean or more, above brakeSpeedMps) that clears the brake hint, s. */
  brakeS: 0.3,
  brakeSpeedMps: 1.5,
  /** How long the key reminder stays, s. */
  keysS: 6,
} as const;

export interface HintInput {
  armed: boolean;
  /** Intent, -1..1. */
  lean: number;
  steer: number;
  /** Wheel speed, m/s, forward positive. */
  speedMps: number;
  dt: number;
}

export class RideHints {
  private readonly seen: Set<HintId>;
  private turnS = 0;
  private brakeS = 0;
  private keysS = 0;
  private ridingS = 0;
  private quietUntil: number = HINT_TIMING.firstS;
  /** Set when a hint became seen in the last step: time to store the list. */
  changed = false;

  constructor(seen: Iterable<HintId> = []) {
    this.seen = new Set([...seen].filter((h) => HINT_ORDER.includes(h)));
  }

  get seenList(): HintId[] {
    return HINT_ORDER.filter((h) => this.seen.has(h));
  }

  private done(hint: HintId) {
    if (this.seen.has(hint)) return;
    this.seen.add(hint);
    this.changed = true;
    this.quietUntil = this.ridingS + HINT_TIMING.gapS;
  }

  /** Advances by one heartbeat; returns the hint to show now, or null. */
  step(input: HintInput): HintId | null {
    this.changed = false;
    const dt = Math.max(0, Math.min(input.dt, 0.25));
    if (!input.armed) {
      this.ridingS = 0;
      this.quietUntil = HINT_TIMING.firstS;
      return null;
    }
    this.ridingS += dt;
    if (Math.abs(input.steer) >= 0.5) this.turnS += dt;
    if (input.lean <= -0.5 && input.speedMps > HINT_TIMING.brakeSpeedMps) this.brakeS += dt;
    if (this.turnS >= HINT_TIMING.turnS) this.done("turn");
    if (this.brakeS >= HINT_TIMING.brakeS) this.done("brake");
    if (this.ridingS < this.quietUntil) return null;
    const next = HINT_ORDER.find((h) => !this.seen.has(h)) ?? null;
    if (next === "keys") {
      this.keysS += dt;
      if (this.keysS >= HINT_TIMING.keysS) {
        this.done("keys");
        return null;
      }
    }
    return next;
  }
}
