import type { ReactNode } from "react";
import type { WorkspaceId } from "../state/ui.ts";
import { ChipWrite, Diverge, SlidersHorizontal, Unicycle } from "../ui/icons.tsx";

export const WORKSPACES: Record<WorkspaceId, { label: string; icon: ReactNode; summary: string }> =
  {
    ride: {
      label: "Ride",
      icon: <Unicycle />,
      summary: "Run firmware in the 3D simulation with a rider.",
    },
    tune: {
      label: "Tune",
      icon: <SlidersHorizontal />,
      summary: "Inspect firmware state, scopes and quirks.",
    },
    compare: {
      label: "Compare",
      icon: <Diverge />,
      summary: "Run two firmwares side by side and find where they differ.",
    },
    flash: {
      label: "Flash",
      icon: <ChipWrite />,
      summary: "Write firmware to a wheel over Bluetooth, behind safety gates.",
    },
  };
