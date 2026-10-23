import { DockLayout } from "../../shell/DockLayout.tsx";
import { EventsPanel } from "./EventsPanel.tsx";
import { RightDock } from "./TelemetryPanel.tsx";
import { Viewport } from "./Viewport.tsx";
import { useCallback, useEffect, useMemo, useState } from "react";
import { engine, useEngineVersion } from "../../engine/index.ts";
import type { HeightMap } from "../../engine/freeRide.ts";
import type { RecordedWorld } from "../../engine/recording.ts";
import { worldApi, type WorldFrame } from "../../engine/world.ts";
import { FreeRidePanel } from "../freeRide/FreeRidePanel.tsx";
import { useUi } from "../../state/ui.ts";
import type { GroundView } from "./ground.ts";
import { WorldView } from "./worldView.ts";

/** A recording's world as a view: its frame needs the world's height at the start. */
function useRecordedWorld(recorded: RecordedWorld | undefined): WorldView | null {
  const [view, setView] = useState<{ recorded: RecordedWorld; view: WorldView } | null>(null);
  useEffect(() => {
    if (!recorded) return;
    let cancelled = false;
    worldApi
      .originZ(recorded.spec, recorded.start_xy_m)
      .then((origin_z_m) => {
        if (!cancelled)
          setView({
            recorded,
            view: new WorldView({
              spec: recorded.spec,
              start_xy_m: recorded.start_xy_m,
              start_heading_rad: recorded.start_heading_rad,
              origin_z_m,
            }),
          });
      })
      .catch((error: unknown) => console.warn("Recorded world unavailable:", error));
    return () => {
      cancelled = true;
    };
  }, [recorded]);
  return view && view.recorded === recorded ? view.view : null;
}

/** A live scenario's world as a view, one per run. */
function useScenarioWorld(frame: WorldFrame | null | undefined): WorldView | null {
  return useMemo(() => (frame ? new WorldView(frame) : null), [frame]);
}

export function RideWorkspace({ active }: { active: boolean }) {
  useEngineVersion();
  const [controls, setControls] = useState(false);
  const [cached, setCached] = useState<{
    map: HeightMap | null;
    world: { frame: WorldFrame; view: WorldView } | null;
    epoch: number;
  } | null>(null);
  const onGround = useCallback(
    (ground: { map: HeightMap | null; world: WorldFrame | null } | null, epoch: number | null) => {
      // Closing controls stops the input. The ground must remain visible until the run changes.
      if (!ground || epoch === null || (!ground.map && !ground.world)) return;
      setCached((previous) =>
        previous?.epoch === epoch &&
        previous.map === ground.map &&
        previous.world?.frame === ground.world
          ? previous
          : {
              map: ground.map,
              // One view per run: the world's frame changes only with a new run.
              world: ground.world
                ? previous?.world?.frame === ground.world
                  ? previous.world
                  : { frame: ground.world, view: new WorldView(ground.world) }
                : null,
              epoch,
            },
      );
    },
    [],
  );
  const recordedWorld = useRecordedWorld(
    engine.kind === "playback" ? engine.recordedWorld : undefined,
  );
  const scenarioWorld = useScenarioWorld(
    engine.kind === "live" && engine.status.runMode !== "free_ride" ? engine.liveWorld : null,
  );
  const live =
    engine.kind === "live" &&
    engine.status.runMode === "free_ride" &&
    engine.status.runEpoch === cached?.epoch
      ? cached
      : null;
  const ground: GroundView | null =
    engine.kind === "playback"
      ? (engine.recordedMap ?? recordedWorld ?? null)
      : (live?.map ?? live?.world?.view ?? scenarioWorld);
  return (
    <DockLayout
      workspace="ride"
      center={
        <Viewport
          active={active}
          map={ground}
          freeControls={controls}
          onFreeControls={() => {
            setControls((open) => !open);
            useUi.getState().toggleDock("right", true);
          }}
        />
      }
      right={
        controls ? (
          <FreeRidePanel
            engine={engine}
            // The keys ride with the dock folded away too: only leaving Free Ride stops them.
            active={active}
            onGround={onGround}
            onExit={() => {
              engine.setScenario(
                engine.scenarioId || engine.scenarios()[0]?.id || "ride-20kmh-stop",
              );
              setControls(false);
            }}
          />
        ) : (
          <RightDock />
        )
      }
      bottom={<EventsPanel />}
    />
  );
}
