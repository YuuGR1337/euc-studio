import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { HeightMap } from "../../engine/freeRide.ts";
import type { WorldFrame } from "../../engine/world.ts";
import type { EngineClient } from "../../engine/types.ts";
import { keyLabel, type FreeRideAction } from "../../lib/rideControls.ts";
import { Button } from "../../ui/Button.tsx";
import { ChevronRight, RotateCcw } from "../../ui/icons.tsx";
import { Kbd } from "../../ui/Kbd.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { SegmentedControl } from "../../ui/SegmentedControl.tsx";
import { KeyBindings } from "./KeyBindings.tsx";
import { FreeRideController, LIGHT_MODES, PEDAL_MODES, offPedals } from "./controller.ts";

export interface FreeRidePanelProps {
  engine: EngineClient;
  /** Must become false on drawer close, route change, or engine source change. */
  active: boolean;
  /** The run's authoritative ground: the world frame (or an old run's test pad). Parent draws it. */
  onGround?: (
    ground: { map: HeightMap | null; world: WorldFrame | null } | null,
    epoch: number | null,
  ) => void;
  /** Optional parent action. It must select a scenario through engine.setScenario. */
  onExit?: () => void;
}

/** Mount in the Ride drawer. Never mount a second controller for the same native run. */
export function FreeRidePanel(props: FreeRidePanelProps) {
  return <FreeRideSession key={props.engine.kind} {...props} />;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section aria-label={title} className="flex flex-col gap-8 border-t border-hairline pt-16">
      <h3 className="text-12 text-fg-2">{title}</h3>
      {children}
    </section>
  );
}

/**
 * Free Ride's play mode: nothing to click before riding. The panel shows what the ride does now,
 * and holds respawn, the wheel's pad, the world and the keys. The firmware is chosen in the run
 * bar.
 */
function FreeRideSession({ engine, active, onGround, onExit }: FreeRidePanelProps) {
  const controller = useMemo(() => new FreeRideController(engine), [engine]);
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const [keysOpen, setKeysOpen] = useState(false);
  useEffect(() => {
    if (active && engine.kind === "live") return controller.connect();
  }, [active, controller, engine]);
  useEffect(() => {
    onGround?.(
      active ? { map: state.map, world: state.world } : null,
      active && state.status?.mode === "free_ride" ? state.status.epoch : null,
    );
  }, [active, onGround, state.map, state.world, state.status?.epoch, state.status?.mode]);
  const status = state.status;
  const hasRide = status?.mode === "free_ride";
  const cap = (action: FreeRideAction) => {
    const code = state.bindings[action][0];
    return code ? keyLabel(code) : null;
  };
  const respawnKey = cap("reset");
  const locked =
    engine.kind !== "live"
      ? "Open the desktop app to use Free ride."
      : !active
        ? "Open free ride controls first."
        : undefined;
  const runLocked = locked ?? (!hasRide ? "No free ride yet." : undefined);
  const backendError =
    state.error ??
    (engine.status.state === "error" || engine.status.state === "halted"
      ? engine.status.message
      : null);
  const boot = hasRide && !status.ready ? status.boot_remaining_s : null;
  const statusLine =
    locked ??
    (state.starting
      ? state.startingKind === "respawn"
        ? "Respawning. The firmware boots again."
        : "Building the world. The firmware boots on the stand."
      : !hasRide
        ? "Starting free ride."
        : state.powered === false
          ? `The wheel is off.${cap("power") ? ` ${cap("power")} powers it on.` : ""}`
          : !status.ready
            ? `The firmware boots on the stand${typeof boot === "number" ? `: ${boot.toFixed(1)} s` : "."}`
            : offPedals(status)
              ? `Off the pedals.${respawnKey ? ` ${respawnKey} respawns here, Shift ${respawnKey} at the start.` : ""}`
              : !state.playing
                ? "Paused. Esc or a riding key goes on."
                : `Riding. ${cap("forward") ?? "The stick"} leans forward.`);
  const pill =
    engine.kind !== "live"
      ? "Desktop only"
      : state.starting || (hasRide && !status.ready)
        ? "Starting"
        : !hasRide
          ? "Starting"
          : offPedals(status)
            ? "Off the pedals"
            : state.powered === false
              ? "Wheel off"
              : state.playing
                ? "Riding"
                : "Paused";
  const seed = state.world?.spec.seed;
  return (
    <Panel
      title="Free ride"
      actions={
        <>
          <Pill tone={pill === "Riding" ? "accent" : "neutral"} dot={pill === "Riding"}>
            {pill}
          </Pill>
          {onExit && (
            <Button size="sm" variant="ghost" onClick={onExit}>
              Scenario
            </Button>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-16 px-16 py-16 text-fg" data-free-ride-panel>
        <p role="status" className="text-13 text-fg">
          {statusLine}
        </p>
        {backendError && (
          <p role="alert" className="text-13 text-danger-text">
            {backendError}
          </p>
        )}
        <div className="flex items-center gap-8">
          <Button
            variant="primary"
            className="flex-1"
            icon={<RotateCcw />}
            disabledReason={runLocked ?? (state.starting ? "A respawn is on its way." : undefined)}
            onClick={() => controller.respawn(false)}
          >
            Respawn {respawnKey && <Kbd>{respawnKey}</Kbd>}
          </Button>
          <Button
            className="flex-1"
            disabledReason={runLocked ?? (state.starting ? "A respawn is on its way." : undefined)}
            onClick={() => controller.respawn(true)}
          >
            At the start {respawnKey && <Kbd>Shift {respawnKey}</Kbd>}
          </Button>
        </div>
        <Section title="Wheel">
          <p className="text-12 text-fg-2">
            Power {cap("power") && <Kbd>{cap("power")}</Kbd>}: tap for the firmware's tap, hold to
            switch the wheel off, press again to switch it on. Handle{" "}
            {cap("handle") && <Kbd>{cap("handle")}</Kbd>}: hold. Both are also in the Display card.
          </p>
          <div className="flex flex-wrap items-center justify-between gap-8">
            <span className="flex items-center gap-4 text-12 text-fg-2">
              Light {cap("light") && <Kbd>{cap("light")}</Kbd>}
            </span>
            <SegmentedControl<string>
              aria-label="Light"
              size="sm"
              value={state.light === null ? "" : String(state.light)}
              onValueChange={(value) => controller.light(Number(value))}
              options={LIGHT_MODES.map((label, i) => ({
                value: String(i),
                label,
                ...(runLocked ? { disabledReason: runLocked } : {}),
              }))}
            />
          </div>
          <div className="flex flex-wrap items-center justify-between gap-8">
            <span className="flex items-center gap-4 text-12 text-fg-2">
              Pedal mode {cap("pedalMode") && <Kbd>{cap("pedalMode")}</Kbd>}
            </span>
            <SegmentedControl<string>
              aria-label="Pedal mode"
              size="sm"
              value={state.pedalMode === null ? "" : String(state.pedalMode)}
              onValueChange={(value) => controller.pedalMode(Number(value))}
              options={PEDAL_MODES.map((label, i) => ({
                value: String(i),
                label,
                ...(runLocked ? { disabledReason: runLocked } : {}),
              }))}
            />
          </div>
          <Button size="sm" disabledReason={runLocked} onClick={controller.horn}>
            Horn {cap("horn") && <Kbd>{cap("horn")}</Kbd>}
          </Button>
          <p className="text-12 text-fg-2">
            Light and pedal mode go to the firmware as the phone app sends them; it may refuse them
            while riding fast.
          </p>
        </Section>
        <Section title="World">
          <div className="flex items-center justify-between gap-8">
            <span className="text-13">{seed === undefined ? "City" : `City, seed ${seed}`}</span>
            <Button
              size="sm"
              disabledReason={locked ?? (state.starting ? "A new run is on its way." : undefined)}
              onClick={controller.newWorld}
            >
              New world
            </Button>
          </div>
        </Section>
        <section aria-label="Keys" className="flex flex-col gap-8 border-t border-hairline pt-16">
          <Button
            size="sm"
            variant="ghost"
            className="justify-start"
            aria-expanded={keysOpen}
            icon={<ChevronRight className={keysOpen ? "rotate-90" : undefined} />}
            onClick={() => {
              if (keysOpen) controller.captureKey(null);
              setKeysOpen(!keysOpen);
            }}
          >
            Keys
          </Button>
          {keysOpen && <KeyBindings state={state} controller={controller} />}
          <p className="text-12 text-fg-2">
            A gamepad rides as soon as its stick moves: stick or triggers lean, LB crouches, X
            respawns (Back at the start), B steps off, Y changes the camera, Start pauses.
          </p>
        </section>
      </div>
    </Panel>
  );
}
