import type { EngineStatus, WheelButton } from "../../engine/types.ts";

export const BUTTON_LEASE_RENEW_MS = 100;
export const ACCESSIBLE_PRESS_MS = 120;
const RELEASE_RETRY_MS = 100;
const RELEASE_RETRIES = 2;

export interface ButtonSession {
  source: object;
  epoch: number;
  setButton(button: WheelButton, pressed: boolean, epoch: number): Promise<string>;
  isCurrent(): boolean;
}
export interface ButtonInputState {
  session: ButtonSession;
  button: WheelButton | null;
  releasing: boolean;
}
interface Hold {
  session: ButtonSession;
  button: WheelButton;
  pending: Set<Promise<string>>;
  heartbeat?: ReturnType<typeof setInterval>;
  pulseEnd?: ReturnType<typeof setTimeout>;
}
interface Release {
  session: ButtonSession;
  button: WheelButton;
  active: boolean;
}

/** A down reaches the engine now. Every release uses the same captured run epoch. */
export class WheelButtonInput {
  private held: Hold | null = null;
  private draining: Release | null = null;

  private changed: (state: ButtonInputState) => void;
  private failed: (session: ButtonSession, error: unknown) => void;
  constructor(
    changed: (state: ButtonInputState) => void = () => {},
    failed: (session: ButtonSession, error: unknown) => void = () => {},
  ) {
    this.changed = changed;
    this.failed = failed;
  }

  subscribe(
    changed: (state: ButtonInputState) => void,
    failed: (session: ButtonSession, error: unknown) => void,
  ): () => void {
    this.changed = changed;
    this.failed = failed;
    return () => {
      this.changed = () => {};
      this.failed = () => {};
    };
  }

  down(session: ButtonSession, button: WheelButton, pulseMs?: number): boolean {
    if (this.held || !session.isCurrent()) return false;
    if (
      this.draining?.session.source === session.source &&
      this.draining.session.epoch === session.epoch
    )
      return false;
    const hold: Hold = { session, button, pending: new Set() };
    this.held = hold;
    this.changed({ session, button, releasing: false });
    this.sendDown(hold);
    // A failing synchronous transport can release the hold inside sendDown.
    if (this.held !== hold) return false;
    hold.heartbeat = setInterval(() => {
      if (this.held !== hold) return;
      if (!session.isCurrent()) {
        this.release();
        return;
      }
      // Never build a queue of renewals behind a slow IPC call.
      if (hold.pending.size === 0) this.sendDown(hold);
    }, BUTTON_LEASE_RENEW_MS);
    if (pulseMs !== undefined) hold.pulseEnd = setTimeout(() => this.release(), pulseMs);
    return true;
  }

  release(): void {
    const hold = this.held;
    if (!hold) {
      const draining = this.draining;
      if (draining && !draining.active) {
        draining.active = true;
        void this.sendUp(draining.session, draining.button).then((ok) =>
          this.finishRelease(draining, ok, RELEASE_RETRIES),
        );
      }
      return;
    }
    this.held = null;
    clearInterval(hold.heartbeat);
    clearTimeout(hold.pulseEnd);
    const { session, button } = hold;
    const draining: Release = { session, button, active: true };
    this.draining = draining;
    this.changed({ session, button: null, releasing: true });
    // Send up immediately, including on blur/unmount. An in-flight down may complete later;
    // send a final up after it, and keep this run locked until the release barrier completes.
    const pending = [...hold.pending];
    const up = this.sendUp(session, button);
    void Promise.all([up, Promise.allSettled(pending)]).then(async ([released]) => {
      // Only an acknowledged up after every pending down can end this interlock.
      const ok = pending.length ? await this.sendUp(session, button) : released;
      this.finishRelease(draining, ok, RELEASE_RETRIES);
    });
  }

  private finishRelease(draining: Release, ok: boolean, retries: number): void {
    if (this.draining !== draining) return;
    if (ok) {
      this.draining = null;
      this.changed({ session: draining.session, button: null, releasing: false });
    } else if (retries > 0) {
      setTimeout(() => {
        if (this.draining !== draining) return;
        void this.sendUp(draining.session, draining.button).then((released) =>
          this.finishRelease(draining, released, retries - 1),
        );
      }, RELEASE_RETRY_MS);
    } else {
      // Stop automatic retries, but retain the lock. A later release() can retry explicitly.
      draining.active = false;
    }
  }

  private invoke(session: ButtonSession, button: WheelButton, pressed: boolean): Promise<string> {
    try {
      return session.setButton(button, pressed, session.epoch);
    } catch (error) {
      return Promise.reject(error);
    }
  }

  private sendDown(hold: Hold): void {
    const request = this.invoke(hold.session, hold.button, true);
    hold.pending.add(request);
    void request
      .catch((error: unknown) => {
        if (hold.session.isCurrent()) this.failed(hold.session, error);
        if (this.held === hold) this.release();
      })
      .finally(() => hold.pending.delete(request));
  }

  private async sendUp(session: ButtonSession, button: WheelButton): Promise<boolean> {
    try {
      await this.invoke(session, button, false);
      return true;
    } catch (error) {
      if (session.isCurrent()) this.failed(session, error);
      return false;
    }
  }
}

/** Native keyboard clicks duplicate a handled key gesture; AT click-only activation does not. */
export class ButtonActivationGuard {
  private target: object | null = null;
  private timer?: ReturnType<typeof setTimeout>;
  keyDown(target: object): void {
    clearTimeout(this.timer);
    this.target = target;
  }
  keyUp(target: object): void {
    this.keyDown(target);
    this.timer = setTimeout(() => {
      this.target = null;
    }, 0);
  }
  acceptsClick(target: object, detail: number): boolean {
    return detail === 0 && this.target !== target;
  }
  dispose(): void {
    clearTimeout(this.timer);
    this.target = null;
  }
}

/** Power can sustain or wake the rail while Handle requires a running, latched controller. */
export function wheelButtonReady(
  status: Pick<EngineStatus, "buttonsReady" | "powerButtonReady">,
  button: WheelButton,
): boolean {
  return status.buttonsReady === true || (button === "power" && status.powerButtonReady === true);
}
