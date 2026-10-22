import { useState } from "react";
import { engine, useEngineVersion } from "../../engine/index.ts";
import {
  describeQuirks,
  normalizeQuirks,
  profileQuirks,
  quirksOf,
  sameQuirks,
  setQuirk,
} from "../../engine/quirkMask.ts";
import { QUIRKS } from "../../engine/quirks.ts";
import type { FirmwareId } from "../../engine/types.ts";
import { Button } from "../../ui/Button.tsx";
import { RotateCcw } from "../../ui/icons.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { Toggle } from "../../ui/Toggle.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";

const FIRMWARE_NOTE: Record<FirmwareId, string> = {
  stock: "The stock image has every quirk.",
  "ours-compat": "Ours-compat is an image: its quirks are fixed when it is built.",
  "ours-clean": "Ours-clean is an image: its quirks are fixed when it is built.",
  "ours-custom": "Ours-custom runs our firmware on the host with these quirks.",
};

/**
 * Tune's quirk switches: which stock behaviours of quirks.def the run keeps. They take effect when
 * the run restarts with "Ours (custom quirks)", our firmware as the host SIL library.
 */
export function QuirkPanel() {
  useEngineVersion();
  const running = quirksOf(engine.firmware, engine.quirks);
  const [draft, setDraft] = useState<{ base: string; ids: string[] } | null>(null);
  // A new run (another firmware or mask) drops edits made against the old one.
  const base = `${engine.firmware}:${running.join(",")}`;
  const ids = draft?.base === base ? draft.ids : running;
  const edit = (next: string[]) => setDraft({ base, ids: normalizeQuirks(next) });
  const custom = engine.firmware === "ours-custom";
  const unchanged = custom && sameQuirks(ids, running);
  const changes = QUIRKS.filter((q) => ids.includes(q.id) !== running.includes(q.id)).length;
  const playback = engine.kind === "playback";
  const why = playback
    ? "Firmware selection is unavailable during recording playback"
    : (engine.firmwares().find((f) => f.id === "ours-custom")?.disabledReason ??
      (unchanged ? "These quirks run now" : undefined));
  const groups = [...new Set(QUIRKS.map((q) => q.group))];
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="tune-quirks">
      <div className="flex shrink-0 flex-wrap items-center gap-8 border-b border-hairline px-16 py-8">
        <span className="num min-w-0 flex-1 truncate text-12 text-fg" title={describeQuirks(ids)}>
          {describeQuirks(ids)}
        </span>
        {changes > 0 && (
          <Pill tone="caution">
            {custom ? `${changes} not applied` : `${changes} from the running firmware`}
          </Pill>
        )}
        <span className="flex items-center gap-4">
          <span className="text-12 text-fg-2">Start from</span>
          <Button size="sm" variant="ghost" onClick={() => edit(profileQuirks("compat"))}>
            Compat
          </Button>
          <Button size="sm" variant="ghost" onClick={() => edit(profileQuirks("clean"))}>
            Clean
          </Button>
        </span>
        <Button
          size="sm"
          variant="primary"
          icon={<RotateCcw />}
          disabledReason={why}
          onClick={() => {
            engine.setFirmware("ours-custom", ids);
            setDraft(null);
          }}
        >
          Restart with these quirks
        </Button>
      </div>
      <p className="shrink-0 border-b border-hairline px-16 py-8 text-12 text-pretty text-fg-2">
        {FIRMWARE_NOTE[engine.firmware]} On keeps the stock behaviour, off fixes it; a restart
        applies them.
      </p>
      <div className="min-h-0 flex-1 overflow-auto" role="table" aria-label="Quirk switches">
        {groups.map((g) => (
          <div key={g} role="rowgroup">
            <div className="label sticky top-0 z-10 flex h-[28px] items-center bg-card px-16">
              {g}
            </div>
            {QUIRKS.filter((q) => q.group === g).map((q) => {
              const on = ids.includes(q.id);
              return (
                <div
                  key={q.id}
                  role="row"
                  data-quirk={q.id}
                  className="grid h-[32px] grid-cols-[208px_56px_minmax(0,1fr)] items-center gap-8 border-b border-hairline px-16 live:hover:bg-raised"
                >
                  <span role="cell" className="num truncate text-12 text-fg">
                    {q.id}
                  </span>
                  <span role="cell">
                    <Toggle
                      className="w-[36px]"
                      label={
                        <span className="sr-only">
                          {q.id}: {on ? "stock behaviour" : "fixed"}
                        </span>
                      }
                      checked={on}
                      onCheckedChange={(next) => edit(setQuirk(ids, q.id, next))}
                    />
                  </span>
                  <Tooltip content={q.stockRef}>
                    <span role="cell" className="cursor-default truncate text-12 text-fg-2">
                      {q.summary}
                    </span>
                  </Tooltip>
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
