import {
  activatePlayback,
  closeRecording,
  currentEngine,
  useEngineVersion,
} from "../engine/index.ts";
import { RecordingPanel } from "../workspaces/recordings/RecordingPanel.tsx";
import { Sheet } from "../ui/Sheet.tsx";
import { IconButton } from "../ui/Button.tsx";
import { X } from "../ui/icons.tsx";
import { motion } from "motion/react";
import { type ReactNode, lazy, Suspense, useState, useRef } from "react";
import { smooth } from "../design/motion.ts";
import { type WorkspaceId, WORKSPACE_IDS, useUi } from "../state/ui.ts";
import { Wait } from "../ui/Wait.tsx";
import { FlashWorkspace } from "../workspaces/Placeholders.tsx";
import { Rail } from "./Rail.tsx";
import { RunBar } from "./RunBar.tsx";
import { StatusBar } from "./StatusBar.tsx";

const RideWorkspace = lazy(() =>
  import("../workspaces/ride/RideWorkspace.tsx").then((m) => ({ default: m.RideWorkspace })),
);
const TuneWorkspace = lazy(() =>
  import("../workspaces/tune/TuneWorkspace.tsx").then((m) => ({ default: m.TuneWorkspace })),
);
const CompareWorkspace = lazy(() =>
  import("../workspaces/compare/CompareWorkspace.tsx").then((m) => ({
    default: m.CompareWorkspace,
  })),
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
      return (
        <Suspense fallback={<Loading step="Loading the Tune workspace" />}>
          <TuneWorkspace active={active} />
        </Suspense>
      );
    case "compare":
      return (
        <Suspense fallback={<Loading step="Loading the Compare workspace" />}>
          <CompareWorkspace active={active} />
        </Suspense>
      );
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
  useEngineVersion();
  const recordingsOpen = useUi((s) => s.recordingsOpen);
  const setRecordingsOpen = useUi((s) => s.setRecordingsOpen);
  const recordingsRef = useRef<HTMLButtonElement>(null);
  const openRecordingRef = useRef<HTMLButtonElement>(null);
  return (
    <div className="flex h-full flex-col bg-bg">
      <RunBar />
      <div className="flex min-h-0 flex-1">
        <Rail
          onRecordings={() => setRecordingsOpen(true)}
          recordingsOpen={recordingsOpen}
          recordingsRef={recordingsRef}
        />
        <Workspaces />
      </div>
      <Sheet
        open={recordingsOpen}
        onOpenChange={setRecordingsOpen}
        title="Recordings"
        returnFocusRef={recordingsRef}
        initialFocusRef={openRecordingRef}
        hideTitle
        className="w-[480px] max-w-[calc(100vw-32px)]"
      >
        <header className="flex h-(--dock-header) shrink-0 items-center gap-8 border-b border-hairline px-16">
          <h2 className="text-15 font-semibold">Recordings</h2>
          <IconButton
            className="ml-auto"
            label="Close recordings"
            icon={<X />}
            onClick={() => setRecordingsOpen(false)}
          />
        </header>
        <RecordingPanel
          active={recordingsOpen}
          openButtonRef={openRecordingRef}
          engine={currentEngine()}
          onOpen={(playback) => {
            activatePlayback(playback);
            useUi.getState().navigate("ride");
          }}
          onClose={closeRecording}
        />
      </Sheet>
      <StatusBar />
    </div>
  );
}
