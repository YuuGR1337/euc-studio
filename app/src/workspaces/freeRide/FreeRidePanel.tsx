import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  RIDER_MODELS,
  isWorldChoice,
  limitsOf,
  type GroundChoice,
  type HeightMap,
} from "../../engine/freeRide.ts";
import type { WorldFrame } from "../../engine/world.ts";
import type { EngineClient } from "../../engine/types.ts";
import { keyLabel } from "../../lib/rideControls.ts";
import { Button, IconButton } from "../../ui/Button.tsx";
import { ChevronRight, Info, Keyboard, Pause, Play, RotateCcw } from "../../ui/icons.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { SegmentedControl } from "../../ui/SegmentedControl.tsx";
import { Select } from "../../ui/Select.tsx";
import { Slider } from "../../ui/Slider.tsx";
import { Tabs, TabPanel } from "../../ui/Tabs.tsx";
import { Toggle } from "../../ui/Toggle.tsx";
import { KeyBindings } from "./KeyBindings.tsx";
import { MapEditor } from "./MapEditor.tsx";
import { WorldEditor } from "./WorldEditor.tsx";
import {
  FreeRideController,
  RESTART_COUNTDOWN_S,
  offPedals,
  sliderTurnMaxDeg,
} from "./controller.ts";

export interface FreeRidePanelProps {
  engine: EngineClient;
  /** Must become false on drawer close, route change, or engine source change. */
  active: boolean;
  /** The run's authoritative ground: the native test pad grid or the world frame. Parent draws it. */
  onGround?: (
    ground: { map: HeightMap | null; world: WorldFrame | null } | null,
    epoch: number | null,
  ) => void;
  /** Optional parent action. It must select a scenario through engine.setScenario. */
  onExit?: () => void;
}
const number = (value: number | null | undefined, unit: string) =>
  value == null ? "Unavailable" : `${value.toFixed(1)} ${unit}`;

/** Mount in the Ride drawer. Never mount a second controller for the same native run. */
export function FreeRidePanel(props: FreeRidePanelProps) {
  return <FreeRideSession key={props.engine.kind} {...props} />;
}
function FreeRideSession({ engine, active, onGround, onExit }: FreeRidePanelProps) {
  const controller = useMemo(() => new FreeRideController(engine), [engine]);
  const state = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  const [tab, setTab] = useState<"controls" | "map">("controls");
  // The ground editor shows the current run's ground until a ground is chosen here.
  const [chosenGround, setGroundKind] = useState<"world" | "map" | null>(null);
  const groundKind =
    chosenGround ??
    (state.status?.mode === "free_ride" && state.status.ground === "map" ? "map" : "world");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [preciseOpen, setPreciseOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (active && engine.kind === "live" && root.current) return controller.connect(root.current);
  }, [active, controller, engine]);
  useEffect(() => {
    onGround?.(
      active ? { map: state.map, world: state.world } : null,
      active && state.status?.mode === "free_ride" ? state.status.epoch : null,
    );
  }, [active, onGround, state.map, state.world, state.status?.epoch, state.status?.mode]);
  const backendError =
    state.error ??
    (engine.status.state === "error" || engine.status.state === "halted"
      ? engine.status.message
      : null);
  const locked =
    engine.kind !== "live"
      ? "Open the desktop app to use Free ride."
      : !active
        ? "Open free ride controls first."
        : undefined;
  const editorLocked =
    locked ??
    (!state.status
      ? "Wait for native status."
      : state.starting
        ? "Starting free ride."
        : state.armed || state.arming || state.playing
          ? "Pause before changing the ground."
          : undefined);
  const hasRide = state.status?.mode === "free_ride";
  const booting = hasRide && !state.status?.ready;
  const restartKey = state.bindings.reset[0] ? keyLabel(state.bindings.reset[0]) : undefined;
  const armReason =
    locked ??
    (engine.status.state === "error" || engine.status.state === "halted"
      ? "Resolve the native engine error first."
      : undefined) ??
    (!state.status?.ready || !hasRide
      ? "Wait for the firmware boot and stand release."
      : state.starting
        ? "Wait for the new run."
        : state.arming
          ? "Waiting for Arm."
          : state.countdown
            ? "Wait for the restart, or cancel it with R."
            : offPedals(state.status)
              ? `The rider is off the wheel. Restart${restartKey ? ` with ${restartKey}` : ""} to ride again.`
              : state.held
                ? "Let go of the riding keys and the stick first."
                : state.source === "gamepad" && state.padIndex === null
                  ? "Connect the gamepad again, or push a riding key to ride with the keyboard."
                  : state.neutralMs < 250
                    ? "Getting ready: the engine needs 250 ms of neutral input first."
                    : state.status.source !== state.source
                      ? "Wait for the native input source."
                      : undefined);
  const stateLabel =
    engine.kind !== "live"
      ? "Desktop only"
      : state.starting
        ? "Starting"
        : state.arming
          ? "Arming"
          : state.armed
            ? "Riding"
            : booting
              ? "Starting"
              : "Paused";
  const limits = limitsOf(state.status);
  const turnMaxDeg = sliderTurnMaxDeg(limits);
  const forwardKey = keyLabel(state.bindings.forward[0] ?? "KeyW");
  const restartHere = limits.restart && state.status?.ground === "world";
  const padName = state.pads.find((pad) => pad.index === state.padIndex)?.id ?? null;
  const settingsLocked =
    locked ??
    (state.armed || state.arming || state.playing
      ? "Pause before changing input settings."
      : undefined);
  const standardPads = state.pads.filter((pad) => pad.standard);
  const modelLocked =
    locked ??
    (!state.status
      ? "Wait for native status."
      : state.starting
        ? "Wait for the new run."
        : state.armed || state.arming || state.playing
          ? "Pause before changing the rider model."
          : undefined);
  const modelChanged = hasRide && state.riderModel !== state.status?.rider_model;
  const actualModel = hasRide
    ? (RIDER_MODELS.find((model) => model.value === state.status?.rider_model)?.label ??
      "Unavailable")
    : "No free ride";
  const motionLocked = locked ?? (!state.armed ? "Arm controls first." : undefined);
  function pauseForEdit() {
    if (!active || engine.kind !== "live") return;
    engine.setPlaying(false);
    controller.pause("Controls paused for editing.");
  }
  function changeTab(value: "controls" | "map") {
    if (active) {
      controller.pause("Controls paused. Push to ride again.");
      if (value === "map" && engine.kind === "live") engine.setPlaying(false);
    }
    setTab(value);
  }
  function apply(ground: GroundChoice) {
    setTab("controls");
    void controller.start(ground);
  }
  const statusLine =
    locked ??
    (state.starting
      ? "Starting firmware on the stand."
      : state.arming
        ? "Waiting for native Arm."
        : !hasRide
          ? "Choose a world to create a ride."
          : booting
            ? state.status?.reason
            : state.countdown
              ? `Restarting ${state.countdown.here ? "here" : "at the start point"} after the countdown. ${restartKey ? `${restartKey} or Esc` : "Esc"} cancels.`
              : state.armed
                ? "Riding by hand."
                : state.reason);
  return (
    <Panel
      title="Free ride"
      actions={
        <>
          <Pill tone={state.armed ? "accent" : "neutral"} dot={state.armed}>
            {stateLabel}
          </Pill>
          {onExit && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                controller.pause("Scenario mode selected.");
                onExit();
              }}
            >
              Scenario
            </Button>
          )}
        </>
      }
    >
      <div
        ref={root}
        tabIndex={0}
        aria-label="Free ride control surface"
        className="text-fg outline-offset-2"
      >
        <Tabs
          aria-label="Free ride"
          value={tab}
          onValueChange={changeTab}
          className="flex flex-col px-16"
          tabs={[
            { value: "controls", label: "Controls" },
            { value: "map", label: "World" },
          ]}
        >
          <div className="border-b border-hairline py-8">
            <p role="status" className="text-12 text-fg-2">
              {statusLine}
            </p>
          </div>
          {backendError && (
            <p role="alert" className="pt-16 text-13 text-danger-text">
              {backendError}
            </p>
          )}
          <fieldset
            data-free-ride-choice
            className="flex min-w-0 flex-col gap-8 border-b border-hairline py-16"
          >
            <div className="flex items-center justify-between gap-8">
              <span className="text-12 text-fg-2">Rider model · next run</span>
              {!locked && !state.starting && (state.armed || state.arming || state.playing) && (
                <Button size="sm" variant="ghost" onClick={pauseForEdit}>
                  Pause to edit
                </Button>
              )}
              <IconButton
                size="sm"
                icon={<Info />}
                label={`Both models are estimates. Feet stay welded to the pedals. External yaw and roll aids remain. Articulated joints respond to motor forces and impacts. ${!limits.crouch_field ? "This engine build has no crouch input." : limits.can_crouch ? "Crouch bends the knees." : "The rigid rider cannot crouch."}`}
              />
            </div>
            <fieldset
              disabled={!!modelLocked}
              className="min-w-0 disabled:opacity-40"
              title={modelLocked}
            >
              <Select
                aria-label="Rider model for next run"
                value={state.riderModel}
                options={[...RIDER_MODELS]}
                className="w-full"
                onValueChange={(value) => {
                  if (!modelLocked) controller.setRiderModel(value);
                }}
              />
            </fieldset>
            {hasRide && <p className="text-12 text-fg-2">Current: {actualModel}</p>}
            {modelChanged && (
              <Button
                disabledReason={modelLocked}
                onClick={() => (state.applied ? apply(state.applied) : changeTab("map"))}
              >
                {state.applied ? "Start new run" : "Choose a world for the new run"}
              </Button>
            )}
          </fieldset>
          <TabPanel
            value="controls"
            forceMount
            hidden={tab !== "controls"}
            className="py-16 data-[state=inactive]:hidden"
          >
            <div className="flex flex-col gap-24">
              <p className="text-12 text-fg-2" data-testid="free-ride-input">
                {state.source === "gamepad"
                  ? `Riding with the gamepad${padName ? ` (${padName})` : ""}.${state.armed ? "" : " A riding key takes over while paused."}`
                  : `Riding with the keyboard${state.armed ? "." : `: push ${forwardKey} to ride.`}${!state.armed && standardPads.length ? " The gamepad takes over when its stick moves while paused." : ""}`}
              </p>
              <div className="flex items-center gap-8">
                {!hasRide && !state.starting ? (
                  <Button
                    variant="primary"
                    className="flex-1"
                    disabledReason={
                      locked ?? (!state.status ? "Wait for native status." : undefined)
                    }
                    onClick={() => changeTab("map")}
                  >
                    Create ride
                  </Button>
                ) : booting && !state.playing && !state.starting ? (
                  <Button
                    variant="primary"
                    className="flex-1"
                    icon={<Play />}
                    disabledReason={
                      locked ??
                      (backendError ? "Resolve the native engine error first." : undefined)
                    }
                    onClick={controller.continueBoot}
                  >
                    Continue startup
                  </Button>
                ) : state.armed ? (
                  <Button
                    variant="primary"
                    className="flex-1"
                    icon={<Pause />}
                    onClick={() => controller.pause()}
                  >
                    Pause ride
                  </Button>
                ) : (
                  <Button
                    variant="primary"
                    className="flex-1"
                    icon={<Play />}
                    disabledReason={armReason}
                    onClick={() => {
                      void controller.arm();
                    }}
                  >
                    Ride
                  </Button>
                )}
                <IconButton
                  label={
                    state.countdown
                      ? "Cancel the restart"
                      : restartHere
                        ? `Restart here: a new run at this spot after a ${RESTART_COUNTDOWN_S} s countdown (the firmware boots again)`
                        : `Restart at the start point after a ${RESTART_COUNTDOWN_S} s countdown (the firmware boots again)`
                  }
                  shortcut={restartKey?.toLowerCase()}
                  icon={<RotateCcw />}
                  active={!!state.countdown}
                  disabledReason={
                    locked ??
                    (!hasRide
                      ? "No free ride to restart."
                      : state.starting
                        ? "Wait for the new run."
                        : undefined)
                  }
                  onClick={controller.toggleCountdown}
                />
                {!state.armed && (
                  <IconButton
                    label="Disarm and pause"
                    icon={<Pause />}
                    disabledReason={locked}
                    onClick={() => controller.pause()}
                  />
                )}
              </div>
              {hasRide && state.source === "keyboard_slider" && (
                <Button
                  size="sm"
                  variant="ghost"
                  className="justify-start"
                  aria-expanded={preciseOpen}
                  icon={<ChevronRight className={preciseOpen ? "rotate-90" : undefined} />}
                  onClick={() => setPreciseOpen(!preciseOpen)}
                >
                  Precise input
                </Button>
              )}
              {hasRide && state.source === "keyboard_slider" && preciseOpen && (
                <div className="flex flex-col gap-16">
                  {(["lean", "turn"] as const).map((axis) => (
                    <div
                      key={axis}
                      data-free-ride-slider={axis}
                      onPointerDownCapture={controller.sliderStart}
                      onPointerUpCapture={controller.sliderRelease}
                      onPointerCancel={controller.sliderRelease}
                      onLostPointerCapture={controller.sliderRelease}
                      onKeyDownCapture={controller.sliderKeyDown}
                      onKeyUpCapture={controller.sliderKeyUp}
                      onBlur={controller.sliderRelease}
                    >
                      <Slider
                        label={axis === "lean" ? "Desired lean" : "Heading rate"}
                        value={state[axis]}
                        min={axis === "lean" ? -limits.lean_max_deg : -turnMaxDeg}
                        max={axis === "lean" ? limits.lean_max_deg : turnMaxDeg}
                        step={axis === "lean" ? 0.25 : 1}
                        decimals={axis === "lean" ? 1 : 0}
                        unit={axis === "lean" ? "°" : "°/s"}
                        disabledReason={motionLocked}
                        onValueChange={(value) => controller.sliderValue(axis, value)}
                      />
                    </div>
                  ))}
                  <div className="flex items-center justify-between gap-8">
                    <Button
                      size="sm"
                      variant="ghost"
                      disabledReason={locked}
                      onClick={() =>
                        controller.pause("Neutral input selected. Controls are paused.")
                      }
                    >
                      Neutral input
                    </Button>
                    <IconButton
                      size="sm"
                      label="Each slider sends its exact value while held, without the speed shaping of the keys and the stick. Sliders return to zero on release."
                      icon={<Keyboard />}
                    />
                  </div>
                </div>
              )}
              {hasRide && (
                <dl className="flex flex-col gap-8 text-12">
                  {(
                    [
                      ["Actual lean", state.status?.actual_lean_deg, "°"],
                      ["Heading", state.status?.heading_deg, "°"],
                      ["Path length", state.status?.path_length_m, "m"],
                    ] as const
                  ).map(([label, value, unit]) => (
                    <div key={label} className="flex items-baseline justify-between gap-16">
                      <dt className="text-fg-2">{label}</dt>
                      <dd className="num">{number(value, unit)}</dd>
                    </div>
                  ))}
                </dl>
              )}
              <div className="flex flex-col gap-8 border-t border-hairline pt-16">
                <div className="flex items-center justify-between gap-8">
                  <p className="text-12 text-fg-2">Rider estimates. Yaw and roll aids active.</p>
                  <IconButton
                    size="sm"
                    label="Manual lean has no speed controller. Neutral input does not ensure braking."
                    icon={<Info />}
                  />
                </div>
                <Button
                  size="sm"
                  variant="ghost"
                  className="justify-start"
                  aria-expanded={settingsOpen}
                  icon={<ChevronRight className={settingsOpen ? "rotate-90" : undefined} />}
                  onClick={() => {
                    if (!settingsOpen) pauseForEdit();
                    else controller.captureKey(null);
                    setSettingsOpen(!settingsOpen);
                  }}
                >
                  Input settings
                </Button>
                {settingsOpen && (
                  <div className="flex flex-col gap-24 py-8">
                    <KeyBindings state={state} controller={controller} />
                    <div className="flex flex-col gap-16">
                      <span className="text-12 text-fg-2">Gamepad</span>
                      <fieldset
                        data-free-ride-choice
                        disabled={!!settingsLocked || !state.gamepadSupported}
                        className="min-w-0 disabled:opacity-40"
                        title={settingsLocked}
                      >
                        <Select
                          aria-label="Gamepad"
                          value={state.padIndex === null ? "none" : String(state.padIndex)}
                          className="w-full"
                          options={[
                            { value: "none", label: "First connected gamepad" },
                            ...standardPads.map((pad) => ({
                              value: String(pad.index),
                              label: pad.id,
                            })),
                          ]}
                          onValueChange={(value) => {
                            if (!settingsLocked)
                              controller.selectPad(value === "none" ? null : Number(value));
                          }}
                        />
                      </fieldset>
                      {(!state.gamepadSupported || standardPads.length === 0) && (
                        <p className="text-12 text-fg-2" role="status">
                          {!state.gamepadSupported
                            ? "Gamepad input is unavailable in this window."
                            : "Connect a gamepad with standard mapping. Stick or triggers lean, LB crouches, X restarts, Y changes the camera, Start pauses."}
                        </p>
                      )}
                      <Slider
                        label="Stick dead zone"
                        value={state.deadzone * 100}
                        min={5}
                        max={30}
                        step={1}
                        unit="%"
                        disabledReason={settingsLocked}
                        onValueChange={(value) => controller.configure({ deadzone: value / 100 })}
                      />
                      <Slider
                        label="Stick response"
                        value={state.exponent}
                        min={1}
                        max={2.5}
                        step={0.1}
                        decimals={1}
                        unit="×"
                        disabledReason={settingsLocked}
                        onValueChange={(exponent) => controller.configure({ exponent })}
                      />
                      <Toggle
                        label="Invert lean"
                        checked={state.invertLean}
                        disabledReason={settingsLocked}
                        onCheckedChange={(invertLean) => controller.configure({ invertLean })}
                      />
                      <Toggle
                        label="Invert turn"
                        checked={state.invertTurn}
                        disabledReason={settingsLocked}
                        onCheckedChange={(invertTurn) => controller.configure({ invertTurn })}
                      />
                    </div>
                  </div>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  className="justify-start"
                  aria-expanded={detailsOpen}
                  icon={<ChevronRight className={detailsOpen ? "rotate-90" : undefined} />}
                  onClick={() => setDetailsOpen(!detailsOpen)}
                >
                  Ride details
                </Button>
                {detailsOpen && (
                  <dl className="flex flex-col gap-8 py-8 text-12">
                    {(
                      [
                        ["Desired lean", number(state.status?.desired_lean_deg, "°")],
                        ["Commanded lean", number(state.status?.commanded_lean_deg, "°")],
                        ["Yaw aid work", number(state.status?.yaw_aid_work_j, "J")],
                        ["Roll aid work", number(state.status?.roll_aid_work_j, "J")],
                        ["Neutral input", `${Math.min(250, Math.floor(state.neutralMs))}/250 ms`],
                        [
                          "Last control reply",
                          state.inputAgeMs === null
                            ? "Unavailable"
                            : `${Math.round(state.inputAgeMs)} ms ago`,
                        ],
                        ["Input rate / timeout", "20 Hz / 250 ms"],
                        ["Wheel", "Begode Extreme"],
                        ["Settings", "Blank"],
                        ["Rider model", state.status?.rider_model ?? "Unavailable"],
                      ] as const
                    ).map(([label, value]) => (
                      <div key={label} className="flex justify-between gap-16">
                        <dt className="text-fg-2">{label}</dt>
                        <dd className="num text-right">{value}</dd>
                      </div>
                    ))}
                    <div className="text-fg-2">
                      <dt className="sr-only">Native status</dt>
                      <dd>{state.status?.reason ?? "Native status is unavailable."}</dd>
                    </div>
                  </dl>
                )}
              </div>
            </div>
          </TabPanel>
          <TabPanel
            value="map"
            forceMount
            hidden={tab !== "map"}
            className="py-16 data-[state=inactive]:hidden"
          >
            <div className="flex flex-col gap-16">
              <SegmentedControl
                aria-label="Ground"
                value={groundKind}
                onValueChange={setGroundKind}
                options={[
                  { value: "world", label: "City world" },
                  { value: "map", label: "Test pad 32 m" },
                ]}
              />
              {groundKind === "world" ? (
                <WorldEditor
                  key={`${state.status?.epoch ?? "none"}:${state.applied ? "applied" : "unknown"}`}
                  applied={
                    state.applied && isWorldChoice(state.applied) ? state.applied.world : null
                  }
                  current={state.world}
                  disabledReason={editorLocked}
                  onApply={(world) => apply({ world })}
                />
              ) : (
                <MapEditor
                  key={`${state.status?.epoch ?? "none"}:${state.applied ? "applied" : "unknown"}`}
                  applied={
                    state.applied && !isWorldChoice(state.applied) ? state.applied.map : null
                  }
                  map={state.map}
                  disabledReason={editorLocked}
                  onApply={(map) => apply({ map })}
                  onPause={
                    active &&
                    engine.kind === "live" &&
                    (state.armed || state.arming || state.playing)
                      ? pauseForEdit
                      : undefined
                  }
                />
              )}
            </div>
          </TabPanel>
        </Tabs>
      </div>
    </Panel>
  );
}
