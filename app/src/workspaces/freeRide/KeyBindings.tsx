import { FREE_RIDE_ACTIONS, keyLabel } from "../../lib/rideControls.ts";
import { Button } from "../../ui/Button.tsx";
import { Kbd } from "../../ui/Kbd.tsx";
import type { ControlSnapshot, FreeRideController } from "./controller.ts";

/**
 * Free Ride's keys: each action with its keys, Change (the next key press binds) and Clear.
 * Escape pauses and cannot be bound; the studio's own shortcuts are refused with a reason.
 */
export function KeyBindings({
  state,
  controller,
}: {
  state: ControlSnapshot;
  controller: FreeRideController;
}) {
  return (
    <div className="flex flex-col gap-8" data-free-ride-keys>
      <span className="text-12 text-fg-2">Keys</span>
      <ul className="flex flex-col gap-4">
        {FREE_RIDE_ACTIONS.map(({ id, label }) => {
          const keys = state.bindings[id];
          const waiting = state.capturing === id;
          return (
            <li key={id} className="flex min-h-(--control-sm) items-center gap-8 text-12">
              <span className="min-w-0 flex-1 text-fg">{label}</span>
              <span className="flex items-center gap-4" aria-label={`${label} keys`}>
                {waiting ? (
                  <span className="text-accent-text" role="status">
                    Press a key
                  </span>
                ) : keys.length ? (
                  keys.map((code) => <Kbd key={code}>{keyLabel(code)}</Kbd>)
                ) : (
                  <span className="text-fg-2">None</span>
                )}
              </span>
              <Button
                size="sm"
                variant="ghost"
                aria-label={waiting ? `Stop changing ${label}` : `Change ${label}`}
                onClick={() => controller.captureKey(waiting ? null : id)}
              >
                {waiting ? "Cancel" : "Change"}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                aria-label={`Clear ${label}`}
                disabledReason={keys.length ? undefined : "No key to clear."}
                onClick={() => controller.clearKeys(id)}
              >
                Clear
              </Button>
            </li>
          );
        })}
      </ul>
      {state.bindError && (
        <p role="alert" className="text-12 text-danger-text">
          {state.bindError}
        </p>
      )}
      <div className="flex items-center justify-between gap-8">
        <p className="text-12 text-fg-2">
          <Kbd>Esc</Kbd> pauses. <Kbd>C</Kbd> changes the camera.
        </p>
        <Button size="sm" variant="ghost" onClick={controller.resetKeys}>
          Default keys
        </Button>
      </div>
    </div>
  );
}
