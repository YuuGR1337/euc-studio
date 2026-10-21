import { Popover } from "radix-ui";
import { Button } from "../../ui/Button.tsx";
import { Toggle } from "../../ui/Toggle.tsx";
import { ChevronDown, Info } from "../../ui/icons.tsx";
import type { HandlerInterval } from "./handlers.ts";

export function HandlerTable({
  enabled,
  onEnabled,
  interval,
  unavailable,
}: {
  enabled: boolean;
  onEnabled: (enabled: boolean) => void;
  interval: HandlerInterval | null;
  unavailable: string | null;
}) {
  const percent = (value: number | null | undefined) =>
    value == null ? "Unavailable" : `${value.toFixed(2)} %`;
  return (
    <section aria-label="ISR handler counters" className="flex h-full min-h-0 flex-col">
      <div className="shrink-0 border-b border-hairline px-16 py-8">
        <div className="flex flex-wrap items-center gap-16">
          <Toggle
            checked={enabled}
            onCheckedChange={onEnabled}
            label="Read handler counters"
            disabledReason={unavailable ?? undefined}
          />
          {enabled && !unavailable && (
            <span className="num text-13">ISR load: {percent(interval?.percent)}</span>
          )}
          <Popover.Root>
            <Popover.Trigger asChild>
              <Button
                size="sm"
                variant="ghost"
                className="group ml-auto gap-8 text-fg-2"
                icon={<Info size={14} aria-hidden="true" />}
              >
                Counter details
                <ChevronDown
                  size={14}
                  aria-hidden="true"
                  className="group-data-[state=open]:rotate-180"
                />
              </Button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content
                side="bottom"
                align="end"
                sideOffset={8}
                collisionPadding={8}
                hideWhenDetached
                className="pop z-50 max-h-(--radix-popover-content-available-height) w-[360px] max-w-[calc(100vw-16px)] origin-(--radix-popover-content-transform-origin) overflow-auto rounded-12 border border-hairline-strong bg-card p-16 text-12 text-fg-2 shadow-overlay"
              >
                <Popover.Title className="sr-only">Handler counter details</Popover.Title>
                <p>Counters use exclusive occupied simulator cycles.</p>
                <p className="mt-8">
                  Values are cycle estimates. They are not physical timing measurements.
                </p>
                <p className="mt-8">
                  Load uses elapsed machine cycles. Session cycles retain the complete counter
                  value.
                </p>
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
        </div>
        {unavailable && (
          <p role="status" className="mt-8 text-12 text-fg-2">
            {unavailable}
          </p>
        )}
        {enabled && !unavailable && (
          <>
            {interval?.elapsed_cycles && (
              <p
                className="num mt-8 truncate text-12 text-fg-2"
                title={`Interval: ${interval.elapsed_cycles} cycles, ${interval.seconds?.toFixed(4)} s.`}
              >
                Interval: {interval.elapsed_cycles} cycles · {interval.seconds?.toFixed(4)} s
              </p>
            )}
            {(!interval || interval.reason) && (
              <p role="status" className="mt-8 text-12 text-fg-2">
                {interval?.reason ?? "Waiting for machine counters."}
              </p>
            )}
            {interval?.truncated && (
              <p className="mt-8 text-12 text-caution">
                The table omits handlers beyond 128 rows. Aggregate load uses the complete ISR
                counter.
              </p>
            )}
          </>
        )}
      </div>
      {!enabled && !unavailable && (
        <p role="status" className="p-16 text-13 text-fg-2">
          Enable handler counters to see ISR load.
        </p>
      )}
      {enabled && !unavailable && interval && (
        <div className="min-h-0 flex-1 overflow-auto">
          <table className="w-full text-left text-12">
            <caption className="sr-only">
              Exclusive occupied cycles per exception. Load uses elapsed machine cycles.
            </caption>
            <thead className="sticky top-0 bg-card text-fg-2">
              <tr>
                <th className="p-8 pl-16">Handler</th>
                <th className="p-8">Exception</th>
                <th className="p-8 text-right">Interval cycles</th>
                <th className="p-8 text-right">Interval load</th>
                <th className="p-8 pr-16 text-right">Session cycles</th>
              </tr>
            </thead>
            <tbody>
              {interval.rows.map((row) => (
                <tr key={row.exception} className="border-t border-hairline hover:bg-raised">
                  <th scope="row" className="num p-8 pl-16 font-normal">
                    {row.name}
                  </th>
                  <td className="num p-8">{row.exception}</td>
                  <td className="num p-8 text-right">{row.cycles ?? "Unavailable"}</td>
                  <td className="num p-8 text-right">{percent(row.percent)}</td>
                  <td className="num p-8 pr-16 text-right">{row.total}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {interval.rows.length === 0 && !interval.reason && (
            <p className="p-16 text-12 text-fg-2">
              No handlers have occupied cycles in this session.
            </p>
          )}
        </div>
      )}
    </section>
  );
}
