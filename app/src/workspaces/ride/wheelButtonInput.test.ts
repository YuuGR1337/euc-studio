import { wheelButtonReady } from "./wheelButtonInput.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ACCESSIBLE_PRESS_MS,
  ButtonActivationGuard,
  WheelButtonInput,
  type ButtonSession,
} from "./wheelButtonInput.ts";

function setup() {
  const setButton = vi.fn<ButtonSession["setButton"]>().mockResolvedValue("ok");
  const failed = vi.fn();
  const changed = vi.fn();
  const source = {};
  let epoch = 7;
  const session: ButtonSession = { source, epoch, setButton, isCurrent: () => epoch === 7 };
  return {
    input: new WheelButtonInput(changed, failed),
    session,
    setButton,
    changed,
    failed,
    changeRun: () => {
      epoch = 8;
    },
  };
}
afterEach(() => {
  vi.useRealTimers();
});

describe("live wheel input", () => {
  it("sends down immediately, renews at 100 ms, and stops renewal on up", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    expect(input.down(session, "power")).toBe(true);
    expect(setButton).toHaveBeenCalledWith("power", true, 7);
    await vi.advanceTimersByTimeAsync(99);
    expect(setButton).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(setButton).toHaveBeenNthCalledWith(2, "power", true, 7);
    input.release();
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    await vi.advanceTimersByTimeAsync(500);
    expect(setButton).toHaveBeenCalledTimes(3);
  });

  it("releases the captured epoch when the run changes", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changeRun } = setup();
    input.down(session, "handle");
    changeRun();
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton).toHaveBeenLastCalledWith("handle", false, 7);
    expect(setButton.mock.calls.every((call) => call[2] === 7)).toBe(true);
    expect(input.down(session, "handle")).toBe(false);
  });

  it("ends a click-only activation after a real 120 ms hold", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    input.down(session, "handle", ACCESSIBLE_PRESS_MS);
    await vi.advanceTimersByTimeAsync(119);
    expect(setButton.mock.calls.every((call) => call[1])).toBe(true);
    await vi.advanceTimersByTimeAsync(1);
    expect(setButton).toHaveBeenLastCalledWith("handle", false, 7);
    await vi.advanceTimersByTimeAsync(500);
    expect(setButton.mock.calls.filter((call) => call[1])).toHaveLength(2);
  });

  it("sends up now and again after a late down acknowledgment", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    let acknowledge!: (value: string) => void;
    setButton.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          acknowledge = resolve;
        }),
    );
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(400);
    expect(setButton).toHaveBeenCalledTimes(1); // No accumulated renewal queue.
    input.release();
    expect(setButton).toHaveBeenNthCalledWith(2, "power", false, 7);
    expect(input.down(session, "power")).toBe(false);
    acknowledge("ok");
    await vi.advanceTimersByTimeAsync(0);
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    expect(setButton).toHaveBeenCalledTimes(3);
    expect(input.down(session, "power")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("releases on cancel and cancels the accessible pulse timer", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    input.down(session, "power", ACCESSIBLE_PRESS_MS);
    await vi.advanceTimersByTimeAsync(20);
    input.release();
    input.release();
    await vi.advanceTimersByTimeAsync(500);
    expect(setButton.mock.calls).toEqual([
      ["power", true, 7],
      ["power", false, 7],
    ]);
  });

  it("keeps a late old-run release out of the replacement run", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changeRun } = setup();
    let acknowledge!: (value: string) => void;
    setButton.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          acknowledge = resolve;
        }),
    );
    input.down(session, "power");
    changeRun();
    input.release();
    const next: ButtonSession = { ...session, epoch: 8, isCurrent: () => true };
    expect(input.down(next, "power")).toBe(true);
    acknowledge("ok");
    await vi.advanceTimersByTimeAsync(0);
    expect(setButton.mock.calls.filter((call) => call[2] === 8)).toEqual([["power", true, 8]]);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(setButton).toHaveBeenLastCalledWith("power", false, 8);
  });

  it("reports a failed down and still releases its captured input", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, failed } = setup();
    setButton.mockRejectedValueOnce(new Error("transport stopped"));
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(500);
    expect(failed).toHaveBeenCalledOnce();
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    expect(setButton.mock.calls.filter((call) => call[1])).toHaveLength(1);
  });

  it("keeps a failed release locked until a successful up, without renewing the old hold", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changed, failed } = setup();
    let level = false;
    let failUp = true;
    const edges: boolean[] = [];
    setButton.mockImplementation(async (_button, pressed) => {
      if (!pressed && failUp) throw new Error("up transport failed");
      if (level !== pressed) edges.push(pressed);
      level = pressed;
      return "ok";
    });
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(failed).toHaveBeenCalledOnce();
    expect(input.down(session, "power")).toBe(false);
    expect(input.down(session, "handle")).toBe(false);
    expect(changed).toHaveBeenLastCalledWith({ session, button: null, releasing: true });
    expect(edges).toEqual([true]);
    await vi.advanceTimersByTimeAsync(99);
    expect(setButton).toHaveBeenCalledTimes(2);
    failUp = false;
    await vi.advanceTimersByTimeAsync(1);
    expect(level).toBe(false);
    expect(changed).toHaveBeenLastCalledWith({ session, button: null, releasing: false });
    expect(input.down(session, "power")).toBe(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(edges).toEqual([true, false, true]);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("bounds failed up retries and permits an explicit release retry after transport recovery", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changed } = setup();
    setButton.mockImplementation(async (_button, pressed) => {
      if (!pressed) throw new Error("up transport failed");
      return "ok";
    });
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    input.release();
    await vi.advanceTimersByTimeAsync(1000);
    expect(setButton.mock.calls.filter((call) => !call[1])).toHaveLength(3);
    expect(setButton.mock.calls.filter((call) => call[1])).toHaveLength(1);
    expect(input.down(session, "power")).toBe(false);
    expect(changed).toHaveBeenLastCalledWith({ session, button: null, releasing: true });
    await vi.advanceTimersByTimeAsync(10_000);
    expect(setButton).toHaveBeenCalledTimes(4);
    setButton.mockResolvedValue("ok");
    input.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    expect(changed).toHaveBeenLastCalledWith({ session, button: null, releasing: false });
    expect(input.down(session, "handle")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("requires a successful final up after a pending down even when the first up succeeded", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changed } = setup();
    let acknowledge!: (value: string) => void;
    setButton
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            acknowledge = resolve;
          }),
      )
      .mockResolvedValueOnce("released before down settled")
      .mockRejectedValueOnce(new Error("final up failed"));
    input.down(session, "power");
    input.release();
    await vi.advanceTimersByTimeAsync(0);
    expect(input.down(session, "power")).toBe(false);
    acknowledge("late down");
    await vi.advanceTimersByTimeAsync(0);
    expect(setButton).toHaveBeenCalledTimes(3);
    expect(input.down(session, "power")).toBe(false);
    expect(changed).toHaveBeenLastCalledWith({ session, button: null, releasing: true });
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    expect(input.down(session, "power")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("keeps releases serialized while an up is pending", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    let reject!: (error: Error) => void;
    setButton.mockImplementationOnce(
      () =>
        new Promise((_resolve, fail) => {
          reject = fail;
        }),
    );
    input.release();
    for (let i = 0; i < 20; i++) input.release();
    await vi.advanceTimersByTimeAsync(1000);
    expect(setButton).toHaveBeenCalledTimes(2);
    expect(input.down(session, "power")).toBe(false);
    reject(new Error("up failed"));
    await vi.advanceTimersByTimeAsync(0);
    expect(input.down(session, "power")).toBe(false);
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton).toHaveBeenCalledTimes(3);
    expect(input.down(session, "power")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("retries captured releases while paused without renewing a down", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    let current = true;
    session.isCurrent = () => current;
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    current = false;
    setButton.mockRejectedValueOnce(new Error("up failed while paused"));
    input.release();
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton.mock.calls).toEqual([
      ["power", true, 7],
      ["power", false, 7],
      ["power", false, 7],
    ]);
    current = true;
    expect(input.down(session, "power")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });

  it("never sends an old release retry to a replacement epoch or clears its interlock", async () => {
    vi.useFakeTimers();
    const { input, session, setButton, changeRun, changed } = setup();
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    setButton.mockRejectedValueOnce(new Error("old up failed"));
    input.release();
    await vi.advanceTimersByTimeAsync(0);
    changeRun();
    const next = { ...session, epoch: 8, isCurrent: () => true };
    input.down(next, "handle");
    await vi.advanceTimersByTimeAsync(0);
    let acknowledge!: (value: string) => void;
    setButton.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          acknowledge = resolve;
        }),
    );
    input.release();
    await vi.advanceTimersByTimeAsync(100);
    expect(input.down(next, "handle")).toBe(false);
    expect(changed).toHaveBeenLastCalledWith({ session: next, button: null, releasing: true });
    expect(setButton.mock.calls.filter((call) => call[2] === 8)).toEqual([
      ["handle", true, 8],
      ["handle", false, 8],
    ]);
    acknowledge("new up succeeded");
    await vi.advanceTimersByTimeAsync(0);
    expect(input.down(next, "handle")).toBe(true);
    input.release();
    await vi.advanceTimersByTimeAsync(0);
  });
});

describe("accessible activation", () => {
  it("accepts click-only activation but blocks pointer and duplicate keyboard clicks", async () => {
    vi.useFakeTimers();
    const guard = new ButtonActivationGuard();
    const target = {};
    expect(guard.acceptsClick(target, 0)).toBe(true);
    expect(guard.acceptsClick(target, 1)).toBe(false);
    guard.keyDown(target);
    expect(guard.acceptsClick(target, 0)).toBe(false);
    guard.keyUp(target);
    expect(guard.acceptsClick(target, 0)).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(guard.acceptsClick(target, 0)).toBe(true);
    guard.dispose();
  });
});

describe("physical button readiness", () => {
  it("admits Power wake separately from Handle input", () => {
    expect(wheelButtonReady({ buttonsReady: false, powerButtonReady: true }, "power")).toBe(true);
    expect(wheelButtonReady({ buttonsReady: false, powerButtonReady: true }, "handle")).toBe(false);
    expect(wheelButtonReady({ buttonsReady: true }, "handle")).toBe(true);
    expect(wheelButtonReady({}, "power")).toBe(false);
  });

  it("renews a Power hold when only Handle readiness falls, then releases when Power becomes unavailable", async () => {
    vi.useFakeTimers();
    const { input, session, setButton } = setup();
    const status = { buttonsReady: true, powerButtonReady: true };
    session.isCurrent = () => wheelButtonReady(status, "power");
    input.down(session, "power");
    await vi.advanceTimersByTimeAsync(0);
    status.buttonsReady = false;
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton.mock.calls).toEqual([
      ["power", true, 7],
      ["power", true, 7],
    ]);
    status.powerButtonReady = false;
    await vi.advanceTimersByTimeAsync(100);
    expect(setButton).toHaveBeenLastCalledWith("power", false, 7);
    await vi.advanceTimersByTimeAsync(1000);
    expect(setButton).toHaveBeenCalledTimes(3);
  });
});
