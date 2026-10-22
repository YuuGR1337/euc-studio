import type { Ref } from "react";
import { LayoutGroup, motion } from "motion/react";
import { snappy } from "../design/motion.ts";
import { cx } from "../lib/cx.ts";
import { WORKSPACE_IDS, useUi } from "../state/ui.ts";
import { Keyboard, LayoutGrid, ScrollText } from "../ui/icons.tsx";
import { Tooltip } from "../ui/Tooltip.tsx";
import { WORKSPACES } from "./workspaces.tsx";

function RailButton({
  label,
  shortcut,
  icon,
  active,
  expanded,
  buttonRef,
  onClick,
}: {
  label: string;
  shortcut?: string;
  icon: React.ReactNode;
  active?: boolean;
  expanded?: boolean;
  buttonRef?: Ref<HTMLButtonElement>;
  onClick: () => void;
}) {
  return (
    <Tooltip content={label} shortcut={shortcut} side="right">
      <button
        ref={buttonRef}
        type="button"
        aria-label={label}
        aria-current={active ? "page" : undefined}
        aria-expanded={expanded}
        aria-haspopup={expanded !== undefined ? "dialog" : undefined}
        onClick={onClick}
        className={cx(
          "group pressable relative grid size-[40px] place-items-center rounded-12",
          active || expanded
            ? "text-accent-text"
            : "text-fg-2 live:hover:bg-card live:hover:text-fg",
        )}
      >
        {active && (
          <motion.span
            layoutId="rail-active"
            transition={snappy}
            className="absolute inset-0 rounded-12 border border-hairline bg-raised"
          />
        )}
        {expanded && (
          <span className="absolute inset-0 rounded-12 border border-hairline bg-raised" />
        )}
        <span className="relative [&_svg]:size-[20px]">{icon}</span>
      </button>
    </Tooltip>
  );
}

export function Rail({
  onRecordings,
  recordingsOpen,
  recordingsRef,
}: {
  onRecordings: () => void;
  recordingsOpen: boolean;
  recordingsRef: Ref<HTMLButtonElement>;
}) {
  const route = useUi((s) => s.route);
  const navigate = useUi((s) => s.navigate);
  const setShortcutsOpen = useUi((s) => s.setShortcutsOpen);
  return (
    <nav
      aria-label="Workspaces"
      className="flex w-(--rail) shrink-0 flex-col items-center gap-4 border-r border-hairline bg-bg py-8"
    >
      <LayoutGroup id="rail">
        {WORKSPACE_IDS.map((id, i) => (
          <RailButton
            key={id}
            label={WORKSPACES[id].label}
            shortcut={`mod+${i + 1}`}
            icon={WORKSPACES[id].icon}
            active={route === id}
            onClick={() => navigate(id)}
          />
        ))}
        <div className="flex-1" />
        <RailButton
          label="Recordings"
          icon={<ScrollText />}
          expanded={recordingsOpen}
          buttonRef={recordingsRef}
          onClick={onRecordings}
        />
        <RailButton
          label="Component gallery"
          icon={<LayoutGrid />}
          active={route === "gallery"}
          onClick={() => navigate("gallery")}
        />
        <RailButton
          label="Keyboard shortcuts"
          shortcut="?"
          icon={<Keyboard />}
          onClick={() => setShortcutsOpen(true)}
        />
      </LayoutGroup>
    </nav>
  );
}
