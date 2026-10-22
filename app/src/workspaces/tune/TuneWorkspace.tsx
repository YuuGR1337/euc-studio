import { useId, useState } from "react";
import { Popover } from "radix-ui";
import { Group, type Layout, Panel as ResizePanel, Separator } from "react-resizable-panels";
import { engine } from "../../engine/index.ts";
import { formatProbe, TUNE_MAX_SELECTED, TUNE_PAGE_SIZE } from "../../engine/tune.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import { Button } from "../../ui/Button.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { TabPanel, Tabs } from "../../ui/Tabs.tsx";
import { ChevronDown, Info, Search } from "../../ui/icons.tsx";
import { ProbeScope } from "./ProbeScope.tsx";
import { useTune } from "./useTune.ts";
import { TriggerControls } from "./TriggerControls.tsx";
import { HandlerTable } from "./HandlerTable.tsx";
import { QuirkPanel } from "./QuirkPanel.tsx";
import { captureKey, type TriggerConfig, type TriggerMode } from "./triggers.ts";

export function TuneWorkspace({ active }: { active: boolean }) {
  const [filter, setFilter] = useState("");
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<string[]>([]);
  const [mode, setMode] = useState<TriggerMode>("free");
  const [source, setSource] = useState("");
  const [thresholdText, setThresholdText] = useState("0");
  const [postSeconds, setPostSeconds] = useState(5);
  const [includeHandlers, setIncludeHandlers] = useState(true);
  const [instrument, setInstrument] = useState<"scopes" | "handlers" | "quirks">("scopes");
  const [layout] = useState(() =>
    readStored<Layout>("layout.tune.probes", { variables: 30, instruments: 70 }),
  );
  const [cursor, setCursor] = useState<{ session: number | null; offset: number | null }>({
    session: null,
    offset: null,
  });
  const numericThreshold = thresholdText.trim() === "" ? NaN : Number(thresholdText);
  const trigger: TriggerConfig = {
    mode,
    source: selected.includes(source) ? source : (selected[0] ?? ""),
    threshold: Number.isFinite(numericThreshold) ? numericThreshold : null,
    postSeconds,
  };
  const filterId = useId();
  const view = useTune(active, filter, page, selected, trigger, includeHandlers);
  const catalog = view.catalog;
  const run = view.snapshot?.run ?? catalog?.run;
  const capture = view.capture?.key === captureKey(trigger, selected) ? view.capture : null;
  const held = capture?.phase === "held";
  const captured = held || capture?.phase === "acquiring";
  const scopeHistory = captured ? capture.history : view.history;
  const windowEnd = (captured ? capture.end : view.snapshot?.t_s) ?? 0;
  const cursorOffset = cursor.session === run?.session_id ? cursor.offset : null;
  const onCursor = (offset: number | null) =>
    setCursor({ session: run?.session_id ?? null, offset });
  const variables = catalog?.variables ?? [];
  const matched = variables.filter((v) =>
    v.name.toLowerCase().includes(filter.toLowerCase().trim()),
  );
  const pageCount = Math.max(1, Math.ceil(matched.length / TUNE_PAGE_SIZE));
  const currentPage = Math.min(page, pageCount - 1);
  const visible = matched.slice(currentPage * TUNE_PAGE_SIZE, (currentPage + 1) * TUNE_PAGE_SIZE);
  const readings = new Map(view.snapshot?.values.map((v) => [v.name, v]));
  const demo = engine.kind !== "live";
  const playback = engine.kind === "playback";
  const reason = demo
    ? playback
      ? "Firmware probes are unavailable during recording playback."
      : "Firmware probes are unavailable in the browser and demo. Open the desktop simulator."
    : view.unavailable
      ? engine.status.message || "The simulator is not ready."
      : view.error;
  const choose = (name: string) => {
    setSelected((names) =>
      names.includes(name)
        ? names.filter((n) => n !== name)
        : names.length < TUNE_MAX_SELECTED
          ? [...names, name]
          : names,
    );
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-8 overflow-hidden p-8">
      <header className="flex shrink-0 flex-wrap items-center gap-8 px-8">
        <h1 className="text-15 font-semibold">Tune</h1>
        <Pill tone="info">Probes read only</Pill>
        <Pill tone={demo || reason ? "caution" : "neutral"}>
          {demo ? (playback ? "Playback" : "Demo") : (run?.state ?? engine.status.state)}
        </Pill>
        {view.snapshot && (
          <span className="num text-12 text-fg-2" aria-label="Simulation time">
            {view.snapshot.t_s.toFixed(3)} s
          </span>
        )}
        <Popover.Root>
          <Popover.Trigger asChild>
            <Button
              variant="ghost"
              className="group ml-auto gap-8 text-12 text-fg-2"
              icon={<Info size={14} aria-hidden="true" />}
            >
              Source and help
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
              className="pop z-50 max-h-[min(70vh,var(--radix-popover-content-available-height))] w-[560px] max-w-[calc(100vw-16px)] origin-(--radix-popover-content-transform-origin) overflow-auto rounded-12 border border-hairline-strong bg-card p-16 text-fg shadow-overlay"
            >
              <Popover.Title className="mb-16 text-13 font-semibold">
                Active probe source
              </Popover.Title>
              <dl className="grid grid-cols-2 gap-16 text-12">
                {[
                  ["Backend", run?.backend ?? engine.status.backend],
                  ["Firmware", run?.firmware ?? "Unavailable"],
                  ["Build", run?.build ?? "Unavailable"],
                  ["Source", run?.source ?? "Unavailable"],
                  ["Scenario", run?.scenario ?? engine.scenarioId],
                  ["Session", run ? String(run.session_id) : "Unavailable"],
                  [
                    "Simulation time",
                    view.snapshot ? `${view.snapshot.t_s.toFixed(3)} s` : "Unavailable",
                  ],
                  ["Read rate", "Up to 4 reads/s, wall time"],
                ].map(([label, value]) => (
                  <div key={label} className="min-w-0">
                    <dt className="mb-4 text-fg-2">{label}</dt>
                    <dd className="num break-words text-fg">{value}</dd>
                  </div>
                ))}
              </dl>
              <div className="mt-16 space-y-8 border-t border-hairline pt-16 text-12 text-fg-2">
                <p>
                  Values use the active Session map. The API does not expose the map file or
                  firmware hash.
                </p>
                <p>
                  Unknown units remain unspecified. The ISR tab uses separate machine cycle
                  counters.
                </p>
                <p>
                  Each scope keeps up to 120 samples over 30 simulation seconds. Paused runs keep
                  one sample per simulation time.
                </p>
                <p>
                  New sessions clear previous scope data. Memory writes, register bit fields and
                  gain editing are unavailable. The Quirks tab restarts the run with Ours-custom and
                  the quirks switched there.
                </p>
                <p>
                  Triggers capture up to 15 simulation seconds after a sampled crossing. Each
                  capture holds at most 240 samples per variable. Edges between polls can be missed.
                </p>
              </div>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </header>

      <div className="min-h-0 flex-1 overflow-auto">
        <Group
          orientation="horizontal"
          id="tune-probes"
          defaultLayout={layout}
          onLayoutChanged={(next) => writeStored("layout.tune.probes", next)}
          className="h-full min-w-[640px]"
        >
          <ResizePanel id="variables" minSize={300} maxSize="50%">
            <Panel
              title="Firmware variables"
              actions={<Pill>{catalog ? `${catalog.total} names` : "Unavailable"}</Pill>}
              bodyClassName="flex flex-col overflow-hidden"
            >
              <div className="shrink-0 border-b border-hairline p-8">
                <label htmlFor={filterId} className="sr-only">
                  Filter firmware variable names
                </label>
                <div className="relative">
                  <Search
                    size={14}
                    aria-hidden="true"
                    className="pointer-events-none absolute inset-y-0 left-8 my-auto text-fg-3"
                  />
                  <input
                    id={filterId}
                    type="search"
                    value={filter}
                    onChange={(event) => {
                      setFilter(event.target.value);
                      setPage(0);
                    }}
                    className="h-(--control-md) w-full rounded-8 border border-hairline bg-raised pr-8 pl-32 text-13 text-fg"
                    placeholder="Filter names"
                  />
                </div>
              </div>
              <div className="min-h-0 flex-1 overflow-auto">
                {!catalog || reason ? (
                  <p role="status" className="p-16 text-12 text-fg-2">
                    {reason ? "Firmware variables are unavailable." : "Loading firmware variables."}
                  </p>
                ) : (
                  <>
                    {!catalog.available && (
                      <p className="p-8 text-12 text-caution">
                        {catalog.reason ?? "The active session has no numeric firmware map."}
                      </p>
                    )}
                    {catalog.truncated && (
                      <p className="p-8 text-12 text-caution">
                        The catalog shows at most 512 valid names.
                      </p>
                    )}
                    <table className="w-full table-fixed text-left text-12">
                      <caption className="sr-only">
                        Current firmware values. Select at most four variables for scopes.
                      </caption>
                      <thead className="sticky top-0 z-10 bg-card text-fg-2">
                        <tr>
                          <th className="w-[40px] p-8">
                            <span className="sr-only">Scope</span>
                          </th>
                          <th className="p-8">Name</th>
                          <th className="w-[88px] p-8 text-right">Value</th>
                          <th className="w-[72px] p-8">Unit</th>
                        </tr>
                      </thead>
                      <tbody>
                        {visible.map((variable) => {
                          const reading = readings.get(variable.name);
                          const checked = selected.includes(variable.name);
                          const disabled = !checked && selected.length >= TUNE_MAX_SELECTED;
                          return (
                            <tr
                              key={variable.name}
                              className={`border-t border-hairline hover:bg-raised ${checked ? "bg-accent-tint" : ""}`}
                            >
                              <td className="p-8 text-center">
                                <input
                                  type="checkbox"
                                  checked={checked}
                                  disabled={disabled}
                                  onChange={() => choose(variable.name)}
                                  aria-label={`Scope ${variable.name}`}
                                  title={
                                    disabled
                                      ? "Select at most four variables."
                                      : `Scope ${variable.name}`
                                  }
                                  className="accent-accent"
                                />
                              </td>
                              <th
                                scope="row"
                                className="num truncate p-8 font-normal"
                                title={variable.name}
                              >
                                {variable.name}
                              </th>
                              <td
                                className="num truncate p-8 text-right"
                                title={
                                  reading?.reason ??
                                  (reading ? formatProbe(reading.value) : undefined)
                                }
                              >
                                {reading ? formatProbe(reading.value) : "Waiting"}
                              </td>
                              <td
                                className="truncate p-8 text-fg-2"
                                title={variable.unit_reference ?? "No documented unit."}
                              >
                                {variable.unit ?? "Unspecified"}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                    {matched.length === 0 && (
                      <p className="p-16 text-13 text-fg-2">No names match this filter.</p>
                    )}
                  </>
                )}
              </div>
              <nav
                aria-label="Variable pages"
                className="flex shrink-0 flex-wrap items-center gap-8 border-t border-hairline p-8"
              >
                <Button
                  size="sm"
                  onClick={() => setPage(Math.max(0, currentPage - 1))}
                  disabledReason={currentPage === 0 ? "This is the first page." : undefined}
                >
                  Previous
                </Button>
                <span className="num text-12 text-fg-2">
                  {currentPage + 1} / {pageCount}
                </span>
                <Button
                  size="sm"
                  onClick={() => setPage(currentPage + 1)}
                  disabledReason={
                    currentPage + 1 >= pageCount ? "This is the last page." : undefined
                  }
                >
                  Next
                </Button>
                <span className="ml-auto text-11 text-fg-2">{selected.length} / 4 scopes</span>
              </nav>
            </Panel>
          </ResizePanel>
          <Separator className="w-[8px] shrink-0" />
          <ResizePanel id="instruments" minSize="40%">
            <Tabs
              value={instrument}
              onValueChange={setInstrument}
              tabs={[
                { value: "scopes", label: "Scopes", count: selected.length },
                { value: "handlers", label: "ISR handlers" },
                { value: "quirks", label: "Quirks" },
              ]}
              aria-label="Tune instruments"
              className="flex h-full min-h-0 flex-col overflow-hidden rounded-12 border border-hairline bg-card [&>[role=tablist]]:h-(--dock-header) [&>[role=tablist]]:shrink-0 [&>[role=tablist]]:border-b [&>[role=tablist]]:border-hairline [&>[role=tablist]]:px-16"
            >
              <TabPanel
                value="scopes"
                className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden"
              >
                {!demo && !view.unavailable && (
                  <TriggerControls
                    config={trigger}
                    selected={selected}
                    thresholdText={thresholdText}
                    onMode={setMode}
                    onSource={setSource}
                    onThreshold={setThresholdText}
                    onPostSeconds={setPostSeconds}
                    capture={capture}
                    canHold={
                      Boolean(view.snapshot) &&
                      selected.some((name) => (view.history[name]?.length ?? 0) > 0)
                    }
                    hold={view.hold}
                    rearm={view.rearm}
                    cursorOffset={cursorOffset}
                    onCursor={onCursor}
                  />
                )}
                <div className="min-h-0 flex-1 overflow-auto bg-bg">
                  {reason ? (
                    <section
                      role="status"
                      className="flex h-full flex-col items-center justify-center gap-8 p-24 text-center"
                    >
                      <h2 className="text-15 font-semibold">Firmware probes are unavailable</h2>
                      <p className="max-w-[480px] text-13 text-fg-2">{reason}</p>
                      {!demo && (
                        <Button size="sm" onClick={view.retry}>
                          Reload probes
                        </Button>
                      )}
                    </section>
                  ) : !catalog ? (
                    <p role="status" className="p-16 text-13 text-fg-2">
                      Loading the active firmware map.
                    </p>
                  ) : selected.length === 0 ? (
                    <div className="flex h-full flex-col items-center justify-center gap-8 p-24 text-center">
                      <h2 className="text-15 font-semibold">Select a variable for a scope</h2>
                      <p className="text-13 text-fg-2">
                        Use the variable browser to add up to four scopes.
                      </p>
                    </div>
                  ) : (
                    <section
                      aria-label="Selected variable scopes"
                      className="grid min-h-full auto-rows-fr gap-8 p-8 [grid-template-columns:repeat(auto-fit,minmax(min(100%,320px),1fr))]"
                    >
                      {selected.map((name) => {
                        const variable = variables.find((v) => v.name === name);
                        const reading = readings.get(name);
                        const points = scopeHistory[name] ?? [];
                        const displayed = held ? points.at(-1)?.value : reading?.value;
                        return (
                          <Panel
                            key={`${run?.session_id ?? "none"}:${name}`}
                            title={
                              <h2 className="num truncate text-12" title={name}>
                                {name}
                              </h2>
                            }
                            actions={
                              <Button
                                size="sm"
                                variant="ghost"
                                onClick={() => choose(name)}
                                aria-label={`Remove ${name}`}
                              >
                                Remove
                              </Button>
                            }
                            className="min-h-[256px] min-w-0"
                            bodyClassName="flex flex-col"
                          >
                            <div className="flex shrink-0 flex-wrap items-center gap-8 px-16 py-8">
                              <span className="num text-20">{formatProbe(displayed)}</span>
                              {held && <Pill tone="caution">Held</Pill>}
                              <span className="text-12 text-fg-2">
                                {variable?.unit ?? "Unit unspecified"}
                              </span>
                              {!held && reading?.reason && (
                                <p className="w-full text-12 text-caution">{reading.reason}</p>
                              )}
                              {!variable && (
                                <p className="w-full text-12 text-caution">
                                  The active map has no such name.
                                </p>
                              )}
                            </div>
                            <div className="min-h-[176px] flex-1 [&>div]:flex [&>div]:h-full [&>div]:flex-col [&_svg]:min-h-[128px] [&_svg]:flex-1">
                              <ProbeScope
                                name={name}
                                points={points}
                                unit={variable?.unit ?? null}
                                windowEnd={windowEnd}
                                cursorOffset={cursorOffset}
                                onCursor={onCursor}
                                triggerAt={capture?.automatic ? capture.at : null}
                                threshold={
                                  mode !== "free" && trigger.source === name
                                    ? trigger.threshold
                                    : null
                                }
                              />
                            </div>
                          </Panel>
                        );
                      })}
                    </section>
                  )}
                </div>
              </TabPanel>
              <TabPanel
                value="quirks"
                className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden"
              >
                <QuirkPanel />
              </TabPanel>
              <TabPanel value="handlers" className="min-h-0 flex-1 data-[state=inactive]:hidden">
                <HandlerTable
                  enabled={includeHandlers}
                  onEnabled={setIncludeHandlers}
                  interval={view.handlers}
                  unavailable={
                    demo
                      ? playback
                        ? "ISR counters are unavailable during recording playback."
                        : "ISR counters are unavailable in the demo."
                      : view.unavailable
                        ? "The simulator is not ready."
                        : null
                  }
                />
              </TabPanel>
            </Tabs>
          </ResizePanel>
        </Group>
      </div>
    </div>
  );
}
