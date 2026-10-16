import { MotionConfig } from "motion/react";
import { useCommands, useHotkeys } from "./shell/commands.tsx";
import { Shell } from "./shell/Shell.tsx";
import { ShortcutsSheet } from "./shell/ShortcutsSheet.tsx";
import { useUi } from "./state/ui.ts";
import { CommandPalette } from "./ui/CommandPalette.tsx";
import { Toaster } from "./ui/Toast.tsx";
import { TooltipProvider } from "./ui/Tooltip.tsx";

export function App() {
  const commands = useCommands();
  useHotkeys(commands);
  const paletteOpen = useUi((s) => s.paletteOpen);
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);

  return (
    // reducedMotion="user": transforms are dropped for people who ask for less motion;
    // digits cross-fade and camera moves cut (handled in RollingNumber and the camera rig).
    <MotionConfig reducedMotion="user">
      <TooltipProvider>
        <Shell />
        <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} commands={commands} />
        <ShortcutsSheet commands={commands} />
        <Toaster />
      </TooltipProvider>
    </MotionConfig>
  );
}
