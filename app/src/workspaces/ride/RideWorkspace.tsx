import { DockLayout } from "../../shell/DockLayout.tsx";
import { EventsPanel } from "./EventsPanel.tsx";
import { RightDock } from "./TelemetryPanel.tsx";
import { Viewport } from "./Viewport.tsx";
import { useCallback, useState } from "react";
import { engine, useEngineVersion } from "../../engine/index.ts";
import type { HeightMap } from "../../engine/freeRide.ts";
import { FreeRidePanel } from "../freeRide/FreeRidePanel.tsx";
import { useUi } from "../../state/ui.ts";

export function RideWorkspace({ active }: { active: boolean }) {
  useEngineVersion();
  const [controls, setControls] = useState(false);
  const [cached, setCached] = useState<{ map: HeightMap; epoch: number } | null>(null);
  const dockOpen = useUi((state) => state.docks.ride.right);
  const onMap = useCallback((map: HeightMap | null, epoch: number | null) => {
    // Closing controls pauses the run. Its ground must remain visible until the run changes.
    if (map && epoch !== null)
      setCached((previous) =>
        previous?.map === map && previous.epoch === epoch ? previous : { map, epoch },
      );
  }, []);
  const liveMap =
    engine.kind === "live" &&
    engine.status.runMode === "free_ride" &&
    engine.status.runEpoch === cached?.epoch
      ? (cached?.map ?? null)
      : null;
  const map = engine.kind === "playback" ? (engine.recordedMap ?? null) : liveMap;
  return (
    <DockLayout
      workspace="ride"
      center={
        <Viewport
          active={active}
          map={map}
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
            active={active && dockOpen}
            onMap={onMap}
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
