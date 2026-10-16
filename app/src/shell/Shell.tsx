import { motion } from "motion/react";
import { type ReactNode, lazy, Suspense, useState } from "react";
import { smooth } from "../design/motion.ts";
import { type WorkspaceId, WORKSPACE_IDS, useUi } from "../state/ui.ts";
import { Wait } from "../ui/Wait.tsx";
import { CompareWorkspace, FlashWorkspace, TuneWorkspace } from "../workspaces/Placeholders.tsx";
import { Rail } from "./Rail.tsx";
import { RunBar } from "./RunBar.tsx";
import { StatusBar } from "./StatusBar.tsx";

const RideWorkspace = lazy(() =>
  import("../workspaces/ride/RideWorkspace.tsx").then((m) => ({ default: m.RideWorkspace })),
);
const Gallery = lazy(() => import("../gallery/Gallery.tsx").then((m) => ({ default: m.Gallery })));

function Loading({ step }: { step: string }) {
  return (
    <div className="grid h-full place-items-center">
      <Wait step={step} />
    </div>
  );
}

function render(id: WorkspaceId, active: boolean): ReactNode {
  switch (id) {
    case "ride":
      return (
        <Suspense fallback={<Loading step="Loading the Ride workspace" />}>
          <RideWorkspace active={active} />
        </Suspense>
      );
    case "tune":
      return <TuneWorkspace />;
    case "compare":
      return <CompareWorkspace />;
    case "flash":
      return <FlashWorkspace />;
  }
}

/**
 * Workspaces stay mounted once visited (the 3D scene and scopes keep their state) and swap with
 * the smooth token: the new one rises in while the old one fades back.
 */
function Workspaces() {
  const route = useUi((s) => s.route);
  const [visited, setVisited] = useState<Set<string>>(() => new Set([route]));
  if (!visited.has(route)) setVisited(new Set([...visited, route]));

  return (
    <main className="relative min-w-0 flex-1">
      {[...WORKSPACE_IDS, "gallery" as const].map((id) => {
        if (!visited.has(id)) return null;
        const active = route === id;
        return (
          <motion.section
            key={id}
            aria-hidden={!active}
            inert={!active}
            data-workspace={id}
            initial={false}
            animate={
              active
                ? { opacity: 1, y: 0, scale: 1, visibility: "visible" as const }
                : {
                    opacity: 0,
                    y: 8,
                    scale: 0.995,
                    transitionEnd: { visibility: "hidden" as const },
                  }
            }
            transition={active ? smooth : { duration: 0.16, ease: "easeIn" }}
            className="absolute inset-0"
          >
            {id === "gallery" ? (
              <Suspense fallback={<Loading step="Loading the gallery" />}>
                <Gallery />
              </Suspense>
            ) : (
              render(id, active)
            )}
          </motion.section>
        );
      })}
    </main>
  );
}

export function Shell() {
  return (
    <div className="flex h-full flex-col bg-bg">
      <RunBar />
      <div className="flex min-h-0 flex-1">
        <Rail />
        <Workspaces />
      </div>
      <StatusBar />
    </div>
  );
}
