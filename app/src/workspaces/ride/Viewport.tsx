import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { smooth } from "../../design/motion.ts";
import { series } from "../../design/tokens.ts";
import { cx } from "../../lib/cx.ts";
import { engine, useEngineVersion, useLive } from "../../engine/index.ts";
import { type CameraMode, cameraContextOf, useUi } from "../../state/ui.ts";
import { Gauge } from "../../ui/Gauge.tsx";
import { Crosshair, Orbit, RotateCcw, Unicycle } from "../../ui/icons.tsx";
import { Button } from "../../ui/Button.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { RollingNumber, RollingText } from "../../ui/RollingNumber.tsx";
import { SegmentedControl } from "../../ui/SegmentedControl.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";
import { Wait } from "../../ui/Wait.tsx";
import { FreeRideOverlay } from "../freeRide/FreeRideOverlay.tsx";
import { PitchProtractor } from "./PitchProtractor.tsx";
import { ManualButton, ManualHud } from "./ManualHud.tsx";
import { useManualRiding } from "./manual.ts";
import { LOW_FPS, type Quality } from "./quality.ts";
import { Scene } from "./Scene.tsx";
import { WheelOutputs } from "./WheelOutputs.tsx";
import { useSoundMix } from "./useSoundMix.ts";
import { slopeAt } from "./terrain.ts";
import { type GroundView, sampleGround } from "./ground.ts";
import { shellHeading, wheelPosition } from "./poses.ts";
import { Vector3 } from "three";

/** While the live engine starts or after it failed: what it is doing, or why it stopped. */
function EngineOverlay() {
  useEngineVersion();
  const { state, message } = engine.status;
  const show =
    engine.kind === "live" && (state === "starting" || state === "error" || state === "halted");
  return (
    <AnimatePresence>
      {show && (
        <motion.div
          className="absolute inset-0 z-20 grid place-items-center bg-bg/70 backdrop-blur-[2px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={smooth}
        >
          {state === "starting" ? (
            <Wait
              step={message || "Starting the simulator"}
              detail="The stock firmware boots in the simulated controller"
              onRetry={() => engine.restart()}
              onCancel={() => engine.setPlaying(false)}
            />
          ) : (
            <div className="flex w-[360px] flex-col gap-8 rounded-12 border border-hairline-strong bg-card p-16">
              <span className="text-13 font-semibold text-fg">
                {state === "halted" ? "The firmware stopped" : "The engine could not start"}
              </span>
              <span className="text-12 break-words text-fg-2">{message}</span>
              <div className="flex gap-8 pt-8">
                <Button size="sm" icon={<RotateCcw />} onClick={() => engine.restart()}>
                  Restart the run
                </Button>
              </div>
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function HudCard({
  children,
  className,
  testId,
}: {
  children: React.ReactNode;
  className?: string;
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      className={cx(
        "rounded-12 border border-hairline bg-card/75 backdrop-blur-[12px] backdrop-saturate-150",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Below this (km/h) the wheel counts as rolling backwards; smaller values are balance noise. */
const REVERSE_KMH = -0.5;

function Speed() {
  const speed = useLive((l) => l.speed, 8, 1);
  const reverse = speed < REVERSE_KMH;
  const digits = "text-44 font-medium tracking-[-0.03em]";
  const unit = <span className="text-13 font-semibold text-fg-2">km/h</span>;
  // Like the wheel's own display, the number is the magnitude; the direction is a tag, so a
  // sign never shifts the card. An invisible "88.8 km/h" holds the width up to 99.9 km/h.
  return (
    <div className="flex flex-col px-16 pt-8 pb-16">
      <span className="flex items-center gap-8">
        <span className="label">Speed</span>
        {reverse && (
          <span className="label text-caution" data-testid="speed-reverse">
            Reverse
          </span>
        )}
      </span>
      <span className="mt-4 grid">
        <span aria-hidden="true" className="invisible flex items-baseline gap-4 [grid-area:1/1]">
          <RollingText text="88.8" className={digits} />
          {unit}
        </span>
        <span className="flex items-baseline gap-4 [grid-area:1/1]">
          <RollingNumber value={Math.abs(speed)} decimals={1} className={cx(digits, "text-fg")} />
          {unit}
        </span>
      </span>
    </div>
  );
}

function Pwm() {
  const pwm = useLive((l) => l.pwm, 8, 0);
  return (
    <div className="grid place-items-center px-8">
      <Gauge
        value={pwm}
        label="PWM"
        unit="%"
        size={84}
        color={series.pwm}
        zones={[
          { from: 70, tone: "caution" },
          { from: 85, tone: "danger" },
        ]}
      />
    </div>
  );
}

function Battery() {
  const pct = useLive((l) => l.battery, 2, 0);
  const volts = useLive((l) => l.voltage, 4, 1);
  const tone = pct < 15 ? "bg-danger" : pct < 30 ? "bg-caution" : "bg-ok";
  return (
    <div className="flex min-w-[112px] flex-col px-16 pt-8 pb-16">
      <Tooltip content="Calculated charge from the battery model. Actual pack behavior needs measured cell data.">
        <span className="label cursor-help" tabIndex={0}>
          Model charge
        </span>
      </Tooltip>
      <span className="mt-4 flex items-baseline gap-2 text-20 font-medium text-fg">
        <RollingNumber value={pct} />
        <span className="text-13 font-semibold text-fg-2">%</span>
      </span>
      <span className="relative mt-8 h-[4px] w-full overflow-hidden rounded-pill bg-track">
        <span
          className={cx(
            "absolute inset-y-0 left-0 rounded-pill transition-[width] duration-(--dur-data) ease-data",
            tone,
          )}
          style={{ width: `${pct}%` }}
        />
      </span>
      <span className="mt-8 text-12 text-fg-2">
        <RollingNumber value={volts} decimals={1} /> V, 32S
      </span>
    </div>
  );
}

function Grade({ map, compact = false }: { map?: GroundView | null; compact?: boolean }) {
  const grade = useLive(
    (l) => {
      if (map) {
        const position = wheelPosition(new Vector3());
        const sample = position && sampleGround(map, position.x, -position.z);
        if (!sample) return Number.NaN;
        const [x, y, z] = sample.normal_world;
        const heading = shellHeading();
        return (-100 * (x * Math.cos(heading) + y * Math.sin(heading))) / z;
      }
      return (engine.kind === "playback" || engine.status.runMode === "free_ride") &&
        !engine.terrain
        ? Number.NaN
        : Math.tan(slopeAt(engine.terrain, l.poses?.[0] ?? l.distance, { z: 0, pitch: 0 }).pitch) *
            100;
    },
    4,
    1,
  );
  const degrees = (Math.atan(grade / 100) * 180) / Math.PI;
  if (compact)
    return (
      <Tooltip
        content={`Road grade along the wheel heading. ${Number.isFinite(degrees) ? degrees.toFixed(1) + "°" : "Ground unavailable"}`}
      >
        <span
          tabIndex={0}
          className="num inline-flex items-center gap-4 whitespace-nowrap rounded-pill border border-hairline bg-card/75 px-8 py-4 text-11 text-fg-2"
        >
          Grade{" "}
          <span className="text-fg">{Number.isFinite(grade) ? grade.toFixed(0) + "%" : "–"}</span>
        </span>
      </Tooltip>
    );
  return (
    <div className="flex min-w-[96px] flex-col px-16 pt-8 pb-16">
      <span className="label">Grade</span>
      <span className="num mt-4 text-20 font-medium text-fg">
        {Number.isFinite(grade) ? grade.toFixed(0) : "–"}
        <span className="ml-2 text-13 text-fg-2">%</span>
      </span>
      <span className="num mt-auto text-12 text-fg-2">
        {Number.isFinite(degrees) ? degrees.toFixed(1) : "–"}°
      </span>
    </div>
  );
}

function Pitch() {
  const pitch = useLive((l) => l.pitch, 8, 1);
  return (
    <HudCard className="px-16 pt-8 pb-8 @max-[760px]/viewport:py-4">
      <div className="flex items-center justify-between gap-8">
        <span className="label">Pitch</span>
        <span className="text-11 text-fg-2 @max-[760px]/viewport:hidden">nose up +</span>
        <span className="num hidden text-13 text-fg @max-[760px]/viewport:inline">
          {Number.isFinite(pitch) ? `${pitch >= 0 ? "+" : ""}${pitch.toFixed(1)}°` : "–"}
        </span>
      </div>
      <PitchProtractor pitch={pitch} className="mt-4 @max-[760px]/viewport:hidden" />
    </HudCard>
  );
}

/** `?scene=off` (tests that do not need the 3D view): no WebGL at all. */
const SCENE_OFF =
  typeof location !== "undefined" && new URLSearchParams(location.search).get("scene") === "off";

export function Viewport({
  active,
  map,
  freeControls,
  onFreeControls,
}: {
  active: boolean;
  map?: GroundView | null;
  freeControls?: boolean;
  onFreeControls?: () => void;
}) {
  const camera = useUi((s) => s.camera);
  const setCamera = useUi((s) => s.setCamera);
  useSoundMix();
  const [scene, setScene] = useState<{ quality: Quality; renderer: string } | null>(null);
  const [drawn, setDrawn] = useState(false);
  const [worldError, setWorldError] = useState<string | null>(null);
  const ready = SCENE_OFF || scene !== null;
  useManualRiding(active);
  // Ride and Free Ride each keep their own camera (Side and Chase by default).
  useEngineVersion();
  const runMode = engine.status.runMode;
  useEffect(() => useUi.getState().setCameraContext(cameraContextOf(runMode)), [runMode]);

  return (
    <div
      className="[container-name:viewport] [container-type:size] relative h-full overflow-hidden rounded-12 border border-hairline bg-bg"
      data-testid="viewport"
      data-scene={SCENE_OFF ? "off" : drawn ? "ready" : "building"}
    >
      <div className="absolute inset-0">
        {SCENE_OFF ? (
          <p className="grid h-full place-items-center text-12 text-fg-3">3D view off</p>
        ) : (
          <Scene
            active={active}
            map={map}
            onReady={setScene}
            onFirstFrame={() => setDrawn(true)}
            onWorldError={setWorldError}
          />
        )}
      </div>

      <EngineOverlay />

      <AnimatePresence>
        {!ready && (
          <motion.div
            className="absolute inset-0 grid place-items-center bg-bg"
            exit={{ opacity: 0 }}
            transition={smooth}
          >
            <Wait step="Building the scene" detail="Lighting, floor and wheel model" />
          </motion.div>
        )}
      </AnimatePresence>

      <div className="pointer-events-none absolute inset-0 flex flex-col justify-between p-16">
        <div className="relative flex items-start justify-between">
          <div className="pointer-events-auto flex items-center gap-8">
            <SegmentedControl<CameraMode>
              aria-label="Camera"
              size="sm"
              value={camera}
              onValueChange={setCamera}
              options={[
                {
                  value: "chase",
                  label: "Chase",
                  icon: <Unicycle />,
                  tooltip: "Camera behind the rider; it widens with speed",
                  shortcut: "c",
                },
                {
                  value: "follow",
                  label: "Side",
                  icon: <Crosshair />,
                  tooltip: "Camera follows the wheel from the side",
                  shortcut: "c",
                },
                {
                  value: "orbit",
                  label: "Orbit",
                  icon: <Orbit />,
                  tooltip: "Drag to orbit, scroll to zoom",
                  shortcut: "c",
                },
              ]}
            />
            <ManualButton />
            {onFreeControls && (
              <Button size="sm" onClick={onFreeControls}>
                {freeControls ? "Telemetry" : "Free ride"}
              </Button>
            )}
            <span className="hidden @max-[900px]/viewport:inline-flex">
              <Grade map={map} compact />
            </span>
          </div>
          <div className="absolute top-0 left-1/2 -translate-x-1/2">
            <ManualHud />
            <FreeRideOverlay />
          </div>
          <div className="pointer-events-auto">
            <Pitch />
          </div>
        </div>
        <div className="flex items-end justify-between gap-16 @max-[760px]/viewport:flex-col-reverse @max-[760px]/viewport:items-stretch @max-[760px]/viewport:gap-8">
          <HudCard
            testId="hud-ride"
            className="pointer-events-auto flex items-stretch divide-x divide-hairline"
          >
            <Speed />
            <Pwm />
            <Battery />
            <div className="@max-[900px]/viewport:hidden">
              <Grade map={map} />
            </div>
          </HudCard>
          <div className="flex flex-col items-end gap-8">
            {worldError && (
              <Tooltip
                side="top"
                content={`The engine did not send world chunks (${worldError.replace(/\.$/, "")}). The physics still rides the world; the view keeps what it drew and tries again.`}
              >
                <span className="pointer-events-auto">
                  <Pill tone="danger" size="md" dot>
                    World view incomplete
                  </Pill>
                </span>
              </Tooltip>
            )}
            {scene?.quality === "low" && (
              <Tooltip
                side="top"
                content={`No GPU acceleration (${scene.renderer.split(" (")[0] || "software WebGL"}). The 3D view draws at ${LOW_FPS} fps without shadows; telemetry is unaffected.`}
              >
                <span className="pointer-events-auto">
                  <Pill tone="neutral" size="md">
                    Reduced 3D quality
                  </Pill>
                </span>
              </Tooltip>
            )}
            <WheelOutputs className="pointer-events-auto" />
          </div>
        </div>
      </div>
    </div>
  );
}
