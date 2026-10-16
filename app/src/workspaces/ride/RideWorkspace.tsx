import { DockLayout } from "../../shell/DockLayout.tsx";
import { EventsPanel } from "./EventsPanel.tsx";
import { TelemetryPanel } from "./TelemetryPanel.tsx";
import { Viewport } from "./Viewport.tsx";

export function RideWorkspace({ active }: { active: boolean }) {
  return (
    <DockLayout
      workspace="ride"
      center={<Viewport active={active} />}
      right={<TelemetryPanel />}
      bottom={<EventsPanel />}
    />
  );
}
