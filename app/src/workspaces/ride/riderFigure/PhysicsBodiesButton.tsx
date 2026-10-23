import { useUi } from "../../../state/ui.ts";
import { IconButton } from "../../../ui/Button.tsx";
import { Box } from "../../../ui/icons.tsx";

/** Draws the rider as the shapes the physics simulates, instead of the rider figure. */
export function PhysicsBodiesButton() {
  const on = useUi((s) => s.physicsBodies);
  const setOn = useUi((s) => s.setPhysicsBodies);
  return (
    <IconButton
      label="Physics bodies: draw the rider as the shapes the physics simulates"
      icon={<Box />}
      active={on}
      onClick={() => setOn(!on)}
      size="sm"
      className="bg-card/75 backdrop-blur-[12px]"
    />
  );
}
