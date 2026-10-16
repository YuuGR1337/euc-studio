import { useUi } from "../state/ui.ts";
import type { PaletteCommand } from "../ui/CommandPalette.tsx";
import { Shortcut } from "../ui/Kbd.tsx";
import { Sheet } from "../ui/Sheet.tsx";

const EXTRA: { group: string; title: string; shortcut: string }[] = [
  { group: "Help", title: "Command palette", shortcut: "mod+k" },
  { group: "Help", title: "Close a sheet or menu", shortcut: "esc" },
];

/** `?`: every shortcut, grouped the same way as the command palette. */
export function ShortcutsSheet({ commands }: { commands: PaletteCommand[] }) {
  const open = useUi((s) => s.shortcutsOpen);
  const setOpen = useUi((s) => s.setShortcutsOpen);
  const rows = [
    ...commands
      .filter((c) => c.shortcut)
      .map((c) => ({ group: c.group, title: c.title, shortcut: c.shortcut as string })),
    ...EXTRA,
  ];
  const groups = [...new Set(rows.map((r) => r.group))];

  return (
    <Sheet
      open={open}
      onOpenChange={setOpen}
      title="Keyboard shortcuts"
      description="Everything here is also in the command palette."
      className="w-[min(720px,calc(100vw-32px))]"
    >
      <div className="grid grid-cols-2 gap-x-32 gap-y-24 overflow-y-auto px-24 pt-24 pb-24">
        {groups.map((g) => (
          <section key={g}>
            <h3 className="label mb-8">{g}</h3>
            <ul>
              {rows
                .filter((r) => r.group === g)
                .map((r) => (
                  <li
                    key={r.title}
                    className="flex h-[32px] items-center justify-between gap-16 border-b border-hairline text-13 text-fg last:border-b-0"
                  >
                    <span className="truncate">{r.title}</span>
                    <Shortcut keys={r.shortcut} />
                  </li>
                ))}
            </ul>
          </section>
        ))}
      </div>
    </Sheet>
  );
}
