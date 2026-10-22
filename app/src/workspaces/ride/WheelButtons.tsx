import { useCallback, useEffect, useRef, useState } from "react";
import { Hand, Power } from "lucide-react";
import { engine, useEngineData, type WheelButton } from "../../engine/index.ts";
import { prepareFirmwareAudio } from "../../engine/buzzerAudio.ts";
import { useUi } from "../../state/ui.ts";
import { Button } from "../../ui/Button.tsx";
import {
  ACCESSIBLE_PRESS_MS,
  ButtonActivationGuard,
  WheelButtonInput,
  wheelButtonReady,
  type ButtonInputState,
  type ButtonSession,
} from "./wheelButtonInput.ts";

const BUTTONS = [
  {
    id: "power",
    label: "Power",
    Icon: Power,
    help: "Tap for lights. Hold to switch off at standstill.",
  },
  {
    id: "handle",
    label: "Handle",
    Icon: Hand,
    help: "At standstill, press twice to disable drive. Press a third time to restore drive.",
  },
] as const;
interface Contact {
  session: ButtonSession;
  button: WheelButton;
  target: HTMLButtonElement;
  pointerId: number | null;
  key: " " | "Enter" | null;
}

/** Momentary controller inputs: down now, release now, with bounded backend leases. */
export function WheelButtons() {
  useEngineData();
  const source = engine;
  const alive = useRef(false);
  const contact = useRef<Contact | null>(null);
  const [state, setState] = useState<ButtonInputState | null>(null);
  const [error, setError] = useState<{ session: ButtonSession; text: string } | null>(null);
  const [guard] = useState(() => new ButtonActivationGuard());
  const detachContact = useCallback(() => {
    const held = contact.current;
    contact.current = null;
    if (!held) return;
    if (held.key !== null) guard.keyUp(held.target);
    if (held.pointerId !== null && held.target.hasPointerCapture(held.pointerId))
      held.target.releasePointerCapture(held.pointerId);
  }, [guard]);
  const [input] = useState(() => new WheelButtonInput());
  const release = useCallback(() => {
    detachContact();
    input.release();
  }, [detachContact, input]);

  useEffect(() => {
    alive.current = true;
    const offInput = input.subscribe(
      (next) => {
        if (next.button === null && contact.current?.session === next.session) detachContact();
        if (
          alive.current &&
          next.session.source === engine &&
          next.session.epoch === engine.status.runEpoch
        )
          setState(next);
      },
      (session, problem) => {
        if (alive.current && session.source === engine && session.epoch === engine.status.runEpoch)
          setError({ session, text: String(problem) });
      },
    );
    let timeVersion = source.time.version;
    let epoch = source.status.runEpoch;
    let scenario = source.scenarioId;
    let firmware = source.firmware;
    const off = source.subscribe(() => {
      const reset =
        source.status.runEpoch !== epoch ||
        source.scenarioId !== scenario ||
        source.firmware !== firmware ||
        (source.time.length === 0 && source.time.version !== timeVersion);
      if (
        reset ||
        engine !== source ||
        !source.playing ||
        source.status.state !== "running" ||
        (contact.current !== null && !wheelButtonReady(source.status, contact.current.button))
      )
        release();
      if (reset) {
        setState(null);
        setError(null);
      }
      timeVersion = source.time.version;
      epoch = source.status.runEpoch;
      scenario = source.scenarioId;
      firmware = source.firmware;
    });
    const hidden = () => {
      if (document.hidden) release();
    };
    window.addEventListener("blur", release);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      alive.current = false;
      release(); // Always send up to the captured source and epoch, even after engine replacement.
      guard.dispose();
      offInput();
      off();
      window.removeEventListener("blur", release);
      document.removeEventListener("visibilitychange", hidden);
    };
  }, [source, input, release, guard, detachContact]);

  const current =
    state?.session.source === source && state.session.epoch === source.status.runEpoch
      ? state
      : null;
  const problem =
    error?.session.source === source && error.session.epoch === source.status.runEpoch
      ? error.text
      : null;
  const disabledReason = (button: WheelButton) =>
    source.kind !== "live"
      ? "Physical buttons need the live simulator."
      : !source.setButton
        ? "Live button control is unavailable."
        : !source.playing ||
            source.status.state !== "running" ||
            source.status.runEpoch === undefined
          ? "Run the simulation to use the buttons."
          : !wheelButtonReady(source.status, button)
            ? "Wheel controls are not ready."
            : current?.releasing
              ? "Releasing the button."
              : undefined;
  const down = (
    button: WheelButton,
    target: HTMLButtonElement,
    pointerId: number | null,
    key: Contact["key"],
    pulse = false,
  ) => {
    const epoch = source.status.runEpoch;
    if (
      disabledReason(button) ||
      contact.current ||
      source !== engine ||
      !source.setButton ||
      epoch === undefined
    )
      return;
    // Unlock only the selected sound route. A physical input never changes the bell preference.
    if (useUi.getState().buzzerSound) void prepareFirmwareAudio(source);
    const session: ButtonSession = {
      source,
      epoch,
      setButton: source.setButton.bind(source),
      isCurrent: () =>
        alive.current &&
        source === engine &&
        source.status.runEpoch === epoch &&
        source.playing &&
        source.status.state === "running" &&
        wheelButtonReady(source.status, button),
    };
    if (pointerId !== null) {
      try {
        target.setPointerCapture(pointerId);
      } catch {
        return;
      }
    }
    contact.current = { session, button, target, pointerId, key };
    setError(null);
    if (!input.down(session, button, pulse ? ACCESSIBLE_PRESS_MS : undefined)) detachContact();
  };

  return (
    <section data-wheel-controls aria-label="Wheel buttons" className="flex flex-col gap-4">
      <div className="grid grid-cols-2 gap-8">
        {BUTTONS.map(({ id, label, Icon, help }) => {
          const pressed = current?.button === id;
          const unavailable = disabledReason(id);
          const detail =
            unavailable ?? `${help} Hold with pointer, Space, or Enter. Release ends the input.`;
          return (
            <Button
              key={id}
              size="md"
              variant={pressed ? "primary" : "secondary"}
              icon={<Icon aria-hidden="true" />}
              className="w-full touch-none px-8! [&_svg]:shrink-0"
              aria-disabled={!!unavailable || undefined}
              aria-label={`${label} button`}
              aria-pressed={pressed}
              aria-description={detail}
              onPointerDown={(event) => {
                if (!event.isPrimary || event.button !== 0) return;
                event.preventDefault();
                event.currentTarget.focus({ preventScroll: true });
                down(id, event.currentTarget, event.pointerId, null);
              }}
              onPointerUp={(event) => {
                if (
                  contact.current?.target === event.currentTarget &&
                  contact.current.pointerId === event.pointerId
                )
                  release();
              }}
              onPointerCancel={(event) => {
                if (
                  contact.current?.target === event.currentTarget &&
                  contact.current.pointerId === event.pointerId
                )
                  release();
              }}
              onLostPointerCapture={(event) => {
                if (
                  contact.current?.target === event.currentTarget &&
                  contact.current.pointerId === event.pointerId
                )
                  release();
              }}
              onBlur={(event) => {
                if (contact.current?.target === event.currentTarget) release();
              }}
              onContextMenu={(event) => event.preventDefault()}
              onKeyDown={(event) => {
                if (event.key !== " " && event.key !== "Enter") return;
                event.preventDefault();
                guard.keyDown(event.currentTarget);
                if (!event.repeat && !event.altKey && !event.ctrlKey && !event.metaKey)
                  down(id, event.currentTarget, null, event.key);
              }}
              onKeyUp={(event) => {
                if (event.key !== " " && event.key !== "Enter") return;
                event.preventDefault();
                guard.keyUp(event.currentTarget);
                if (
                  contact.current?.target === event.currentTarget &&
                  contact.current.key === event.key
                )
                  release();
              }}
              onClick={(event) => {
                event.preventDefault();
                if (!guard.acceptsClick(event.currentTarget, event.detail) || contact.current)
                  return;
                event.currentTarget.focus({ preventScroll: true });
                down(id, event.currentTarget, null, null, true);
              }}
            >
              {label}
            </Button>
          );
        })}
      </div>
      {problem && (
        <div role="alert" className="flex flex-col items-start gap-4 text-11 text-danger-text">
          <p>{problem}</p>
          {current?.releasing && (
            <Button
              size="sm"
              onClick={() => {
                setError(null);
                release();
              }}
            >
              Retry release
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
