// A comparison and the ride never run at once. Starting a comparison pauses the ride and remembers
// whether it was playing; the ride plays again once the comparison is over and the Compare
// workspace is left. While a comparison runs, the ride stays paused: anything that plays it (Space
// in Ride, a late status from the backend) pauses it again, and in Ride a toast says why.

import { compare } from "../../engine/compare.ts";
import { engine } from "../../engine/index.ts";
import { useUi } from "../../state/ui.ts";
import { toast } from "../../ui/toast-store.ts";

const comparing = () => compare.status.state === "starting" || compare.status.state === "running";

/** Installs the gate; returns its removal. */
export function installRideGate(): () => void {
  let resume = false;
  const maybeResume = () => {
    if (resume && !comparing() && useUi.getState().route !== "compare") {
      resume = false;
      engine.setPlaying(true);
    }
  };
  const hold = () => {
    if (!comparing() || !engine.playing) return;
    if (useUi.getState().route === "ride") {
      toast({
        tone: "info",
        title: "The ride waits for the comparison",
        description: "A comparison runs in Compare (⌘3). Stop it there to ride now.",
      });
    }
    engine.setPlaying(false);
  };
  const offCompare = compare.subscribe(() => {
    if (comparing()) {
      if (engine.playing) {
        resume = true;
        engine.setPlaying(false);
      }
    } else {
      maybeResume();
    }
  });
  const offEngine = engine.subscribe(hold);
  const offUi = useUi.subscribe((s, prev) => {
    if (s.route !== prev.route) maybeResume();
  });
  return () => {
    offCompare();
    offEngine();
    offUi();
  };
}
