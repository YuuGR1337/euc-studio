import { useState } from "react";
import {
  type CompareFirmwareId,
  type CompareSummary,
  type Outcome,
  compare,
  useCompareVersion,
} from "../../engine/compare.ts";
import { cx } from "../../lib/cx.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import { Diverge, ScrollText } from "../../ui/icons.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { Pill, type PillTone } from "../../ui/Pill.tsx";
import { Tabs } from "../../ui/Tabs.tsx";
import { Toggle } from "../../ui/Toggle.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";
import { QUIRKS } from "./quirkList.ts";
import { type Quirk, hasQuirk } from "./quirks.ts";
import {
  VERDICT_LABEL,
  type SeriesVerdict,
  divergenceText,
  formatDiff,
  formatLimit,
  formatTime,
  overallVerdict,
  seriesLabel,
  seriesVerdict,
  summaryRows,
} from "./summaryFormat.ts";

type Tab = "summary" | "quirks";

const VERDICT_TONE: Record<SeriesVerdict, PillTone> = {
  identical: "neutral",
  within: "ok",
  over: "danger",
  unjudged: "neutral",
};

/** Columns of the summary table (fixed, so nothing moves while the numbers change). */
const COLUMNS = "grid-cols-[minmax(168px,1fr)_112px_72px_104px_88px_112px_92px]";

function SummaryTable({ summary }: { summary: CompareSummary }) {
  const { rows, identical } = summaryRows(summary);
  return (
    <div role="table" aria-label="Differences per channel" className="min-w-[880px]">
      <div
        role="row"
        className={cx(
          "sticky top-0 z-10 grid h-[32px] items-center gap-16 border-b border-hairline bg-card px-16",
          COLUMNS,
        )}
      >
        {["Channel or variable", "max |A−B|", "at", "RMS", "first", "limit", "verdict"].map(
          (h, i) => (
            <span
              key={h}
              role="columnheader"
              className={cx("label truncate", i > 0 && i < 6 && "text-right")}
            >
              {h}
            </span>
          ),
        )}
      </div>
      {rows.map((s) => {
        const verdict = seriesVerdict(s);
        return (
          <div
            key={`${s.group}-${s.name}`}
            role="row"
            className={cx(
              "grid h-[32px] items-center gap-16 border-b border-hairline px-16 last:border-b-0 live:hover:bg-raised",
              COLUMNS,
            )}
          >
            <span role="cell" className="flex min-w-0 items-baseline gap-8">
              <span className="truncate text-12 font-medium text-fg">
                {s.group === "channel" ? seriesLabel(s.name) : s.name}
              </span>
              {s.group === "channel" && (
                <span className="num truncate text-11 text-fg-3">{s.name}</span>
              )}
              {s.group === "var" && <span className="text-11 text-fg-3">variable</span>}
            </span>
            <span role="cell" className="num truncate text-right text-12 text-fg">
              {formatDiff(s.max_abs, s.unit)}
            </span>
            <span role="cell" className="num text-right text-12 text-fg-2">
              {formatTime(s.at_max_s)}
            </span>
            <span role="cell" className="num truncate text-right text-12 text-fg-2">
              {formatDiff(s.rms, s.unit)}
            </span>
            <span role="cell" className="num text-right text-12 text-fg-2">
              {formatTime(s.first_s)}
            </span>
            <span role="cell" className="num truncate text-right text-12 text-fg-2">
              {formatLimit(s)}
            </span>
            <span role="cell">
              <Pill tone={VERDICT_TONE[verdict]}>{VERDICT_LABEL[verdict]}</Pill>
            </span>
          </div>
        );
      })}
      <div className="flex h-[32px] items-center px-16 text-12 text-fg-2">
        {identical > 0
          ? `${identical} more ${identical === 1 ? "series is" : "series are"} identical and not judged.`
          : "Every series differs or is judged."}
        {summary.vars_unpaired.length > 0 && (
          <Tooltip content={summary.vars_unpaired.join(", ")}>
            <span className="ml-8 cursor-default underline decoration-hairline-strong underline-offset-4">
              {summary.vars_unpaired.length} variables only one run names
            </span>
          </Tooltip>
        )}
      </div>
    </div>
  );
}

function Checks({ label, outcome }: { label: string; outcome: Outcome }) {
  const passed = outcome.checks.filter((c) => c.passed).length;
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <span className="flex items-center gap-8 text-12 text-fg">
        <span className="font-semibold">{label}</span>
        <Pill tone={passed === outcome.checks.length ? "ok" : "danger"}>
          {passed} of {outcome.checks.length} checks
        </Pill>
        <span className="num text-fg-2">top {outcome.top_speed_kmh.toFixed(1)} km/h</span>
      </span>
      {outcome.checks
        .filter((c) => !c.passed)
        .map((c) => (
          <span key={c.what} className="truncate text-12 text-danger-text">
            {c.what}: {c.measured}
          </span>
        ))}
    </div>
  );
}

function SummaryBody() {
  const summary = compare.summary;
  const { state } = compare.status;
  if (!summary) {
    return (
      <div className="grid h-full place-items-center px-16 text-center text-13 text-fg-2">
        {state === "starting"
          ? "The summary fills in as soon as both runs move."
          : "Run a comparison: the summary shows how far the runs part, per channel, and when."}
      </div>
    );
  }
  const verdict = overallVerdict(summary);
  const first = summary.first_divergence;
  const ble = summary.ble;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="grid shrink-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-16 border-b border-hairline px-16 py-8">
        <Pill tone={verdict.tone} dot={summary.done ? true : "live"} size="md">
          {verdict.label}
        </Pill>
        <span className="flex min-w-0 items-center gap-8 text-12" data-testid="first-divergence">
          {first ? (
            <>
              <span className="shrink-0 font-semibold whitespace-nowrap text-fg">
                First divergence
              </span>
              <span className="num shrink-0 whitespace-nowrap text-fg">
                {formatTime(first.t_s, 3)}
              </span>
              <span className="num truncate text-fg-2">{divergenceText(first)}</span>
            </>
          ) : (
            <span className="text-fg-2">
              {summary.done ? "No divergence" : "No divergence so far"}
            </span>
          )}
        </span>
        <Tooltip
          content={`${compare.tolerance?.file ?? "No tolerance file"}: ${
            compare.tolerance?.note ?? "the limits CI judges compat against stock with"
          }`}
        >
          <span className="num max-w-[280px] cursor-default truncate text-12 text-fg-2">
            {summary.samples.toLocaleString("en-US")} samples,{" "}
            {compare.tolerance?.file.split("/").pop() ?? "no tolerance"}
          </span>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <SummaryTable summary={summary} />
        {(ble || summary.outcome_a || summary.check_disagreements.length > 0) && (
          <div className="flex flex-col gap-8 border-t border-hairline px-16 py-8">
            {ble && (
              <span className="num text-12 text-fg-2">
                BLE: {ble.frames_a} / {ble.frames_b} frames,{" "}
                {ble.first_frame
                  ? `frame ${ble.first_frame.index} differs (${ble.first_frame.field})`
                  : "identical"}
                ; text between frames {ble.text_identical ? "identical" : "differs"}
              </span>
            )}
            {summary.check_disagreements.map((d) => (
              <span key={d} className="text-12 text-danger-text">
                {d}
              </span>
            ))}
            {summary.outcome_a && summary.outcome_b && (
              <div className="grid grid-cols-2 gap-16">
                <Checks label="A" outcome={summary.outcome_a} />
                <Checks label="B" outcome={summary.outcome_b} />
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function QuirkCell({ value }: { value: boolean | null }) {
  if (value === null) return <span className="text-12 text-fg-3">unknown</span>;
  return value ? (
    <span className="flex items-center gap-4 text-12 text-caution">
      <span className="size-[6px] rounded-pill bg-caution" /> stock
    </span>
  ) : (
    <span className="flex items-center gap-4 text-12 text-ok">
      <span className="size-[6px] rounded-pill bg-ok" /> fixed
    </span>
  );
}

function QuirksBody({ a, b }: { a: CompareFirmwareId; b: CompareFirmwareId }) {
  const [onlyDiff, setOnlyDiff] = useState(() => readStored("compare.quirksOnlyDiff", true));
  const differs = (q: Quirk) => hasQuirk(a, q) !== hasQuirk(b, q);
  const shown = QUIRKS.filter((q) => !onlyDiff || differs(q));
  const count = QUIRKS.filter(differs).length;
  const groups = [...new Set(shown.map((q) => q.group))];
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-16 border-b border-hairline px-16 py-8">
        <Pill tone="info">Read only</Pill>
        <span className="min-w-0 flex-1 text-12 text-pretty text-fg-2">
          Quirks are fixed when an image is built: the engine runs images on the emulated
          controller. Switching one quirk at a time needs the SIL build in the engine.
        </span>
        <Toggle
          className="shrink-0"
          label={
            <span className="text-12 whitespace-nowrap">Only where A and B differ ({count})</span>
          }
          checked={onlyDiff}
          onCheckedChange={(v) => {
            setOnlyDiff(v);
            writeStored("compare.quirksOnlyDiff", v);
          }}
        />
      </div>
      <div className="min-h-0 flex-1 overflow-auto" role="table" aria-label="Quirks per firmware">
        {shown.length === 0 && (
          <p className="px-16 py-16 text-13 text-fg-2">
            Both firmwares have the same quirks. Turn the filter off to see the whole list.
          </p>
        )}
        {groups.map((g) => (
          <div key={g} role="rowgroup">
            <div className="label sticky top-0 z-10 flex h-[28px] items-center bg-card px-16">
              {g}
            </div>
            {shown
              .filter((q) => q.group === g)
              .map((q) => (
                <div
                  key={q.id}
                  role="row"
                  className="grid h-[32px] grid-cols-[208px_72px_72px_minmax(0,1fr)] items-center gap-8 border-b border-hairline px-16 live:hover:bg-raised"
                >
                  <span role="cell" className="num truncate text-12 text-fg">
                    {q.id}
                  </span>
                  <span role="cell">
                    <QuirkCell value={hasQuirk(a, q)} />
                  </span>
                  <span role="cell">
                    <QuirkCell value={hasQuirk(b, q)} />
                  </span>
                  <Tooltip content={q.stockRef}>
                    <span role="cell" className="cursor-default truncate text-12 text-fg-2">
                      {q.summary}
                    </span>
                  </Tooltip>
                </div>
              ))}
          </div>
        ))}
      </div>
    </div>
  );
}

/** The Compare workspace's bottom dock: the summary of the differences, and the quirk list. */
export function CompareDetails({ a, b }: { a: CompareFirmwareId; b: CompareFirmwareId }) {
  useCompareVersion();
  const [tab, setTab] = useState<Tab>(() =>
    readStored<Tab>("compare.tab", "summary") === "quirks" ? "quirks" : "summary",
  );
  return (
    <Panel
      title={
        <Tabs<Tab>
          aria-label="Compare details"
          value={tab}
          onValueChange={(t) => {
            setTab(t);
            writeStored("compare.tab", t);
          }}
          tabs={[
            { value: "summary", label: "Summary" },
            { value: "quirks", label: "Quirks" },
          ]}
          className="-mb-px h-(--dock-header) [&>div]:h-full"
        />
      }
      icon={tab === "summary" ? <Diverge /> : <ScrollText />}
      bodyClassName="flex flex-col overflow-hidden"
    >
      {tab === "summary" ? <SummaryBody /> : <QuirksBody a={a} b={b} />}
    </Panel>
  );
}
