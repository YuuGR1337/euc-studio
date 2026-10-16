import { Command } from "cmdk";
import { motion } from "motion/react";
import { type ReactNode, useMemo, useState } from "react";
import { snappy } from "../design/motion.ts";
import { cx } from "../lib/cx.ts";
import { matchCommand } from "../lib/match.ts";
import { Search } from "./icons.tsx";
import { Kbd, Shortcut } from "./Kbd.tsx";
import { Sheet } from "./Sheet.tsx";

export interface PaletteCommand {
  id: string;
  title: string;
  group: string;
  icon: ReactNode;
  shortcut?: string | undefined;
  keywords?: string[];
  /** Shown on the right instead of a shortcut (for example "Current"). */
  hint?: string;
  run: () => void;
}

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commands: PaletteCommand[];
}

/** ⌘K: every action in the app, searchable, with its shortcut. */
export function CommandPalette({ open, onOpenChange, commands }: CommandPaletteProps) {
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title="Command palette"
      hideTitle
      className="w-[min(600px,calc(100vw-32px))]"
    >
      <PaletteBody commands={commands} onRun={() => onOpenChange(false)} autoFocus />
    </Sheet>
  );
}

/** The palette content without the modal (also rendered statically in the gallery). */
export function PaletteBody({
  commands,
  onRun,
  initialSearch = "",
  autoFocus = false,
}: {
  commands: PaletteCommand[];
  onRun?: () => void;
  initialSearch?: string;
  /** Focus the search field on mount (the modal palette; not the gallery preview). */
  autoFocus?: boolean;
}) {
  const [search, setSearch] = useState(initialSearch);
  const [selected, setSelected] = useState(commands[0]?.id ?? "");
  const groups = useMemo(() => {
    const map = new Map<string, PaletteCommand[]>();
    for (const c of commands) map.set(c.group, [...(map.get(c.group) ?? []), c]);
    return [...map];
  }, [commands]);

  return (
    <Command
      label="Commands"
      value={selected}
      onValueChange={setSelected}
      filter={(_value, query, keywords) =>
        matchCommand(query, keywords?.[0] ?? "", keywords?.slice(1))
      }
      loop
      className="flex min-h-0 flex-col"
    >
      <div className="flex h-[52px] shrink-0 items-center gap-8 border-b border-hairline px-16">
        <Search className="size-[18px] shrink-0 text-fg-3" />
        <Command.Input
          value={search}
          onValueChange={setSearch}
          placeholder="Type a command or search"
          className="h-full min-w-0 flex-1 bg-transparent text-15 text-fg outline-none placeholder:text-fg-3"
          // The modal palette exists to be typed into, so its field takes focus.
          autoFocus={autoFocus}
        />
        <Kbd>Esc</Kbd>
      </div>
      <Command.List className="min-h-0 flex-1 scroll-py-8 overflow-y-auto p-8">
        <Command.Empty className="px-8 py-24 text-center text-13 text-fg-2">
          No command matches “{search}”.
        </Command.Empty>
        {groups.map(([group, items]) => (
          <Command.Group
            key={group}
            heading={group}
            className="[&_[cmdk-group-heading]]:label [&_[cmdk-group-heading]]:px-8 [&_[cmdk-group-heading]]:pt-8 [&_[cmdk-group-heading]]:pb-4 [&+&]:mt-8"
          >
            {items.map((c) => (
              <Command.Item
                key={c.id}
                value={c.id}
                keywords={[c.title, c.group, ...(c.keywords ?? [])]}
                onSelect={() => {
                  onRun?.();
                  // Let the sheet start closing before the action changes the screen.
                  requestAnimationFrame(() => c.run());
                }}
                className={cx(
                  "group relative flex h-[36px] cursor-default items-center gap-8 rounded-8 px-8 text-13 outline-none",
                  "text-fg-2 data-[selected=true]:text-fg",
                )}
              >
                {selected === c.id && (
                  <motion.span
                    layoutId="palette-highlight"
                    transition={snappy}
                    className="absolute inset-0 rounded-8 bg-raised"
                  />
                )}
                <span className="relative text-fg-3 group-data-[selected=true]:text-accent-text [&_svg]:size-[16px]">
                  {c.icon}
                </span>
                <span className="relative flex-1 truncate font-medium">{c.title}</span>
                {c.hint && <span className="relative text-12 text-fg-2-safe">{c.hint}</span>}
                {c.shortcut && <Shortcut keys={c.shortcut} className="relative" />}
              </Command.Item>
            ))}
          </Command.Group>
        ))}
      </Command.List>
      <footer className="flex h-[36px] shrink-0 items-center gap-16 border-t border-hairline px-16 text-12 text-fg-2">
        <span className="flex items-center gap-4">
          <Kbd>↑</Kbd>
          <Kbd>↓</Kbd>
          <span className="ml-2">Move</span>
        </span>
        <span className="flex items-center gap-4">
          <Kbd>↵</Kbd>
          <span className="ml-2">Run</span>
        </span>
        <span className="ml-auto">{commands.length} commands</span>
      </footer>
    </Command>
  );
}
