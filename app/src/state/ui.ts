import { create } from "zustand";
import { DEFAULT_ACCENT, accents, type AccentName } from "../design/tokens.ts";
import { readStored, writeStored } from "../lib/storage.ts";

export type WorkspaceId = "ride" | "tune" | "compare" | "flash";
export const WORKSPACE_IDS: WorkspaceId[] = ["ride", "tune", "compare", "flash"];
export type Route = WorkspaceId | "gallery";
/** Chase: behind the rider. Follow ("Side" in the view): the 3/4 side view. Orbit: dragged. */
export type CameraMode = "chase" | "follow" | "orbit";
export const CAMERA_MODES: readonly CameraMode[] = ["chase", "follow", "orbit"];
/**
 * The C key's cycle: from the context's default to Orbit, then the third view, then back. Ride:
 * Side, Orbit, Chase. Free Ride: Chase, Orbit, Side.
 */
export const nextCamera = (mode: CameraMode, context: CameraContext): CameraMode => {
  const first = DEFAULT_CAMERAS[context];
  const third = first === "chase" ? "follow" : "chase";
  return mode === first ? "orbit" : mode === "orbit" ? third : first;
};
/**
 * Each context keeps its own camera: Ride (scenarios, replays) starts on Side, the analysis view
 * where pitch and suspension read best; Free Ride starts on Chase.
 */
export type CameraContext = "ride" | "freeRide";
export const DEFAULT_CAMERAS: Readonly<Record<CameraContext, CameraMode>> = {
  ride: "follow",
  freeRide: "chase",
};
/** The camera context of a run mode. */
export const cameraContextOf = (runMode: string | undefined): CameraContext =>
  runMode === "free_ride" ? "freeRide" : "ride";
const storedCameras = (value: unknown): Record<CameraContext, CameraMode> => {
  const stored = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const pick = (context: CameraContext) =>
    CAMERA_MODES.includes(stored[context] as CameraMode)
      ? (stored[context] as CameraMode)
      : DEFAULT_CAMERAS[context];
  return { ride: pick("ride"), freeRide: pick("freeRide") };
};
const initialCameras = storedCameras(readStored<unknown>("camera.v2", null));
export type DockId = "right" | "bottom";
export type RightTab = "telemetry" | "phone";

type DockState = Record<DockId, boolean>;

interface UiState {
  route: Route;
  /** Last workspace, so leaving the gallery returns there. */
  workspace: WorkspaceId;
  accent: AccentName;
  /** The camera of the current context (cameras[cameraContext]). */
  camera: CameraMode;
  cameraContext: CameraContext;
  cameras: Record<CameraContext, CameraMode>;
  docks: Record<WorkspaceId, DockState>;
  paletteOpen: boolean;
  /** What the right dock shows in Ride. */
  rightTab: RightTab;
  shortcutsOpen: boolean;
  recordingsOpen: boolean;
  /** Bumped to ask the dock layout to reset to defaults. */
  layoutEpoch: number;
  /** Play the wheel's buzzer through the speakers (on by default). */
  buzzerSound: boolean;
  /** The master volume of the buzzer and the ride sound, 0 to 1 (the slider's position). */
  soundVolume: number;
  /** Mute the buzzer and the ride sound, keeping the volume. */
  soundMuted: boolean;

  navigate: (route: Route) => void;
  setAccent: (accent: AccentName) => void;
  /** Sets the camera of the current context, and remembers it for that context. */
  setCamera: (camera: CameraMode) => void;
  /** Switches to a context's own camera (the 3D view calls it when the run mode changes). */
  setCameraContext: (context: CameraContext) => void;
  toggleDock: (dock: DockId, open?: boolean) => void;
  resetLayout: () => void;
  setPaletteOpen: (open: boolean) => void;
  setRightTab: (tab: RightTab) => void;
  setShortcutsOpen: (open: boolean) => void;
  setRecordingsOpen: (open: boolean) => void;
  setBuzzerSound: (on: boolean) => void;
  setSoundVolume: (volume: number) => void;
  setSoundMuted: (muted: boolean) => void;
}

const DEFAULT_DOCKS: DockState = { right: true, bottom: true };

function routeFromHash(): Route {
  const h = typeof location !== "undefined" ? location.hash.replace(/^#\/?/, "") : "";
  if (h === "gallery") return "gallery";
  return (WORKSPACE_IDS as string[]).includes(h) ? (h as WorkspaceId) : "ride";
}

function applyAccent(accent: AccentName) {
  if (typeof document === "undefined") return;
  if (accent === DEFAULT_ACCENT) delete document.documentElement.dataset.accent;
  else document.documentElement.dataset.accent = accent;
}

const initialRoute = routeFromHash();
const storedAccent = readStored<string>("accent", DEFAULT_ACCENT);
const initialAccent: AccentName =
  storedAccent in accents ? (storedAccent as AccentName) : DEFAULT_ACCENT;
applyAccent(initialAccent);

/** The stored master volume, 0 to 1; 0.8 by default and for anything unreadable. */
function storedVolume(): number {
  const value = readStored<unknown>("soundVolume", 0.8);
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1
    ? value
    : 0.8;
}

export const useUi = create<UiState>((set, get) => ({
  route: initialRoute,
  workspace: initialRoute === "gallery" ? "ride" : initialRoute,
  accent: initialAccent,
  // camera.v2 keeps one camera per context (2026-10-22); the older "camera" choice is not read.
  camera: initialCameras.ride,
  cameraContext: "ride",
  cameras: initialCameras,
  docks: readStored("docks", {
    ride: DEFAULT_DOCKS,
    tune: DEFAULT_DOCKS,
    compare: DEFAULT_DOCKS,
    flash: DEFAULT_DOCKS,
  }),
  paletteOpen: false,
  rightTab: readStored<RightTab>("rightTab", "telemetry") === "phone" ? "phone" : "telemetry",
  shortcutsOpen: false,
  recordingsOpen: false,
  layoutEpoch: 0,
  buzzerSound: readStored<boolean>("buzzerSound", true) !== false,
  soundVolume: storedVolume(),
  soundMuted: readStored<boolean>("soundMuted", false) === true,

  navigate: (route) => {
    if (get().route === route) return;
    set(route === "gallery" ? { route } : { route, workspace: route });
    const hash = `#/${route}`;
    if (location.hash !== hash) history.replaceState(null, "", hash);
  },
  setAccent: (accent) => {
    applyAccent(accent);
    writeStored("accent", accent);
    set({ accent });
  },
  setCamera: (camera) => {
    const cameras = { ...get().cameras, [get().cameraContext]: camera };
    writeStored("camera.v2", cameras);
    set({ camera, cameras });
  },
  setCameraContext: (cameraContext) => {
    if (get().cameraContext === cameraContext) return;
    set({ cameraContext, camera: get().cameras[cameraContext] });
  },
  toggleDock: (dock, open) => {
    const { workspace, docks } = get();
    const current = docks[workspace];
    const next = { ...docks, [workspace]: { ...current, [dock]: open ?? !current[dock] } };
    writeStored("docks", next);
    set({ docks: next });
  },
  resetLayout: () => {
    const { workspace, docks, layoutEpoch } = get();
    const next = { ...docks, [workspace]: DEFAULT_DOCKS };
    writeStored("docks", next);
    set({ docks: next, layoutEpoch: layoutEpoch + 1 });
  },
  setPaletteOpen: (paletteOpen) => set({ paletteOpen }),
  setRightTab: (rightTab) => {
    writeStored("rightTab", rightTab);
    set({ rightTab });
  },
  setShortcutsOpen: (shortcutsOpen) => set({ shortcutsOpen }),
  setRecordingsOpen: (recordingsOpen) => set({ recordingsOpen }),
  setBuzzerSound: (buzzerSound) => {
    writeStored("buzzerSound", buzzerSound);
    set({ buzzerSound });
  },
  setSoundVolume: (volume) => {
    const soundVolume = Math.min(1, Math.max(0, Number.isFinite(volume) ? volume : 0));
    writeStored("soundVolume", soundVolume);
    set({ soundVolume });
  },
  setSoundMuted: (soundMuted) => {
    writeStored("soundMuted", soundMuted);
    set({ soundMuted });
  },
}));

if (typeof window !== "undefined") {
  window.addEventListener("hashchange", () => useUi.getState().navigate(routeFromHash()));
}
